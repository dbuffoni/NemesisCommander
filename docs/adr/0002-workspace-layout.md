# 0002. Workspace layout and crate boundaries

- Status: Accepted
- Date: 2026-11-15

## Context

The architecture lists the crates `nmc-vfs`, `nmc-providers`, `nmc-ops`,
`nmc-transfer`, `nmc-index`, `nmc-proto` and `nmc-script`. Implementation showed
a few needs that do not fit cleanly in that list.

## Decision

All crates live in `crates/` in one Cargo workspace. Shared dependency
versions are declared once in `[workspace.dependencies]`. In addition to
the crates of the architecture:

- **`nmc-hash`** holds streaming checksums (BLAKE3, XXH3, SHA-256). It is
  needed by providers (server-side hashing), by `nmc-ops` (verification) and
  by the CLI, and must not drag `nmc-ops` into provider crates.
- **`walk`** (recursive traversal) lives in `nmc-vfs`, because both
  `nmc-ops` and `nmc-index` need it and it only depends on the `Provider`
  trait.
- **`nmc-providers::memory`**: an in-memory provider with configurable
  capabilities. It is the reference implementation of the trait semantics
  and lets tests simulate weak backends (no atomic rename, no resume)
  without network servers.
- **Retry** of failed steps is implemented in `nmc-ops::Executor` (as
  `RetryPolicy`) rather than in `nmc-transfer`, because the executor is the
  only place that knows which steps run in a parallel batch. `nmc-transfer`
  decides the policy; `nmc-ops` applies it.

The crate dependency direction is strictly
`nmc-vfs ← nmc-hash ← nmc-providers / nmc-ops ← nmc-transfer ← front ends`.

## Consequences

Crates stay small and individually testable. Front ends (CLI, TUI, GUI,
agent) never talk to concrete providers directly; they go through a
`ProviderRegistry`.
