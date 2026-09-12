//! MessagePack encoding of the registry types, per `docs/spec/wire-protocol.md` §4.
//!
//! Records are maps keyed by the WIT field name verbatim, variants are
//! single-entry maps keyed by the case name, enums are the bare case name.

use rmpv::Value;

use crate::types::{ArtifactRef, Endpoint, InterfaceRef, Provider, ProviderKind, RegistryError};

type Result<T> = std::result::Result<T, RegistryError>;

pub(crate) fn record(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Map(
        fields
            .into_iter()
            .map(|(name, value)| (Value::from(name), value))
            .collect(),
    )
}

pub(crate) fn variant(case: &str, payload: Value) -> Value {
    Value::Map(vec![(Value::from(case), payload)])
}

/// Splits a WIT `result<T, registry-error>` into the arm it carries.
pub(crate) fn result(value: &Value) -> Result<Value> {
    let (case, payload) = case(value, "result")?;
    match case.as_str() {
        "ok" => Ok(payload.clone()),
        "err" => Err(RegistryError::from_value(payload)?),
        other => Err(RegistryError::invalid(format!(
            "`{other}` is not a result case"
        ))),
    }
}

pub(crate) fn encode(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    rmpv::encode::write_value(&mut bytes, value).expect("writing to a Vec cannot fail");
    bytes
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Value> {
    rmpv::decode::read_value(&mut &bytes[..])
        .map_err(|e| RegistryError::invalid(format!("undecodable msgpack: {e}")))
}

impl Provider {
    pub fn to_value(&self) -> Value {
        record([
            ("id", Value::from(self.id.as_str())),
            ("kind", self.kind.to_value()),
            (
                "interfaces",
                Value::Array(self.interfaces.iter().map(InterfaceRef::to_value).collect()),
            ),
            ("endpoint", self.endpoint.to_value()),
            ("ttl-secs", Value::from(self.ttl_secs)),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let fields = map(value, "provider")?;
        Ok(Self {
            id: string(field(fields, "id", "provider")?, "provider.id")?,
            kind: ProviderKind::from_value(field(fields, "kind", "provider")?)?,
            interfaces: array(
                field(fields, "interfaces", "provider")?,
                "provider.interfaces",
            )?
            .iter()
            .map(InterfaceRef::from_value)
            .collect::<Result<_>>()?,
            endpoint: Endpoint::from_value(field(fields, "endpoint", "provider")?)?,
            ttl_secs: u32_of(field(fields, "ttl-secs", "provider")?, "provider.ttl-secs")?,
        })
    }
}

impl ProviderKind {
    pub fn to_value(self) -> Value {
        Value::from(match self {
            Self::Component => "component",
            Self::Service => "service",
        })
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        match string(value, "provider.kind")?.as_str() {
            "component" => Ok(Self::Component),
            "service" => Ok(Self::Service),
            other => Err(RegistryError::invalid(format!(
                "`{other}` is not a provider-kind"
            ))),
        }
    }
}

impl InterfaceRef {
    pub fn to_value(&self) -> Value {
        record([
            ("name", Value::from(self.name.as_str())),
            ("version", Value::from(self.version.as_str())),
            ("shape-digest", Value::from(self.shape_digest.as_str())),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let fields = map(value, "interface-ref")?;
        Ok(Self {
            name: string(
                field(fields, "name", "interface-ref")?,
                "interface-ref.name",
            )?,
            version: string(
                field(fields, "version", "interface-ref")?,
                "interface-ref.version",
            )?,
            shape_digest: string(
                field(fields, "shape-digest", "interface-ref")?,
                "interface-ref.shape-digest",
            )?,
        })
    }
}

impl Endpoint {
    pub fn to_value(&self) -> Value {
        match self {
            Self::Nats(subject) => variant("nats", Value::from(subject.as_str())),
            Self::Artifact(artifact) => variant("artifact", artifact.to_value()),
        }
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let (case, payload) = case(value, "endpoint")?;
        match case.as_str() {
            "nats" => Ok(Self::Nats(string(payload, "endpoint.nats")?)),
            "artifact" => Ok(Self::Artifact(ArtifactRef::from_value(payload)?)),
            other => Err(RegistryError::invalid(format!(
                "`{other}` is not an endpoint case"
            ))),
        }
    }
}

impl ArtifactRef {
    pub fn to_value(&self) -> Value {
        record([
            ("uri", Value::from(self.uri.as_str())),
            ("sha256", Value::from(self.sha256.as_str())),
        ])
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let fields = map(value, "artifact-ref")?;
        Ok(Self {
            uri: string(field(fields, "uri", "artifact-ref")?, "artifact-ref.uri")?,
            sha256: string(
                field(fields, "sha256", "artifact-ref")?,
                "artifact-ref.sha256",
            )?,
        })
    }
}

impl RegistryError {
    pub fn to_value(&self) -> Value {
        match self {
            Self::Conflict(why) => variant("conflict", Value::from(why.as_str())),
            Self::NotFound => variant("not-found", Value::Nil),
            Self::Invalid(why) => variant("invalid", Value::from(why.as_str())),
            Self::Unavailable(why) => variant("unavailable", Value::from(why.as_str())),
        }
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let (case, payload) = case(value, "registry-error")?;
        match case.as_str() {
            "conflict" => Ok(Self::Conflict(string(payload, "registry-error.conflict")?)),
            "not-found" => Ok(Self::NotFound),
            "invalid" => Ok(Self::Invalid(string(payload, "registry-error.invalid")?)),
            "unavailable" => Ok(Self::Unavailable(string(
                payload,
                "registry-error.unavailable",
            )?)),
            other => Err(RegistryError::invalid(format!(
                "`{other}` is not a registry-error case"
            ))),
        }
    }
}

fn map<'a>(value: &'a Value, what: &str) -> Result<&'a [(Value, Value)]> {
    value
        .as_map()
        .map(Vec::as_slice)
        .ok_or_else(|| RegistryError::invalid(format!("{what} must be a map")))
}

pub(crate) fn array<'a>(value: &'a Value, what: &str) -> Result<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| RegistryError::invalid(format!("{what} must be an array")))
}

