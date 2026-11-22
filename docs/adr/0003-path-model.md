# 0003. Paths are URIs with nested archive containers

- Status: Accepted
- Date: 2026-11-15

## Context

The architecture describes paths as URIs (`file://`, `sftp://`, `s3://`,
`agent://`, `zip://…!/`). We need a precise, unambiguous syntax, including
archives stored on remote hosts and archives inside archives.

## Decision

- `VfsPath` has a scheme, an authority (`user@host:port`), a normalized
  absolute path, and, for archive schemes only, a *container* `VfsPath`.
- Archive URIs are `scheme://<container>!/<inner path>`; the container is
  everything up to the **last** `!/`, so nesting is unambiguous:
  `zip://zip:///a.zip!/b.zip!/c`.
- Archive schemes are a closed list (`ARCHIVE_SCHEMES`); a `!/` inside an
  ordinary SFTP path is therefore just part of a file name.
- A bare absolute path (`/home/me`, `C:\Users`) is shorthand for `file://`.
- `parent()` of an archive root returns the directory containing the
  archive file, matching what "go up" means to a user.
- Paths are stored as UTF-8 `String`s.

## Consequences

- Paths are cheap to compare, hash, serialize (as their URI string) and
  send over the agent protocol.
- **Limitation:** file names that are not valid UTF-8 (possible on Linux)
  are shown with replacement characters and cannot be addressed exactly.
  This is rare in practice; supporting it later means switching the inner
  path to bytes and percent-encoding the URI form.
- Plugin-provided archive formats will need a way to extend
  `ARCHIVE_SCHEMES` (phase 4).
