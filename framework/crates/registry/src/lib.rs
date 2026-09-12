//! The `ardo314:registry@0.1.0` service: providers announce themselves here,
//! and hosts ask it who implements an interface.
//!
//! `docs/spec/registry.md` is normative for the behaviour the WIT types cannot
//! express — lifecycle, staleness and shape conflicts.

mod client;
mod server;
mod store;
mod types;
mod wire;

pub use client::{Client, Registration, Resolution};
pub use server::serve;
pub use store::{Limits, Registry};
pub use types::{ArtifactRef, Endpoint, InterfaceRef, Provider, ProviderKind, RegistryError};

/// The interface providers call to announce and renew themselves.
pub const REGISTRATION_INTERFACE: &str = "ardo314:registry/registration@0.1.0";

/// The interface hosts call to find providers.
pub const DISCOVERY_INTERFACE: &str = "ardo314:registry/discovery@0.1.0";

/// Default name of the JetStream key-value bucket holding registrations.
pub const DEFAULT_BUCKET: &str = "wit-registry";
