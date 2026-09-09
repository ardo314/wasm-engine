//! Conversion between wasmtime's dynamic [`Val`] and the MessagePack wire form.
//!
//! Encoding is driven by [`WitType`] rather than [`Type`] so the codec can be
//! exercised without an [`Engine`](wasmtime::Engine): `Val` is freely
//! constructible, `Type` is not. The host converts `Type` once at link time via
//! [`WitType::from_component_type`] and reuses the result per call.

use rmpv::Value;
use wasmtime::component::{Type, Val};

use crate::{CodecError, Case, Field, WitType};

/// Encodes positional arguments or results.
pub fn vals_to_msgpack(vals: &[Val], types: &[WitType]) -> Result<Vec<Value>, CodecError> {
    if vals.len() != types.len() {
        return Err(CodecError::ArityMismatch {
            expected: types.len(),
            found: vals.len(),
        });
    }
    vals.iter()
        .zip(types)
        .map(|(val, ty)| val_to_msgpack(val, ty))
        .collect()
}

/// Decodes positional arguments or results.
pub fn msgpack_to_vals(values: &[Value], types: &[WitType]) -> Result<Vec<Val>, CodecError> {
    if values.len() != types.len() {
        return Err(CodecError::ArityMismatch {
            expected: types.len(),
            found: values.len(),
        });
    }
    values
        .iter()
        .zip(types)
        .map(|(value, ty)| msgpack_to_val(value, ty))
        .collect()
}

pub fn val_to_msgpack(val: &Val, ty: &WitType) -> Result<Value, CodecError> {
    let mismatch = || CodecError::TypeMismatch {
        expected: ty.kind(),
        found: describe_val(val),
    };

    Ok(match (ty, val) {
        (WitType::Bool, Val::Bool(v)) => Value::Boolean(*v),
        (WitType::S8, Val::S8(v)) => Value::from(*v),
        (WitType::U8, Val::U8(v)) => Value::from(*v),
        (WitType::S16, Val::S16(v)) => Value::from(*v),
        (WitType::U16, Val::U16(v)) => Value::from(*v),
        (WitType::S32, Val::S32(v)) => Value::from(*v),
        (WitType::U32, Val::U32(v)) => Value::from(*v),
        (WitType::S64, Val::S64(v)) => Value::from(*v),
        (WitType::U64, Val::U64(v)) => Value::from(*v),
        (WitType::F32, Val::Float32(v)) => Value::F32(*v),
        (WitType::F64, Val::Float64(v)) => Value::F64(*v),
        (WitType::Char, Val::Char(v)) => Value::from(v.to_string()),
        (WitType::String, Val::String(v)) => Value::from(v.as_str()),

        (WitType::List(inner), Val::List(items)) => Value::Array(
            items
                .iter()
                .map(|item| val_to_msgpack(item, inner))
                .collect::<Result<_, _>>()?,
        ),

        (WitType::FixedList(inner, len), Val::FixedLengthList(items)) => {
            if items.len() != *len as usize {
                return Err(CodecError::ArityMismatch {
                    expected: *len as usize,
                    found: items.len(),
                });
            }
            Value::Array(
                items
                    .iter()
                    .map(|item| val_to_msgpack(item, inner))
                    .collect::<Result<_, _>>()?,
            )
        }

        (WitType::Tuple(types), Val::Tuple(items)) => {
            if items.len() != types.len() {
                return Err(CodecError::ArityMismatch {
                    expected: types.len(),
                    found: items.len(),
                });
            }
            Value::Array(
                items
                    .iter()
                    .zip(types)
                    .map(|(item, ty)| val_to_msgpack(item, ty))
                    .collect::<Result<_, _>>()?,
            )
        }

        (WitType::Record(fields), Val::Record(entries)) => {
            let mut encoded = Vec::with_capacity(fields.len());
            for field in fields {
                let (_, value) = entries
                    .iter()
                    .find(|(name, _)| *name == field.name)
                    .ok_or_else(|| CodecError::MissingField(field.name.clone()))?;
                encoded.push((
                    Value::from(field.name.as_str()),
                    val_to_msgpack(value, &field.ty)?,
                ));
            }
            Value::Map(encoded)
        }

        (WitType::Variant(cases), Val::Variant(name, payload)) => {
            let case = find_case(cases, name)?;
            let encoded = match (&case.payload, payload) {
                (Some(ty), Some(value)) => val_to_msgpack(value, ty)?,
                (None, None) => Value::Nil,
                (Some(_), None) => return Err(CodecError::MissingField(name.clone())),
                (None, Some(_)) => {
                    return Err(CodecError::TypeMismatch {
                        expected: "a payload-less variant case",
                        found: format!("case `{name}` with a payload"),
                    });
                }
            };
            Value::Map(vec![(Value::from(name.as_str()), encoded)])
        }

        (WitType::Enum(names), Val::Enum(name)) => {
            if !names.contains(name) {
                return Err(CodecError::UnknownCase {
                    kind: "enum",
                    name: name.clone(),
                });
            }
            Value::from(name.as_str())
        }

        (WitType::Flags(names), Val::Flags(set)) => {
            for flag in set {
                if !names.contains(flag) {
                    return Err(CodecError::UnknownCase {
                        kind: "flags",
                        name: flag.clone(),
                    });
                }
            }
            // Declaration order, so the encoding is canonical.
            Value::Array(
                names
                    .iter()
                    .filter(|name| set.contains(name))
                    .map(|name| Value::from(name.as_str()))
                    .collect(),
            )
        }

        (WitType::Option(inner), Val::Option(value)) => match value {
            None => Value::Array(vec![]),
            Some(value) => Value::Array(vec![val_to_msgpack(value, inner)?]),
        },

        (WitType::Result { ok, err }, Val::Result(value)) => {
            let (key, payload, ty) = match value {
                Ok(payload) => ("ok", payload, ok),
                Err(payload) => ("err", payload, err),
            };
            let encoded = match (ty, payload) {
                (Some(ty), Some(value)) => val_to_msgpack(value, ty)?,
                (None, None) => Value::Nil,
                _ => return Err(mismatch()),
            };
            Value::Map(vec![(Value::from(key), encoded)])
        }

        (_, Val::Resource(_)) => return Err(CodecError::UnsupportedType("resource")),
        (_, Val::Future(_)) => return Err(CodecError::UnsupportedType("future")),
        (_, Val::Stream(_)) => return Err(CodecError::UnsupportedType("stream")),
        (_, Val::ErrorContext(_)) => return Err(CodecError::UnsupportedType("error-context")),

        _ => return Err(mismatch()),
    })
}

