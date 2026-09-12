//! Deciding how each of a component's imports gets satisfied.
//!
//! The preference order is in-process first: a live instance, then a component
//! provider the host can fetch and instantiate, then a service on NATS, then
//! failure. Only the *decision* lives here — performing the links is the job of
//! the in-process linker and the NATS import proxy.
//!
//! An interface the scanner flagged [`Linkage::InProcess`] never reaches the
//! service step: its signatures cannot cross a process boundary, so a `service`
//! provider would only fail at the first call. `docs/spec/registry.md` §2.

// Any variant naming an `InterfaceId` is over the lint's threshold, because an
// `InterfaceId` is. Failing to resolve happens once per load, never in a loop.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use sha2::{Digest, Sha256};
use wasm_protocol::InterfaceId;
use wasm_registry::{ArtifactRef, Endpoint, ProviderKind, RegistryError, Resolution};
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::scan::{ComponentScan, InterfaceUse, ScanError};

/// Where the registry is asked who implements an interface.
///
/// A trait so the resolver can be exercised without a NATS cluster; the real
/// implementation is [`wasm_registry::Client`].
pub trait Discovery {
    fn resolve(
        &self,
        name: &str,
        version_req: &str,
    ) -> impl Future<Output = Result<Vec<Resolution>, RegistryError>> + Send;
}

impl Discovery for wasm_registry::Client {
    fn resolve(
        &self,
        name: &str,
        version_req: &str,
    ) -> impl Future<Output = Result<Vec<Resolution>, RegistryError>> + Send {
        wasm_registry::Client::resolve(self, name, version_req)
    }
}

/// Where a `component` provider's bytes come from.
///
/// The resolver verifies the digest itself, so an implementation only has to
/// produce bytes for a URI — whatever scheme it understands.
pub trait Fetch {
    fn fetch(
        &self,
        artifact: &ArtifactRef,
    ) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send;
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct FetchError(String);

impl FetchError {
    pub fn new(message: impl std::fmt::Display) -> Self {
        Self(message.to_string())
    }
}

/// What to do when nothing implements an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Missing {
    /// Refuse to load the component.
    #[default]
    Fail,
    /// Link a stub that traps when called, so the rest of the component runs.
    Trap,
}

pub struct Resolver<D, F> {
    engine: Engine,
    discovery: D,
    fetcher: F,
    missing: Missing,
}

impl<D: Discovery + Sync, F: Fetch + Sync> Resolver<D, F> {
    pub fn new(engine: Engine, discovery: D, fetcher: F) -> Self {
        Self {
            engine,
            discovery,
            fetcher,
            missing: Missing::default(),
        }
    }

    pub fn on_missing(mut self, missing: Missing) -> Self {
        self.missing = missing;
        self
    }

    /// Works out how to satisfy `root` and everything it pulls in.
    ///
    /// `live` is what this host already has instantiated; anything it exports
    /// is preferred over fetching or calling out.
    pub async fn plan(&self, live: &[Component], root: Component) -> Result<Plan, ResolveError> {
        let mut ctx = Context::default();
        for (index, component) in live.iter().enumerate() {
            for export in ComponentScan::new(&self.engine, component)?.exports() {
                ctx.provided
                    .entry(export.id.clone())
                    .or_insert(ComponentKey::Live(index));
            }
        }

        let root = self.add(root, &mut ctx).await?;
        Ok(Plan {
            nodes: ctx.nodes,
            root,
        })
    }

