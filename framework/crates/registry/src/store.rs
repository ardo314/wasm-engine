//! Provider registrations, persisted in a JetStream key-value bucket.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_nats::jetstream::{self, kv};
use futures::TryStreamExt;
use rmpv::Value;
use semver::{Version, VersionReq};

use crate::types::{Endpoint, InterfaceRef, Provider, ProviderKind, RegistryError};
use crate::wire;

type Result<T> = std::result::Result<T, RegistryError>;

/// Bounds the registry imposes on the TTL a provider proposes.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub min_ttl: Duration,
    pub max_ttl: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            min_ttl: Duration::from_secs(5),
            max_ttl: Duration::from_secs(3600),
        }
    }
}

impl Limits {
    fn clamp(&self, ttl_secs: u32) -> u32 {
        let min = self.min_ttl.as_secs().max(1);
        let max = self.max_ttl.as_secs().max(min);
        u64::from(ttl_secs).clamp(min, max) as u32
    }
}

/// The registry's state. Cheap to clone; all clones share one bucket.
#[derive(Clone)]
pub struct Registry {
    bucket: kv::Store,
    limits: Limits,
}

impl Registry {
    /// Opens the bucket, creating it if this is the first registryd to start.
    ///
    /// The bucket's `max_age` is a garbage collector, not the expiry mechanism:
    /// entries carry their own deadline so that each provider gets the TTL it
    /// asked for rather than the bucket's.
    pub async fn open(
        jetstream: &jetstream::Context,
        bucket: &str,
        limits: Limits,
    ) -> anyhow::Result<Self> {
        let bucket = match jetstream.get_key_value(bucket).await {
            Ok(bucket) => bucket,
            Err(_) => {
                jetstream
                    .create_key_value(kv::Config {
                        bucket: bucket.to_owned(),
                        description: "wit interface providers".to_owned(),
                        history: 1,
                        max_age: limits.max_ttl,
                        ..Default::default()
                    })
                    .await?
            }
        };
        Ok(Self { bucket, limits })
    }

    /// Creates or replaces the entry for `provider.id`.
    pub async fn register(&self, provider: Provider) -> Result<()> {
        let provider = validate(provider, &self.limits)?;
        self.reject_conflicts(&provider).await?;
        self.write(Registration {
            expires_at_ms: now_ms() + u64::from(provider.ttl_secs) * 1_000,
            provider,
        })
        .await
    }

    pub async fn deregister(&self, id: &str) -> Result<()> {
        let key = key(id)?;
        if self.live(&key).await?.is_none() {
            return Err(RegistryError::NotFound);
        }
        self.bucket
            .delete(&key)
            .await
            .map_err(RegistryError::unavailable)
    }

    /// Extends the entry by its TTL. Unknown or already expired ids are
    /// `not-found`: the provider is expected to register again.
    pub async fn heartbeat(&self, id: &str) -> Result<()> {
        let key = key(id)?;
        let mut registration = self.live(&key).await?.ok_or(RegistryError::NotFound)?;
        registration.expires_at_ms = now_ms() + u64::from(registration.provider.ttl_secs) * 1_000;
        self.write(registration).await
    }

    /// Every live provider of `name` at a version satisfying `version_req`.
    ///
    /// An empty list is a success: the question was answered, and the answer
    /// was "nobody".
    pub async fn resolve(&self, name: &str, version_req: &str) -> Result<Vec<Provider>> {
        let wanted = VersionReq::parse(version_req).map_err(|e| {
            RegistryError::invalid(format!("`{version_req}` is not a version requirement: {e}"))
        })?;

        let mut providers: Vec<Provider> = self
            .live_registrations()
            .await?
            .into_iter()
            .map(|registration| registration.provider)
            .filter(|provider| {
                provider.interfaces.iter().any(|interface| {
                    interface.name == name
                        && Version::parse(&interface.version)
                            .is_ok_and(|version| wanted.matches(&version))
                })
            })
            .collect();
        providers.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(providers)
    }

    /// Every interface some live provider claims, deduplicated.
    pub async fn list_interfaces(&self) -> Result<Vec<InterfaceRef>> {
        let mut interfaces: Vec<InterfaceRef> = self
            .live_registrations()
            .await?
            .into_iter()
            .flat_map(|registration| registration.provider.interfaces)
            .collect();
        interfaces.sort();
        interfaces.dedup();
        Ok(interfaces)
    }

