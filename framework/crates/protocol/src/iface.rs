use std::fmt;
use std::str::FromStr;

use crate::ProtocolError;

/// A fully qualified WIT interface, e.g. `ardo314:math/vector3d@0.0.3`.
///
/// This is exactly the form wasmtime uses for component import and export
/// names, so it can be round-tripped through the linker without translation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceId {
    namespace: String,
    package: String,
    interface: String,
    version: semver::Version,
}

impl InterfaceId {
    pub fn new(
        namespace: impl Into<String>,
        package: impl Into<String>,
        interface: impl Into<String>,
        version: semver::Version,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            package: package.into(),
            interface: interface.into(),
            version,
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn package(&self) -> &str {
        &self.package
    }

    pub fn interface(&self) -> &str {
        &self.interface
    }

    pub fn version(&self) -> &semver::Version {
        &self.version
    }
}

impl FromStr for InterfaceId {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let malformed =
            |why: &'static str| -> ProtocolError { ProtocolError::MalformedInterfaceId(s.to_owned(), why) };

        let (package_part, rest) = s.split_once('/').ok_or_else(|| malformed("expected `/`"))?;
        let (namespace, package) = package_part
            .split_once(':')
            .ok_or_else(|| malformed("expected `:` before `/`"))?;
        let (interface, version) = rest
            .split_once('@')
            .ok_or_else(|| malformed("expected `@` and a version"))?;

        if namespace.is_empty() || package.is_empty() || interface.is_empty() {
            return Err(malformed("namespace, package and interface must be non-empty"));
        }

        let version = semver::Version::parse(version)
            .map_err(|e| ProtocolError::InvalidVersion(s.to_owned(), e))?;

        Ok(Self::new(namespace, package, interface, version))
    }
}

impl fmt::Display for InterfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}/{}@{}",
            self.namespace, self.package, self.interface, self.version
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_round_trips() {
        let id: InterfaceId = "ardo314:math/vector3d@0.0.3".parse().unwrap();
        assert_eq!(id.namespace(), "ardo314");
        assert_eq!(id.package(), "math");
        assert_eq!(id.interface(), "vector3d");
        assert_eq!(id.version(), &semver::Version::new(0, 0, 3));
        assert_eq!(id.to_string(), "ardo314:math/vector3d@0.0.3");
    }

    #[test]
    fn keeps_prerelease_and_build_metadata() {
        let text = "a:b/c@1.0.0-rc.1+build.5";
        let id: InterfaceId = text.parse().unwrap();
        assert_eq!(id.to_string(), text);
    }

    #[test]
    fn rejects_missing_parts() {
        for bad in [
            "ardo314:math/vector3d",
            "ardo314/vector3d@0.0.3",
            "ardo314:math@0.0.3",
            ":math/vector3d@0.0.3",
            "ardo314:math/vector3d@not-a-version",
        ] {
            assert!(bad.parse::<InterfaceId>().is_err(), "expected `{bad}` to fail");
        }
    }
}
