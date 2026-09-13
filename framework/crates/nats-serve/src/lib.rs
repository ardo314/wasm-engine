//! Serving a loaded component's exports over NATS.
//!
//! The inverse of `wasm-nats-link`: instead of turning a guest call into a
//! request, this turns a request into a guest call, so a component running
//! here can answer hosts that never loaded it.
//!
//! Replicas subscribe to the same queue group, so the cluster spreads calls
//! across them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_nats::Subscriber;
use futures::StreamExt;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use wasm_host::{Host, Running};
use wasm_protocol::{
    ErrorCode, FunctionShape, InterfaceId, InterfaceShape, Reply, Request, Subject, WireError,
    msgpack_to_vals, vals_to_msgpack,
};
use wasm_registry::{Endpoint, InterfaceRef, Provider, ProviderKind};
use wasmtime::component::Val;

/// How many calls may be in flight before the adapter stops taking messages.
pub const DEFAULT_CONCURRENCY: usize = 32;

/// Publishes what a host is running.
#[derive(Clone)]
pub struct Adapter {
    nats: async_nats::Client,
    concurrency: usize,
}

impl Adapter {
    pub fn new(nats: async_nats::Client) -> Self {
        Self {
            nats,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }

    /// Bounds in-flight calls. The subscription stops being drained once this
    /// many are outstanding, which is what pushes back on the cluster.
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// Answers calls to `interface` by dispatching them to `running`.
    ///
    /// Serving stops when the returned [`Served`] is dropped.
    pub async fn serve(
        &self,
        host: Arc<Host>,
        running: Running,
        interface: InterfaceId,
        shape: InterfaceShape,
    ) -> Result<Served, ServeError> {
        let requests = self
            .nats
            .queue_subscribe(
                Subject::interface_wildcard(&interface),
                Subject::queue_group(&interface),
            )
            .await
            .map_err(|e| ServeError::Subscribe(interface.to_string(), e.to_string()))?;

        let handled = Arc::new(AtomicU64::new(0));
        let served = Served {
            handled: Arc::clone(&handled),
            task: tokio::spawn(run(
                self.nats.clone(),
                requests,
                host,
                running,
                interface,
                Arc::new(shape),
                Arc::new(Semaphore::new(self.concurrency)),
                handled,
            )),
        };
        Ok(served)
    }
}

/// A live subscription. Dropping it stops serving.
pub struct Served {
    handled: Arc<AtomicU64>,
    task: JoinHandle<()>,
}

impl Served {
    /// How many requests this replica has answered. Mostly useful for
    /// confirming that a queue group is actually spreading load.
    pub fn handled(&self) -> u64 {
        self.handled.load(Ordering::Relaxed)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The registration that tells the cluster about a served interface.
///
/// `kind` is the caller's to choose: `component` when the artifact can be
/// fetched and loaded elsewhere, `service` when only this endpoint is
/// reachable. See `docs/spec/registry.md` §2.
pub fn provider(
    id: impl Into<String>,
    kind: ProviderKind,
    endpoint: Endpoint,
    ttl_secs: u32,
    served: &[(InterfaceId, InterfaceShape)],
) -> Provider {
    Provider {
        id: id.into(),
        kind,
        interfaces: served
            .iter()
            .map(|(interface, shape)| InterfaceRef {
                name: format!(
                    "{}:{}/{}",
                    interface.namespace(),
                    interface.package(),
                    interface.interface()
                ),
                version: interface.version().to_string(),
                shape_digest: shape.digest(),
            })
            .collect(),
        endpoint,
        ttl_secs,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("could not subscribe for `{0}`: {1}")]
    Subscribe(String, String),
}

#[allow(clippy::too_many_arguments)]
async fn run(
    nats: async_nats::Client,
    mut requests: Subscriber,
    host: Arc<Host>,
    running: Running,
    interface: InterfaceId,
    shape: Arc<InterfaceShape>,
    permits: Arc<Semaphore>,
    handled: Arc<AtomicU64>,
) {
    while let Some(message) = requests.next().await {
        let Some(reply_to) = message.reply else {
            continue;
        };
        // Taken before the work is spawned, so a saturated adapter stops
        // draining the subscription instead of queueing without limit.
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            return;
        };

        let nats = nats.clone();
        let host = Arc::clone(&host);
        let interface = interface.clone();
        let shape = Arc::clone(&shape);
        let handled = Arc::clone(&handled);
        tokio::spawn(async move {
            let reply = answer(&host, running, &interface, &shape, &message.payload).await;
            if let Ok(payload) = reply.encode() {
                let _ = nats.publish(reply_to, payload.into()).await;
            }
            handled.fetch_add(1, Ordering::Relaxed);
            drop(permit);
        });
    }
}

async fn answer(
    host: &Host,
    running: Running,
    interface: &InterfaceId,
    shape: &InterfaceShape,
    payload: &[u8],
) -> Reply {
    let request = match Request::decode(payload) {
        Ok(request) => request,
        // Without a decodable envelope there is no id to answer under.
        Err(e) => {
            return Reply::err(
                "",
                WireError::new(ErrorCode::BadRequest, format!("undecodable request: {e}")),
            );
        }
    };

    match dispatch(host, running, interface, shape, &request).await {
        Ok(results) => Reply::ok(&request.id, results),
        Err(error) => Reply::err(&request.id, error),
    }
}

async fn dispatch(
    host: &Host,
    running: Running,
    interface: &InterfaceId,
    shape: &InterfaceShape,
    request: &Request,
) -> Result<Vec<wasm_protocol::MsgpackValue>, WireError> {
    if request.iface != interface.to_string() {
        return Err(WireError::new(
            ErrorCode::NotFound,
            format!(
                "this provider serves `{interface}`, not `{}`",
                request.iface
            ),
        ));
    }

    let function: &FunctionShape = shape
        .functions()
        .iter()
        .find(|candidate| candidate.name == request.func)
        .ok_or_else(|| {
            WireError::new(
                ErrorCode::NotFound,
                format!("`{interface}` has no function `{}`", request.func),
            )
        })?;

    let params = msgpack_to_vals(&request.args, &function.params).map_err(|e| {
        WireError::new(
            ErrorCode::BadRequest,
            format!("`{}` got unusable arguments: {e}", request.func),
        )
    })?;

    let mut results = vec![Val::Bool(false); function.results.len()];
    let call = host.call(running, interface, &request.func, &params, &mut results);

    match request.deadline_ms {
        Some(deadline) => tokio::time::timeout(Duration::from_millis(deadline), call)
            .await
            .map_err(|_| {
                WireError::new(
                    ErrorCode::DeadlineExceeded,
                    format!("`{}` did not finish within {deadline}ms", request.func),
                )
            })?,
        None => call.await,
    }
    .map_err(|e| {
        WireError::new(
            ErrorCode::Internal,
            format!("`{}` failed: {e}", request.func),
        )
    })?;

    vals_to_msgpack(&results, &function.results).map_err(|e| {
        WireError::new(
            ErrorCode::Internal,
            format!("`{}` returned unencodable results: {e}", request.func),
        )
    })
}
