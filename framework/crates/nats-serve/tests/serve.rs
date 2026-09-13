//! Integration tests against the NATS service in `docker-compose.yml`.
//!
//! Skipped when nothing answers on `$NATS_URL`.

use std::sync::Arc;
use std::time::Duration;

use async_nats::Client;
use wasm_host::{Discovery, Fetch, FetchError, Host, Proxy, Resolver, engine};
use wasm_nats_serve::Adapter;
use wasm_protocol::{
    ErrorCode, FunctionShape, InterfaceId, InterfaceShape, Reply, Request, Subject, WitType,
};
use wasm_registry::{ArtifactRef, Endpoint, Provider, ProviderKind, RegistryError, Resolution};
use wasmtime::component::{Component, Val};

/// Exports `adder`, adding two numbers.
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

/// Imports `adder` and exports `sum`.
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

const CALLER: &str = "test:fixture/caller@1.0.0";

struct NoRegistry;
impl Discovery for NoRegistry {
    async fn resolve(&self, _: &str, _: &str) -> Result<Vec<Resolution>, RegistryError> {
        Ok(Vec::new())
    }
}

struct NoArtifacts;
impl Fetch for NoArtifacts {
    async fn fetch(&self, _: &ArtifactRef) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::new("none"))
    }
}

/// Answers with the served component as a `service` provider.
struct OneService {
    interface: InterfaceId,
    digest: String,
}

impl Discovery for OneService {
    async fn resolve(&self, _: &str, _: &str) -> Result<Vec<Resolution>, RegistryError> {
        Ok(vec![Resolution {
            provider: Provider {
                id: "adder-over-nats".to_owned(),
                kind: ProviderKind::Service,
                interfaces: vec![],
                endpoint: Endpoint::Nats(Subject::interface_wildcard(&self.interface)),
                ttl_secs: 30,
            },
            interface: self.interface.clone(),
            shape_digest: self.digest.clone(),
        }])
    }
}

async fn connect() -> Option<Client> {
    let url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_owned());
    match async_nats::ConnectOptions::new()
        .connection_timeout(Duration::from_secs(2))
        .connect(&url)
        .await
    {
        Ok(client) => Some(client),
        Err(e) => {
            eprintln!("skipping: no NATS at {url} ({e}); run `docker compose up -d nats`");
            None
        }
    }
}

/// Subjects are cluster-wide, so each test gets its own interface version.
fn unique_interface(tag: &str) -> InterfaceId {
    format!("test:fixture/adder@1.0.{tag}").parse().unwrap()
}

fn shape() -> InterfaceShape {
    InterfaceShape::new(vec![FunctionShape::new(
        "add",
        vec![WitType::U32, WitType::U32],
        vec![WitType::U32],
    )])
}

/// A host running the provider component, with its `adder` published.
///
/// The component always exports `adder@1.0.0`; `interface` is the name it is
/// served under, so tests can have a subject to themselves.
async fn serve_provider(
    nats: Client,
    interface: &InterfaceId,
    concurrency: usize,
) -> (Arc<Host>, wasm_nats_serve::Served) {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());
    let provider = Component::new(&engine, PROVIDER).expect("provider compiles");
    let plan = Resolver::new(engine.clone(), NoRegistry, NoArtifacts)
        .plan(&[], provider)
        .await
        .expect("plan");
    let running = host.instantiate(plan).await.expect("instantiates");

    let host = Arc::new(host);
    let served = Adapter::new(nats)
        .with_concurrency(concurrency)
        .serve(Arc::clone(&host), running, interface.clone(), shape())
        .await
        .expect("serves");

    (host, served)
}

