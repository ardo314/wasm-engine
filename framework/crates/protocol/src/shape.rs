//! Structural fingerprint of an interface, per `docs/spec/wire-protocol.md` §6.
//!
//! Two providers of the same interface and version whose digests differ were
//! built against incompatible definitions. The registry stores this so a host
//! can reject such a provider before calling it rather than after.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::WitType;

/// One function's contribution to the digest.
///
/// Parameter *names* are absent deliberately: arguments travel positionally, so
/// renaming a parameter cannot break a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionShape {
    pub name: String,
    pub params: Vec<WitType>,
    pub results: Vec<WitType>,
}

impl FunctionShape {
    pub fn new(name: impl Into<String>, params: Vec<WitType>, results: Vec<WitType>) -> Self {
        Self {
            name: name.into(),
            params,
            results,
        }
    }
}

/// The functions of a single interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceShape {
    functions: Vec<FunctionShape>,
}

impl InterfaceShape {
    /// Ordering of the input does not matter; functions are sorted by name so a
    /// cosmetic reordering in the WIT source cannot raise a false conflict.
    pub fn new(mut functions: Vec<FunctionShape>) -> Self {
        functions.sort_by(|a, b| a.name.cmp(&b.name));
        Self { functions }
    }

    pub fn functions(&self) -> &[FunctionShape] {
        &self.functions
    }

    /// The exact bytes that get hashed. Public so a mismatch can be diffed
    /// instead of guessed at.
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for function in &self.functions {
            out.push_str(&function.name);
            render_list(&mut out, &function.params);
            out.push_str("->");
            render_list(&mut out, &function.results);
            out.push(';');
        }
        out
    }

    /// Lowercase hex sha256 of [`Self::canonical`].
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.canonical().as_bytes());
        hasher
            .finalize()
            .iter()
            .fold(String::with_capacity(64), |mut acc, byte| {
                let _ = write!(acc, "{byte:02x}");
                acc
            })
    }
}

fn render_list(out: &mut String, types: &[WitType]) {
    out.push('(');
    for (i, ty) in types.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        render(out, ty);
    }
    out.push(')');
}

