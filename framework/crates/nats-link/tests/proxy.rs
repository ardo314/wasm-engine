//! Integration tests against the NATS service in `docker-compose.yml`.
//!
//! Skipped when nothing answers on `$NATS_URL`, so `cargo test --workspace`
//! still passes without `docker compose up -d nats`.

use std::time::Duration;

use async_nats::Client;
use futures::StreamExt;
use wasm_nats_link::Proxy;
use wasm_protocol::{
    ErrorCode, FunctionShape, InterfaceId, InterfaceShape, Reply, Request, Subject, WireError,
    WitType,
};
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store};

const ADDER: &str = "test:fixture/adder@1.0.0";
const CALLER: &str = "test:fixture/caller@1.0.0";

/// Imports `adder`, exports `sum`, and does nothing but forward.
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

/// The same arithmetic, in-process, to compare the wire path against.
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

fn shape() -> InterfaceShape {
    InterfaceShape::new(vec![FunctionShape::new(
        "add",
        vec![WitType::U32, WitType::U32],
        vec![WitType::U32],
    )])
}

fn engine() -> Engine {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).expect("engine")
}

/// `None` when no NATS is reachable, which the caller treats as "skip".
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

/// Subjects are cluster-wide, so each test gets its own interface version and
/// therefore its own subject.
fn unique_interface(tag: &str) -> InterfaceId {
    format!("test:fixture/adder@1.0.{}", tag).parse().unwrap()
}

/// Serves `add` on `interface` until the returned handle is dropped.
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

/// Instantiates `CONSUMER` with its import satisfied over NATS.
async fn consumer_over_nats(
    engine: &Engine,
    proxy: &Proxy,
    interface: &InterfaceId,
) -> (Store<()>, wasmtime::component::Instance) {
    let mut linker = Linker::<()>::new(engine);
    proxy
        .define(&mut linker, interface, &shape())
        .expect("define");

    let mut store = Store::new(engine, ());
    let component = Component::new(engine, CONSUMER).expect("consumer compiles");
    let instance = linker
        .instantiate_async(&mut store, &component)
        .await
        .expect("instantiates");
    (store, instance)
}

async fn call_sum(
    store: &mut Store<()>,
    instance: wasmtime::component::Instance,
    a: u32,
    b: u32,
) -> wasmtime::Result<Val> {
    let caller: InterfaceId = CALLER.parse().unwrap();
    let owner = instance
        .get_export_index(&mut *store, None, &caller.to_string())
        .expect("caller instance");
    let index = instance
        .get_export_index(&mut *store, Some(&owner), "sum")
        .expect("sum export");
    let func = instance.get_func(&mut *store, index).expect("sum func");

    let mut results = vec![Val::U32(0)];
    func.call_async(&mut *store, &[Val::U32(a), Val::U32(b)], &mut results)
        .await?;
    Ok(results.remove(0))
}

#[tokio::test]
async fn a_guest_call_reaches_a_service_and_comes_back() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("1");
    let server = serve_adder(nats.clone(), interface.clone());

    let engine = engine();
    let proxy = Proxy::new(nats);
    let (mut store, instance) = consumer_over_nats(&engine, &proxy, &interface).await;

    let sum = call_sum(&mut store, instance, 3, 4).await.expect("sum");
    assert_eq!(sum, Val::U32(7));

    server.abort();
}

#[tokio::test]
async fn the_wire_path_agrees_with_calling_a_component_directly() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("2");
    let server = serve_adder(nats.clone(), interface.clone());

    let engine = engine();
    let proxy = Proxy::new(nats);
    let (mut remote_store, remote) = consumer_over_nats(&engine, &proxy, &interface).await;

    // The same arithmetic, in-process.
    let mut local_store = Store::new(&engine, ());
    let provider = Component::new(&engine, PROVIDER).expect("provider compiles");
    let local = Linker::<()>::new(&engine)
        .instantiate_async(&mut local_store, &provider)
        .await
        .expect("provider instantiates");
    let owner = local
        .get_export_index(&mut local_store, None, ADDER)
        .expect("adder instance");
    let index = local
        .get_export_index(&mut local_store, Some(&owner), "add")
        .expect("add export");
    let add = local.get_func(&mut local_store, index).expect("add func");

    for (a, b) in [(0, 0), (1, 2), (40, 2), (1_000, 337)] {
        let mut direct = vec![Val::U32(0)];
        add.call_async(&mut local_store, &[Val::U32(a), Val::U32(b)], &mut direct)
            .await
            .expect("direct");

        let over_nats = call_sum(&mut remote_store, remote, a, b)
            .await
            .expect("over nats");

        assert_eq!(direct[0], over_nats, "{a} + {b}");
    }

    server.abort();
}

#[tokio::test]
async fn a_service_that_never_answers_traps_rather_than_hanging() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("3");

    // Subscribed, so the call is not no-responders — just silent.
    let subject = Subject::new(interface.clone(), "add").to_string();
    let mut silent = nats.subscribe(subject).await.expect("subscribe");
    let server = tokio::spawn(async move { while silent.next().await.is_some() {} });

    let engine = engine();
    let proxy = Proxy::with_deadline(nats, Duration::from_millis(300));
    let (mut store, instance) = consumer_over_nats(&engine, &proxy, &interface).await;

    let started = std::time::Instant::now();
    let error = call_sum(&mut store, instance, 1, 1)
        .await
        .expect_err("nothing answers");

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the deadline should end the call, not a hang"
    );
    let message = format!("{error:#}");
    assert!(
        message.contains(ErrorCode::DeadlineExceeded.as_str()),
        "{message}"
    );

    server.abort();
}

#[tokio::test]
async fn an_error_reply_becomes_a_trap_carrying_the_code() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("4");

    let subject = Subject::new(interface.clone(), "add").to_string();
    let serving = nats.clone();
    let mut requests = nats.subscribe(subject).await.expect("subscribe");
    let server = tokio::spawn(async move {
        while let Some(message) = requests.next().await {
            let request = Request::decode(&message.payload).expect("a request");
            let reply = Reply::err(
                &request.id,
                WireError::new(ErrorCode::NotFound, "the tenant went away"),
            )
            .encode()
            .expect("encodable");
            if let Some(to) = message.reply {
                serving.publish(to, reply.into()).await.expect("publish");
            }
        }
    });

    let engine = engine();
    let proxy = Proxy::new(nats);
    let (mut store, instance) = consumer_over_nats(&engine, &proxy, &interface).await;

    let error = call_sum(&mut store, instance, 1, 1)
        .await
        .expect_err("the service refused");
    let message = format!("{error:#}");
    assert!(message.contains(ErrorCode::NotFound.as_str()), "{message}");
    assert!(message.contains("the tenant went away"), "{message}");

    server.abort();
}

#[tokio::test]
async fn a_provider_that_subscribes_late_is_caught_by_the_retry() {
    let Some(nats) = connect().await else { return };
    let interface = unique_interface("5");

    // Nobody is listening yet, so the first attempt is no-responders.
    let late = nats.clone();
    let late_interface = interface.clone();
    let server = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        serve_adder(late, late_interface).await.expect("served");
    });

    let engine = engine();
    let proxy = Proxy::new(nats);
    let (mut store, instance) = consumer_over_nats(&engine, &proxy, &interface).await;

    let sum = call_sum(&mut store, instance, 2, 5)
        .await
        .expect("the retry finds the provider");
    assert_eq!(sum, Val::U32(7));

    server.abort();
}
