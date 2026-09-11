//! Integration tests against the NATS service in `docker-compose.yml`.
//!
//! They are skipped when nothing answers on `$NATS_URL`, so `cargo test
//! --workspace` still passes on a machine that has not run
//! `docker compose up -d nats`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_nats::Client;
use async_nats::jetstream::Context;
use futures::StreamExt;
use rmpv::Value;
use tokio::sync::{Mutex, MutexGuard};
use tokio::task::JoinHandle;
use wasm_protocol::{ErrorCode, InterfaceId, Reply, Request, Subject, WireError};
use wasm_registry::{
    DISCOVERY_INTERFACE, Endpoint, InterfaceRef, Limits, Provider, ProviderKind,
    REGISTRATION_INTERFACE, Registry, RegistryError, serve,
};

const MATH: &str = "ardo314:math/vector3d";
const DIGEST_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const DIGEST_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// Registry subjects are cluster-wide, so two harnesses must not overlap.
static SERIAL: Mutex<()> = Mutex::const_new(());

struct Harness {
    client: Client,
    jetstream: Context,
    bucket: String,
    server: JoinHandle<anyhow::Result<()>>,
    calls: AtomicU64,
    _serial: MutexGuard<'static, ()>,
}

impl Harness {
    /// `None` when no NATS is reachable, which the caller treats as "skip".
    async fn start(limits: Limits) -> Option<Self> {
        let serial = SERIAL.lock().await;
        let url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_owned());
        let client = match async_nats::ConnectOptions::new()
            .connection_timeout(Duration::from_secs(2))
            .connect(&url)
            .await
        {
            Ok(client) => client,
            Err(e) => {
                eprintln!("skipping: no NATS at {url} ({e}); run `docker compose up -d nats`");
                return None;
            }
        };

        let bucket = format!(
            "wit-registry-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let jetstream = async_nats::jetstream::new(client.clone());
        let registry = Registry::open(&jetstream, &bucket, limits).await.unwrap();
        let server = tokio::spawn(serve(client.clone(), registry));
        client.flush().await.unwrap();

        Some(Self {
            client,
            jetstream,
            bucket,
            server,
            calls: AtomicU64::new(0),
            _serial: serial,
        })
    }

    async fn stop(self) {
        self.jetstream.delete_key_value(&self.bucket).await.unwrap();
        // Unsubscribes and flushes, which ends `serve` before the next harness
        // subscribes to the same subjects.
        self.client.drain().await.unwrap();
        let _ = self.server.await;
    }

    async fn call(
        &self,
        iface: &str,
        func: &str,
        args: Vec<Value>,
    ) -> Result<Vec<Value>, WireError> {
        let subject = Subject::new(iface.parse::<InterfaceId>().unwrap(), func);
        let id = format!("{func}-{}", self.calls.fetch_add(1, Ordering::Relaxed));
        let payload = Request::new(id, iface, func, args).encode().unwrap();

        let message = tokio::time::timeout(
            Duration::from_secs(5),
            self.client.request(subject.to_string(), payload.into()),
        )
        .await
        .expect("the registry did not answer")
        .expect("request failed");

        Reply::decode(&message.payload).unwrap().into_result()
    }

    async fn register(&self, provider: &Provider) -> Result<(), RegistryError> {
        let result = self
            .call(
                REGISTRATION_INTERFACE,
                "register",
                vec![provider.to_value()],
            )
            .await;
        wit_result(result).map(drop)
    }

    async fn deregister(&self, id: &str) -> Result<(), RegistryError> {
        let result = self
            .call(REGISTRATION_INTERFACE, "deregister", vec![Value::from(id)])
            .await;
        wit_result(result).map(drop)
    }

    async fn heartbeat(&self, id: &str) -> Result<(), RegistryError> {
        let result = self
            .call(REGISTRATION_INTERFACE, "heartbeat", vec![Value::from(id)])
            .await;
        wit_result(result).map(drop)
    }

