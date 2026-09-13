//! Linking is exercised against hand-written component WAT, because a dummy
//! module from WIT traps rather than computing anything, and the numbers are
//! the point here.

mod support;

use support::{ADDER, CALLER, CONSUMER, PROVIDER};
use wasm_host::{Fetch, FetchError, Host, LinkError, Missing, Resolver, engine};
use wasm_protocol::InterfaceId;
use wasmtime::component::{Component, Val};

/// Exports `adder` too, but traps instead of answering.
const TRAPPING_PROVIDER: &str = r#"
(component
  (core module $m
    (func (export "add") (param i32 i32) (result i32)
      unreachable))
  (core instance $i (instantiate $m))
  (func $add (param "a" u32) (param "b" u32) (result u32)
    (canon lift (core func $i "add")))
  (instance $adder (export "add" (func $add)))
  (export "test:fixture/adder@1.0.0" (instance $adder))
)
"#;

/// The resolver is not the subject here; these tests hand it a registry that
/// knows nothing and rely on the live-component step.
#[derive(Default)]
struct NoRegistry;

impl wasm_host::Discovery for NoRegistry {
    async fn resolve(
        &self,
        _name: &str,
        _version_req: &str,
    ) -> Result<Vec<wasm_registry::Resolution>, wasm_registry::RegistryError> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct NoArtifacts;

impl Fetch for NoArtifacts {
    async fn fetch(&self, _artifact: &wasm_registry::ArtifactRef) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::new("no artifacts in this test"))
    }
}

fn adder() -> InterfaceId {
    ADDER.parse().unwrap()
}

fn caller() -> InterfaceId {
    CALLER.parse().unwrap()
}

/// Loads `provider`, then loads `CONSUMER` against it, returning the host and
/// the consumer's instance.
async fn host_with(provider: &str) -> (Host, wasm_host::Running) {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());
    let resolver = Resolver::new(engine.clone(), NoRegistry, NoArtifacts);

    let provider = Component::new(&engine, provider).expect("provider compiles");
    let plan = resolver.plan(&[], provider).await.expect("provider plan");
    host.instantiate(plan).await.expect("provider instantiates");

    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = resolver
        .plan(&host.live(), consumer)
        .await
        .expect("consumer plan");
    let instance = host.instantiate(plan).await.expect("consumer instantiates");

    (host, instance)
}

#[tokio::test]
async fn a_forwarded_call_returns_what_the_provider_computed() {
    let (host, consumer) = host_with(PROVIDER).await;

    let mut results = vec![Val::U32(0)];
    host.call(
        consumer,
        &caller(),
        "sum",
        &[Val::U32(3), Val::U32(4)],
        &mut results,
    )
    .await
    .expect("sum");

    assert_eq!(results, vec![Val::U32(7)]);
}

#[tokio::test]
async fn forwarding_agrees_with_calling_the_provider_directly() {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());
    let resolver = Resolver::new(engine.clone(), NoRegistry, NoArtifacts);

    let provider = Component::new(&engine, PROVIDER).expect("provider compiles");
    let plan = resolver.plan(&[], provider).await.expect("provider plan");
    let provider_instance = host.instantiate(plan).await.expect("provider instantiates");

    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = resolver
        .plan(&host.live(), consumer)
        .await
        .expect("consumer plan");
    let consumer_instance = host.instantiate(plan).await.expect("consumer instantiates");

    for (a, b) in [(0, 0), (1, 2), (40, 2), (u32::MAX - 1, 1)] {
        let mut direct = vec![Val::U32(0)];
        host.call(
            provider_instance,
            &adder(),
            "add",
            &[Val::U32(a), Val::U32(b)],
            &mut direct,
        )
        .await
        .expect("direct");

        let mut forwarded = vec![Val::U32(0)];
        host.call(
            consumer_instance,
            &caller(),
            "sum",
            &[Val::U32(a), Val::U32(b)],
            &mut forwarded,
        )
        .await
        .expect("forwarded");

        assert_eq!(direct, forwarded, "{a} + {b}");
    }
}

#[tokio::test]
async fn a_trap_in_the_callee_surfaces_in_the_caller() {
    let (host, consumer) = host_with(TRAPPING_PROVIDER).await;

    let mut results = vec![Val::U32(0)];
    let error = host
        .call(
            consumer,
            &caller(),
            "sum",
            &[Val::U32(3), Val::U32(4)],
            &mut results,
        )
        .await
        .expect_err("the provider traps");
    assert!(
        error.to_string().contains("unreachable"),
        "the caller should see the callee's own trap, got: {error}"
    );
}

/// `docs/spec/linking.md` §3. The poisoning itself is wasmtime's behaviour;
/// what this asserts is that it stops at the isolation group.
#[tokio::test]
async fn a_trap_is_contained_to_its_own_store() {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());
    let resolver = Resolver::new(engine.clone(), NoRegistry, NoArtifacts);

    let good = Component::new(&engine, PROVIDER).expect("provider compiles");
    let plan = resolver.plan(&[], good).await.expect("plan");
    let good = host.instantiate(plan).await.expect("instantiates");

    let trapping = Component::new(&engine, TRAPPING_PROVIDER).expect("compiles");
    let plan = resolver.plan(&[], trapping).await.expect("plan");
    let trapping = host.instantiate(plan).await.expect("instantiates");

    assert_eq!(host.stores(), 2, "nothing forces these two to share");

    let mut results = vec![Val::U32(0)];
    host.call(
        trapping,
        &adder(),
        "add",
        &[Val::U32(1), Val::U32(1)],
        &mut results,
    )
    .await
    .expect_err("traps");

    let mut results = vec![Val::U32(0)];
    host.call(
        good,
        &adder(),
        "add",
        &[Val::U32(20), Val::U32(22)],
        &mut results,
    )
    .await
    .expect("an unrelated component is untouched by someone else's trap");
    assert_eq!(results, vec![Val::U32(42)]);
}

