<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/branding/pairnox-wordmark-on-dark-1100.png">
    <img src="assets/branding/pairnox-wordmark-on-light-1100.png" alt="Pairnox" width="300">
  </picture>
</p>

# Pairnox

_Formerly NemesisCommander (`nmc`)._

A fast dual-pane file manager for local and remote files, written in Rust.

- A desktop window (Qt 6) and a terminal interface that work the same way,
  driven by the keyboard: no toolbars; F9 finds any command, F1 shows the shortcuts.
- Local folders, SFTP, SMB, WebDAV, S3 and archives (ZIP, 7-Zip, tar, RAR)
  with the same commands.
- A built-in viewer for text, hex, Markdown, Word, spreadsheets, slides, PDF,
  3D models and pictures, also as a quick view beside the files.
- Background transfers with a queue, verification and resume after crashes.
- Multi-rename with templates and a preview of the new names, or with
  rules written by Claude (`pnxr ai rename`).
- A small static agent for Linux servers (`pnxr agent` over SSH, `pnxr serve`
  over mutual TLS) for fast remote work and server-to-server copies.
- Plugins and scripts in TypeScript, in a sandbox, tested with `pnxr test`
  and, with your Anthropic API key, written by Claude (`pnxr ai new`).
- `pnxr`, a command line for all of it.

![Pairnox](docs/manual/images/panels-1280x800.png)

Read the **[manual](docs/manual/README.md)** for everything it does. Design
decisions are in [docs/adr](docs/adr), scripting in
[docs/scripting.md](docs/scripting.md).

## Install

Download the AppImage (window and `pnxr`) or a static `pnxr-agent` from the
[releases](https://github.com/dbuffoni/Pairnox/releases). Releases are
for Linux (x86_64; the agent also for aarch64); other systems can build from
source. The SSH connections use the system's OpenSSH client.

## Building

```sh
cargo build --release              # pnxr: terminal interface, CLI, agent
cargo build --release -p pnxr-gui    # the window (needs Qt 6.4 or later)
cargo test --workspace
```

You need a C compiler (bundled SQLite and zstd). Contributions are welcome:
see [CONTRIBUTING.md](CONTRIBUTING.md); security reports go through
[SECURITY.md](SECURITY.md).

Build the window with its own cargo command: built together with `pnxr`, it
would link the script engine it does not need (ADR 0020).

Linux releases are an AppImage of the window plus static agents for x86_64
and aarch64, built by `.github/workflows/release.yml`, which also measures
sizes and idle memory with `scripts/footprint.py` and fails over budget
(ADR 0020).

## License

MIT, © 2026 Daniele Buffoni, see [LICENSE](LICENSE). The files in `assets/` (logo,
icons) are released under the same MIT license. Release builds read RAR archives with
libarchive (BSD-2-Clause, bundled in `crates/pnxr-libarchive`).