    /// Two providers may not claim one interface and version with differing
    /// shape digests: the definitions disagree, and a host picking either would
    /// fail unpredictably at the first call touching the differing type.
    async fn reject_conflicts(&self, provider: &Provider) -> Result<()> {
        let mut claimed: HashMap<(&str, &str), (&str, &str)> = HashMap::new();
        let others = self.live_registrations().await?;
        for other in others.iter().filter(|o| o.provider.id != provider.id) {
            for interface in &other.provider.interfaces {
                claimed.insert(
                    (&interface.name, &interface.version),
                    (&interface.shape_digest, &other.provider.id),
                );
            }
        }

        for interface in &provider.interfaces {
            let key = (interface.name.as_str(), interface.version.as_str());
            if let Some((digest, owner)) = claimed.get(&key)
                && *digest != interface.shape_digest
            {
                return Err(RegistryError::Conflict(format!(
                    "`{}@{}` is already registered by `{owner}` with shape-digest {digest}",
                    interface.name, interface.version
                )));
            }
        }
        Ok(())
    }

    async fn write(&self, registration: Registration) -> Result<()> {
        let key = key(&registration.provider.id)?;
        self.bucket
            .put(&key, wire::encode(&registration.to_value()).into())
            .await
            .map(|_| ())
            .map_err(RegistryError::unavailable)
    }

    async fn live(&self, key: &str) -> Result<Option<Registration>> {
        let Some(bytes) = self
            .bucket
            .get(key)
            .await
            .map_err(RegistryError::unavailable)?
        else {
            return Ok(None);
        };
        let registration = Registration::from_value(&wire::decode(&bytes)?)?;
        Ok((registration.expires_at_ms > now_ms()).then_some(registration))
    }

    async fn live_registrations(&self) -> Result<Vec<Registration>> {
        let keys: Vec<String> = self
            .bucket
            .keys()
            .await
            .map_err(RegistryError::unavailable)?
            .try_collect()
            .await
            .map_err(RegistryError::unavailable)?;

        let mut registrations = Vec::with_capacity(keys.len());
        for key in keys {
            // An entry written by a newer registryd is skipped rather than
            // failing every resolve in the cluster.
            if let Ok(Some(registration)) = self.live(&key).await {
                registrations.push(registration);
            }
        }
        Ok(registrations)
    }
}

/// One bucket entry: a provider plus the wall-clock instant it stops counting.
struct Registration {
    provider: Provider,
    expires_at_ms: u64,
}

impl Registration {
    fn to_value(&self) -> Value {
        wire::record([
            ("provider", self.provider.to_value()),
            ("expires-at-ms", Value::from(self.expires_at_ms)),
        ])
    }

    fn from_value(value: &Value) -> Result<Self> {
        let fields = value
            .as_map()
            .ok_or_else(|| RegistryError::invalid("registration must be a map"))?;
        let lookup = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key.as_str() == Some(name))
                .map(|(_, value)| value)
                .ok_or_else(|| RegistryError::invalid(format!("registration is missing `{name}`")))
        };
        Ok(Self {
            provider: Provider::from_value(lookup("provider")?)?,
            expires_at_ms: lookup("expires-at-ms")?
                .as_u64()
                .ok_or_else(|| RegistryError::invalid("expires-at-ms must be an integer"))?,
        })
    }
}

/// Rejects registrations the registry cannot serve, and clamps the proposed TTL.
fn validate(provider: Provider, limits: &Limits) -> Result<Provider> {
    key(&provider.id)?;

    match (&provider.kind, &provider.endpoint) {
        (ProviderKind::Component, Endpoint::Artifact(_))
        | (ProviderKind::Service, Endpoint::Nats(_)) => {}
        (ProviderKind::Component, _) => {
            return Err(RegistryError::invalid(
                "a component provider must carry an artifact endpoint",
            ));
        }
        (ProviderKind::Service, _) => {
            return Err(RegistryError::invalid(
                "a service provider must carry a nats endpoint",
            ));
        }
    }

    if provider.interfaces.is_empty() {
        return Err(RegistryError::invalid(
            "a provider must claim at least one interface",
        ));
    }

    for interface in &provider.interfaces {
        // Name and version have to recombine into the form subjects are built
        // from, or the provider is unaddressable.
        format!("{}@{}", interface.name, interface.version)
            .parse::<wasm_protocol::InterfaceId>()
            .map_err(|e| {
                RegistryError::invalid(format!(
                    "`{}@{}` is not a fully qualified interface: {e}",
                    interface.name, interface.version
                ))
            })?;

        let hex = |b: u8| matches!(b, b'0'..=b'9' | b'a'..=b'f');
        if interface.shape_digest.len() != 64 || !interface.shape_digest.bytes().all(hex) {
            return Err(RegistryError::invalid(format!(
                "shape-digest of `{}` is not a lowercase hex sha256",
                interface.name
            )));
        }
    }

    Ok(Provider {
        ttl_secs: limits.clamp(provider.ttl_secs),
        ..provider
    })
}

