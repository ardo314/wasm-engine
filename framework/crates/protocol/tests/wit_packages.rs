//! Every WIT package in `wit/` must resolve. A broken package would otherwise
//! only surface when someone runs `cargo component build`.

use std::path::PathBuf;

use wit_parser::Resolve;

fn wit_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../wit")
}

fn resolve(package: &str) -> Resolve {
    let path = wit_dir().join(package);
    let mut resolve = Resolve::default();
    resolve
        .push_path(&path)
        .unwrap_or_else(|e| panic!("failed to resolve {}: {e:?}", path.display()));
    resolve
}

#[test]
fn every_package_resolves() {
    for entry in std::fs::read_dir(wit_dir()).expect("wit/ is readable") {
        let entry = entry.expect("readable entry");
        if entry.file_type().expect("file type").is_dir() {
            resolve(&entry.file_name().to_string_lossy());
        }
    }
}

#[test]
fn registry_exposes_the_documented_surface() {
    let resolve = resolve("registry");

    let world = resolve
        .worlds
        .iter()
        .find(|(_, world)| world.name == "registry")
        .expect("a world named `registry`")
        .1;
    assert_eq!(
        world.exports.len(),
        3,
        "expected types, registration and discovery"
    );

    for (interface, expected) in [
        ("registration", vec!["deregister", "heartbeat", "register"]),
        ("discovery", vec!["list-interfaces", "resolve"]),
    ] {
        let found = resolve
            .interfaces
            .iter()
            .find(|(_, iface)| iface.name.as_deref() == Some(interface))
            .unwrap_or_else(|| panic!("an interface named `{interface}`"))
            .1;

        let mut functions: Vec<&str> = found.functions.keys().map(String::as_str).collect();
        functions.sort_unstable();
        assert_eq!(functions, expected, "functions of `{interface}`");
    }
}