#[tokio::test]
async fn a_loaded_component_answers_a_bare_request() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("1");
    let (_host, served) = serve_provider(nats.clone(), &interface, 4).await;

    let payload = Request::new(
        "call-1",
        interface.to_string(),
        "add",
        vec![3.into(), 4.into()],
    )
    .encode()
    .expect("encodable");
    let message = nats
        .request(Subject::new(interface, "add").to_string(), payload.into())
        .await
        .expect("answered");

    let results = Reply::decode(&message.payload)
        .expect("a reply")
        .into_result()
        .expect("ok");
    assert_eq!(results, vec![rmpv::Value::from(7)]);
    assert_eq!(served.handled(), 1);
}

/// The acceptance criterion: a host that never loaded the component still
/// calls it, through the resolver and the import proxy.
#[tokio::test]
async fn a_host_that_never_loaded_the_component_can_call_it() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("2");
    let (_provider_host, _served) = serve_provider(nats.clone(), &interface, 4).await;

    let engine = engine().expect("engine");
    let mut consumer_host = Host::new(engine.clone()).with_remote(Proxy::new(nats));

    let registry = OneService {
        interface: interface.clone(),
        digest: shape().digest(),
    };
    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");

    // The consumer imports `adder@1.0.0`; the resolver asks the registry for
    // it and is handed this test's own version to address.
    let plan = Resolver::new(engine.clone(), registry, NoArtifacts)
        .plan(&[], consumer)
        .await
        .expect("resolves to the service");
    let running = consumer_host.instantiate(plan).await.expect("instantiates");

    let mut results = vec![Val::U32(0)];
    consumer_host
        .call(
            running,
            &CALLER.parse().unwrap(),
            "sum",
            &[Val::U32(20), Val::U32(22)],
            &mut results,
        )
        .await
        .expect("the call crosses the cluster");

    assert_eq!(results, vec![Val::U32(42)]);
}

#[tokio::test]
async fn two_replicas_share_the_load() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("3");
    let (_host_a, a) = serve_provider(nats.clone(), &interface, 4).await;
    let (_host_b, b) = serve_provider(nats.clone(), &interface, 4).await;

    let subject = Subject::new(interface.clone(), "add").to_string();
    for n in 0..20u32 {
        let payload = Request::new(
            format!("call-{n}"),
            interface.to_string(),
            "add",
            vec![n.into(), 1.into()],
        )
        .encode()
        .expect("encodable");
        let message = nats
            .request(subject.clone(), payload.into())
            .await
            .expect("answered");
        let results = Reply::decode(&message.payload)
            .expect("a reply")
            .into_result()
            .expect("ok");
        assert_eq!(results, vec![rmpv::Value::from(n + 1)]);
    }

    assert_eq!(
        a.handled() + b.handled(),
        20,
        "every call was answered once"
    );
    assert!(
        a.handled() > 0 && b.handled() > 0,
        "the queue group should spread the load: {} and {}",
        a.handled(),
        b.handled()
    );
}

#[tokio::test]
async fn an_unknown_function_is_not_found() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("4");
    let (_host, _served) = serve_provider(nats.clone(), &interface, 4).await;

    let payload = Request::new("call-1", interface.to_string(), "subtract", vec![])
        .encode()
        .expect("encodable");
    let message = nats
        .request(
            Subject::new(interface, "subtract").to_string(),
            payload.into(),
        )
        .await
        .expect("answered");

    let error = Reply::decode(&message.payload)
        .expect("a reply")
        .into_result()
        .expect_err("no such function");
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn arguments_of_the_wrong_shape_are_a_bad_request() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("5");
    let (_host, _served) = serve_provider(nats.clone(), &interface, 4).await;

    let payload = Request::new(
        "call-1",
        interface.to_string(),
        "add",
        vec!["three".into(), 4.into()],
    )
    .encode()
    .expect("encodable");
    let message = nats
        .request(Subject::new(interface, "add").to_string(), payload.into())
        .await
        .expect("answered");

    let error = Reply::decode(&message.payload)
        .expect("a reply")
        .into_result()
        .expect_err("a string is not a u32");
    assert_eq!(error.code, ErrorCode::BadRequest);
}
