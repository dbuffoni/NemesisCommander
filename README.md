# SuperCommander

A fast dual-pane file manager for local and remote files, written in Rust.

- A desktop window (Qt 6) and a terminal interface that work the same way,
  driven by the keyboard: no toolbars; F9 finds any command, F1 shows the shortcuts.
- Local folders, SFTP, SMB, WebDAV, S3 and archives (ZIP, 7-Zip, tar, RAR)
  with the same commands.
- A built-in viewer for text, hex, Markdown, Word, spreadsheets, slides, PDF,
  3D models and pictures, also as a quick view beside the files.
- Background transfers with a queue, verification and resume after crashes.
- A small static agent for Linux servers (`sc agent` over SSH, `sc serve`
  over mutual TLS) for fast remote work and server-to-server copies.
- Plugins and scripts in TypeScript, in a sandbox.
- `sc`, a command line for all of it.

![SuperCommander](docs/manual/images/panels-1280x800.png)

Read the **[manual](docs/manual/README.md)** for everything it does. Design
decisions are in [docs/adr](docs/adr), scripting in
[docs/scripting.md](docs/scripting.md).

## Building

```sh
cargo build --release              # sc: terminal interface, CLI, agent
cargo build --release -p sc-gui    # the window (needs Qt 6.8 or later)
cargo test --workspace
```

Linux releases are an AppImage of the window plus static agents for x86_64
and aarch64, built by `.github/workflows/release.yml`.

## License

MIT, see [LICENSE](LICENSE). Release builds read RAR archives with the unRAR
library, which has its own license: see [TODO.md](TODO.md).
