//! Hosting side of the framework: loading components and working out what they
//! need before deciding how to satisfy it.

mod link;
mod resolve;
mod scan;

pub use link::{Host, LinkError};
pub use resolve::{
    Binding, ComponentKey, Discovery, Fetch, FetchError, Missing, Node, Plan, ResolveError,
    Resolver, Service, ShapeMismatch, Source,
};
pub use scan::{ComponentScan, InterfaceUse, Linkage, ScanError};

use wasmtime::{Config, Engine};

/// The engine components are compiled and instantiated with.
///
/// `Config::async_support` is deprecated and a no-op in wasmtime 48; async is
/// always on, which is what lets a host function park a guest call on a NATS
/// round trip. See `docs/spec/linking.md` for what that does and does not buy.
pub fn engine() -> wasmtime::Result<Engine> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config)
}
