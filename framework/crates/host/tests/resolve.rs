//! The preference order, end to end, against a registry that is a `Vec`.

mod support;

use std::collections::HashMap;
use std::future::Future;

use sha2::{Digest, Sha256};
use wasm_host::{
    ComponentKey, ComponentScan, Discovery, Fetch, FetchError, Missing, ResolveError, Resolver,
    Source, engine,
};
use wasm_registry::{
    ArtifactRef, Endpoint, InterfaceRef, Provider, ProviderKind, RegistryError, Resolution,
};
use wasmtime::Engine;
use wasmtime::component::Component;

const STORE: &str = "test:fixture/store@1.0.0";

/// A consumer of `store`, and a component that implements it.
const WIT: &str = "
    package test:fixture@1.0.0;

    interface store {
        put: func(key: string, value: list<u8>) -> result<_, string>;
    }

    interface other { noop: func(); }

    world consumer { import store; }
    world provider { export store; }
    world unrelated { export other; }
";

/// `store`, but with a signature no wire protocol can carry.
const RESOURCE_WIT: &str = "
    package test:fixture@1.0.0;

    interface store {
        resource handle { constructor(); }
        put: func(slot: borrow<handle>) -> result<_, string>;
    }

    world consumer { import store; }
";

#[derive(Default)]
struct FakeRegistry(Vec<Resolution>);

impl Discovery for FakeRegistry {
    fn resolve(
        &self,
        name: &str,
        _version_req: &str,
    ) -> impl Future<Output = Result<Vec<Resolution>, RegistryError>> + Send {
        let matched: Vec<Resolution> = self
            .0
            .iter()
            .filter(|resolution| {
                let id = &resolution.interface;
                format!("{}:{}/{}", id.namespace(), id.package(), id.interface()) == name
            })
            .cloned()
            .collect();
        async move { Ok(matched) }
    }
}

#[derive(Default)]
struct FakeArtifacts(HashMap<String, Vec<u8>>);

impl Fetch for FakeArtifacts {
    fn fetch(
        &self,
        artifact: &ArtifactRef,
    ) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send {
        let found = self.0.get(&artifact.uri).cloned();
        let uri = artifact.uri.clone();
        async move { found.ok_or_else(|| FetchError::new(format!("no artifact at `{uri}`"))) }
    }
}

fn provider(id: &str, kind: ProviderKind, digest: &str, endpoint: Endpoint) -> Resolution {
    Resolution {
        provider: Provider {
            id: id.to_owned(),
            kind,
            interfaces: vec![InterfaceRef {
                name: "test:fixture/store".to_owned(),
                version: "1.0.0".to_owned(),
                shape_digest: digest.to_owned(),
            }],
            endpoint,
            ttl_secs: 30,
        },
        interface: STORE.parse().unwrap(),
        shape_digest: digest.to_owned(),
    }
}

