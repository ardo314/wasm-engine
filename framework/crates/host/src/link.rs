//! Wiring a [`Plan`] into live instances.
//!
//! Everything runs in one store, so satisfying an import in-process is a
//! re-entrant `call_async` on the target's [`Func`] with the caller's own
//! values — no copy, no encoding. `docs/spec/linking.md` is normative for the
//! topology and for what a trap does to the store.

// As in `resolve`: naming an `InterfaceId` alone exceeds the lint's threshold.
#![allow(clippy::result_large_err)]

use wasm_protocol::InterfaceId;
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Component, Func, Instance, Linker, Val};
use wasmtime::{Engine, Store};

use crate::resolve::{ComponentKey, Plan, Source};

/// A set of components instantiated together, and the store they share.
pub struct Host {
    engine: Engine,
    store: Store<()>,
    loaded: Vec<Loaded>,
}

struct Loaded {
    component: Component,
    instance: Instance,
}

impl Host {
    pub fn new(engine: Engine) -> Self {
        let store = Store::new(&engine, ());
        Self {
            engine,
            store,
            loaded: Vec::new(),
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// What this host already has running, in the order
    /// [`ComponentKey::Live`] indexes. Hand this to `Resolver::plan`.
    pub fn live(&self) -> Vec<Component> {
        self.loaded
            .iter()
            .map(|loaded| loaded.component.clone())
            .collect()
    }

    /// Instantiates every component in `plan`, returning the root's instance.
    ///
    /// The plan is ordered dependencies-first, so each component's imports are
    /// already running by the time it is reached.
    pub async fn instantiate(&mut self, plan: Plan) -> Result<Instance, LinkError> {
        let base = self.loaded.len();
        let root = plan.root_index();

        for node in plan.into_nodes() {
            let mut linker = Linker::new(&self.engine);
            let mut needs_stubs = false;

            for binding in &node.imports {
                match &binding.source {
                    Source::InProcess(key) => {
                        let target = match key {
                            ComponentKey::Live(index) => *index,
                            ComponentKey::Planned(index) => base + index,
                        };
                        let component = self.loaded[target].component.clone();
                        let instance = self.loaded[target].instance;
                        self.wire(&mut linker, &binding.interface, &component, instance)?;
                    }
                    Source::Trap => needs_stubs = true,
                    Source::Nats(service) => {
                        return Err(LinkError::NoRemoteTransport(service.interface.clone()));
                    }
                }
            }

            if needs_stubs {
                linker
                    .define_unknown_imports_as_traps(&node.component)
                    .map_err(LinkError::Wasmtime)?;
            }

            let instance = linker
                .instantiate_async(&mut self.store, &node.component)
                .await
                .map_err(LinkError::Wasmtime)?;
            self.loaded.push(Loaded {
                component: node.component,
                instance,
            });
        }

        Ok(self.loaded[base + root].instance)
    }

    /// Calls `function` of `interface` on `instance`.
    pub async fn call(
        &mut self,
        instance: Instance,
        interface: &InterfaceId,
        function: &str,
        params: &[Val],
        results: &mut [Val],
    ) -> Result<(), LinkError> {
        let func = func_of(&mut self.store, instance, interface, function)?;
        func.call_async(&mut self.store, params, results)
            .await
            .map_err(LinkError::Wasmtime)
    }

    /// Defines every function of `interface` as a forward to the target.
    ///
    /// The names come from the provider's own type rather than from a scan, so
    /// interfaces carrying resources — which have no wire shape — link too.
    fn wire(
        &mut self,
        linker: &mut Linker<()>,
        interface: &InterfaceId,
        component: &Component,
        instance: Instance,
    ) -> Result<(), LinkError> {
        let name = interface.to_string();
        let functions = functions_of(&self.engine, component, &name);
        if functions.is_empty() {
            return Err(LinkError::NotExported(interface.clone()));
        }

        let mut defined = linker.instance(&name).map_err(LinkError::Wasmtime)?;
        for function in functions {
            let func = func_of(&mut self.store, instance, interface, &function)?;
            defined
                .func_new_async(&function, move |store, _ty, params, results| {
                    Box::new(async move { func.call_async(store, params, results).await })
                })
                .map_err(LinkError::Wasmtime)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("`{0}` is not exported by the component meant to provide it")]
    NotExported(InterfaceId),

    #[error("`{interface}` has no function `{function}`")]
    NoSuchFunction {
        interface: InterfaceId,
        function: String,
    },

    #[error("`{0}` has to be called over NATS, which this host cannot do yet")]
    NoRemoteTransport(InterfaceId),

    #[error("{0:#}")]
    Wasmtime(wasmtime::Error),
}

fn func_of(
    store: &mut Store<()>,
    instance: Instance,
    interface: &InterfaceId,
    function: &str,
) -> Result<Func, LinkError> {
    let missing = || LinkError::NoSuchFunction {
        interface: interface.clone(),
        function: function.to_owned(),
    };

    let owner = instance
        .get_export_index(&mut *store, None, &interface.to_string())
        .ok_or_else(missing)?;
    let index = instance
        .get_export_index(&mut *store, Some(&owner), function)
        .ok_or_else(missing)?;
    instance.get_func(&mut *store, index).ok_or_else(missing)
}

fn functions_of(engine: &Engine, component: &Component, interface: &str) -> Vec<String> {
    component
        .component_type()
        .exports(engine)
        .find(|(name, _)| *name == interface)
        .and_then(|(_, item)| match item.ty {
            ComponentItem::ComponentInstance(instance) => Some(
                instance
                    .exports(engine)
                    .filter(|(_, item)| matches!(item.ty, ComponentItem::ComponentFunc(_)))
                    .map(|(name, _)| name.to_owned())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}