    async fn resolve(&self, name: &str, version_req: &str) -> Result<Vec<Provider>, RegistryError> {
        let result = self
            .call(
                DISCOVERY_INTERFACE,
                "resolve",
                vec![Value::from(name), Value::from(version_req)],
            )
            .await;
        wit_result(result).map(|value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|provider| Provider::from_value(provider).unwrap())
                .collect()
        })
    }

    async fn list_interfaces(&self) -> Result<Vec<InterfaceRef>, RegistryError> {
        let result = self
            .call(DISCOVERY_INTERFACE, "list-interfaces", vec![])
            .await;
        wit_result(result).map(|value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|interface| InterfaceRef::from_value(interface).unwrap())
                .collect()
        })
    }
}

/// Peels the WIT `result<T, registry-error>` out of the reply's single value.
fn wit_result(reply: Result<Vec<Value>, WireError>) -> Result<Value, RegistryError> {
    let values = reply.expect("the call itself failed");
    let [value] = values.as_slice() else {
        panic!("expected exactly one result, found {values:?}");
    };
    match value.as_map().map(Vec::as_slice) {
        Some([(case, payload)]) if case.as_str() == Some("ok") => Ok(payload.clone()),
        Some([(case, payload)]) if case.as_str() == Some("err") => {
            Err(RegistryError::from_value(payload).unwrap())
        }
        _ => panic!("{value:?} is not a result"),
    }
}

fn service(id: &str, digest: &str) -> Provider {
    Provider {
        id: id.to_owned(),
        kind: ProviderKind::Service,
        interfaces: vec![InterfaceRef {
            name: MATH.to_owned(),
            version: "0.0.3".to_owned(),
            shape_digest: digest.to_owned(),
        }],
        endpoint: Endpoint::Nats(format!("wit.ardo314.math.0_0_3.vector3d.*#{id}")),
        ttl_secs: 30,
    }
}

fn limits(min_ttl_secs: u64) -> Limits {
    Limits {
        min_ttl: Duration::from_secs(min_ttl_secs),
        max_ttl: Duration::from_secs(60),
    }
}

#[tokio::test]
async fn registers_resolves_heartbeats_and_expires() {
    let Some(harness) = Harness::start(limits(1)).await else {
        return;
    };

    let provider = Provider {
        ttl_secs: 1,
        ..service("math-1", DIGEST_A)
    };
    harness.register(&provider).await.unwrap();

    let resolved = harness.resolve(MATH, "^0.0.3").await.unwrap();
    assert_eq!(resolved, vec![provider.clone()]);

    harness.heartbeat("math-1").await.unwrap();
    assert_eq!(harness.resolve(MATH, "^0.0.3").await.unwrap().len(), 1);

    // Two missed heartbeats, and the entry stops counting without anyone
    // deregistering it.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(harness.resolve(MATH, "^0.0.3").await.unwrap(), vec![]);
    assert_eq!(
        harness.heartbeat("math-1").await,
        Err(RegistryError::NotFound)
    );

    harness.stop().await;
}

#[tokio::test]
async fn deregisters_immediately() {
    let Some(harness) = Harness::start(limits(5)).await else {
        return;
    };

    harness
        .register(&service("math-1", DIGEST_A))
        .await
        .unwrap();
    harness.deregister("math-1").await.unwrap();

    assert_eq!(harness.resolve(MATH, "^0.0.3").await.unwrap(), vec![]);
    assert_eq!(
        harness.deregister("math-1").await,
        Err(RegistryError::NotFound)
    );

    harness.stop().await;
}

#[tokio::test]
async fn rejects_a_conflicting_shape_digest() {
    let Some(harness) = Harness::start(limits(5)).await else {
        return;
    };

    harness
        .register(&service("math-1", DIGEST_A))
        .await
        .unwrap();

    let conflict = harness.register(&service("math-2", DIGEST_B)).await;
    assert!(
        matches!(conflict, Err(RegistryError::Conflict(_))),
        "expected a conflict, got {conflict:?}"
    );

    // Agreeing on the shape is all it takes to join.
    harness
        .register(&service("math-2", DIGEST_A))
        .await
        .unwrap();
    assert_eq!(harness.resolve(MATH, "^0.0.3").await.unwrap().len(), 2);

    harness.stop().await;
}