    /// Scans `component`, resolves its imports, and appends it to the plan.
    ///
    /// Children land before their parent, so instantiating the nodes in order
    /// never reaches a component whose dependencies are not yet up. Boxed
    /// because a fetched component's imports come back through here.
    fn add<'a>(
        &'a self,
        component: Component,
        ctx: &'a mut Context,
    ) -> Pin<Box<dyn Future<Output = Result<usize, ResolveError>> + Send + 'a>> {
        Box::pin(async move {
            let scan = ComponentScan::new(&self.engine, &component)?;

            let mut imports = Vec::with_capacity(scan.imports().len());
            for used in scan.imports() {
                imports.push(Binding {
                    interface: used.id.clone(),
                    source: self.source_for(used, ctx).await?,
                });
            }

            let exports: Vec<InterfaceId> =
                scan.exports().iter().map(|used| used.id.clone()).collect();
            ctx.nodes.push(Node {
                component,
                imports,
                exports: exports.clone(),
            });

            let index = ctx.nodes.len() - 1;
            for export in exports {
                ctx.provided
                    .entry(export)
                    .or_insert(ComponentKey::Planned(index));
            }
            Ok(index)
        })
    }

    async fn source_for(
        &self,
        used: &InterfaceUse,
        ctx: &mut Context,
    ) -> Result<Source, ResolveError> {
        // 1. Already here.
        if let Some(key) = ctx.provided.get(&used.id) {
            return Ok(Source::InProcess(*key));
        }

        if ctx.pending.contains(&used.id) {
            let mut cycle: Vec<String> = ctx.pending.iter().map(InterfaceId::to_string).collect();
            cycle.push(used.id.to_string());
            return Err(ResolveError::Cycle(cycle));
        }

        let offers = self.offers(used).await?;

        // 2. A component the host can load, which is another subtree.
        if let Some((provider, artifact)) = offers.artifact {
            ctx.pending.push(used.id.clone());
            let loaded = self.load(&artifact, ctx).await;
            ctx.pending.pop();

            let loaded = loaded?;
            if !ctx.nodes[loaded].exports.contains(&used.id) {
                return Err(ResolveError::NotExported {
                    interface: used.id.clone(),
                    provider,
                    uri: artifact.uri,
                });
            }
            return Ok(Source::InProcess(ComponentKey::Planned(loaded)));
        }

        // 3. A service, but only for signatures that survive the wire.
        if let Some(service) = offers.service {
            return Ok(Source::Nats(service));
        }

        if !offers.mismatched.is_empty() {
            let (provider, found) = offers.mismatched.into_iter().next().expect("non-empty");
            return Err(ResolveError::ShapeMismatch(Box::new(ShapeMismatch {
                interface: used.id.clone(),
                provider,
                expected: offers.expected.unwrap_or_default(),
                found,
            })));
        }

        // 4. Nothing.
        match self.missing {
            Missing::Trap => Ok(Source::Trap),
            Missing::Fail if !used.is_wire_encodable() => {
                Err(ResolveError::InProcessOnly(used.id.clone()))
            }
            Missing::Fail => Err(ResolveError::Unsatisfied(used.id.clone())),
        }
    }

    /// Splits what the registry knows about `used` into what the host can act on.
    async fn offers(&self, used: &InterfaceUse) -> Result<Offers, ResolveError> {
        let name = format!(
            "{}:{}/{}",
            used.id.namespace(),
            used.id.package(),
            used.id.interface()
        );
        let expected = used.shape().map(|shape| shape.digest());
        let resolved = self
            .discovery
            .resolve(&name, &format!("^{}", used.id.version()))
            .await?;

        let mut offers = Offers {
            expected: expected.clone(),
            ..Offers::default()
        };
        for resolution in resolved {
            // An in-process-only interface has no wire form, so a digest is not
            // something either side can have computed.
            if let Some(expected) = &expected
                && &resolution.shape_digest != expected
            {
                offers
                    .mismatched
                    .push((resolution.provider.id, resolution.shape_digest));
                continue;
            }

            match (resolution.provider.kind, resolution.provider.endpoint) {
                (ProviderKind::Component, Endpoint::Artifact(artifact)) => {
                    offers
                        .artifact
                        .get_or_insert((resolution.provider.id, artifact));
                }
                (ProviderKind::Service, endpoint @ Endpoint::Nats(_))
                    if used.is_wire_encodable() =>
                {
                    offers.service.get_or_insert(Service {
                        provider: resolution.provider.id,
                        interface: resolution.interface,
                        endpoint,
                    });
                }
                // A kind that disagrees with its endpoint, or a `service`
                // offering an interface that cannot be encoded. Neither is
                // actionable, and neither is something the registry could have
                // caught on the host's behalf.
                _ => {}
            }
        }
        Ok(offers)
    }

    async fn load(&self, artifact: &ArtifactRef, ctx: &mut Context) -> Result<usize, ResolveError> {
        let bytes = self
            .fetcher
            .fetch(artifact)
            .await
            .map_err(|source| ResolveError::Fetch {
                uri: artifact.uri.clone(),
                source,
            })?;

        let found = hex(Sha256::digest(&bytes).as_slice());
        if found != artifact.sha256 {
            return Err(ResolveError::ArtifactMismatch {
                uri: artifact.uri.clone(),
                expected: artifact.sha256.clone(),
                found,
            });
        }

        let component =
            Component::new(&self.engine, &bytes).map_err(|e| ResolveError::Compile {
                uri: artifact.uri.clone(),
                reason: e.to_string(),
            })?;
        self.add(component, ctx).await
    }
}