/// Fully expanded structural form. Aliases are already resolved by the time a
/// [`WitType`] exists, so nothing here can depend on how a type was named.
fn render(out: &mut String, ty: &WitType) {
    match ty {
        WitType::List(inner) => {
            out.push_str("list<");
            render(out, inner);
            out.push('>');
        }
        WitType::FixedList(inner, len) => {
            out.push_str("list<");
            render(out, inner);
            let _ = write!(out, ",{len}>");
        }
        WitType::Tuple(types) => {
            out.push_str("tuple");
            render_list(out, types);
        }
        WitType::Record(fields) => {
            out.push_str("record{");
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&field.name);
                out.push(':');
                render(out, &field.ty);
            }
            out.push('}');
        }
        WitType::Variant(cases) => {
            out.push_str("variant{");
            for (i, case) in cases.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&case.name);
                if let Some(payload) = &case.payload {
                    out.push('(');
                    render(out, payload);
                    out.push(')');
                }
            }
            out.push('}');
        }
        WitType::Enum(names) => {
            let _ = write!(out, "enum{{{}}}", names.join(","));
        }
        WitType::Flags(names) => {
            let _ = write!(out, "flags{{{}}}", names.join(","));
        }
        WitType::Option(inner) => {
            out.push_str("option<");
            render(out, inner);
            out.push('>');
        }
        WitType::Result { ok, err } => {
            out.push_str("result<");
            match ok {
                Some(ty) => render(out, ty),
                None => out.push('_'),
            }
            out.push(',');
            match err {
                Some(ty) => render(out, ty),
                None => out.push('_'),
            }
            out.push('>');
        }
        primitive => out.push_str(primitive.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Case, Field};

    fn vector3d() -> WitType {
        WitType::Tuple(vec![WitType::F32, WitType::F32, WitType::F32])
    }

    fn add() -> FunctionShape {
        FunctionShape::new("add", vec![vector3d(), vector3d()], vec![vector3d()])
    }

    fn shape(functions: Vec<FunctionShape>) -> InterfaceShape {
        InterfaceShape::new(functions)
    }

    #[test]
    fn renders_the_documented_grammar() {
        assert_eq!(
            shape(vec![add()]).canonical(),
            "add(tuple(f32,f32,f32),tuple(f32,f32,f32))->(tuple(f32,f32,f32));"
        );
    }

    #[test]
    fn renders_every_constructor() {
        let cases = [
            (WitType::Bool, "bool"),
            (WitType::String, "string"),
            (WitType::list(WitType::U8), "list<u8>"),
            (WitType::FixedList(Box::new(WitType::U8), 4), "list<u8,4>"),
            (WitType::Tuple(vec![WitType::S8]), "tuple(s8)"),
            (
                WitType::Record(vec![Field::new("a", WitType::U8)]),
                "record{a:u8}",
            ),
            (
                WitType::Variant(vec![
                    Case::new("empty", None),
                    Case::new("full", Some(WitType::U8)),
                ]),
                "variant{empty,full(u8)}",
            ),
            (WitType::Enum(vec!["a".into(), "b".into()]), "enum{a,b}"),
            (WitType::Flags(vec!["a".into(), "b".into()]), "flags{a,b}"),
            (WitType::option(WitType::U8), "option<u8>"),
            (
                WitType::result(Some(WitType::U8), Some(WitType::String)),
                "result<u8,string>",
            ),
            (
                WitType::result(None, Some(WitType::String)),
                "result<_,string>",
            ),
            (WitType::result(None, None), "result<_,_>"),
        ];

        for (ty, expected) in cases {
            let mut rendered = String::new();
            render(&mut rendered, &ty);
            assert_eq!(rendered, expected);
        }
    }

    #[test]
    fn digest_is_stable_across_runs() {
        assert_eq!(shape(vec![add()]).digest(), shape(vec![add()]).digest());
        assert_eq!(shape(vec![add()]).digest().len(), 64);
    }

    #[test]
    fn digest_ignores_function_order() {
        let other = FunctionShape::new("sub", vec![vector3d()], vec![vector3d()]);
        assert_eq!(
            shape(vec![add(), other.clone()]).digest(),
            shape(vec![other, add()]).digest()
        );
    }

    #[test]
    fn digest_changes_when_a_function_is_renamed() {
        let renamed = FunctionShape::new("plus", add().params, add().results);
        assert_ne!(shape(vec![add()]).digest(), shape(vec![renamed]).digest());
    }

    #[test]
    fn digest_changes_when_a_field_is_renamed_reordered_or_retyped() {
        let base = |fields: Vec<Field>| {
            shape(vec![FunctionShape::new(
                "f",
                vec![WitType::Record(fields)],
                vec![],
            )])
            .digest()
        };

        let original = base(vec![
            Field::new("a", WitType::U8),
            Field::new("b", WitType::U8),
        ]);
        let renamed = base(vec![
            Field::new("a", WitType::U8),
            Field::new("c", WitType::U8),
        ]);
        // Reordering is wire-compatible, since records decode by name. It still
        // changes the digest: equal digests promise identical bytes, which is
        // the stronger and more useful guarantee.
        let reordered = base(vec![
            Field::new("b", WitType::U8),
            Field::new("a", WitType::U8),
        ]);
        let retyped = base(vec![
            Field::new("a", WitType::U16),
            Field::new("b", WitType::U8),
        ]);

        for (label, other) in [
            ("renamed", renamed),
            ("reordered", reordered),
            ("retyped", retyped),
        ] {
            assert_ne!(original, other, "{label} should change the digest");
        }
    }

    #[test]
    fn digest_distinguishes_params_from_results() {
        let a = shape(vec![FunctionShape::new("f", vec![WitType::U8], vec![])]);
        let b = shape(vec![FunctionShape::new("f", vec![], vec![WitType::U8])]);
        assert_ne!(a.digest(), b.digest());
    }

    /// `variant{a}` and `enum{a}` encode differently, so they must not collide.
    #[test]
    fn digest_distinguishes_similar_constructors() {
        let variant = shape(vec![FunctionShape::new(
            "f",
            vec![WitType::Variant(vec![Case::new("a", None)])],
            vec![],
        )]);
        let enumeration = shape(vec![FunctionShape::new(
            "f",
            vec![WitType::Enum(vec!["a".into()])],
            vec![],
        )]);
        assert_ne!(variant.digest(), enumeration.digest());
    }
}
