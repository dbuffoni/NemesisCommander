# 0002. Workspace layout and crate boundaries

- Status: Accepted
- Date: 2026-11-15

## Context

The architecture lists the crates `sc-vfs`, `sc-providers`, `sc-ops`,
`sc-transfer`, `sc-index`, `sc-proto` and `sc-script`. Implementation showed
a few needs that do not fit cleanly in that list.

## Decision

All crates live in `crates/` in one Cargo workspace. Shared dependency
versions are declared once in `[workspace.dependencies]`. In addition to
the crates of the architecture:

- **`sc-hash`** holds streaming checksums (BLAKE3, XXH3, SHA-256). It is
  needed by providers (server-side hashing), by `sc-ops` (verification) and
  by the CLI, and must not drag `sc-ops` into provider crates.
- **`walk`** (recursive traversal) lives in `sc-vfs`, because both
  `sc-ops` and `sc-index` need it and it only depends on the `Provider`
  trait.
- **`sc-providers::memory`**: an in-memory provider with configurable
  capabilities. It is the reference implementation of the trait semantics
  and lets tests simulate weak backends (no atomic rename, no resume)
  without network servers.
- **Retry** of failed steps is implemented in `sc-ops::Executor` (as
  `RetryPolicy`) rather than in `sc-transfer`, because the executor is the
  only place that knows which steps run in a parallel batch. `sc-transfer`
  decides the policy; `sc-ops` applies it.

The crate dependency direction is strictly
`sc-vfs ← sc-hash ← sc-providers / sc-ops ← sc-transfer ← front ends`.

## Consequences

Crates stay small and individually testable. Front ends (CLI, TUI, GUI,
agent) never talk to concrete providers directly; they go through a
`ProviderRegistry`.
