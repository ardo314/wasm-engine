# WIT Wire Protocol v1

Status: draft · Applies to `wasm-protocol` 0.1

Defines how a call to a WIT interface function is carried over NATS so that a
WebAssembly component and a native service are indistinguishable to a caller.

This document is normative and language-agnostic. Implementations exist in Rust
(`framework/crates/protocol`) and Python (`framework/py/wasmlink`); both are
validated against the shared fixtures in `tests/conformance/vectors/`.

## 1. Scope

Covered: the WIT value types listed in §4, request/reply invocation, error
propagation.

Not covered in v1: `resource` handles, `future<T>`, `stream<T>`, and any form of
streaming or pub/sub. Interfaces using these can only be linked in-process.

## 2. Subjects

A function is addressed by a NATS subject derived from its fully qualified WIT
name. Given `ardo314:math/vector3d@0.0.3` and function `add-f32`:

```
wit.ardo314.math.0_0_3.vector3d.add-f32
└┬┘ └──┬──┘ └┬─┘ └─┬─┘ └──┬───┘ └──┬──┘
 │     │     │     │      │        └ function name, kebab-case, verbatim
 │     │     │     │      └ interface name, kebab-case, verbatim
 │     │     │     └ version, dots replaced by underscores
 │     │     └ package name
 │     └ namespace
 └ fixed prefix
```

Rules:

- The prefix is always `wit`.
- `.` in the semver is replaced with `_` because `.` is the NATS subject
  separator. Pre-release and build metadata are included verbatim after the same
  substitution (`1.0.0-rc.1` becomes `1_0_0-rc_1`).
- All other segments are copied verbatim from the WIT identifier. WIT
  identifiers are already restricted to `[a-z0-9-]`, so no escaping is needed.
- A version is mandatory. Unversioned WIT packages are rejected.

Providers subscribe using a **queue group** equal to the interface's fully
qualified name (`ardo314:math/vector3d@0.0.3`), so that replicas of the same
provider load-balance while distinct interfaces stay independent.

### 2.1 Version matching

Subjects encode an exact version, so callers must resolve a version requirement
to a concrete version through the registry before constructing a subject. A
provider that wishes to serve several compatible versions subscribes to one
subject per version.

## 3. Envelope

Both request and reply are a MessagePack **map with string keys**. Unknown keys
MUST be ignored by receivers so that fields can be added without a version bump.

### 3.1 Request

| Key           | Type            | Required | Meaning                                             |
| ------------- | --------------- | -------- | --------------------------------------------------- |
| `v`           | uint            | yes      | Envelope version. `1` for this document.            |
| `id`          | string          | yes      | Unique call id, echoed in the reply. Used in logs.  |
| `iface`       | string          | yes      | Fully qualified interface, e.g. `ardo314:math/vector3d@0.0.3`. |
| `func`        | string          | yes      | Function name, kebab-case.                          |
| `args`        | array           | yes      | Positional arguments, encoded per §4.               |
| `deadline_ms` | uint            | no       | Caller's remaining budget in milliseconds.          |
| `trace`       | map<string,str> | no       | Propagated tracing context.                         |

`iface` and `func` duplicate information already present in the subject. They are
carried anyway so a receiver can validate that it was not mis-routed, and so
recorded payloads are self-describing.

### 3.2 Reply

Exactly one of `ok` or `err` is present.

| Key   | Type  | Meaning                                                    |
| ----- | ----- | ---------------------------------------------------------- |
| `v`   | uint  | Envelope version. `1`.                                     |
| `id`  | string| Echo of the request `id`.                                  |
| `ok`  | array | Positional results, encoded per §4. Empty for `-> ()`.     |
| `err` | map   | Error, see §5.                                             |

Results are an array even when a function returns a single value, so that
multi-return WIT functions need no special case.

## 4. Type mapping

Encoding is driven by the function's declared WIT signature; the receiver knows
the expected type of every position and does not infer types from the payload.
This keeps the encoding compact while staying unambiguous.

| WIT type              | MessagePack                                                     |
| --------------------- | --------------------------------------------------------------- |
| `bool`                | bool                                                            |
| `s8` … `s64`          | int                                                             |
| `u8` … `u64`          | uint                                                            |
| `f32`, `f64`          | float                                                           |
| `char`                | str, exactly one Unicode scalar value                           |
| `string`              | str (UTF-8)                                                     |
| `list<T>`             | array of `T`                                                    |
| `list<T, N>`          | array of `T`, length exactly `N`                                |
| `tuple<A, B>`         | array `[A, B]`                                                  |
| `record`              | map, keys are field names verbatim (kebab-case)                 |
| `variant`             | map with exactly one entry, `{case-name: payload}`              |
| `enum`                | str, the case name verbatim                                     |
| `flags`               | array of str, names of set flags, declaration order             |
| `option<T>`           | array: `[]` for `none`, `[value]` for `some`                    |
| `result<T, E>`        | map with exactly one entry, `{"ok": T}` or `{"err": E}`         |

