# Bundled libarchive

libarchive 3.8.1 (BSD-2-Clause, see COPYING), taken from the `libarchive2-sys`
crate's copy of the upstream release. Changes: the tests, the `tar`, `cpio`,
`cat` and `unzip` programs and the documentation were removed, and the
`add_subdirectory` lines for them deleted from the CMake files. Nothing else
is changed. `build.rs` builds only the static library, with every optional
library (zlib, bzip2, xz, zstd, lz4, OpenSSL, libxml2 ...) switched off.
