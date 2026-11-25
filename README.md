<p align="center"><img src="assets/branding/nemesiscommander-logo.png" alt="NemesisCommander" width="360"></p>

# NemesisCommander

A fast dual-pane file manager for local and remote files, written in Rust.

- A desktop window (Qt 6) and a terminal interface that work the same way,
  driven by the keyboard: no toolbars; F9 finds any command, F1 shows the shortcuts.
- Local folders, SFTP, SMB, WebDAV, S3 and archives (ZIP, 7-Zip, tar, RAR)
  with the same commands.
- A built-in viewer for text, hex, Markdown, Word, spreadsheets, slides, PDF,
  3D models and pictures, also as a quick view beside the files.
- Background transfers with a queue, verification and resume after crashes.
- Multi-rename with templates and a preview of the new names, or with
  rules written by Claude (`nmc ai rename`).
- A small static agent for Linux servers (`nmc agent` over SSH, `nmc serve`
  over mutual TLS) for fast remote work and server-to-server copies.
- Plugins and scripts in TypeScript, in a sandbox, tested with `nmc test`
  and, with your Anthropic API key, written by Claude (`nmc ai new`).
- `nmc`, a command line for all of it.

![NemesisCommander](docs/manual/images/panels-1280x800.png)

Read the **[manual](docs/manual/README.md)** for everything it does. Design
decisions are in [docs/adr](docs/adr), scripting in
[docs/scripting.md](docs/scripting.md).

## Install

Download the AppImage (window and `nmc`) or a static `nmc-agent` from the
[releases](https://github.com/dbuffoni/NemesisCommander/releases). Releases are
for Linux (x86_64; the agent also for aarch64); other systems can build from
source. The SSH connections use the system's OpenSSH client.

## Building

```sh
cargo build --release              # nmc: terminal interface, CLI, agent
cargo build --release -p nmc-gui    # the window (needs Qt 6.8 or later)
cargo test --workspace
```

You need a C compiler (bundled SQLite and zstd). Contributions are welcome:
see [CONTRIBUTING.md](CONTRIBUTING.md); security reports go through
[SECURITY.md](SECURITY.md).

Build the window with its own cargo command: built together with `nmc`, it
would link the script engine it does not need (ADR 0020).

Linux releases are an AppImage of the window plus static agents for x86_64
and aarch64, built by `.github/workflows/release.yml`, which also measures
sizes and idle memory with `scripts/footprint.py` and fails over budget
(ADR 0020).

## License

MIT, see [LICENSE](LICENSE). Release builds read RAR archives with
libarchive (BSD-2-Clause, bundled in `crates/nmc-libarchive`).
