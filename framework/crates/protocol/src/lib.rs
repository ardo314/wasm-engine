//! Wire protocol for invoking WIT interface functions across process boundaries.
//!
//! See `docs/spec/wire-protocol.md` for the normative definition. This crate is
//! the Rust implementation of that document.
//!
//! The `val-codec` feature adds conversion to and from wasmtime's dynamic
//! [`Val`](wasmtime::component::Val) representation. It is off by default so
//! native service SDKs do not pull in a wasm runtime.

mod envelope;
mod error;
mod iface;
mod shape;
mod subject;
mod ty;

#[cfg(feature = "val-codec")]
mod val;

pub use envelope::{ENVELOPE_VERSION, Reply, Request};
pub use error::{CodecError, ErrorCode, ProtocolError, WireError};
pub use iface::InterfaceId;
pub use shape::{FunctionShape, InterfaceShape};
pub use subject::Subject;
pub use ty::{Case, Field, WitType};

#[cfg(feature = "val-codec")]
pub use val::{msgpack_to_val, msgpack_to_vals, val_to_msgpack, vals_to_msgpack};

pub use rmpv::Value as MsgpackValue;
