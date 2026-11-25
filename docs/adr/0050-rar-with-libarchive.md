# 0050. RAR is read with libarchive

Date: 2026-11-25. Replaces the RAR part of [ADR 0006](0006-archives.md) and
the licence decision of 2026-11-15.

## Context

NemesisCommander is MIT. The `unrar` crate links RARLAB's unRAR source, whose
licence forbids re-creating the RAR compressor and is not OSI-approved, so a
release containing it had to carry a special notice. Only reading and
extracting RAR is needed; nothing creates RAR files.

## Decision

- RAR 3/4 and RAR 5 are read with **libarchive** (BSD-2-Clause), which has its
  own reader written without RARLAB's code.
- Not a Rust binding: the crates that wrap libarchive link whatever libraries
  the build machine has (zlib, lz4, libxml2, OpenSSL, ACL ...), which breaks
  portable builds. Instead `crates/nmc-libarchive` bundles the libarchive
  3.8.1 source (`vendor/`, see `vendor/NMC-README.md`), builds it as a static
  library with CMake and **every optional library off** (RAR needs none), and
  exposes a tiny reader (open, next member, read data) over about a dozen
  `extern "C"` functions. No dynamic dependency is added to the executable.
- The feature keeps its name: `nmc-archive/rar` (and `rar` on the CLI and
  window), on in release builds, off in the static agent builds.
- Not supported, with a clear message instead of a crash: encrypted archives
  and multi-volume sets.
- Extracting one file of a solid archive decodes the archive from its start up
  to that file (the format allows nothing else).
- Tests use small samples from libarchive's own suite
  (`crates/nmc-archive/tests/rar-samples`); CI fails if the `unrar` crate is
  back in `cargo tree`.

## Consequences

- The release carries no unRAR code; libarchive's licence goes in the
  third-party notices.
- A C compiler and CMake are needed to build with `rar` (the CI runners have
  both).
- The repository grows by the vendored libarchive source (about 4.6 MB).
