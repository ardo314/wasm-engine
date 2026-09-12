//! The `ardo314:registry@0.1.0` types, as Rust.
//!
//! Their MessagePack encoding lives in [`crate::wire`]; it is the encoding
//! `docs/spec/wire-protocol.md` §4 prescribes for the WIT definitions in
//! `wit/registry/world.wit`.

/// Everything the registry knows about one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub id: String,
    pub kind: ProviderKind,
    pub interfaces: Vec<InterfaceRef>,
    pub endpoint: Endpoint,
    pub ttl_secs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// A wasm component a host may fetch and instantiate in-process.
    Component,
    /// An ordinary process, reachable only over the wire.
    Service,
}

/// One interface a provider claims to implement, at one version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceRef {
    /// `namespace:package/interface`, without the version.
    pub name: String,
    pub version: String,
    pub shape_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Nats(String),
    Artifact(ArtifactRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRef {
    pub uri: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("not found")]
    NotFound,
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("unavailable: {0}")]
    Unavailable(String),
}

impl RegistryError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub(crate) fn unavailable(message: impl std::fmt::Display) -> Self {
        Self::Unavailable(message.to_string())
    }
}
