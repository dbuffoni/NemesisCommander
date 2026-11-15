# TODO

Things decided but not finished, and notes to act on before a public
release.

## Licensing

- **RAR support uses a non-free library.** Release builds (the AppImage)
  enable the `rar` feature, which links RARLAB's unRAR source through the
  `unrar` crate. Its license allows free use and redistribution but forbids
  using the code to recreate the RAR compression algorithm, and it is not
  OSI-approved. SuperCommander itself is MIT; the AppImage must ship
  unRAR's license text in its third-party notices, and distributions that
  only accept free software should build without `--features rar`
  (archives then open everything except RAR).
- **Third-party notices.** Ship a notices file in the AppImage with the
  licenses of Qt (LGPLv3, dynamically linked, with where to get its
  source), unRAR, V8 and the Rust dependencies (`cargo about` can generate
  them).
