# Registry

Status: draft · Applies to `ardo314:registry@0.1.0`

Defines how providers announce themselves and how hosts find them. The WIT
definition is `wit/registry/world.wit`; this document covers the behaviour that
the types alone cannot express.

## 1. What the registry is for

A host loading a component discovers imports it cannot satisfy locally. The
registry answers one question: *who claims to implement this interface, and can
I load them in-process or must I call them remotely?*

The registry reports facts and nothing more. It does not rank providers, and it
does not decide how a link is made — that policy lives in the host, which
prefers in-process linking and falls back to the wire only when no loadable
provider exists. Keeping the policy out of the registry means a host can change
its preferences without a cluster-wide migration.

## 2. Provider kinds

`provider-kind` is the field that matters most.

| Kind        | Meaning                                                              |
| ----------- | -------------------------------------------------------------------- |
| `component` | A wasm component. A host may fetch and instantiate it in-process.    |
| `service`   | An ordinary process. Reachable only over the wire.                   |

A `component` provider carries an `artifact` endpoint; a `service` provider
carries a `nats` endpoint. A component that also serves its exports over the
wire registers twice, once under each kind, because the two offers have
genuinely different capabilities.

The distinction is not cosmetic. An interface whose signatures use `resource`,
`future` or `stream` cannot cross a process boundary at all under the current
wire protocol, so for such interfaces a `service` provider is useless and a host
must refuse it rather than fail at the first call.

## 3. Lifecycle

```
register ──► live ──► deregister ──► gone
               │
               └── no heartbeat within ttl-secs ──► expired ──► gone
```

- `register` creates or **replaces** the entry with that `id`. A restarted
  provider reusing its id updates its registration rather than duplicating it,
  which is why ids must be stable across restarts of the same deployment.
- `heartbeat` extends the entry by `ttl-secs`.
- `deregister` removes it immediately. This is an optimisation, not a
  requirement: a provider that dies without deregistering expires on its own.
- `heartbeat` on an expired or unknown id returns `not-found`. The caller must
  treat that as an instruction to `register` again, not as a fatal error — it
  happens routinely when a provider is partitioned from the registry for longer
  than its TTL.

### TTL

`ttl-secs` is proposed by the provider and may be clamped by the registry to its
own bounds. Providers should heartbeat at roughly `ttl-secs / 3` so that two
consecutive lost heartbeats do not expire a healthy provider.

Short TTLs detect death faster but cost more heartbeat traffic. The trade-off is
the provider's to make, within the registry's bounds.

## 4. Staleness

**`resolve` is not a liveness check.** A returned provider may already be dead —
up to `ttl-secs` may pass before its entry expires. Callers must treat resolution
as a hint and handle failure at call time.

Concretely, a host that resolves a `service` provider and then gets no responder
on its subject should retry resolution rather than concluding the interface is
unavailable. The wire protocol's `not-found` error code covers exactly this case.

The converse also holds: a provider that registered moments ago may not yet be
visible to every caller. Registration is not required to be instantly globally
visible, only eventually so.

Clients may cache resolutions. A cache that is invalidated by watching the
registry for changes will be more responsive than one that polls, but neither is
authoritative — the call itself is the only proof a provider is alive.

## 5. Shape conflicts

`interface-ref.shape-digest` is the digest defined in
`docs/spec/wire-protocol.md` §6.

If a provider registers an interface at a name and version already claimed with
a *different* digest, the registry rejects the registration with `conflict`. The
two definitions disagree despite claiming the same version, and silently
admitting both would let a host pick either and fail unpredictably at the first
call that touches the differing type.

The correct response to `conflict` is to bump the interface version, not to
retry. This is the mechanism that makes an unversioned change to a WIT file
noisy rather than silent.

A host should additionally compare the digest it derived from the importing
component against the digest advertised by the provider, and refuse a provider
that disagrees. The registry cannot do this on the host's behalf, because it
never sees the importing component.

## 6. Errors

| Variant       | When                                                            |
| ------------- | --------------------------------------------------------------- |
| `conflict`    | Interface and version already claimed with a different digest.  |
| `not-found`   | No such provider id, or the registration already expired.       |
| `invalid`     | Malformed input, such as an unparseable version requirement.    |
| `unavailable` | The registry could not service the request; retrying may work.  |

`resolve` returning an empty list is a success, not `not-found`: the question
"who implements this?" was answered, and the answer was "nobody".