pub fn msgpack_to_val(value: &Value, ty: &WitType) -> Result<Val, CodecError> {
    let mismatch = || CodecError::TypeMismatch {
        expected: ty.kind(),
        found: describe_value(value),
    };

    Ok(match ty {
        WitType::Bool => Val::Bool(value.as_bool().ok_or_else(mismatch)?),
        WitType::S8 => Val::S8(narrow_signed(value, "s8")?),
        WitType::U8 => Val::U8(narrow_unsigned(value, "u8")?),
        WitType::S16 => Val::S16(narrow_signed(value, "s16")?),
        WitType::U16 => Val::U16(narrow_unsigned(value, "u16")?),
        WitType::S32 => Val::S32(narrow_signed(value, "s32")?),
        WitType::U32 => Val::U32(narrow_unsigned(value, "u32")?),
        WitType::S64 => Val::S64(as_i64(value, "s64")?),
        WitType::U64 => Val::U64(as_u64(value, "u64")?),

        // Either float width is accepted so languages without a distinct f32
        // can still interoperate.
        WitType::F32 => Val::Float32(match value {
            Value::F32(v) => *v,
            Value::F64(v) => *v as f32,
            _ => return Err(mismatch()),
        }),
        WitType::F64 => Val::Float64(match value {
            Value::F32(v) => *v as f64,
            Value::F64(v) => *v,
            _ => return Err(mismatch()),
        }),

        WitType::Char => {
            let text = as_str(value, mismatch)?;
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Val::Char(c),
                _ => return Err(CodecError::NotAChar(text.to_owned())),
            }
        }

        WitType::String => Val::String(as_str(value, mismatch)?.to_owned()),

        WitType::List(inner) => Val::List(
            as_array(value, mismatch)?
                .iter()
                .map(|item| msgpack_to_val(item, inner))
                .collect::<Result<_, _>>()?,
        ),

        WitType::FixedList(inner, len) => {
            let items = as_array(value, mismatch)?;
            if items.len() != *len as usize {
                return Err(CodecError::ArityMismatch {
                    expected: *len as usize,
                    found: items.len(),
                });
            }
            Val::FixedLengthList(
                items
                    .iter()
                    .map(|item| msgpack_to_val(item, inner))
                    .collect::<Result<_, _>>()?,
            )
        }

        WitType::Tuple(types) => {
            let items = as_array(value, mismatch)?;
            if items.len() != types.len() {
                return Err(CodecError::ArityMismatch {
                    expected: types.len(),
                    found: items.len(),
                });
            }
            Val::Tuple(
                items
                    .iter()
                    .zip(types)
                    .map(|(item, ty)| msgpack_to_val(item, ty))
                    .collect::<Result<_, _>>()?,
            )
        }

        WitType::Record(fields) => {
            let entries = as_map(value, mismatch)?;
            let mut decoded = Vec::with_capacity(fields.len());
            for field in fields {
                let raw = lookup(entries, &field.name)
                    .ok_or_else(|| CodecError::MissingField(field.name.clone()))?;
                decoded.push((field.name.clone(), msgpack_to_val(raw, &field.ty)?));
            }
            Val::Record(decoded)
        }

        WitType::Variant(cases) => {
            let (name, payload) = single_entry(value, mismatch)?;
            let case = find_case(cases, name)?;
            let payload = match &case.payload {
                Some(ty) => Some(Box::new(msgpack_to_val(payload, ty)?)),
                None => None,
            };
            Val::Variant(name.to_owned(), payload)
        }

        WitType::Enum(names) => {
            let name = as_str(value, mismatch)?;
            if !names.iter().any(|declared| declared == name) {
                return Err(CodecError::UnknownCase {
                    kind: "enum",
                    name: name.to_owned(),
                });
            }
            Val::Enum(name.to_owned())
        }

        WitType::Flags(names) => {
            let mut set = Vec::new();
            for raw in as_array(value, mismatch)? {
                let flag = as_str(raw, mismatch)?;
                if !names.iter().any(|declared| declared == flag) {
                    return Err(CodecError::UnknownCase {
                        kind: "flags",
                        name: flag.to_owned(),
                    });
                }
                set.push(flag.to_owned());
            }
            Val::Flags(set)
        }

        WitType::Option(inner) => match as_array(value, mismatch)? {
            [] => Val::Option(None),
            [value] => Val::Option(Some(Box::new(msgpack_to_val(value, inner)?))),
            other => {
                return Err(CodecError::ArityMismatch {
                    expected: 1,
                    found: other.len(),
                });
            }
        },

        WitType::Result { ok, err } => {
            let (key, payload) = single_entry(value, mismatch)?;
            let (ty, wrap): (&Option<Box<WitType>>, fn(Option<Box<Val>>) -> Val) = match key {
                "ok" => (ok, |v| Val::Result(Ok(v))),
                "err" => (err, |v| Val::Result(Err(v))),
                other => {
                    return Err(CodecError::UnknownCase {
                        kind: "result",
                        name: other.to_owned(),
                    });
                }
            };
            let decoded = match ty {
                Some(ty) => Some(Box::new(msgpack_to_val(payload, ty)?)),
                None => None,
            };
            wrap(decoded)
        }
    })
}

