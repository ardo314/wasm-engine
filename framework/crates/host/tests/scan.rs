//! The scanner is exercised against components synthesised from WIT rather than
//! against build artefacts, so it does not depend on `cargo component` having
//! run.

mod support;

use wasm_host::{ComponentScan, Linkage, engine};
use wasmtime::component::Component;
use wit_parser::{Resolve, WorldId};

fn component(resolve: &Resolve, world: WorldId) -> Component {
    Component::new(&engine().expect("engine"), support::encode(resolve, world))
        .expect("component compiles")
}

fn from_source(wit: &str, world: &str) -> Component {
    support::component_of(&engine().expect("engine"), wit, world)
}

#[test]
fn math_exports_every_interface_and_imports_nothing() {
    let mut resolve = Resolve::default();
    let package = resolve
        .push_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../wit/math"))
        .expect("wit/math resolves")
        .0;
    let world = resolve
        .select_world(&[package], Some("math"))
        .expect("world");

    let engine = engine().expect("engine");
    let scan = ComponentScan::new(&engine, &component(&resolve, world)).expect("scan");

    assert!(
        scan.imports().is_empty(),
        "math needs nothing: {:?}",
        scan.imports()
            .iter()
            .map(|u| u.id.to_string())
            .collect::<Vec<_>>()
    );

    let exported: Vec<String> = scan.exports().iter().map(|u| u.id.to_string()).collect();
    assert!(
        exported.contains(&"ardo314:math/vector3d@0.0.3".to_owned()),
        "{exported:?}"
    );
    assert_eq!(
        exported.len(),
        resolve.worlds[world].exports.len(),
        "one per `export` in the world: {exported:?}"
    );
    assert!(
        scan.exports().iter().all(|used| used.is_wire_encodable()),
        "every math interface is plain data"
    );

    let vector3d = scan
        .export(&"ardo314:math/vector3d@0.0.3".parse().unwrap())
        .expect("vector3d is exported");
    let functions: Vec<&str> = vector3d
        .shape()
        .expect("a wire shape")
        .functions()
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(functions.contains(&"cross"), "{functions:?}");
}

#[test]
fn an_interface_taking_a_resource_is_in_process_only() {
    let scan_of = |wit: &str| {
        let engine = engine().expect("engine");
        ComponentScan::new(&engine, &from_source(wit, "consumer")).expect("scan")
    };

    let scan = scan_of(
        "
        package test:fixture@1.0.0;

        interface store {
            resource handle {
                constructor();
            }
            put: func(slot: borrow<handle>, value: string);
        }

        world consumer {
            import store;
        }
        ",
    );

    let import = scan
        .import(&"test:fixture/store@1.0.0".parse().unwrap())
        .expect("store is imported");
    assert!(!import.is_wire_encodable());
    assert!(import.shape().is_none());
    let Linkage::InProcess { reason, .. } = &import.linkage else {
        panic!("expected in-process linkage, got {:?}", import.linkage);
    };
    assert!(reason.contains("resource"), "{reason}");
}

#[test]
fn a_plain_data_import_is_wire_encodable() {
    let wit = "
        package test:fixture@1.0.0;

        interface store {
            put: func(key: string, value: list<u8>) -> result<_, string>;
        }

        world consumer {
            import store;
        }
        ";

    let engine = engine().expect("engine");
    let scan = ComponentScan::new(&engine, &from_source(wit, "consumer")).expect("scan");

    let import = scan
        .import(&"test:fixture/store@1.0.0".parse().unwrap())
        .expect("store is imported");
    let shape = import.shape().expect("a wire shape");
    assert_eq!(shape.functions().len(), 1);
    assert_eq!(shape.functions()[0].name, "put");
}
