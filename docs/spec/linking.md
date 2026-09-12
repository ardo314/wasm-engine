# Linking

Status: draft · Applies to the host in `framework/crates/host`

Defines how a component's imports are connected to implementations once the
resolver has decided what satisfies each one.

## 1. One store per host

A host owns a single `wasmtime::Store` and instantiates every component into
it, one `Instance` per component.

Forwarding an in-process call is then a direct re-entrant `Func::call_async` on
the same store. Nothing is serialised, copied or re-encoded: the caller's
lowered arguments are the callee's, and a `resource` handle stays valid because
both instances share one resource table.

The alternative — a store per component — buys isolation of fuel, epoch
deadlines and traps, but costs a `Val` copy on every cross-component call, and
makes forwarding structurally awkward: a host function running with `&mut` on
the caller's store would need `&mut` on the callee's at the same time, which
Rust will not give and which deadlocks under interior mutability the moment two
components call each other.

This section is provisional. §2 and §3 record two consequences of it that were
not anticipated when it was chosen, and either may be reason enough to pay the
copy.

## 2. Guest execution within a host is serial

`Func::call_async` holds the store exclusively for the duration of the call.
Two guest calls into the same host therefore do not overlap, even when one is
parked on a host future such as a NATS round trip.

This is a property of the component model as wasmtime 48 implements it, not a
choice. Wasmtime's concurrent API — `func_new_concurrent`, `call_concurrent`,
`Store::run_concurrent` — refuses any import whose WIT type is a plain
function:

```
type mismatch with async: this import's WIT type is a plain (non-`async`)
function, but was satisfied with `func_new_concurrent`/`func_wrap_concurrent`,
which is only for `async func`-typed imports
```

Overlapping calls are therefore available only to interfaces declared
`async func` in WIT. Until the interfaces in `wit/` are declared that way and
the guest toolchains support it, a host is one logical thread of execution and
concurrency comes from running more than one host.

**A slow remote import blocks every component in its host.** A deployment that
cannot tolerate that must not place the affected component in a shared host.

## 3. A trap poisons the host

A trap in a callee surfaces in the caller as a trap: the forwarding host
function returns the error from `call_async` unchanged, and wasmtime unwinds
the caller's fiber. The caller sees the callee's own backtrace.

**The store does not survive it.** Once a trap has escaped, every subsequent
call into *any* instance in that store fails with:

```
wasm trap: cannot enter component instance
```

This is store-wide, not instance-wide: an unrelated component instantiated
before the trap, and one instantiated after it, are both unreachable. Combined
with §1, that means one misbehaving component takes down every component
sharing its host.

A host that has taken a trap must therefore be discarded and rebuilt. Callers
cannot recover in place.

This is a poor property for a multi-tenant host and is the strongest argument
against §1's single store. It is recorded here as the measured behaviour of
wasmtime 48, not as a design goal.

`post-return` needs no handling here. `Func::call` and `Func::call_async` both
invoke it as part of the call.

## 4. Unsatisfied imports

Under `Missing::Fail` the resolver refuses to produce a plan, so the linker
never sees an unsatisfied import.

Under `Missing::Trap` the host calls `Linker::define_unknown_imports_as_traps`,
which stubs every remaining import with a function that traps when called. The
component loads and everything it can do without that import still works.

## 5. Remote imports

An import the resolver bound to a service is satisfied by `wasm-nats-link`,
which defines each of the interface's functions as a NATS request/reply. The
shape the *importing* component was built against is what calls are encoded
with, so it travels on the binding rather than being re-derived.

A host with no proxy refuses such a binding outright rather than loading a
component whose import would fail at the first call.