fn field<'a>(fields: &'a [(Value, Value)], name: &str, what: &str) -> Result<&'a Value> {
    fields
        .iter()
        .find(|(key, _)| key.as_str() == Some(name))
        .map(|(_, value)| value)
        .ok_or_else(|| RegistryError::invalid(format!("{what} is missing `{name}`")))
}

fn case<'a>(value: &'a Value, what: &str) -> Result<(String, &'a Value)> {
    match map(value, what)? {
        [(case, payload)] => Ok((string(case, what)?, payload)),
        _ => Err(RegistryError::invalid(format!(
            "{what} must be a map of exactly one case"
        ))),
    }
}

pub(crate) fn string(value: &Value, what: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| RegistryError::invalid(format!("{what} must be a string")))
}

fn u32_of(value: &Value, what: &str) -> Result<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| RegistryError::invalid(format!("{what} must fit in a u32")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Provider {
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
    fn provider_round_trips() {
        for provider in [
            service(),
            Provider {
                kind: ProviderKind::Component,
                endpoint: Endpoint::Artifact(ArtifactRef {
                    uri: "oci://example.invalid/math:0.0.3".into(),
                    sha256: "b".repeat(64),
                }),
                ..service()
            },
        ] {
            let encoded = encode(&provider.to_value());
            let decoded = Provider::from_value(&decode(&encoded).unwrap()).unwrap();
            assert_eq!(decoded, provider);
        }
    }

    #[test]
    fn errors_round_trip() {
        for error in [
            RegistryError::Conflict("digests disagree".into()),
            RegistryError::NotFound,
            RegistryError::Invalid("bad version".into()),
            RegistryError::Unavailable("bucket is gone".into()),
        ] {
            assert_eq!(RegistryError::from_value(&error.to_value()).unwrap(), error);
        }
    }

    #[test]
    fn records_are_maps_keyed_by_the_wit_field_name() {
        let value = service().to_value();
        let fields = value.as_map().unwrap();
        let names: Vec<&str> = fields.iter().map(|(k, _)| k.as_str().unwrap()).collect();
        assert_eq!(names, ["id", "kind", "interfaces", "endpoint", "ttl-secs"]);
    }

    #[test]
    fn enums_encode_as_the_bare_case_name() {
        assert_eq!(ProviderKind::Component.to_value(), Value::from("component"));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let mut fields = service().to_value().as_map().unwrap().clone();
        fields.push((Value::from("invented-later"), Value::from(7)));
        assert_eq!(
            Provider::from_value(&Value::Map(fields)).unwrap(),
            service()
        );
    }

    #[test]
    fn missing_fields_are_rejected() {
        let fields: Vec<_> = service()
            .to_value()
            .as_map()
            .unwrap()
            .iter()
            .filter(|(key, _)| key.as_str() != Some("endpoint"))
            .cloned()
            .collect();
        assert!(matches!(
            Provider::from_value(&Value::Map(fields)),
            Err(RegistryError::Invalid(_))
        ));
    }
}
