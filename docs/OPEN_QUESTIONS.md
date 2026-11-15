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
