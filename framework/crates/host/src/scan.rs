//! What a component asks for, and what it offers.
//!
//! The scan runs once per component, at load time, and is the input to
//! resolution: an interface whose every signature projects into [`WitType`] can
//! be satisfied in-process *or* over NATS, while one that does not can only
//! ever be linked in-process.

use wasm_protocol::{
    CodecError, FunctionShape, InterfaceId, InterfaceShape, ProtocolError, WitType,
};
use wasmtime::Engine;
use wasmtime::component::Component;
use wasmtime::component::types::{
    ComponentExtern, ComponentFunc, ComponentInstance, ComponentItem,
};

/// The interfaces a component imports and exports.
#[derive(Debug, Clone)]
pub struct ComponentScan {
    imports: Vec<InterfaceUse>,
    exports: Vec<InterfaceUse>,
}

impl ComponentScan {
    pub fn new(engine: &Engine, component: &Component) -> Result<Self, ScanError> {
        let ty = component.component_type();
        Ok(Self {
            imports: collect(engine, ty.imports(engine))?,
            exports: collect(engine, ty.exports(engine))?,
        })
    }

    pub fn imports(&self) -> &[InterfaceUse] {
        &self.imports
    }

    pub fn exports(&self) -> &[InterfaceUse] {
        &self.exports
    }

    pub fn import(&self, id: &InterfaceId) -> Option<&InterfaceUse> {
        self.imports.iter().find(|used| &used.id == id)
    }

    pub fn export(&self, id: &InterfaceId) -> Option<&InterfaceUse> {
        self.exports.iter().find(|used| &used.id == id)
    }
}

/// One interface a component imports or exports.
#[derive(Debug, Clone)]
pub struct InterfaceUse {
    pub id: InterfaceId,
    pub linkage: Linkage,
}

impl InterfaceUse {
    /// `None` when the interface is in-process-only: there is no wire form to
    /// take a digest of, so it can neither be registered nor resolved.
    pub fn shape(&self) -> Option<&InterfaceShape> {
        match &self.linkage {
            Linkage::Wire(shape) => Some(shape),
            Linkage::InProcess { .. } => None,
        }
    }

    pub fn is_wire_encodable(&self) -> bool {
        matches!(self.linkage, Linkage::Wire(_))
    }
}

/// How an interface may be satisfied.
#[derive(Debug, Clone)]
pub enum Linkage {
    /// Every signature projects into the wire vocabulary, so a provider may be
    /// another component in this process or a service on NATS.
    Wire(InterfaceShape),

    /// A signature uses a type the protocol cannot carry — a resource, future,
    /// stream or map. Only in-process linking can satisfy this interface, and
    /// it must never be resolved over NATS.
    InProcess {
        function: String,
        /// The codec's account of what it could not project.
        reason: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("`{0}` is not a versioned WIT interface name")]
    NotAnInterface(String, #[source] ProtocolError),
}

fn collect<'a>(
    engine: &Engine,
    items: impl Iterator<Item = (&'a str, ComponentExtern<'a>)>,
) -> Result<Vec<InterfaceUse>, ScanError> {
    items
        .filter_map(|(name, item)| match item.ty {
            ComponentItem::ComponentInstance(instance) => Some((name, instance)),
            // Bare functions, core modules and nested components carry no
            // interface identity, so nothing can be resolved for them.
            _ => None,
        })
        .map(|(name, instance)| scan_instance(engine, name, &instance))
        .collect()
}

fn scan_instance(
    engine: &Engine,
    name: &str,
    instance: &ComponentInstance,
) -> Result<InterfaceUse, ScanError> {
    let id = name
        .parse::<InterfaceId>()
        .map_err(|e| ScanError::NotAnInterface(name.to_owned(), e))?;

    let mut functions = Vec::new();
    for (function, item) in instance.exports(engine) {
        let ComponentItem::ComponentFunc(func) = item.ty else {
            continue;
        };
        match project(function, &func) {
            Ok(shape) => functions.push(shape),
            Err(linkage) => return Ok(InterfaceUse { id, linkage }),
        }
    }

    Ok(InterfaceUse {
        id,
        linkage: Linkage::Wire(InterfaceShape::new(functions)),
    })
}

fn project(name: &str, func: &ComponentFunc) -> Result<FunctionShape, Linkage> {
    let in_process = |e: CodecError| Linkage::InProcess {
        function: name.to_owned(),
        reason: e.to_string(),
    };

    let params = func
        .params()
        .map(|(_, ty)| WitType::from_component_type(&ty))
        .collect::<Result<Vec<_>, _>>()
        .map_err(in_process)?;
    let results = func
        .results()
        .map(|ty| WitType::from_component_type(&ty))
        .collect::<Result<Vec<_>, _>>()
        .map_err(in_process)?;

    Ok(FunctionShape::new(name, params, results))
}
