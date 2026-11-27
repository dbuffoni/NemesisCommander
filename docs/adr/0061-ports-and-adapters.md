# 0061. Ports and adapters for the interfaces

- Status: Accepted
- Date: 2026-11-27

## Context

The front end (`pnxr-frontend`, about 17k lines, shared by the terminal and the
window) called adapters directly: the system keyring, `~/.netrc`, S3 key files,
SSH, the agent installer, terminals, Git and `lsblk`. Tests of those paths either
touched the real system or used process-wide overrides. `pnxr-vfs`, meant to hold
types and traits only, also held the keyring and netrc code.

## Decision

A reduced hexagonal layout, one step at a time, each with its own regression
test and no trait without a second user (a fake):

1. **`pnxr-secrets`** is a new crate with the keyring backends, `netrc` and the
   `SecretStore` trait (`SystemSecrets`, `MemorySecrets`). `pnxr-vfs` is back to
   types and traits. Providers still read logins with `netrc::lookup`, which
   tries the session, the keyring and the file.
2. **`HostTools`** (in `pnxr-frontend::ports`): starting a terminal, reading Git
   status, listing disks. `SystemHost` runs the code of `terminal`, `git` and
   `disks`; `FakeHost` starts nothing. The app takes it with `App::set_host`.
3. **`Remotes`** (in `pnxr-app`): SSH targets, S3 keys, the `pnxr serve` install.
   `SystemRemotes` calls `pnxr-ssh`, `pnxr-s3` and `pnxr-agent`; `FakeRemotes`
   runs nothing. The front end no longer lists `pnxr-agent`, `pnxr-ssh` or
   `pnxr-s3` as dependencies.
4. `app.rs` lost its two longest groups of methods to `app/events.rs`
   (background work) and `app/keys.rs` (key handling); no behaviour changed and
   every existing key-driven test passes untouched.
5. **A guard test** (`pnxr-frontend/tests/architecture.rs`) fails if
   `pnxr-frontend` or `pnxr-tui` list an adapter crate under `[dependencies]`.

`AppContext` (the composition root, `pnxr-app`) now also carries `secrets` and
`remotes`; tests replace them.

## Consequences

- Dialogs for logins, S3 keys, the server install, terminals, Git and disks are
  tested with fakes: no keyring, no SSH, no process.
- Left as they are on purpose: `pnxr-ops` and `pnxr-config` (file access for
  speed and for settings), the Git *actions*, plugin programs and external
  editors (they run through the existing `External` mechanism), and the code of
  `terminal`, `git` and `disks`, which stays in the front end as the adapter
  behind `HostTools`. Moving it to its own crate is possible later.
- Without the `remote` feature `SystemRemotes` has no S3; the front end asks for
  `pnxr-app/remote` as before, so desktop builds are unchanged.
- Footprint budgets are unchanged in principle (same code, other crates);
  `scripts/footprint.py` in CI confirms it.
