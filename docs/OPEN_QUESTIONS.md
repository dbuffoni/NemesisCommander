# Open questions for the project owner

Decisions that should not be taken without the owner.

## Decided

- **License: MIT** (2026-11-15). The `LICENSE` file from the initial commit
  stays; every crate declares `license = "MIT"`.
- **RAR in release builds: enabled, read with libarchive** (2026-11-25,
  [ADR 0050](adr/0050-rar-with-libarchive.md)); this replaces the unRAR
  decision of 2026-11-15. Qt's LGPL notices are listed in
  [`TODO.md`](../TODO.md).

## Open

1. **Windows and macOS packages of the window.** The window builds on the
   same code everywhere, but only the Linux AppImage is produced. Installers
   for Windows (MSI/NSIS with windeployqt) and macOS (signed .dmg with
   macdeployqt) need signing certificates that only the owner can provide.

2. **High-level functions in phase 4.** The roadmap puts "TypeScript
   extensions and high-level functions" in phase 4, while the base version
   leaves out folder synchronization and disk utilities (space used,
   cleanup, duplicates). Extensions are done (ADR 0015, 0016); the
   high-level functions are not started. Build them natively next, leave
   them to plugins, or keep them out for now?

3. **Plugin trust.** A grant belongs to a plugin's name and request
   (ADR 0016). If plugins are shared publicly, should they be signed, and
   should NemesisCommander keep a list of known publishers?
