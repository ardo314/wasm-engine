//! Integration tests against the registry in `docker-compose.yml`.
//!
//! Skipped when nothing answers on `$OCI_REGISTRY`, so `cargo test
//! --workspace` still passes without `docker compose up -d registry`.

use std::time::Duration;

use oci_client::client::{ClientConfig, ClientProtocol};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use oci_wasm::{WasmClient, WasmConfig};
use sha2::{Digest, Sha256};
use wasm_host::{Discovery, Host, ResolveError, Resolver, engine};
use wasm_oci_fetch::OciFetcher;
use wasm_protocol::InterfaceId;
use wasm_registry::{ArtifactRef, Endpoint, Provider, ProviderKind, RegistryError, Resolution};
use wasmtime::component::{Component, Val};

const ADDER: &str = "test:fixture/adder@1.0.0";
const CALLER: &str = "test:fixture/caller@1.0.0";

const PROVIDER: &str = r#"
(component
  (core module $m
    (func (export "add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add))
  (core instance $i (instantiate $m))
  (func $add (param "a" u32) (param "b" u32) (result u32)
    (canon lift (core func $i "add")))
  (instance $adder (export "add" (func $add)))
  (export "test:fixture/adder@1.0.0" (instance $adder))
)
"#;

const CONSUMER: &str = r#"
(component
  (import "test:fixture/adder@1.0.0" (instance $adder
    (export "add" (func (param "a" u32) (param "b" u32) (result u32)))))
  (alias export $adder "add" (func $add))
  (core func $add-lowered (canon lower (func $add)))
  (core module $m
    (import "adder" "add" (func $add (param i32 i32) (result i32)))
    (func (export "sum") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      call $add))
  (core instance $i (instantiate $m
    (with "adder" (instance (export "add" (func $add-lowered))))))
  (func $sum (param "a" u32) (param "b" u32) (result u32)
    (canon lift (core func $i "sum")))
  (instance $caller (export "sum" (func $sum)))
  (export "test:fixture/caller@1.0.0" (instance $caller))
)
"#;

fn registry_host() -> String {
    std::env::var("OCI_REGISTRY").unwrap_or_else(|_| "localhost:5000".to_owned())
}

fn fetcher() -> OciFetcher {
    OciFetcher::insecure(vec![registry_host()], RegistryAuth::Anonymous)
}

/// Pushes `wasm` and returns the artifact that points at it, or `None` when no
/// registry is reachable.
async fn push(tag: &str, wasm: Vec<u8>) -> Option<ArtifactRef> {
    let uri = format!("{}/test/adder:{tag}", registry_host());
    let reference: Reference = uri.parse().expect("a reference");

    let client = WasmClient::new(Client::new(ClientConfig {
        protocol: ClientProtocol::HttpsExcept(vec![registry_host()]),
        ..ClientConfig::default()
    }));
    let (config, layer) = WasmConfig::from_raw_component(wasm.clone(), None)
        .expect("the component has a parseable world");

    let push = client.push(&reference, &RegistryAuth::Anonymous, layer, config, None);
    match tokio::time::timeout(Duration::from_secs(10), push).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            eprintln!("skipping: no registry at {} ({e:#})", registry_host());
            return None;
        }
        Err(_) => {
            eprintln!("skipping: registry at {} did not answer", registry_host());
            return None;
        }
    }

    Some(ArtifactRef {
        uri: format!("oci://{uri}"),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
    })
}

/// Answers with one `component` provider pointing at `artifact`.
struct OneComponent {
    artifact: ArtifactRef,
    digest: String,
}

impl Discovery for OneComponent {
    async fn resolve(&self, _: &str, _: &str) -> Result<Vec<Resolution>, RegistryError> {
        Ok(vec![Resolution {
            provider: Provider {
                id: "adder-component".to_owned(),
                kind: ProviderKind::Component,
                interfaces: vec![],
                endpoint: Endpoint::Artifact(self.artifact.clone()),
                ttl_secs: 30,
            },
            interface: ADDER.parse().unwrap(),
            shape_digest: self.digest.clone(),
        }])
    }
}

fn adder() -> InterfaceId {
    ADDER.parse().unwrap()
}

fn caller() -> InterfaceId {
    CALLER.parse().unwrap()
}

fn expected_digest(engine: &wasmtime::Engine) -> String {
    let consumer = Component::new(engine, CONSUMER).expect("consumer compiles");
    wasm_host::ComponentScan::new(engine, &consumer)
        .expect("scan")
        .import(&adder())
        .expect("adder is imported")
        .shape()
        .expect("a wire shape")
        .digest()
}

#[tokio::test]
async fn a_component_provider_is_pulled_from_the_registry_and_run() {
    let wasm = wat::parse_str(PROVIDER).expect("provider compiles");
    let Some(artifact) = push("v1", wasm).await else {
        return;
    };

    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());
    let registry = OneComponent {
        artifact,
        digest: expected_digest(&engine),
    };

    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = Resolver::new(engine.clone(), registry, fetcher())
        .plan(&[], consumer)
        .await
        .expect("the provider is pulled and planned");
    assert_eq!(plan.nodes().len(), 2, "the pulled component is a node");

    let instance = host.instantiate(plan).await.expect("instantiates");
    let mut results = vec![Val::U32(0)];
    host.call(
        instance,
        &caller(),
        "sum",
        &[Val::U32(20), Val::U32(22)],
        &mut results,
    )
    .await
    .expect("sum");

    assert_eq!(results, vec![Val::U32(42)]);
}

#[tokio::test]
async fn an_artifact_that_does_not_match_its_digest_is_refused() {
    let wasm = wat::parse_str(PROVIDER).expect("provider compiles");
    let Some(mut artifact) = push("v2", wasm).await else {
        return;
    };
    artifact.sha256 = "0".repeat(64);

    let engine = engine().expect("engine");
    let registry = OneComponent {
        artifact,
        digest: expected_digest(&engine),
    };

    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let error = Resolver::new(engine.clone(), registry, fetcher())
        .plan(&[], consumer)
        .await
        .expect_err("the bytes are not what was promised");

    assert!(
        matches!(error, ResolveError::ArtifactMismatch { .. }),
        "{error}"
    );
}
