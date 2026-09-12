//! The other end of [`crate::serve`]: how a host finds providers and how a
//! provider announces itself.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use async_nats::jetstream::kv;
use futures::StreamExt;
use rmpv::Value;
use semver::{Version, VersionReq};
use tokio::task::JoinHandle;
use wasm_protocol::{ErrorCode, InterfaceId, Reply, Request, Subject, WireError};

use crate::types::{InterfaceRef, Provider, RegistryError};
use crate::{DISCOVERY_INTERFACE, REGISTRATION_INTERFACE, wire};

type Result<T> = std::result::Result<T, RegistryError>;

/// How long a registry call may take before it counts as unavailable.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// A connection to the registry. Cheap to clone; clones share one cache.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    nats: async_nats::Client,
    cache: Mutex<Cache>,
    calls: AtomicU64,
    watcher: OnceLock<JoinHandle<()>>,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<(String, String), Vec<Resolution>>,
    /// Bumped on every invalidation, so a resolution that raced one is dropped
    /// rather than stored stale.
    generation: u64,
    /// Set once nothing is invalidating the cache any more.
    stopped: bool,
}

/// A provider, and the exact interface version to address it at: subjects
/// encode a version, and a requirement is not a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub provider: Provider,
    pub interface: InterfaceId,
    pub shape_digest: String,
}

impl Client {
    /// Connects to the registry, invalidating cached resolutions from `bucket`.
    ///
    /// The bucket must already exist, which it does once a registryd has
    /// started; a client that could not watch it would cache forever.
    pub async fn connect(nats: async_nats::Client, bucket: &str) -> Result<Self> {
        let jetstream = async_nats::jetstream::new(nats.clone());
        let bucket = jetstream
            .get_key_value(bucket)
            .await
            .map_err(RegistryError::unavailable)?;
        let changes = bucket
            .watch_all()
            .await
            .map_err(RegistryError::unavailable)?;

        let inner = Arc::new(Inner {
            nats,
            cache: Mutex::new(Cache::default()),
            calls: AtomicU64::new(0),
            watcher: OnceLock::new(),
        });
        let watcher = tokio::spawn(invalidate(Arc::downgrade(&inner), changes));
        let _ = inner.watcher.set(watcher);
        Ok(Self { inner })
    }

    /// Every live provider of `name` at a version satisfying `version_req`.
    ///
    /// Answers from the cache until the bucket says something changed. Neither
    /// answer is proof a provider is alive — see `docs/spec/registry.md` §4.
    pub async fn resolve(&self, name: &str, version_req: &str) -> Result<Vec<Resolution>> {
        let wanted = VersionReq::parse(version_req).map_err(|e| {
            RegistryError::invalid(format!("`{version_req}` is not a version requirement: {e}"))
        })?;

        let key = (name.to_owned(), version_req.to_owned());
        let generation = {
            let cache = self.inner.cache.lock().unwrap();
            if let Some(resolved) = cache.entries.get(&key) {
                return Ok(resolved.clone());
            }
            cache.generation
        };

        let providers = self
            .call(
                DISCOVERY_INTERFACE,
                "resolve",
                vec![Value::from(name), Value::from(version_req)],
            )
            .await?;
        let resolved: Vec<Resolution> = wire::array(&providers, "resolve")?
            .iter()
            .map(Provider::from_value)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|provider| Resolution::pick(provider, name, &wanted))
            .collect();

        let mut cache = self.inner.cache.lock().unwrap();
        if !cache.stopped && cache.generation == generation {
            cache.entries.insert(key, resolved.clone());
        }
        Ok(resolved)
    }

    /// Every interface some live provider claims. Not cached: it is a
    /// diagnostic, not a hot path.
    pub async fn list_interfaces(&self) -> Result<Vec<InterfaceRef>> {
        let interfaces = self
            .call(DISCOVERY_INTERFACE, "list-interfaces", vec![])
            .await?;
        wire::array(&interfaces, "list-interfaces")?
            .iter()
            .map(InterfaceRef::from_value)
            .collect()
    }

    /// Announces `provider` and keeps it alive until the returned registration
    /// is dropped.
    pub async fn register(&self, provider: Provider) -> Result<Registration> {
        self.announce(&provider).await?;

        let provider = Arc::new(provider);
        let heartbeat = tokio::spawn(beat(self.clone(), Arc::clone(&provider)));
        Ok(Registration {
            client: self.clone(),
            provider,
            heartbeat,
        })
    }

    /// Removes a registration this client does not hold a [`Registration`] for.
    pub async fn deregister(&self, id: &str) -> Result<()> {
        self.call(REGISTRATION_INTERFACE, "deregister", vec![Value::from(id)])
            .await
            .map(drop)
    }

    async fn announce(&self, provider: &Provider) -> Result<()> {
        self.call(
            REGISTRATION_INTERFACE,
            "register",
            vec![provider.to_value()],
        )
        .await
        .map(drop)
    }

    async fn heartbeat(&self, id: &str) -> Result<()> {
        self.call(REGISTRATION_INTERFACE, "heartbeat", vec![Value::from(id)])
            .await
            .map(drop)
    }

    /// One request/reply, unwrapped down to the WIT `result`'s `ok` arm.
    async fn call(&self, iface: &str, func: &str, args: Vec<Value>) -> Result<Value> {
        let interface: InterfaceId = iface
            .parse()
            .map_err(|e| RegistryError::invalid(format!("`{iface}` is unaddressable: {e}")))?;
        let id = format!(
            "{func}-{}",
            self.inner.calls.fetch_add(1, Ordering::Relaxed)
        );
        let payload = Request::new(id, iface, func, args)
            .encode()
            .map_err(|e| RegistryError::invalid(format!("unencodable request: {e}")))?;

        let reply = tokio::time::timeout(
            CALL_TIMEOUT,
            self.inner
                .nats
                .request(Subject::new(interface, func).to_string(), payload.into()),
        )
        .await
        .map_err(|_| RegistryError::unavailable("the registry did not answer"))?
        .map_err(RegistryError::unavailable)?;

        let values = Reply::decode(&reply.payload)
            .map_err(|e| RegistryError::unavailable(format!("undecodable reply: {e}")))?
            .into_result()
            .map_err(transport_error)?;
        let [value] = values.as_slice() else {
            return Err(RegistryError::unavailable(format!(
                "`{func}` returned {} values, expected one",
                values.len()
            )));
        };
        wire::result(value)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(watcher) = self.watcher.get() {
            watcher.abort();
        }
    }
}

