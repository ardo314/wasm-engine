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

pub const ADDER: &str = "test:fixture/adder@1.0.0";
pub const CALLER: &str = "test:fixture/caller@1.0.0";

/// Exports `adder`, adding two numbers for real.
pub const PROVIDER: &str = r#"
(component
  (core module $m
    (func (export "add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add))
  (core instance $i (instantiate $m))
  (func $add (param "a" u32) (param "b" u32) (result u32)
    (canon lift (core func $i "add")))
  (instance $adder (export "add" (func $add)))
  (export "test:fixture/adder@1.0.0" (instance $adder))
)
"#;

/// Imports `adder` and exports `sum`, which is nothing but a forward.
pub const CONSUMER: &str = r#"
(component
  (import "test:fixture/adder@1.0.0" (instance $adder
    (export "add" (func (param "a" u32) (param "b" u32) (result u32)))))
  (alias export $adder "add" (func $add))
  (core func $add-lowered (canon lower (func $add)))
  (core module $m
    (import "adder" "add" (func $add (param i32 i32) (result i32)))
    (func (export "sum") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      call $add))
  (core instance $i (instantiate $m
    (with "adder" (instance (export "add" (func $add-lowered))))))
  (func $sum (param "a" u32) (param "b" u32) (result u32)
    (canon lift (core func $i "sum")))
  (instance $caller (export "sum" (func $sum)))
  (export "test:fixture/caller@1.0.0" (instance $caller))
)
"#;
