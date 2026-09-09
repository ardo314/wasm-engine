use std::fmt;
use std::str::FromStr;

use crate::{InterfaceId, ProtocolError};

const PREFIX: &str = "wit";

/// The NATS subject a single WIT function is addressed by.
///
/// `ardo314:math/vector3d@0.0.3` function `add-f32` becomes
/// `wit.ardo314.math.0_0_3.vector3d.add-f32`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Subject {
    interface: InterfaceId,
    function: String,
}

impl Subject {
    pub fn new(interface: InterfaceId, function: impl Into<String>) -> Self {
        Self {
            interface,
            function: function.into(),
        }
    }

    pub fn interface(&self) -> &InterfaceId {
        &self.interface
    }

    pub fn function(&self) -> &str {
        &self.function
    }

    /// Wildcard subject matching every function of an interface, for providers
    /// that dispatch on the trailing token themselves.
    pub fn interface_wildcard(interface: &InterfaceId) -> String {
        format!(
            "{PREFIX}.{}.{}.{}.{}.*",
            interface.namespace(),
            interface.package(),
            encode_version(interface.version()),
            interface.interface(),
        )
    }

    /// Queue group shared by all replicas serving the same interface.
    pub fn queue_group(interface: &InterfaceId) -> String {
        interface.to_string()
    }
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{PREFIX}.{}.{}.{}.{}.{}",
            self.interface.namespace(),
            self.interface.package(),
            encode_version(self.interface.version()),
            self.interface.interface(),
            self.function,
        )
    }
}

impl FromStr for Subject {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let malformed =
            |why: &'static str| -> ProtocolError { ProtocolError::MalformedSubject(s.to_owned(), why) };

        let parts: Vec<&str> = s.split('.').collect();
        let [prefix, namespace, package, version, interface, function] = parts.as_slice() else {
            return Err(malformed("expected exactly 6 dot-separated tokens"));
        };
        if *prefix != PREFIX {
            return Err(malformed("expected the `wit` prefix"));
        }

        let version = semver::Version::parse(&decode_version(version))
            .map_err(|e| ProtocolError::InvalidVersion(s.to_owned(), e))?;

        Ok(Self::new(
            InterfaceId::new(*namespace, *package, *interface, version),
            *function,
        ))
    }
}

/// `.` is the NATS subject separator, so versions carry `_` instead. Semver
/// identifiers are `[0-9A-Za-z-]`, so this substitution is reversible.
fn encode_version(version: &semver::Version) -> String {
    version.to_string().replace('.', "_")
}

fn decode_version(encoded: &str) -> String {
    encoded.replace('_', ".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(text: &str) -> InterfaceId {
        text.parse().unwrap()
    }

    #[test]
    fn builds_the_documented_subject() {
        let subject = Subject::new(iface("ardo314:math/vector3d@0.0.3"), "add-f32");
        assert_eq!(subject.to_string(), "wit.ardo314.math.0_0_3.vector3d.add-f32");
    }

    #[test]
    fn round_trips_through_parsing() {
        for text in [
            "wit.ardo314.math.0_0_3.vector3d.add-f32",
            "wit.a.b.1_0_0-rc_1.c.d",
            "wit.a.b.2_1_0+build_5.c.d",
        ] {
            let parsed: Subject = text.parse().unwrap();
            assert_eq!(parsed.to_string(), text);
        }
    }

    #[test]
    fn wildcard_and_queue_group() {
        let id = iface("ardo314:math/vector3d@0.0.3");
        assert_eq!(
            Subject::interface_wildcard(&id),
            "wit.ardo314.math.0_0_3.vector3d.*"
        );
        assert_eq!(Subject::queue_group(&id), "ardo314:math/vector3d@0.0.3");
    }

    #[test]
    fn rejects_wrong_shape() {
        for bad in [
            "ardo314.math.0_0_3.vector3d.add",
            "wit.ardo314.math.0_0_3.vector3d",
            "wit.ardo314.math.0_0_3.vector3d.add.extra",
            "wit.ardo314.math.nope.vector3d.add",
        ] {
            assert!(bad.parse::<Subject>().is_err(), "expected `{bad}` to fail");
        }
    }
}