#[tokio::test]
async fn replaces_the_entry_when_a_provider_re_registers() {
    let Some(harness) = Harness::start(limits(5)).await else {
        return;
    };

    harness
        .register(&service("math-1", DIGEST_A))
        .await
        .unwrap();
    // A restart may bring a new shape with it; the provider's own entry is not
    // a conflict with itself.
    harness
        .register(&service("math-1", DIGEST_B))
        .await
        .unwrap();

    let resolved = harness.resolve(MATH, "^0.0.3").await.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].interfaces[0].shape_digest, DIGEST_B);

    harness.stop().await;
}

#[tokio::test]
async fn load_balances_two_providers_by_queue_group() {
    let Some(harness) = Harness::start(limits(5)).await else {
        return;
    };

    harness
        .register(&service("math-1", DIGEST_A))
        .await
        .unwrap();
    harness
        .register(&service("math-2", DIGEST_A))
        .await
        .unwrap();

    let resolved = harness.resolve(MATH, "^0.0.3").await.unwrap();
    assert_eq!(resolved.len(), 2, "both providers must resolve");

    // Subjects encode an exact version, which resolution is what hands back.
    let interface: InterfaceId = format!(
        "{}@{}",
        resolved[0].interfaces[0].name, resolved[0].interfaces[0].version
    )
    .parse()
    .unwrap();
    let queue_group = Subject::queue_group(&interface);
    assert_eq!(queue_group, "ardo314:math/vector3d@0.0.3");

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<usize>();
    for replica in 0..2 {
        let mut calls = harness
            .client
            .queue_subscribe(Subject::interface_wildcard(&interface), queue_group.clone())
            .await
            .unwrap();
        let sender = sender.clone();
        tokio::spawn(async move {
            while calls.next().await.is_some() && sender.send(replica).is_ok() {}
        });
    }
    drop(sender);
    harness.client.flush().await.unwrap();

    let subject = Subject::new(interface, "add").to_string();
    const CALLS: usize = 50;
    for _ in 0..CALLS {
        harness
            .client
            .publish(subject.clone(), Vec::new().into())
            .await
            .unwrap();
    }
    harness.client.flush().await.unwrap();

    let mut tally = [0usize; 2];
    for _ in 0..CALLS {
        let replica = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("a call went unanswered")
            .unwrap();
        tally[replica] += 1;
    }
    assert!(
        tally[0] > 0 && tally[1] > 0,
        "the queue group sent every call to one replica: {tally:?}"
    );

    harness.stop().await;
}

#[tokio::test]
async fn lists_every_live_interface_once() {
    let Some(harness) = Harness::start(limits(5)).await else {
        return;
    };

    harness
        .register(&service("math-1", DIGEST_A))
        .await
        .unwrap();
    harness
        .register(&service("math-2", DIGEST_A))
        .await
        .unwrap();

    let interfaces = harness.list_interfaces().await.unwrap();
    assert_eq!(
        interfaces,
        vec![InterfaceRef {
            name: MATH.to_owned(),
            version: "0.0.3".to_owned(),
            shape_digest: DIGEST_A.to_owned(),
        }]
    );

    harness.stop().await;
}

#[tokio::test]
async fn reports_malformed_input_as_invalid() {
    let Some(harness) = Harness::start(limits(5)).await else {
        return;
    };

    assert!(matches!(
        harness.resolve(MATH, "not a requirement").await,
        Err(RegistryError::Invalid(_))
    ));

    let unaddressable = Provider {
        interfaces: vec![InterfaceRef {
            name: "vector3d".to_owned(),
            version: "0.0.3".to_owned(),
            shape_digest: DIGEST_A.to_owned(),
        }],
        ..service("math-1", DIGEST_A)
    };
    assert!(matches!(
        harness.register(&unaddressable).await,
        Err(RegistryError::Invalid(_))
    ));

    harness.stop().await;
}

#[tokio::test]
async fn an_unserved_function_is_a_transport_error() {
    let Some(harness) = Harness::start(limits(5)).await else {
        return;
    };

    let error = harness
        .call(REGISTRATION_INTERFACE, "renew", vec![])
        .await
        .expect_err("the registry has no `renew`");
    assert_eq!(error.code, ErrorCode::NotFound);

    harness.stop().await;
}
