# AGENTS.md

Guidance for coding agents working in this repository.

## Workflow: one branch per issue

Every unit of work starts from a GitHub issue and ends in a squash-merged pull
request. Do not commit to `main`.

1. **Branch.** Cut a fresh branch from an up-to-date `main`, named
   `<type>/<issue-number>-<slug>` — for example `feat/4-import-scanner`,
   `fix/12-flags-ordering`, `docs/1-registry-spec`. Types: `feat`, `fix`,
   `docs`, `chore`.
2. **Work.** Keep the branch scoped to that one issue. If you discover adjacent
   work, open a new issue rather than widening the branch.
3. **Verify.** Run the checks in [Commands](#commands) before opening the PR.
4. **Open a PR.** Target `main`, and link the issue with `Closes #N` in the body
   so it closes on merge. Summarise what changed and how it was verified.
5. **Stop and wait.** **Never merge without explicit approval from the
   repository owner.** Opening the PR ends the agent's turn. Do not use
   `--auto`, do not enable auto-merge, do not merge your own PR because CI went
   green.
6. **Merge on approval only.** Once the owner approves, squash merge and delete
   the branch:

   ```pwsh
   gh pr merge <number> --squash --delete-branch
   ```

The repository is configured to enforce the shape of this: squash is the only
permitted merge method, the head branch is deleted automatically on merge, and
the squash commit takes its title and body from the PR. Approval is a human
gate, not a mechanical one — the settings do not enforce it for you.

## Layout

```
wit/                       WIT packages — the source of truth for interfaces
framework/crates/          the framework itself
  host/                    scanning what a component imports, and deciding
                           how each import gets satisfied
  protocol/                wire format: subjects, envelope, Val <-> msgpack
  registry/                registryd: providers in a JetStream KV bucket, and
                           the client hosts and services reach it with
examples/                  reference components and services
docs/spec/                 normative protocol specifications
```

## Commands

```pwsh
cargo test --workspace                              # native crates
cargo test -p wasm-protocol --features val-codec    # includes the wasmtime bridge
cargo fmt --all
cargo clippy --workspace --all-targets
cargo component build -p math --release             # wasm components (devcontainer)
docker compose up -d nats                           # dependency for integration tests
```

Integration tests skip themselves when no NATS answers on `$NATS_URL`
(`nats://127.0.0.1:4222` by default), so bring the compose service up before
trusting a green `cargo test --workspace`.

`cargo-component` is not installed on every machine; it is present in the
devcontainer defined by `.devcontainer/Dockerfile`.

## Conventions

- **`docs/spec/` is normative.** Changing an encoding rule means changing the
  spec, every implementation of it, and the conformance vectors in the same PR.
  Code and spec must not drift.
- **The host is type-driven, not codegen-driven.** `WitType` is derived from
  `wasmtime::component::Type` at link time, so adding a WIT interface requires
  no host changes. Do not introduce per-interface host code.
- **Keep wasmtime out of service SDKs.** `wasm-protocol` gates its wasmtime
  bridge behind the `val-codec` feature so native services do not link a wasm
  runtime. Check with `cargo tree` before adding a dependency.
- **Comment only what the code cannot say.** No restating the next line, no
  multi-paragraph doc comments where one line will do.
- Do not create markdown files documenting your changes; the PR body is where
  that belongs.