/// Provider ids become bucket keys, so they are restricted to a charset that
/// cannot escape the key it is placed in.
fn key(id: &str) -> Result<String> {
    let legal = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
    if id.is_empty() || id.len() > 128 || !id.bytes().all(legal) {
        return Err(RegistryError::invalid(format!(
            "`{id}` is not a provider id: expected 1-128 characters of [A-Za-z0-9_-]"
        )));
    }
    Ok(id.to_owned())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ArtifactRef;

    fn limits() -> Limits {
        Limits {
            min_ttl: Duration::from_secs(5),
            max_ttl: Duration::from_secs(60),
        }
    }

    fn provider() -> Provider {
        Provider {
            id: "math-1".into(),
            kind: ProviderKind::Service,
            interfaces: vec![InterfaceRef {
                name: "ardo314:math/vector3d".into(),
                version: "0.0.3".into(),
                shape_digest: "a".repeat(64),
            }],
            endpoint: Endpoint::Nats("wit.ardo314.math.0_0_3.vector3d.*".into()),
            ttl_secs: 30,
        }
    }

    #[test]
    fn clamps_the_proposed_ttl() {
        assert_eq!(limits().clamp(1), 5);
        assert_eq!(limits().clamp(30), 30);
        assert_eq!(limits().clamp(u32::MAX), 60);

        let clamped = validate(
            Provider {
                ttl_secs: 1,
                ..provider()
            },
            &limits(),
        )
        .unwrap();
        assert_eq!(clamped.ttl_secs, 5);
    }

    #[test]
    fn rejects_ids_that_would_escape_their_key() {
        for id in ["", "a.b", "../other", "with space", &"x".repeat(129)] {
            assert!(key(id).is_err(), "expected `{id}` to be rejected");
        }
        assert_eq!(key("math-1_A").unwrap(), "math-1_A");
    }

    #[test]
    fn rejects_endpoints_that_contradict_the_kind() {
        let mismatched = Provider {
            kind: ProviderKind::Component,
            ..provider()
        };
        assert!(validate(mismatched, &limits()).is_err());

        let matched = Provider {
            kind: ProviderKind::Component,
            endpoint: Endpoint::Artifact(ArtifactRef {
                uri: "oci://example.invalid/math:0.0.3".into(),
                sha256: "b".repeat(64),
            }),
            ..provider()
        };
        assert!(validate(matched, &limits()).is_ok());
    }

    #[test]
    fn rejects_unaddressable_interfaces() {
        for (name, version) in [
            ("ardo314:math/vector3d", "not-a-version"),
            ("vector3d", "0.0.3"),
            ("ardo314:math", "0.0.3"),
        ] {
            let provider = Provider {
                interfaces: vec![InterfaceRef {
                    name: name.into(),
                    version: version.into(),
                    shape_digest: "a".repeat(64),
                }],
                ..provider()
            };
            assert!(
                validate(provider, &limits()).is_err(),
                "expected `{name}@{version}` to be rejected"
            );
        }
    }

    #[test]
    fn rejects_digests_that_are_not_a_lowercase_hex_sha256() {
        for digest in ["A".repeat(64), "a".repeat(63), "g".repeat(64)] {
            let provider = Provider {
                interfaces: vec![InterfaceRef {
                    shape_digest: digest.clone(),
                    ..provider().interfaces[0].clone()
                }],
                ..provider()
            };
            assert!(
                validate(provider, &limits()).is_err(),
                "expected `{digest}` to be rejected"
            );
        }
    }

    #[test]
    fn registration_round_trips() {
        let registration = Registration {
            provider: provider(),
            expires_at_ms: 1_700_000_000_000,
        };
        let decoded = Registration::from_value(&registration.to_value()).unwrap();
        assert_eq!(decoded.provider, registration.provider);
        assert_eq!(decoded.expires_at_ms, registration.expires_at_ms);
    }
}
