# Open questions for the project owner

Decisions that should not be taken without the owner.

1. **License.** GPL (like Double Commander) or MIT/Apache-2.0 (friendlier to
   plugins and integrations)? Note: the repository's initial commit already
   contains an MIT `LICENSE` file created by GitHub. It was left untouched;
   `Cargo.toml` deliberately declares no license until this is decided.

2. **RAR in release builds.** RAR reading needs RARLAB's unRAR library,
   whose license is not OSI-approved. It is behind the Cargo feature `rar`
   (off by default). Enable it in the AppImage? Debian ships it in
   `non-free`; with a GPL license the combination is questionable.

3. **Dependency licenses for the chosen license.** All current dependencies
   are MIT, Apache-2.0, BSD or similar, except optional `unrar` (see 2).
   `smb2` is MIT/Apache. If the project picks MIT/Apache, nothing changes;
   if GPL, the same set is compatible.

4. **Qt in the AppImage.** The window links Qt 6 dynamically and the
   AppImage bundles Qt's shared libraries (LGPLv3). That is allowed with any
   of the licenses above, but the AppImage should then ship Qt's license
   text and say where Qt's source is. To do once the project license is
   chosen, together with a third-party notices file.

5. **Windows and macOS packages of the window.** The window builds on the
   same code everywhere, but only the Linux AppImage is produced. Installers
   for Windows (MSI/NSIS with windeployqt) and macOS (signed .dmg with
   macdeployqt) need signing certificates that only the owner can provide.