/// A component to instantiate and what to wire its imports to.
#[derive(Debug)]
pub struct Node {
    pub component: Component,
    pub imports: Vec<Binding>,
    pub exports: Vec<InterfaceId>,
}

/// One import, and where its implementation comes from.
#[derive(Debug, Clone)]
pub struct Binding {
    pub interface: InterfaceId,
    pub source: Source,
}

#[derive(Debug, Clone)]
pub enum Source {
    /// Another component in this host.
    InProcess(ComponentKey),
    /// A service, called over NATS.
    Nats(Service),
    /// Nothing implements this; calling it traps. Only under [`Missing::Trap`].
    Trap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub provider: String,
    /// The exact version to address the provider at; a subject carries a
    /// version, and the import's requirement is not one.
    pub interface: InterfaceId,
    pub endpoint: Endpoint,
}

/// Which component satisfies an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKey {
    /// Index into the `live` slice given to [`Resolver::plan`].
    Live(usize),
    /// Index into [`Plan::nodes`].
    Planned(usize),
}

/// Everything that has to happen before the root component can run.
#[derive(Debug)]
pub struct Plan {
    nodes: Vec<Node>,
    root: usize,
}

impl Plan {
    /// Dependencies first: instantiating in this order never reaches a
    /// component whose imports are not yet satisfiable.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn root(&self) -> &Node {
        &self.nodes[self.root]
    }

    pub fn root_index(&self) -> usize {
        self.root
    }

    /// Consumes the plan, still dependencies-first.
    pub fn into_nodes(self) -> Vec<Node> {
        self.nodes
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("nothing implements `{0}`")]
    Unsatisfied(InterfaceId),

    #[error("`{0}` cannot cross a process boundary, and no loadable component implements it")]
    InProcessOnly(InterfaceId),

    #[error("dependency cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),

    #[error(transparent)]
    ShapeMismatch(Box<ShapeMismatch>),

    #[error("`{uri}` hashes to {found}, expected {expected}")]
    ArtifactMismatch {
        uri: String,
        expected: String,
        found: String,
    },

    #[error("fetching `{uri}`")]
    Fetch {
        uri: String,
        #[source]
        source: FetchError,
    },

    #[error("`{uri}` is not a usable component: {reason}")]
    Compile { uri: String, reason: String },

    #[error("provider `{provider}` claims `{interface}`, but `{uri}` does not export it")]
    NotExported {
        interface: InterfaceId,
        provider: String,
        uri: String,
    },

    #[error(transparent)]
    Scan(#[from] ScanError),

    #[error(transparent)]
    Registry(#[from] RegistryError),
}

/// A provider that claims an interface the importing component would not
/// recognise. `docs/spec/registry.md` §5.
#[derive(Debug, thiserror::Error)]
#[error(
    "provider `{provider}` offers `{interface}` with shape {found}, but the importing component expects {expected}"
)]
pub struct ShapeMismatch {
    pub interface: InterfaceId,
    pub provider: String,
    pub expected: String,
    pub found: String,
}

#[derive(Default)]
struct Context {
    nodes: Vec<Node>,
    provided: HashMap<InterfaceId, ComponentKey>,
    /// Interfaces whose provider is still being resolved, innermost last.
    pending: Vec<InterfaceId>,
}

#[derive(Default)]
struct Offers {
    expected: Option<String>,
    artifact: Option<(String, ArtifactRef)>,
    service: Option<Service>,
    /// Provider id and the digest it advertised, kept so a total absence of
    /// usable providers can say why.
    mismatched: Vec<(String, String)>,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(64), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}
