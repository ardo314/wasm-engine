use serde::{Deserialize, Serialize};

/// Machine-readable transport and dispatch failure codes.
///
/// A WIT function returning `result<T, E>` does not use these; its error arm is
/// a successful call. These describe failures to *perform* the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    NotFound,
    BadRequest,
    Unsupported,
    DeadlineExceeded,
    Internal,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotFound => "not-found",
            Self::BadRequest => "bad-request",
            Self::Unsupported => "unsupported",
            Self::DeadlineExceeded => "deadline-exceeded",
            Self::Internal => "internal",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `err` arm of a reply envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<rmpv::Value>,
}

impl WireError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: rmpv::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WireError {}

/// Failure to parse or construct protocol-level identifiers and envelopes.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("malformed interface id `{0}`: {1}")]
    MalformedInterfaceId(String, &'static str),

    #[error("invalid version in `{0}`: {1}")]
    InvalidVersion(String, semver::Error),

    #[error("malformed subject `{0}`: {1}")]
    MalformedSubject(String, &'static str),

    #[error("unsupported envelope version {found}, expected {expected}")]
    UnsupportedEnvelopeVersion { found: u8, expected: u8 },

    #[error("reply must carry exactly one of `ok` or `err`")]
    AmbiguousReply,

    #[error(transparent)]
    Encode(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    Decode(#[from] rmp_serde::decode::Error),
}

/// Failure to convert a value between its in-memory and wire representations.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("expected {expected}, found {found}")]
    TypeMismatch {
        expected: &'static str,
        found: String,
    },

    #[error("value {value} is out of range for {ty}")]
    OutOfRange { value: String, ty: &'static str },

    #[error("`{name}` is not a case of this {kind}")]
    UnknownCase { kind: &'static str, name: String },

    #[error("missing record field `{0}`")]
    MissingField(String),

    #[error("expected {expected} elements, found {found}")]
    ArityMismatch { expected: usize, found: usize },

    #[error("{0} is not supported by this protocol version")]
    UnsupportedType(&'static str),

    #[error("char must be exactly one Unicode scalar value, found {0:?}")]
    NotAChar(String),
}

impl From<CodecError> for WireError {
    fn from(err: CodecError) -> Self {
        let code = match err {
            CodecError::UnsupportedType(_) => ErrorCode::Unsupported,
            _ => ErrorCode::BadRequest,
        };
        WireError::new(code, err.to_string())
    }
}
