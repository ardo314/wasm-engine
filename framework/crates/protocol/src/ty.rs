use serde::{Deserialize, Serialize};

/// A named field of a `record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: WitType,
}

impl Field {
    pub fn new(name: impl Into<String>, ty: WitType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

/// A case of a `variant`, optionally carrying a payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub payload: Option<WitType>,
}

impl Case {
    pub fn new(name: impl Into<String>, payload: Option<WitType>) -> Self {
        Self {
            name: name.into(),
            payload,
        }
    }
}

/// The subset of the WIT type system this protocol can carry over the wire.
///
/// Deliberately independent of wasmtime: native service SDKs and the codegen
/// need this vocabulary without linking a wasm runtime, and it can be built by
/// hand in tests where [`wasmtime::component::Type`] cannot.
///
/// `resource`, `future`, `stream` and `error-context` are absent by design —
/// interfaces using them can only be linked in-process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WitType {
    Bool,
    S8,
    U8,
    S16,
    U16,
    S32,
    U32,
    S64,
    U64,
    F32,
    F64,
    Char,
    String,
    List(Box<WitType>),
    FixedList(Box<WitType>, u32),
    Tuple(Vec<WitType>),
    Record(Vec<Field>),
    Variant(Vec<Case>),
    Enum(Vec<String>),
    Flags(Vec<String>),
    Option(Box<WitType>),
    Result {
        ok: Option<Box<WitType>>,
        err: Option<Box<WitType>>,
    },
}

impl WitType {
    pub fn list(inner: WitType) -> Self {
        Self::List(Box::new(inner))
    }

    pub fn option(inner: WitType) -> Self {
        Self::Option(Box::new(inner))
    }

    pub fn result(ok: Option<WitType>, err: Option<WitType>) -> Self {
        Self::Result {
            ok: ok.map(Box::new),
            err: err.map(Box::new),
        }
    }

    /// Name used in diagnostics and in the shape digest.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::S8 => "s8",
            Self::U8 => "u8",
            Self::S16 => "s16",
            Self::U16 => "u16",
            Self::S32 => "s32",
            Self::U32 => "u32",
            Self::S64 => "s64",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Char => "char",
            Self::String => "string",
            Self::List(_) => "list",
            Self::FixedList(..) => "fixed-length list",
            Self::Tuple(_) => "tuple",
            Self::Record(_) => "record",
            Self::Variant(_) => "variant",
            Self::Enum(_) => "enum",
            Self::Flags(_) => "flags",
            Self::Option(_) => "option",
            Self::Result { .. } => "result",
        }
    }
}