impl WitType {
    /// Projects a wasmtime component type into the wire vocabulary.
    ///
    /// Fails for types this protocol cannot carry, which is how the host learns
    /// an interface must be linked in-process rather than over NATS.
    pub fn from_component_type(ty: &Type) -> Result<Self, CodecError> {
        Ok(match ty {
            Type::Bool => Self::Bool,
            Type::S8 => Self::S8,
            Type::U8 => Self::U8,
            Type::S16 => Self::S16,
            Type::U16 => Self::U16,
            Type::S32 => Self::S32,
            Type::U32 => Self::U32,
            Type::S64 => Self::S64,
            Type::U64 => Self::U64,
            Type::Float32 => Self::F32,
            Type::Float64 => Self::F64,
            Type::Char => Self::Char,
            Type::String => Self::String,

            Type::List(list) => Self::List(Box::new(Self::from_component_type(&list.ty())?)),

            Type::FixedLengthList(list) => Self::FixedList(
                Box::new(Self::from_component_type(&list.ty())?),
                list.len(),
            ),

            Type::Tuple(tuple) => Self::Tuple(
                tuple
                    .types()
                    .map(|ty| Self::from_component_type(&ty))
                    .collect::<Result<_, _>>()?,
            ),

            Type::Record(record) => Self::Record(
                record
                    .fields()
                    .map(|field| {
                        Ok(Field::new(field.name, Self::from_component_type(&field.ty)?))
                    })
                    .collect::<Result<_, CodecError>>()?,
            ),

            Type::Variant(variant) => Self::Variant(
                variant
                    .cases()
                    .map(|case| {
                        let payload = case
                            .ty
                            .as_ref()
                            .map(Self::from_component_type)
                            .transpose()?;
                        Ok(Case::new(case.name, payload))
                    })
                    .collect::<Result<_, CodecError>>()?,
            ),

            Type::Enum(ty) => Self::Enum(ty.names().map(str::to_owned).collect()),
            Type::Flags(ty) => Self::Flags(ty.names().map(str::to_owned).collect()),

            Type::Option(ty) => Self::Option(Box::new(Self::from_component_type(&ty.ty())?)),

            Type::Result(ty) => Self::Result {
                ok: ty
                    .ok()
                    .map(|ty| Self::from_component_type(&ty))
                    .transpose()?
                    .map(Box::new),
                err: ty
                    .err()
                    .map(|ty| Self::from_component_type(&ty))
                    .transpose()?
                    .map(Box::new),
            },

            Type::Own(_) | Type::Borrow(_) => {
                return Err(CodecError::UnsupportedType("resource"));
            }
            Type::Future(_) => return Err(CodecError::UnsupportedType("future")),
            Type::Stream(_) => return Err(CodecError::UnsupportedType("stream")),
            Type::ErrorContext => return Err(CodecError::UnsupportedType("error-context")),
            Type::Map(_) => return Err(CodecError::UnsupportedType("map")),
        })
    }
}