fn artifact(uri: &str, bytes: &[u8]) -> ArtifactRef {
    ArtifactRef {
        uri: uri.to_owned(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}

/// The digest the consumer derives for `store`, which is what a provider has to
/// agree with.
fn expected_digest(engine: &Engine, consumer: &Component) -> String {
    ComponentScan::new(engine, consumer)
        .expect("scan")
        .import(&STORE.parse().unwrap())
        .expect("store is imported")
        .shape()
        .expect("a wire shape")
        .digest()
}

#[tokio::test]
async fn a_live_component_beats_a_registered_service() {
    let engine = engine().expect("engine");
    let consumer = support::component_of(&engine, WIT, "consumer");
    let live = support::component_of(&engine, WIT, "provider");
    let digest = expected_digest(&engine, &consumer);

    let registry = FakeRegistry(vec![provider(
        "remote",
        ProviderKind::Service,
        &digest,
        Endpoint::Nats("wit.test.fixture.1_0_0.store".to_owned()),
    )]);
    let resolver = Resolver::new(engine, registry, FakeArtifacts::default());

    let plan = resolver
        .plan(std::slice::from_ref(&live), consumer)
        .await
        .expect("plan");

    assert_eq!(
        plan.nodes().len(),
        1,
        "the live component is not re-planned"
    );
    assert!(matches!(
        plan.root().imports[0].source,
        Source::InProcess(ComponentKey::Live(0))
    ));
}

#[tokio::test]
async fn a_component_provider_is_fetched_verified_and_instantiated_first() {
    let engine = engine().expect("engine");
    let consumer = support::component_of(&engine, WIT, "consumer");
    let digest = expected_digest(&engine, &consumer);

    let bytes = support::bytes_of(WIT, "provider");
    let artifact = artifact("oci://example/store:1", &bytes);
    let registry = FakeRegistry(vec![provider(
        "loadable",
        ProviderKind::Component,
        &digest,
        Endpoint::Artifact(artifact.clone()),
    )]);
    let artifacts = FakeArtifacts(HashMap::from([(artifact.uri.clone(), bytes)]));

    let plan = Resolver::new(engine, registry, artifacts)
        .plan(&[], consumer)
        .await
        .expect("plan");

    assert_eq!(plan.nodes().len(), 2);
    assert_eq!(plan.root_index(), 1, "dependencies are instantiated first");
    assert!(matches!(
        plan.root().imports[0].source,
        Source::InProcess(ComponentKey::Planned(0))
    ));
    assert_eq!(plan.nodes()[0].exports[0].to_string(), STORE);
}

#[tokio::test]
async fn a_corrupted_artifact_is_refused() {
    let engine = engine().expect("engine");
    let consumer = support::component_of(&engine, WIT, "consumer");
    let digest = expected_digest(&engine, &consumer);

    let bytes = support::bytes_of(WIT, "provider");
    let mut claimed = artifact("oci://example/store:1", &bytes);
    claimed.sha256 = "0".repeat(64);
    let registry = FakeRegistry(vec![provider(
        "loadable",
        ProviderKind::Component,
        &digest,
        Endpoint::Artifact(claimed.clone()),
    )]);
    let artifacts = FakeArtifacts(HashMap::from([(claimed.uri.clone(), bytes)]));

    let error = Resolver::new(engine, registry, artifacts)
        .plan(&[], consumer)
        .await
        .expect_err("the digest does not match");

    assert!(
        matches!(error, ResolveError::ArtifactMismatch { .. }),
        "{error}"
    );
}

#[tokio::test]
async fn an_artifact_that_does_not_export_what_was_claimed_is_refused() {
    let engine = engine().expect("engine");
    let consumer = support::component_of(&engine, WIT, "consumer");
    let digest = expected_digest(&engine, &consumer);

    let bytes = support::bytes_of(WIT, "unrelated");
    let artifact = artifact("oci://example/liar:1", &bytes);
    let registry = FakeRegistry(vec![provider(
        "liar",
        ProviderKind::Component,
        &digest,
        Endpoint::Artifact(artifact.clone()),
    )]);
    let artifacts = FakeArtifacts(HashMap::from([(artifact.uri.clone(), bytes)]));

    let error = Resolver::new(engine, registry, artifacts)
        .plan(&[], consumer)
        .await
        .expect_err("the artifact does not implement store");

    assert!(matches!(error, ResolveError::NotExported { .. }), "{error}");
}

#[tokio::test]
async fn only_a_service_provider_means_the_wire() {
    let engine = engine().expect("engine");
    let consumer = support::component_of(&engine, WIT, "consumer");
    let digest = expected_digest(&engine, &consumer);

    let registry = FakeRegistry(vec![provider(
        "remote",
        ProviderKind::Service,
        &digest,
        Endpoint::Nats("wit.test.fixture.1_0_0.store".to_owned()),
    )]);

    let plan = Resolver::new(engine, registry, FakeArtifacts::default())
        .plan(&[], consumer)
        .await
        .expect("plan");

    let Source::Nats(service) = &plan.root().imports[0].source else {
        panic!(
            "expected a service, got {:?}",
            plan.root().imports[0].source
        );
    };
    assert_eq!(service.provider, "remote");
    assert_eq!(service.interface.to_string(), STORE);
}

#[tokio::test]
async fn a_provider_whose_shape_disagrees_is_rejected() {
    let engine = engine().expect("engine");
    let consumer = support::component_of(&engine, WIT, "consumer");

    let registry = FakeRegistry(vec![provider(
        "stale",
        ProviderKind::Service,
        &"a".repeat(64),
        Endpoint::Nats("wit.test.fixture.1_0_0.store".to_owned()),
    )]);

    let error = Resolver::new(engine, registry, FakeArtifacts::default())
        .plan(&[], consumer)
        .await
        .expect_err("the shapes disagree");

    let ResolveError::ShapeMismatch(mismatch) = &error else {
        panic!("expected a shape mismatch, got {error}");
    };
    assert_eq!(mismatch.provider, "stale");
    assert_eq!(mismatch.found, "a".repeat(64));
}

#[tokio::test]
async fn an_in_process_only_interface_refuses_a_service() {
    let engine = engine().expect("engine");
    let consumer = support::component_of(&engine, RESOURCE_WIT, "consumer");

    let registry = FakeRegistry(vec![provider(
        "remote",
        ProviderKind::Service,
        "",
        Endpoint::Nats("wit.test.fixture.1_0_0.store".to_owned()),
    )]);

    let error = Resolver::new(engine, registry, FakeArtifacts::default())
        .plan(&[], consumer)
        .await
        .expect_err("a resource cannot cross a process boundary");

    assert!(matches!(error, ResolveError::InProcessOnly(_)), "{error}");
}

#[tokio::test]
async fn nothing_is_an_error_unless_traps_are_allowed() {
    let engine = engine().expect("engine");
    let consumer = || support::component_of(&engine, WIT, "consumer");

    let error = Resolver::new(
        engine.clone(),
        FakeRegistry::default(),
        FakeArtifacts::default(),
    )
    .plan(&[], consumer())
    .await
    .expect_err("nobody implements store");
    assert!(matches!(error, ResolveError::Unsatisfied(_)), "{error}");

    let plan = Resolver::new(
        engine.clone(),
        FakeRegistry::default(),
        FakeArtifacts::default(),
    )
    .on_missing(Missing::Trap)
    .plan(&[], consumer())
    .await
    .expect("permissive loads anyway");
    assert!(matches!(plan.root().imports[0].source, Source::Trap));
}

#[tokio::test]
async fn a_cycle_is_reported_by_interface_name() {
    let wit = "
        package test:cycle@1.0.0;

        interface a { ping: func() -> u32; }
        interface b { pong: func() -> u32; }

        world root { import a; }
        world left { export a; import b; }
        world right { export b; import a; }
    ";

    let engine = engine().expect("engine");
    let root = support::component_of(&engine, wit, "root");
    let left = support::bytes_of(wit, "left");
    let right = support::bytes_of(wit, "right");

    let digest_of = |component: &Component, interface: &str| {
        ComponentScan::new(&engine, component)
            .expect("scan")
            .import(&interface.parse().unwrap())
            .expect("imported")
            .shape()
            .expect("a wire shape")
            .digest()
    };
    let a_digest = digest_of(&root, "test:cycle/a@1.0.0");
    let b_digest = digest_of(
        &Component::new(&engine, &left).expect("left compiles"),
        "test:cycle/b@1.0.0",
    );

    let offer = |id: &str, interface: &str, digest: &str, uri: &str, bytes: &[u8]| Resolution {
        provider: Provider {
            id: id.to_owned(),
            kind: ProviderKind::Component,
            interfaces: vec![],
            endpoint: Endpoint::Artifact(artifact(uri, bytes)),
            ttl_secs: 30,
        },
        interface: interface.parse().unwrap(),
        shape_digest: digest.to_owned(),
    };

    let registry = FakeRegistry(vec![
        offer("left", "test:cycle/a@1.0.0", &a_digest, "mem://left", &left),
        offer(
            "right",
            "test:cycle/b@1.0.0",
            &b_digest,
            "mem://right",
            &right,
        ),
    ]);
    let artifacts = FakeArtifacts(HashMap::from([
        ("mem://left".to_owned(), left),
        ("mem://right".to_owned(), right),
    ]));

    let error = Resolver::new(engine, registry, artifacts)
        .plan(&[], root)
        .await
        .expect_err("a and b need each other");

    let ResolveError::Cycle(path) = &error else {
        panic!("expected a cycle, got {error}");
    };
    assert_eq!(
        path,
        &[
            "test:cycle/a@1.0.0",
            "test:cycle/b@1.0.0",
            "test:cycle/a@1.0.0"
        ]
    );
}