/// A live registration, heartbeating in the background until it is dropped.
pub struct Registration {
    client: Client,
    provider: Arc<Provider>,
    heartbeat: JoinHandle<()>,
}

impl Registration {
    pub fn id(&self) -> &str {
        &self.provider.id
    }

    /// Removes the entry now instead of leaving it to expire.
    pub async fn deregister(self) -> Result<()> {
        self.client.deregister(&self.provider.id).await
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.heartbeat.abort();
    }
}

impl Resolution {
    /// The highest version of `name` this provider offers that satisfies
    /// `wanted`, or nothing if it offers none.
    fn pick(provider: Provider, name: &str, wanted: &VersionReq) -> Option<Self> {
        let interface = provider
            .interfaces
            .iter()
            .filter(|interface| interface.name == name)
            .filter_map(|interface| {
                let version = Version::parse(&interface.version).ok()?;
                wanted.matches(&version).then_some((version, interface))
            })
            .max_by(|(a, _), (b, _)| a.cmp(b))?;

        let (version, interface) = interface;
        Some(Self {
            interface: format!("{name}@{version}").parse().ok()?,
            shape_digest: interface.shape_digest.clone(),
            provider,
        })
    }
}

/// Renews `provider` for as long as the task lives, re-announcing it if the
/// registry has forgotten it — which `docs/spec/registry.md` §3 makes routine.
async fn beat(client: Client, provider: Arc<Provider>) {
    let mut ticker = tokio::time::interval(heartbeat_interval(provider.ttl_secs));
    ticker.tick().await;
    loop {
        ticker.tick().await;
        if let Err(RegistryError::NotFound) = client.heartbeat(&provider.id).await {
            let _ = client.announce(&provider).await;
        }
    }
}

/// Two consecutive losses must not expire a healthy provider.
fn heartbeat_interval(ttl_secs: u32) -> Duration {
    Duration::from_secs(u64::from(ttl_secs / 3).max(1))
}

/// Drops every cached resolution whenever the bucket changes. A delete carries
/// no payload, so there is no telling which interfaces an entry held.
async fn invalidate(inner: Weak<Inner>, mut changes: kv::Watch) {
    while changes.next().await.is_some() {
        let Some(inner) = inner.upgrade() else { return };
        let mut cache = inner.cache.lock().unwrap();
        cache.entries.clear();
        cache.generation += 1;
    }

    if let Some(inner) = inner.upgrade() {
        let mut cache = inner.cache.lock().unwrap();
        cache.entries.clear();
        cache.stopped = true;
    }
}

fn transport_error(error: WireError) -> RegistryError {
    match error.code {
        ErrorCode::NotFound => RegistryError::NotFound,
        ErrorCode::BadRequest => RegistryError::Invalid(error.message),
        _ => RegistryError::Unavailable(error.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Endpoint, ProviderKind};

    fn provider(versions: &[&str]) -> Provider {
        Provider {
            id: "math-1".into(),
            kind: ProviderKind::Service,
            interfaces: versions
                .iter()
                .map(|version| InterfaceRef {
                    name: "ardo314:math/vector3d".into(),
                    version: (*version).into(),
                    shape_digest: "a".repeat(64),
                })
                .collect(),
            endpoint: Endpoint::Nats("wit.ardo314.math.0_0_3.vector3d.*".into()),
            ttl_secs: 30,
        }
    }

    fn pick(versions: &[&str], req: &str) -> Option<String> {
        Resolution::pick(
            provider(versions),
            "ardo314:math/vector3d",
            &VersionReq::parse(req).unwrap(),
        )
        .map(|resolved| resolved.interface.version().to_string())
    }

    #[test]
    fn resolves_a_requirement_to_the_highest_version_satisfying_it() {
        assert_eq!(
            pick(&["1.1.0", "1.3.0", "1.2.0"], "^1.1"),
            Some("1.3.0".into())
        );
        assert_eq!(pick(&["1.1.0", "2.0.0"], "=1.1.0"), Some("1.1.0".into()));
        // A 0.0.x release is compatible with nothing but itself.
        assert_eq!(
            pick(&["0.0.3", "0.0.5", "0.0.4"], "^0.0.3"),
            Some("0.0.3".into())
        );
    }

    #[test]
    fn a_provider_of_another_interface_is_no_resolution() {
        assert_eq!(pick(&["1.0.0"], "^2"), None);
        assert!(
            Resolution::pick(
                provider(&["1.0.0"]),
                "ardo314:math/vector2d",
                &VersionReq::STAR
            )
            .is_none()
        );
    }

    #[test]
    fn heartbeats_leave_room_for_two_losses() {
        assert_eq!(heartbeat_interval(30), Duration::from_secs(10));
        assert_eq!(heartbeat_interval(1), Duration::from_secs(1));
    }
}