fn find_case<'a>(cases: &'a [Case], name: &str) -> Result<&'a Case, CodecError> {
    cases
        .iter()
        .find(|case| case.name == name)
        .ok_or_else(|| CodecError::UnknownCase {
            kind: "variant",
            name: name.to_owned(),
        })
}

fn as_str<'a>(
    value: &'a Value,
    mismatch: impl Fn() -> CodecError,
) -> Result<&'a str, CodecError> {
    value.as_str().ok_or_else(mismatch)
}

fn as_array<'a>(
    value: &'a Value,
    mismatch: impl Fn() -> CodecError,
) -> Result<&'a [Value], CodecError> {
    value.as_array().map(Vec::as_slice).ok_or_else(mismatch)
}

fn as_map<'a>(
    value: &'a Value,
    mismatch: impl Fn() -> CodecError,
) -> Result<&'a [(Value, Value)], CodecError> {
    match value {
        Value::Map(entries) => Ok(entries),
        _ => Err(mismatch()),
    }
}

fn lookup<'a>(entries: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

/// Reads the single `{name: payload}` entry used by `variant` and `result`.
fn single_entry<'a>(
    value: &'a Value,
    mismatch: impl Fn() -> CodecError,
) -> Result<(&'a str, &'a Value), CodecError> {
    let entries = as_map(value, &mismatch)?;
    let [(key, payload)] = entries else {
        return Err(CodecError::ArityMismatch {
            expected: 1,
            found: entries.len(),
        });
    };
    Ok((key.as_str().ok_or_else(mismatch)?, payload))
}

fn as_i64(value: &Value, ty: &'static str) -> Result<i64, CodecError> {
    value
        .as_i64()
        .ok_or_else(|| CodecError::TypeMismatch {
            expected: ty,
            found: describe_value(value),
        })
}

fn as_u64(value: &Value, ty: &'static str) -> Result<u64, CodecError> {
    value
        .as_u64()
        .ok_or_else(|| CodecError::TypeMismatch {
            expected: ty,
            found: describe_value(value),
        })
}

