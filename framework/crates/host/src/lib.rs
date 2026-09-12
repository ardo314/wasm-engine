//! Hosting side of the framework: loading components and working out what they
//! need before deciding how to satisfy it.

mod resolve;
mod scan;

pub use resolve::{
    Binding, ComponentKey, Discovery, Fetch, FetchError, Missing, Node, Plan, ResolveError,
    Resolver, Service, ShapeMismatch, Source,
};
pub use scan::{ComponentScan, InterfaceUse, Linkage, ScanError};

use wasmtime::{Config, Engine};

/// The engine components are compiled and instantiated with.
///
/// Async is always on in wasmtime 48 — `Config::async_support` is a no-op — but
/// it is what makes an import satisfied over NATS possible: the calling
/// component parks until the reply arrives.
pub fn engine() -> wasmtime::Result<Engine> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config)
}