/// The other half of §3: within a group there is no containment.
#[tokio::test]
async fn a_trap_poisons_the_rest_of_its_own_group() {
    let (host, consumer) = host_with(TRAPPING_PROVIDER).await;

    let mut results = vec![Val::U32(0)];
    host.call(
        consumer,
        &caller(),
        "sum",
        &[Val::U32(3), Val::U32(4)],
        &mut results,
    )
    .await
    .expect_err("the provider traps");

    let mut results = vec![Val::U32(0)];
    host.call(
        consumer,
        &caller(),
        "sum",
        &[Val::U32(3), Val::U32(4)],
        &mut results,
    )
    .await
    .expect_err("and the caller's own store is poisoned by the trap it took");
}

/// `docs/spec/linking.md` §1 groups these components into one store, which is
/// the right rule — but forwarding a guest-owned resource is not implemented,
/// so the link fails after the grouping succeeds. See #30.
#[tokio::test]
async fn a_resource_carrying_interface_cannot_be_linked_yet() {
    const WIT: &str = "
        package test:res@1.0.0;

        interface store {
            resource handle { constructor(); }
            put: func(slot: borrow<handle>);
        }

        world provider { export store; }
        world consumer { import store; }
    ";

    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());
    let resolver = Resolver::new(engine.clone(), NoRegistry, NoArtifacts);

    let provider = support::component_of(&engine, WIT, "provider");
    let plan = resolver.plan(&[], provider).await.expect("provider plan");
    host.instantiate(plan).await.expect("provider instantiates");
    assert_eq!(host.stores(), 1);

    let consumer = support::component_of(&engine, WIT, "consumer");
    let plan = resolver
        .plan(&host.live(), consumer)
        .await
        .expect("the resolver is happy: a live component exports it");

    let error = host
        .instantiate(plan)
        .await
        .expect_err("but the resource type is never defined on the linker");
    assert!(
        error
            .to_string()
            .contains("resource implementation is missing"),
        "{error}"
    );
    assert_eq!(
        host.stores(),
        1,
        "grouping still put them together, which is what #30 will need"
    );
}

/// The converse: plain data crosses a store boundary, so nothing is shared.
#[tokio::test]
async fn a_plain_data_interface_leaves_components_separated() {
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
    let consumer = host.instantiate(plan).await.expect("consumer instantiates");

    assert_eq!(host.stores(), 2, "each gets its own");

    let mut results = vec![Val::U32(0)];
    host.call(
        consumer,
        &caller(),
        "sum",
        &[Val::U32(20), Val::U32(22)],
        &mut results,
    )
    .await
    .expect("and the call still crosses between them");
    assert_eq!(results, vec![Val::U32(42)]);
}

#[tokio::test]
async fn an_unsatisfied_import_traps_only_when_called() {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());

    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = Resolver::new(engine.clone(), NoRegistry, NoArtifacts)
        .on_missing(Missing::Trap)
        .plan(&[], consumer)
        .await
        .expect("permissive plan");

    let instance = host
        .instantiate(plan)
        .await
        .expect("a component with no provider still loads");

    let mut results = vec![Val::U32(0)];
    host.call(
        instance,
        &caller(),
        "sum",
        &[Val::U32(1), Val::U32(1)],
        &mut results,
    )
    .await
    .expect_err("the stub traps");
}

#[tokio::test]
async fn a_service_binding_is_refused_until_the_nats_proxy_exists() {
    let engine = engine().expect("engine");
    let mut host = Host::new(engine.clone());

    let digest = {
        let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
        wasm_host::ComponentScan::new(&engine, &consumer)
            .expect("scan")
            .import(&adder())
            .expect("adder is imported")
            .shape()
            .expect("a wire shape")
            .digest()
    };

    struct OnlyAService(String);
    impl wasm_host::Discovery for OnlyAService {
        async fn resolve(
            &self,
            _name: &str,
            _version_req: &str,
        ) -> Result<Vec<wasm_registry::Resolution>, wasm_registry::RegistryError> {
            Ok(vec![wasm_registry::Resolution {
                provider: wasm_registry::Provider {
                    id: "remote".to_owned(),
                    kind: wasm_registry::ProviderKind::Service,
                    interfaces: vec![],
                    endpoint: wasm_registry::Endpoint::Nats("wit.test".to_owned()),
                    ttl_secs: 30,
                },
                interface: ADDER.parse().unwrap(),
                shape_digest: self.0.clone(),
            }])
        }
    }

    let consumer = Component::new(&engine, CONSUMER).expect("consumer compiles");
    let plan = Resolver::new(engine.clone(), OnlyAService(digest), NoArtifacts)
        .plan(&[], consumer)
        .await
        .expect("the resolver is happy to pick a service");

    let error = host
        .instantiate(plan)
        .await
        .expect_err("but the host cannot call one yet");
    assert!(matches!(error, LinkError::NoRemoteTransport(_)), "{error}");
}
