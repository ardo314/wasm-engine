use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ProtocolError, WireError};

/// Envelope version implemented by this crate.
pub const ENVELOPE_VERSION: u8 = 1;

/// A call travelling towards a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub v: u8,
    pub id: String,
    pub iface: String,
    pub func: String,
    pub args: Vec<rmpv::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trace: BTreeMap<String, String>,
}

impl Request {
    pub fn new(
        id: impl Into<String>,
        iface: impl Into<String>,
        func: impl Into<String>,
        args: Vec<rmpv::Value>,
    ) -> Self {
        Self {
            v: ENVELOPE_VERSION,
            id: id.into(),
            iface: iface.into(),
            func: func.into(),
            args,
            deadline_ms: None,
            trace: BTreeMap::new(),
        }
    }

    pub fn with_deadline_ms(mut self, deadline_ms: u64) -> Self {
        self.deadline_ms = Some(deadline_ms);
        self
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(rmp_serde::to_vec_named(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let request: Self = rmp_serde::from_slice(bytes)?;
        if request.v != ENVELOPE_VERSION {
            return Err(ProtocolError::UnsupportedEnvelopeVersion {
                found: request.v,
                expected: ENVELOPE_VERSION,
            });
        }
        Ok(request)
    }
}

/// The provider's answer. Exactly one of `ok` or `err` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub v: u8,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<Vec<rmpv::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<WireError>,
}

impl Reply {
    pub fn ok(id: impl Into<String>, results: Vec<rmpv::Value>) -> Self {
        Self {
            v: ENVELOPE_VERSION,
            id: id.into(),
            ok: Some(results),
            err: None,
        }
    }

    pub fn err(id: impl Into<String>, error: WireError) -> Self {
        Self {
            v: ENVELOPE_VERSION,
            id: id.into(),
            ok: None,
            err: Some(error),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(rmp_serde::to_vec_named(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let reply: Self = rmp_serde::from_slice(bytes)?;
        if reply.v != ENVELOPE_VERSION {
            return Err(ProtocolError::UnsupportedEnvelopeVersion {
                found: reply.v,
                expected: ENVELOPE_VERSION,
            });
        }
        if reply.ok.is_some() == reply.err.is_some() {
            return Err(ProtocolError::AmbiguousReply);
        }
        Ok(reply)
    }

    /// Collapses the envelope into the shape callers actually branch on.
    pub fn into_result(self) -> Result<Vec<rmpv::Value>, WireError> {
        match (self.ok, self.err) {
            (Some(values), _) => Ok(values),
            (None, Some(error)) => Err(error),
            (None, None) => Err(WireError::new(
                crate::ErrorCode::Internal,
                "reply carried neither `ok` nor `err`",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    #[test]
    fn request_round_trips() {
        let request = Request::new(
            "call-1",
            "ardo314:math/vector3d@0.0.3",
            "add",
            vec![rmpv::Value::F32(1.5), rmpv::Value::F32(2.5)],
        )
        .with_deadline_ms(5_000);

        let decoded = Request::decode(&request.encode().unwrap()).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn optional_request_fields_are_omitted() {
        let lean = Request::new("id", "a:b/c@1.0.0", "f", vec![]);
        let fat = lean.clone().with_deadline_ms(1);
        assert!(lean.encode().unwrap().len() < fat.encode().unwrap().len());
    }

    #[test]
    fn ok_reply_round_trips() {
        let reply = Reply::ok("call-1", vec![rmpv::Value::F32(4.0)]);
        let decoded = Reply::decode(&reply.encode().unwrap()).unwrap();
        assert_eq!(decoded.into_result().unwrap(), vec![rmpv::Value::F32(4.0)]);
    }

    #[test]
    fn err_reply_round_trips() {
        let reply = Reply::err("call-1", WireError::new(ErrorCode::NotFound, "no provider"));
        let decoded = Reply::decode(&reply.encode().unwrap()).unwrap();
        let error = decoded.into_result().unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(error.message, "no provider");
    }

    #[test]
    fn rejects_replies_carrying_both_arms() {
        let ambiguous = Reply {
            v: ENVELOPE_VERSION,
            id: "x".into(),
            ok: Some(vec![]),
            err: Some(WireError::new(ErrorCode::Internal, "boom")),
        };
        let bytes = ambiguous.encode().unwrap();
        assert!(matches!(
            Reply::decode(&bytes),
            Err(ProtocolError::AmbiguousReply)
        ));
    }

    #[test]
    fn rejects_unknown_envelope_version() {
        let mut request = Request::new("id", "a:b/c@1.0.0", "f", vec![]);
        request.v = 99;
        let bytes = rmp_serde::to_vec_named(&request).unwrap();
        assert!(matches!(
            Request::decode(&bytes),
            Err(ProtocolError::UnsupportedEnvelopeVersion { found: 99, .. })
        ));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let mut fields = vec![
            ("v".to_string(), rmpv::Value::from(ENVELOPE_VERSION)),
            ("id".to_string(), rmpv::Value::from("id")),
            ("iface".to_string(), rmpv::Value::from("a:b/c@1.0.0")),
            ("func".to_string(), rmpv::Value::from("f")),
            ("args".to_string(), rmpv::Value::Array(vec![])),
        ];
        fields.push(("from-the-future".to_string(), rmpv::Value::from(true)));

        let map = rmpv::Value::Map(
            fields
                .into_iter()
                .map(|(k, v)| (rmpv::Value::from(k), v))
                .collect(),
        );
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, &map).unwrap();

        let decoded = Request::decode(&bytes).unwrap();
        assert_eq!(decoded.func, "f");
    }
}