Notes:

- **Records use named keys, not positional arrays.** This costs bytes but means
  a field reordering in WIT is not a silent wire-compatibility break, and it
  makes payloads readable in polyglot debugging. Fields are emitted in
  declaration order. Receivers must ignore map keys they do not recognise, for
  the same reason as §3, and must reject a payload missing a declared field.
- **`option` is an array, not a bare `nil`.** The obvious encoding (`nil` for
  `none`, the value otherwise) is ambiguous for `option<option<T>>` and for
  `option<T>` where `T` itself encodes to `nil`. The zero/one-element array
  costs one byte and is always unambiguous.
- A `variant` case without a payload encodes its value as `nil`. Likewise
  `result` with a payload-less arm, e.g. `result<_, string>` success is
  `{"ok": nil}`.
- Integers must be encoded in the smallest MessagePack representation that fits,
  and receivers must accept any representation that round-trips to the declared
  type. Encoders must not emit a value outside the declared type's range.
- `f32` is encoded as a MessagePack float32 and `f64` as float64. Receivers must
  accept either width and convert, so that languages without a distinct 32-bit
  float can interoperate.

## 5. Errors

An error is a map:

| Key       | Type   | Required | Meaning                                |
| --------- | ------ | -------- | -------------------------------------- |
| `code`    | str    | yes      | Machine-readable code from the list below. |
| `message` | str    | yes      | Human-readable, not parsed by callers. |
| `detail`  | any    | no       | Free-form supplementary data.          |

Codes:

| Code               | Meaning                                                       |
| ------------------ | ------------------------------------------------------------- |
| `not-found`        | No provider serves this interface or function.                |
| `bad-request`      | Envelope malformed, or arguments did not match the signature. |
| `unsupported`      | Signature uses a feature outside this protocol version.       |
| `deadline-exceeded`| Provider gave up before producing a result.                   |
| `internal`         | Provider failed while executing the call.                     |

A WIT function returning `result<T, E>` is **not** an error at this layer: the
`E` arm is a successful call whose payload happens to be the error case. `err`
in the envelope is reserved for transport and dispatch failures. A host bridging
an envelope `err` into a guest call raises a trap.

## 6. Shape digest

The registry advertises a digest per interface so a caller can detect that a
provider was built against an incompatible definition of the same version.

The digest is the lowercase hex `sha256` of a canonical rendering of the
interface. The rendering is exactly:

```
interface := function*                  functions sorted by name, no separator
function  := name params "->" params ";"
params    := "(" [ type { "," type } ] ")"

type      := "bool" | "s8" | "u8" | "s16" | "u16" | "s32" | "u32"
           | "s64" | "u64" | "f32" | "f64" | "char" | "string"
           | "list<" type ">"
           | "list<" type "," length ">"
           | "tuple(" [ type { "," type } ] ")"
           | "record{" [ field { "," field } ] "}"
           | "variant{" [ case { "," case } ] "}"
           | "enum{" [ name { "," name } ] "}"
           | "flags{" [ name { "," name } ] "}"
           | "option<" type ">"
           | "result<" arm "," arm ">"

field     := name ":" type
case      := name [ "(" type ")" ]
arm       := type | "_"
```

For example, `add: func(lhs: vector3d, rhs: vector3d) -> vector3d` where
`vector3d` is `tuple<f32, f32, f32>` renders as:

```
add(tuple(f32,f32,f32),tuple(f32,f32,f32))->(tuple(f32,f32,f32));
```

What does and does not contribute:

- **Functions are sorted by name**, so reordering them in the WIT source cannot
  raise a false conflict. They are addressed individually by subject, so their
  order carries no meaning.
- **Parameter names are excluded.** Arguments travel positionally, so renaming a
  parameter cannot break a peer.
- **Field, case, enum and flag names are included, in declaration order.**
  Reordering record fields is in fact wire-compatible, since records decode by
  name — but it changes the encoded bytes, because §4 emits fields in
  declaration order. Including order means equal digests promise *identical
  encodings*, which is a stronger and more useful guarantee than mere mutual
  decodability.
- **Type aliases are resolved.** The rendering is fully structural, so
  `vector3d` and a bare `tuple<f32, f32, f32>` are indistinguishable, as they
  are on the wire.
- Documentation comments, whitespace, and the order of independent type
  declarations do not contribute.

Two interfaces with equal digests encode identically under this protocol.
Unequal digests mean the two definitions disagree; they do not necessarily mean
the two are unable to talk to each other.
