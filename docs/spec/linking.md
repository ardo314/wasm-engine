# Linking

Status: draft · Applies to the host in `framework/crates/host`

Defines how a component's imports are connected to implementations once the
resolver has decided what satisfies each one.

## 1. One store per component, except where it cannot be

Each component the host instantiates gets its own `wasmtime::Store`.

Two components must share a store when the interface linking them has no wire
shape — that is, when it carries a `resource`, `future` or `stream`. Those
handles live in one store's table and cannot be passed to another, so
separating such components would make the interface unlinkable. This is the
same distinction the import scanner already draws: `Linkage::InProcess` means
"cannot cross a process boundary", and it means "cannot cross a store" for the
same reason.

Components joined by that rule form an **isolation group**, computed
transitively. A group is one store. Everything else is separate.

An import that cannot cross stores, bound to a live component already in a
different group, is refused: one store cannot be two.

The grouping is in place; forwarding a guest-owned resource between two
components is not. The linker defines an interface's functions and not its
resource types, so such a link fails at instantiation with `resource
implementation is missing`. Grouping them correctly is what makes fixing that
possible.

## 2. What a cross-store call costs

Within a group, forwarding is a direct re-entrant `Func::call_async` on the
shared store — the caller's lowered arguments are the callee's, and nothing is
copied.

Across groups, the arguments are cloned into owned `Val`s and the callee's
store is locked for the duration of the call. That is far cheaper than the
msgpack round trip a remote import costs, and it buys §3 and §4.

## 3. A trap is contained to its group

A trap in a callee surfaces in the caller as a trap: the forwarding host
function returns the error from `call_async` unchanged, and the caller sees the
callee's own backtrace.

Once a trap has escaped, every subsequent call into **any instance in that
store** fails with `wasm trap: cannot enter component instance`. This is
store-wide, not instance-wide, and it is permanent: a poisoned group has to be
discarded and rebuilt.

The blast radius is therefore the isolation group. Components in other groups
are unaffected — though a component that called into the group and received the
error will itself have trapped, and so poisoned its own group in turn. Only
components that were not on the call chain survive.

`post-return` needs no handling. `Func::call` and `Func::call_async` both
invoke it as part of the call.

## 4. Guest calls within a store are serial

`Func::call_async` holds its store exclusively for the duration of the call.
Two calls into the same group therefore do not overlap, even when one is parked
on a host future such as a NATS round trip.

This is a property of the component model as wasmtime 48 implements it, not a
choice. Wasmtime's concurrent API — `func_new_concurrent`, `call_concurrent`,
`Store::run_concurrent` — refuses any import whose WIT type is a plain
function:

```
type mismatch with async: this import's WIT type is a plain (non-`async`)
function, but was satisfied with `func_new_concurrent`/`func_wrap_concurrent`,
which is only for `async func`-typed imports
```

Overlapping calls *within* a group would need the interfaces in `wit/` declared
`async func`, and the guest toolchains to support it.

Across groups there is no such limit. A component parked on a slow remote
import holds only its own store, so components in other groups keep running.
Since a component shares a store only when resources force it, a slow import
usually blocks the component that made it and nothing else.

## 5. Unsatisfied imports

Under `Missing::Fail` the resolver refuses to produce a plan, so the linker
never sees an unsatisfied import.

Under `Missing::Trap` the host calls `Linker::define_unknown_imports_as_traps`,
which stubs every remaining import with a function that traps when called. The
component loads and everything it can do without that import still works.

## 6. Remote imports

An import the resolver bound to a service is satisfied by `wasm-nats-link`,
which defines each of the interface's functions as a NATS request/reply. The
shape the *importing* component was built against is what calls are encoded
with, so it travels on the binding rather than being re-derived.

A host with no proxy refuses such a binding outright rather than loading a
component whose import would fail at the first call.
