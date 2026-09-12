//! #5's end-to-end criterion, finally assertable: the same component, once
//! against a live component and once against a service on NATS.
//!
//! Skipped when nothing answers on `$NATS_URL`.

mod support;

use std::time::Duration;

use async_nats::Client;
use futures::StreamExt;
use support::{ADDER, CALLER, CONSUMER, PROVIDER};
use wasm_host::{
    ComponentScan, Discovery, Fetch, FetchError, Host, LinkError, Proxy, Resolver, engine,
};
use wasm_protocol::{InterfaceId, Reply, Request, Subject};
use wasm_registry::{ArtifactRef, Endpoint, Provider, ProviderKind, RegistryError, Resolution};
use wasmtime::Engine;
use wasmtime::component::{Component, Val};

struct NoArtifacts;

impl Fetch for NoArtifacts {
    async fn fetch(&self, _artifact: &ArtifactRef) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::new("no artifacts in this test"))
    }
}

struct NoRegistry;

impl Discovery for NoRegistry {
    async fn resolve(&self, _: &str, _: &str) -> Result<Vec<Resolution>, RegistryError> {
        Ok(Vec::new())
    }
}

/// Answers with one `service` provider, at the digest the consumer expects.
struct OneService {
    interface: InterfaceId,
    digest: String,
}

impl Discovery for OneService {
    async fn resolve(&self, _: &str, _: &str) -> Result<Vec<Resolution>, RegistryError> {
        Ok(vec![Resolution {
            provider: Provider {
                id: "math-service".to_owned(),
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

/// A native service implementing `add`, which is what the component does.
fn serve_adder(nats: Client, interface: InterfaceId) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let subject = Subject::new(interface, "add").to_string();
        let mut requests = nats.subscribe(subject).await.expect("subscribe");
        while let Some(message) = requests.next().await {
            let request = Request::decode(&message.payload).expect("a request");
            let sum: u64 = request
                .args
                .iter()
                .map(|arg| arg.as_u64().expect("an integer"))
                .sum();
            let reply = Reply::ok(&request.id, vec![rmpv::Value::from(sum)])
                .encode()
                .expect("encodable");
            if let Some(to) = message.reply {
                nats.publish(to, reply.into()).await.expect("publish");
            }
        }
    })
}

fn adder() -> InterfaceId {
    ADDER.parse().unwrap()
}

fn caller() -> InterfaceId {
    CALLER.parse().unwrap()
}

/// What the consumer expects `adder` to look like.
fn expected_digest(engine: &Engine) -> String {
    let consumer = Component::new(engine, CONSUMER).expect("consumer compiles");
    ComponentScan::new(engine, &consumer)
        .expect("scan")
        .import(&adder())
        .expect("adder is imported")
        .shape()
        .expect("a wire shape")
        .digest()
}

async fn sum_in_process(a: u32, b: u32) -> Val {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());
    let resolver = Resolver::new(engine.clone(), NoRegistry, NoArtifacts);

    let provider = Component::new(&engine, PROVIDER).expect("provider compiles");
    let plan = resolver.plan(&[], provider).await.expect("provider plan");
    host.instantiate(plan).await.expect("provider instantiates");

    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = resolver
        .plan(&host.live(), consumer)
        .await
        .expect("consumer plan");
    let instance = host.instantiate(plan).await.expect("consumer instantiates");

    let mut results = vec![Val::U32(0)];
    host.call(
        instance,
        &caller(),
        "sum",
        &[Val::U32(a), Val::U32(b)],
        &mut results,
    )
    .await
    .expect("in-process sum");
    results.remove(0)
}

async fn sum_over_nats(nats: Client, a: u32, b: u32) -> Val {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone()).with_remote(Proxy::new(nats));

    let registry = OneService {
        interface: adder(),
        digest: expected_digest(&engine),
    };
    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = Resolver::new(engine.clone(), registry, NoArtifacts)
        .plan(&[], consumer)
        .await
        .expect("consumer plan");
    let instance = host.instantiate(plan).await.expect("consumer instantiates");

    let mut results = vec![Val::U32(0)];
    host.call(
        instance,
        &caller(),
        "sum",
        &[Val::U32(a), Val::U32(b)],
        &mut results,
    )
    .await
    .expect("remote sum");
    results.remove(0)
}

#[tokio::test]
async fn the_same_component_agrees_in_process_and_over_nats() {
    let Some(nats) = connect().await else { return };
    let server = serve_adder(nats.clone(), adder());

    for (a, b) in [(0, 0), (1, 2), (40, 2), (1_000, 337)] {
        let local = sum_in_process(a, b).await;
        let remote = sum_over_nats(nats.clone(), a, b).await;
        assert_eq!(local, remote, "{a} + {b}");
    }

    server.abort();
}

#[tokio::test]
async fn a_host_without_a_proxy_refuses_a_service() {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());

    let registry = OneService {
        interface: adder(),
        digest: expected_digest(&engine),
    };
    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = Resolver::new(engine.clone(), registry, NoArtifacts)
        .plan(&[], consumer)
        .await
        .expect("the resolver still picks the service");

    let error = host
        .instantiate(plan)
        .await
        .expect_err("but nothing can call it");
    assert!(matches!(error, LinkError::NoRemoteTransport(_)), "{error}");
}
