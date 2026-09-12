//! Satisfying an import over NATS.
//!
//! Each function of the interface becomes a host function that encodes its
//! arguments per `docs/spec/wire-protocol.md`, does a request/reply, and lifts
//! the answer back into the guest's results.
//!
//! Wasmtime runs guest code on a fiber, so the `await` on the round trip
//! suspends the calling guest rather than the host thread. It does, however,
//! hold the store for the duration — see `docs/spec/linking.md` §2.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_nats::client::RequestErrorKind;
use wasm_protocol::{
    ErrorCode, InterfaceId, InterfaceShape, Reply, Request, Subject, WireError, WitType,
    msgpack_to_vals, vals_to_msgpack,
};
use wasmtime::component::{Linker, Val};

/// How long a call may take before it is abandoned, when the caller does not
/// say otherwise.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

/// Pause before the one retry. Without it the retry lands in the same instant
/// as the first attempt and cannot catch a provider that is still subscribing.
const RETRY_DELAY: Duration = Duration::from_millis(50);

/// Turns calls on an imported interface into NATS request/reply.
#[derive(Clone)]
pub struct Proxy {
    inner: Arc<Inner>,
}

struct Inner {
    nats: async_nats::Client,
    deadline: Duration,
    calls: AtomicU64,
}

impl Proxy {
    pub fn new(nats: async_nats::Client) -> Self {
        Self::with_deadline(nats, DEFAULT_DEADLINE)
    }

    pub fn with_deadline(nats: async_nats::Client, deadline: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                nats,
                deadline,
                calls: AtomicU64::new(0),
            }),
        }
    }

    /// Defines every function in `shape` on `linker` as a call to whoever is
    /// serving `interface`.
    ///
    /// `shape` carries the types each call is encoded against, which is why an
    /// interface with no wire shape cannot be satisfied this way.
    pub fn define<T: Send + 'static>(
        &self,
        linker: &mut Linker<T>,
        interface: &InterfaceId,
        shape: &InterfaceShape,
    ) -> Result<(), LinkError> {
        let mut instance = linker
            .instance(&interface.to_string())
            .map_err(LinkError::Wasmtime)?;

        for function in shape.functions() {
            let call = Call {
                proxy: self.clone(),
                subject: Subject::new(interface.clone(), &function.name).to_string(),
                iface: interface.to_string(),
                function: function.name.clone(),
                params: function.params.clone(),
                results: function.results.clone(),
            };

            instance
                .func_new_async(&function.name, move |_store, _ty, params, results| {
                    let call = call.clone();
                    Box::new(async move { call.invoke(params, results).await })
                })
                .map_err(LinkError::Wasmtime)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Call {
    proxy: Proxy,
    subject: String,
    iface: String,
    function: String,
    params: Vec<WitType>,
    results: Vec<WitType>,
}

impl Call {
    async fn invoke(&self, params: &[Val], results: &mut [Val]) -> wasmtime::Result<()> {
        let args = vals_to_msgpack(params, &self.params)?;

        let deadline = self.proxy.inner.deadline;
        let id = format!(
            "{}-{}",
            self.function,
            self.proxy.inner.calls.fetch_add(1, Ordering::Relaxed)
        );
        let payload = Request::new(&id, &self.iface, &self.function, args)
            .with_deadline_ms(deadline.as_millis() as u64)
            .encode()?;

        let reply = self.round_trip(payload, deadline).await?;
        let values = Reply::decode(&reply)?.into_result()?;

        let lifted = msgpack_to_vals(&values, &self.results)?;
        if lifted.len() != results.len() {
            return Err(WireError::new(
                ErrorCode::BadRequest,
                format!(
                    "`{}` returned {} values, expected {}",
                    self.function,
                    lifted.len(),
                    results.len()
                ),
            )
            .into());
        }
        results.clone_from_slice(&lifted);
        Ok(())
    }

    /// One request, retried once if nobody was listening.
    ///
    /// No-responders is routinely a provider that died between resolution and
    /// the call, or one that has not finished subscribing —
    /// `docs/spec/registry.md` §4. Anything else is reported as it happened.
    async fn round_trip(&self, payload: Vec<u8>, deadline: Duration) -> Result<Vec<u8>, WireError> {
        let started = tokio::time::Instant::now();

        for attempt in 0..2 {
            let left = deadline.saturating_sub(started.elapsed());
            if left.is_zero() {
                break;
            }

            let request = self
                .proxy
                .inner
                .nats
                .request(self.subject.clone(), payload.clone().into());

            match tokio::time::timeout(left, request).await {
                Ok(Ok(message)) => return Ok(message.payload.to_vec()),
                Ok(Err(e)) if e.kind() == RequestErrorKind::NoResponders && attempt == 0 => {
                    tokio::time::sleep(RETRY_DELAY.min(left)).await;
                    continue;
                }
                Ok(Err(e)) if e.kind() == RequestErrorKind::NoResponders => {
                    return Err(WireError::new(
                        ErrorCode::NotFound,
                        format!("nobody is serving `{}`", self.subject),
                    ));
                }
                Ok(Err(e)) => {
                    return Err(WireError::new(
                        ErrorCode::Internal,
                        format!("`{}` failed: {e}", self.subject),
                    ));
                }
                Err(_) => break,
            }
        }

        Err(WireError::new(
            ErrorCode::DeadlineExceeded,
            format!(
                "`{}` did not answer within {}ms",
                self.subject,
                deadline.as_millis()
            ),
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("{0:#}")]
    Wasmtime(wasmtime::Error),
}
