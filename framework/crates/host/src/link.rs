//! Wiring a [`Plan`] into live instances.
//!
//! Each component gets its own store unless an interface it is linked by
//! cannot cross one, which keeps a trap from taking down components that had
//! nothing to do with it. `docs/spec/linking.md` is normative for the
//! topology, the grouping rule and what a trap does.

// As in `resolve`: naming an `InterfaceId` alone exceeds the lint's threshold.
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use tokio::sync::Mutex;
use wasm_nats_link::Proxy;
use wasm_protocol::InterfaceId;
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Component, Func, Instance, Linker, Val};
use wasmtime::{Engine, Store};

use crate::resolve::{Binding, ComponentKey, Node, Plan, Source};

/// One store, shared by the components that cannot be separated.
type Group = Arc<Mutex<Store<()>>>;

/// A set of components running together, and the stores they occupy.
pub struct Host {
    engine: Engine,
    groups: Vec<Group>,
    loaded: Vec<Loaded>,
    remote: Option<Proxy>,
}

/// A component this host is running. Which store it is in is not the
/// caller's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Running(usize);

struct Loaded {
    component: Component,
    instance: Instance,
    group: usize,
}

impl Host {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            groups: Vec::new(),
            loaded: Vec::new(),
            remote: None,
        }
    }

    /// Lets this host satisfy imports over NATS. Without one, a plan that
    /// resolved to a service is refused.
    pub fn with_remote(mut self, proxy: Proxy) -> Self {
        self.remote = Some(proxy);
        self
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

    /// How many stores this host is using. One per isolation group.
    pub fn stores(&self) -> usize {
        self.groups.len()
    }

    /// Instantiates every component in `plan`, returning the root's instance.
    ///
    /// The plan is ordered dependencies-first, so each component's imports are
    /// already running by the time it is reached.
    pub async fn instantiate(&mut self, plan: Plan) -> Result<Running, LinkError> {
        let base = self.loaded.len();
        let root = plan.root_index();
        let nodes = plan.into_nodes();
        let groups = self.assign_groups(&nodes)?;

        for (index, node) in nodes.into_iter().enumerate() {
            let group = groups[index];
            let mut linker = Linker::new(&self.engine);
            let mut needs_stubs = false;

            for binding in &node.imports {
                match &binding.source {
                    Source::InProcess(key) => {
                        self.wire(&mut linker, binding, self.resolve_key(base, key), group)
                            .await?;
                    }
                    Source::Trap => needs_stubs = true,
                    Source::Nats(service) => {
                        let proxy = self.remote.as_ref().ok_or_else(|| {
                            LinkError::NoRemoteTransport(binding.interface.clone())
                        })?;
                        let shape = binding
                            .shape
                            .as_ref()
                            .ok_or_else(|| LinkError::NotEncodable(binding.interface.clone()))?;
                        proxy
                            .define(&mut linker, &service.interface, shape)
                            .map_err(|e| LinkError::Wasmtime(e.into()))?;
                    }
                }
            }

            if needs_stubs {
                linker
                    .define_unknown_imports_as_traps(&node.component)
                    .map_err(LinkError::Wasmtime)?;
            }

            let store = Arc::clone(&self.groups[group]);
            let instance = linker
                .instantiate_async(&mut *store.lock().await, &node.component)
                .await
                .map_err(LinkError::Wasmtime)?;
            self.loaded.push(Loaded {
                component: node.component,
                instance,
                group,
            });
        }

        Ok(Running(base + root))
    }

    /// Calls `function` of `interface` on a component this host is running.
    pub async fn call(
        &self,
        running: Running,
        interface: &InterfaceId,
        function: &str,
        params: &[Val],
        results: &mut [Val],
    ) -> Result<(), LinkError> {
        let loaded = &self.loaded[running.0];
        let store = Arc::clone(&self.groups[loaded.group]);
        let mut store = store.lock().await;
        let func = func_of(&mut store, loaded.instance, interface, function)?;
        func.call_async(&mut *store, params, results)
            .await
            .map_err(LinkError::Wasmtime)
    }

    /// Which store each node belongs in.
    ///
    /// A binding with no wire shape carries resources, futures or streams,
    /// whose handles belong to one store's table and cannot be passed to
    /// another. Those two components have to share a store; everything else
    /// gets its own.
    fn assign_groups(&mut self, nodes: &[Node]) -> Result<Vec<usize>, LinkError> {
        let mut parent: Vec<usize> = (0..nodes.len()).collect();

        for (index, node) in nodes.iter().enumerate() {
            for binding in must_share(node) {
                if let Source::InProcess(ComponentKey::Planned(target)) = &binding.source {
                    union(&mut parent, index, *target);
                }
            }
        }

        // A set joined to something already running has to join its store.
        let mut anchor: Vec<Option<usize>> = vec![None; nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            for binding in must_share(node) {
                if let Source::InProcess(ComponentKey::Live(target)) = &binding.source {
                    let group = self.loaded[*target].group;
                    let root = find(&mut parent, index);
                    match anchor[root] {
                        Some(existing) if existing != group => {
                            return Err(LinkError::CannotShareStore(binding.interface.clone()));
                        }
                        _ => anchor[root] = Some(group),
                    }
                }
            }
        }

        let mut assigned = vec![usize::MAX; nodes.len()];
        for index in 0..nodes.len() {
            let root = find(&mut parent, index);
            if assigned[root] == usize::MAX {
                assigned[root] = match anchor[root] {
                    Some(group) => group,
                    None => {
                        self.groups
                            .push(Arc::new(Mutex::new(Store::new(&self.engine, ()))));
                        self.groups.len() - 1
                    }
                };
            }
            assigned[index] = assigned[root];
        }
        Ok(assigned)
    }

    fn resolve_key(&self, base: usize, key: &ComponentKey) -> usize {
        match key {
            ComponentKey::Live(index) => *index,
            ComponentKey::Planned(index) => base + index,
        }
    }

    /// Defines every function of the binding's interface as a forward to the
    /// component at `target`.
    ///
    /// Only functions. An interface that also exports a resource type needs
    /// that type defined on the linker, which is not implemented — see #30.
    async fn wire(
        &self,
        linker: &mut Linker<()>,
        binding: &Binding,
        target: usize,
        group: usize,
    ) -> Result<(), LinkError> {
        let interface = &binding.interface;
        let name = interface.to_string();
        let provider = &self.loaded[target];
        let functions = functions_of(&self.engine, &provider.component, &name);
        if functions.is_empty() {
            return Err(LinkError::NotExported(interface.clone()));
        }

        let store = Arc::clone(&self.groups[provider.group]);
        let instance = provider.instance;
        let same_store = provider.group == group;

        let mut funcs = Vec::with_capacity(functions.len());
        {
            let mut guard = store.lock().await;
            for function in &functions {
                funcs.push(func_of(&mut guard, instance, interface, function)?);
            }
        }

        let mut defined = linker.instance(&name).map_err(LinkError::Wasmtime)?;
        for (function, func) in functions.iter().zip(funcs) {
            if same_store {
                defined
                    .func_new_async(function, move |store, _ty, params, results| {
                        Box::new(async move { func.call_async(store, params, results).await })
                    })
                    .map_err(LinkError::Wasmtime)?;
            } else {
                let store = Arc::clone(&store);
                defined
                    .func_new_async(function, move |_store, _ty, params, results| {
                        let store = Arc::clone(&store);
                        // Owned, because the callee's store is not ours.
                        let params = params.to_vec();
                        Box::new(async move {
                            let mut lifted = vec![Val::Bool(false); results.len()];
                            let mut guard = store.lock().await;
                            func.call_async(&mut *guard, &params, &mut lifted).await?;
                            results.clone_from_slice(&lifted);
                            Ok(())
                        })
                    })
                    .map_err(LinkError::Wasmtime)?;
            }
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

    #[error("`{0}` resolved to a service, but this host has no NATS proxy")]
    NoRemoteTransport(InterfaceId),

    #[error("`{0}` has no wire shape, so it cannot be called over NATS")]
    NotEncodable(InterfaceId),

    #[error(
        "`{0}` cannot cross a store, but the components it links are already in different ones"
    )]
    CannotShareStore(InterfaceId),

    #[error("{0:#}")]
    Wasmtime(wasmtime::Error),
}

/// The bindings that pin two components to the same store: no wire shape
/// means the interface carries a resource, future or stream, whose handles
/// belong to one store's table.
fn must_share(node: &Node) -> impl Iterator<Item = &Binding> {
    node.imports
        .iter()
        .filter(|binding| binding.shape.is_none())
}

fn find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[b] = a;
    }
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
