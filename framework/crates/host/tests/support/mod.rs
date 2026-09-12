//! Components synthesised from WIT, so the tests need nothing built by
//! `cargo component`.

// Each test binary compiles this module and uses only part of it.
#![allow(dead_code)]

use wasmtime::Engine;
use wasmtime::component::Component;
use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::{ManglingAndAbi, Resolve, WorldId};

/// A real component with the given world's types, implemented by stubs.
pub fn encode(resolve: &Resolve, world: WorldId) -> Vec<u8> {
    let mut module = wit_component::dummy_module(resolve, world, ManglingAndAbi::Standard32);
    wit_component::embed_component_metadata(&mut module, resolve, world, StringEncoding::UTF8)
        .expect("metadata embeds");
    ComponentEncoder::default()
        .module(&module)
        .expect("module is accepted")
        .validate(true)
        .encode()
        .expect("component encodes")
}

pub fn bytes_of(wit: &str, world: &str) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let package = resolve
        .push_str("fixture.wit", wit)
        .expect("fixture resolves");
    let world = resolve
        .select_world(&[package], Some(world))
        .expect("world");
    encode(&resolve, world)
}

pub fn component_of(engine: &Engine, wit: &str, world: &str) -> Component {
    Component::new(engine, bytes_of(wit, world)).expect("component compiles")
}