fn narrow_signed<T>(value: &Value, ty: &'static str) -> Result<T, CodecError>
where
    T: TryFrom<i64>,
{
    let raw = as_i64(value, ty)?;
    T::try_from(raw).map_err(|_| CodecError::OutOfRange {
        value: raw.to_string(),
        ty,
    })
}

fn narrow_unsigned<T>(value: &Value, ty: &'static str) -> Result<T, CodecError>
where
    T: TryFrom<u64>,
{
    let raw = as_u64(value, ty)?;
    T::try_from(raw).map_err(|_| CodecError::OutOfRange {
        value: raw.to_string(),
        ty,
    })
}

fn describe_value(value: &Value) -> String {
    match value {
        Value::Nil => "nil",
        Value::Boolean(_) => "bool",
        Value::Integer(_) => "integer",
        Value::F32(_) => "f32",
        Value::F64(_) => "f64",
        Value::String(_) => "string",
        Value::Binary(_) => "binary",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Ext(..) => "ext",
    }
    .to_owned()
}

fn describe_val(val: &Val) -> String {
    match val {
        Val::Bool(_) => "bool",
        Val::S8(_) => "s8",
        Val::U8(_) => "u8",
        Val::S16(_) => "s16",
        Val::U16(_) => "u16",
        Val::S32(_) => "s32",
        Val::U32(_) => "u32",
        Val::S64(_) => "s64",
        Val::U64(_) => "u64",
        Val::Float32(_) => "f32",
        Val::Float64(_) => "f64",
        Val::Char(_) => "char",
        Val::String(_) => "string",
        Val::List(_) => "list",
        Val::FixedLengthList(_) => "fixed-length list",
        Val::Map(_) => "map",
        Val::Record(_) => "record",
        Val::Tuple(_) => "tuple",
        Val::Variant(..) => "variant",
        Val::Enum(_) => "enum",
        Val::Option(_) => "option",
        Val::Result(_) => "result",
        Val::Flags(_) => "flags",
        Val::Resource(_) => "resource",
        Val::Future(_) => "future",
        Val::Stream(_) => "stream",
        Val::ErrorContext(_) => "error-context",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encoding then decoding must be the identity.
    #[track_caller]
    fn round_trip(val: Val, ty: &WitType) -> Value {
        let encoded = val_to_msgpack(&val, ty).expect("encode");
        let decoded = msgpack_to_val(&encoded, ty).expect("decode");
        assert_eq!(decoded, val, "round trip changed the value");
        encoded
    }

    fn vector3d() -> WitType {
        WitType::Tuple(vec![WitType::F32, WitType::F32, WitType::F32])
    }

    fn matrix2x2() -> WitType {
        WitType::Record(vec![
            Field::new("m00", WitType::F32),
            Field::new("m10", WitType::F32),
            Field::new("m01", WitType::F32),
            Field::new("m11", WitType::F32),
        ])
    }

    /// `ardo314:math`'s `plane`: a record whose field is itself a tuple alias.
    fn plane() -> WitType {
        WitType::Record(vec![
            Field::new("normal", vector3d()),
            Field::new("d", WitType::F32),
        ])
    }

    fn f32s(values: [f32; 3]) -> Val {
        Val::Tuple(values.map(Val::Float32).to_vec())
    }

    #[test]
    fn math_tuple_types_round_trip() {
        let encoded = round_trip(f32s([1.0, -2.5, 3.25]), &vector3d());
        assert_eq!(
            encoded,
            Value::Array(vec![Value::F32(1.0), Value::F32(-2.5), Value::F32(3.25)])
        );
    }

    #[test]
    fn math_record_types_round_trip_with_named_keys() {
        let val = Val::Record(vec![
            ("m00".into(), Val::Float32(1.0)),
            ("m10".into(), Val::Float32(0.0)),
            ("m01".into(), Val::Float32(0.0)),
            ("m11".into(), Val::Float32(1.0)),
        ]);
        let encoded = round_trip(val, &matrix2x2());

        let Value::Map(entries) = &encoded else {
            panic!("records must encode as maps, got {encoded:?}");
        };
        let keys: Vec<_> = entries.iter().filter_map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["m00", "m10", "m01", "m11"]);
    }

    #[test]
    fn nested_record_of_tuple_round_trips() {
        let val = Val::Record(vec![
            ("normal".into(), f32s([0.0, 1.0, 0.0])),
            ("d".into(), Val::Float32(-4.0)),
        ]);
        round_trip(val, &plane());
    }

    #[test]
    fn record_field_order_follows_the_declaration_not_the_value() {
        let shuffled = Val::Record(vec![
            ("d".into(), Val::Float32(-4.0)),
            ("normal".into(), f32s([0.0, 1.0, 0.0])),
        ]);
        let encoded = val_to_msgpack(&shuffled, &plane()).unwrap();
        let Value::Map(entries) = &encoded else {
            unreachable!()
        };
        let keys: Vec<_> = entries.iter().filter_map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["normal", "d"]);
    }

    #[test]
    fn every_integer_width_round_trips_at_its_bounds() {
        for (ty, val) in [
            (WitType::S8, Val::S8(i8::MIN)),
            (WitType::S8, Val::S8(i8::MAX)),
            (WitType::U8, Val::U8(u8::MAX)),
            (WitType::S16, Val::S16(i16::MIN)),
            (WitType::U16, Val::U16(u16::MAX)),
            (WitType::S32, Val::S32(i32::MIN)),
            (WitType::U32, Val::U32(u32::MAX)),
            (WitType::S64, Val::S64(i64::MIN)),
            (WitType::U64, Val::U64(u64::MAX)),
        ] {
            round_trip(val, &ty);
        }
    }

    #[test]
    fn integers_outside_the_declared_range_are_rejected() {
        let too_big = Value::from(300u16);
        assert!(matches!(
            msgpack_to_val(&too_big, &WitType::U8),
            Err(CodecError::OutOfRange { .. })
        ));
    }

    #[test]
    fn f32_accepts_either_float_width() {
        let as_f64 = Value::F64(0.5);
        assert_eq!(
            msgpack_to_val(&as_f64, &WitType::F32).unwrap(),
            Val::Float32(0.5)
        );
    }

    #[test]
    fn integers_are_not_accepted_as_floats() {
        assert!(matches!(
            msgpack_to_val(&Value::from(1), &WitType::F32),
            Err(CodecError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn option_uses_a_zero_or_one_element_array() {
        let ty = WitType::option(WitType::String);

        let none = round_trip(Val::Option(None), &ty);
        assert_eq!(none, Value::Array(vec![]));

        let some = round_trip(
            Val::Option(Some(Box::new(Val::String("hi".into())))),
            &ty,
        );
        assert_eq!(some, Value::Array(vec![Value::from("hi")]));
    }

    /// The reason `option` is not encoded as a bare `nil`.
    #[test]
    fn nested_option_stays_unambiguous() {
        let ty = WitType::option(WitType::option(WitType::U8));

        let outer_none = val_to_msgpack(&Val::Option(None), &ty).unwrap();
        let inner_none =
            val_to_msgpack(&Val::Option(Some(Box::new(Val::Option(None)))), &ty).unwrap();

        assert_ne!(outer_none, inner_none);
        round_trip(Val::Option(None), &ty);
        round_trip(Val::Option(Some(Box::new(Val::Option(None)))), &ty);
    }

    #[test]
    fn result_round_trips_both_arms_including_payload_less_ones() {
        let ty = WitType::result(Some(WitType::U32), Some(WitType::String));
        round_trip(Val::Result(Ok(Some(Box::new(Val::U32(7))))), &ty);
        round_trip(
            Val::Result(Err(Some(Box::new(Val::String("nope".into()))))),
            &ty,
        );

        let bare = WitType::result(None, Some(WitType::String));
        let encoded = round_trip(Val::Result(Ok(None)), &bare);
        assert_eq!(encoded, Value::Map(vec![(Value::from("ok"), Value::Nil)]));
    }

    #[test]
    fn variant_round_trips_with_and_without_payload() {
        let ty = WitType::Variant(vec![
            Case::new("empty", None),
            Case::new("full", Some(WitType::U32)),
        ]);
        round_trip(Val::Variant("empty".into(), None), &ty);
        round_trip(
            Val::Variant("full".into(), Some(Box::new(Val::U32(3)))),
            &ty,
        );
    }

    #[test]
    fn undeclared_cases_are_rejected() {
        let ty = WitType::Enum(vec!["red".into(), "green".into()]);
        assert!(matches!(
            val_to_msgpack(&Val::Enum("blue".into()), &ty),
            Err(CodecError::UnknownCase { .. })
        ));
        assert!(matches!(
            msgpack_to_val(&Value::from("blue"), &ty),
            Err(CodecError::UnknownCase { .. })
        ));
    }

    #[test]
    fn flags_encode_in_declaration_order() {
        let ty = WitType::Flags(vec!["a".into(), "b".into(), "c".into()]);
        let val = Val::Flags(vec!["c".into(), "a".into()]);

        let encoded = val_to_msgpack(&val, &ty).unwrap();
        assert_eq!(
            encoded,
            Value::Array(vec![Value::from("a"), Value::from("c")])
        );

        // Decoding normalises the order, so this is canonical rather than a
        // strict round trip.
        assert_eq!(
            msgpack_to_val(&encoded, &ty).unwrap(),
            Val::Flags(vec!["a".into(), "c".into()])
        );
    }

    #[test]
    fn lists_and_fixed_lists_round_trip() {
        round_trip(
            Val::List(vec![Val::U8(1), Val::U8(2)]),
            &WitType::list(WitType::U8),
        );
        round_trip(
            Val::FixedLengthList(vec![Val::U8(1), Val::U8(2)]),
            &WitType::FixedList(Box::new(WitType::U8), 2),
        );
    }

    #[test]
    fn fixed_list_length_is_enforced() {
        let ty = WitType::FixedList(Box::new(WitType::U8), 2);
        assert!(matches!(
            msgpack_to_val(&Value::Array(vec![Value::from(1)]), &ty),
            Err(CodecError::ArityMismatch {
                expected: 2,
                found: 1
            })
        ));
    }

    #[test]
    fn char_requires_exactly_one_scalar_value() {
        round_trip(Val::Char('é'), &WitType::Char);
        for bad in ["", "ab"] {
            assert!(matches!(
                msgpack_to_val(&Value::from(bad), &WitType::Char),
                Err(CodecError::NotAChar(_))
            ));
        }
    }

    #[test]
    fn missing_record_fields_are_reported_by_name() {
        let partial = Value::Map(vec![(Value::from("m00"), Value::F32(1.0))]);
        let err = msgpack_to_val(&partial, &matrix2x2()).unwrap_err();
        assert!(matches!(err, CodecError::MissingField(name) if name == "m10"));
    }

    #[test]
    fn unknown_record_keys_are_ignored() {
        let extra = Value::Map(vec![
            (Value::from("normal"), Value::Array(vec![
                Value::F32(0.0),
                Value::F32(1.0),
                Value::F32(0.0),
            ])),
            (Value::from("d"), Value::F32(-4.0)),
            (Value::from("added-later"), Value::from(true)),
        ]);
        assert!(msgpack_to_val(&extra, &plane()).is_ok());
    }

    #[test]
    fn positional_arity_is_checked() {
        let types = [WitType::U8, WitType::U8];
        assert!(matches!(
            vals_to_msgpack(&[Val::U8(1)], &types),
            Err(CodecError::ArityMismatch {
                expected: 2,
                found: 1
            })
        ));
    }

    #[test]
    fn argument_lists_round_trip() {
        let types = [vector3d(), WitType::F32];
        let vals = [f32s([1.0, 2.0, 3.0]), Val::Float32(0.5)];
        let encoded = vals_to_msgpack(&vals, &types).unwrap();
        assert_eq!(msgpack_to_vals(&encoded, &types).unwrap(), vals);
    }

    #[test]
    fn mismatched_shapes_are_rejected() {
        assert!(matches!(
            val_to_msgpack(&Val::U8(1), &WitType::String),
            Err(CodecError::TypeMismatch { .. })
        ));
        assert!(matches!(
            msgpack_to_val(&Value::from("x"), &vector3d()),
            Err(CodecError::TypeMismatch { .. })
        ));
    }
}

