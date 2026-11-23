# 0006. Archives are read-only providers; packing is a separate operation

- Status: Accepted
- Date: 2026-11-15

## Context

The base version must read and write zip, tar, gz, xz, bz2, zstd and 7z,
read rar, and browse archives like folders, all natively.

## Decision

- **Browsing** is an `ArchiveProvider` per archive scheme (`zip`, `tar`,
  `7z`, `rar`, and `gz`/`xz`/`bz2`/`zst` for single compressed files, which
  appear as a folder with one file). The provider is read-only. Extracting
  is an ordinary `nmc-ops` copy out of the archive, so it gets conflict
  handling, progress, verification and the transfer queue for free.
- **Packing** (`nmc_archive::pack`) walks sources on any provider and feeds a
  blocking writer thread through bounded channels. The archive is written to
  a hidden part file next to the target and renamed at the end (or written
  to a temp file and uploaded for remote targets).
- `tar://` sniffs its compression from magic bytes, so one scheme serves
  `.tar`, `.tar.gz`, `.tar.xz`, `.tar.bz2` and `.tar.zst`.
- Archive libraries need random access. Archives that are not on the local
  disk (remote, or nested inside another archive) are first copied to a
  temporary file, kept for the provider's lifetime.
- Member names containing `..` are dropped while indexing, so a malicious
  archive can never make an extraction escape its target directory.
- Updating an existing archive in place (add/delete members) is not part of
  this decision; it can later be done as "repack to a part file, rename".
- **RAR** reading uses the `unrar` crate (bindings to RARLAB's unRAR C++
  library). unRAR's license is not OSI-approved (it forbids using the code
  to re-create RAR compression), so it is behind the optional Cargo feature
  `rar`, off by default. Whether release builds enable it is an open
  question tied to the project license (see `docs/OPEN_QUESTIONS.md`).
- 7z has no portable symlink entry; symlinks are stored as small files
  containing the link target.
- URI schemes may start with a digit (deviation from RFC 3986) so that the
  natural `7z://` scheme works.

## Consequences

- Browsing a large remote archive costs one full download first.
- Reading at an offset inside an archive skips data by decompressing it;
  archive providers therefore do not advertise `SEEK_READ`, and resumed
  copies out of archives restart the file.
