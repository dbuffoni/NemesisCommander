# Architecture Decision Records

Each file records one decision: the context, what was decided, and its
consequences. Records are immutable once accepted; a later record can
supersede an earlier one. Records that only logged the content of a release
batch (cards done, small choices) were removed in 0.3.0: the manual describes
what the program does, `CHANGELOG.md` lists releases, and the git history keeps
the old text. Their numbers are not reused, so some are missing below.

Record 0029 describes the rename from the former name and so is the only one
that still names it.

| #    | Title | Status |
|------|-------|--------|
| 0001 | [Record architecture decisions](0001-record-architecture-decisions.md) | Accepted |
| 0002 | [Workspace layout and crate boundaries](0002-workspace-layout.md) | Accepted |
| 0003 | [Paths are URIs with nested archive containers](0003-path-model.md) | Accepted |
| 0004 | [Operations are plans of idempotent steps](0004-plans-of-idempotent-steps.md) | Accepted |
| 0005 | [Transfer journal in SQLite, planning at start time](0005-transfer-journal.md) | Accepted |
| 0006 | [Archives are read-only providers; packing is a separate operation](0006-archives.md) | Accepted |
| 0007 | [Front ends share one model; the TUI is an event-driven state machine](0007-front-ends-share-one-model.md) | Accepted |
| 0008 | [The agent speaks gRPC over the SSH session's standard streams](0008-agent-protocol.md) | Accepted |
| 0009 | [SSH connections go through the system's OpenSSH client](0009-openssh-client-as-transport.md) | Accepted |
| 0010 | [WebDAV over reqwest, logins from .netrc](0010-webdav.md) | Accepted |
| 0011 | [S3 with a small signed-request client, settings from ~/.aws](0011-s3.md) | Accepted |
| 0012 | [SMB with the pure-Rust smb2 client](0012-smb.md) | Accepted |
| 0013 | [The Qt window draws a screen description; QML stays thin](0013-qt-gui.md) | Accepted |
| 0014 | [`nmc serve`: mutual TLS from a private authority, signed one-time tokens](0014-serve-mtls-and-tokens.md) | Accepted |
| 0015 | [TypeScript scripts: deno_core, one `nmc` global, no access by default](0015-typescript-scripts.md) | Accepted |
| 0016 | [Plugins: a folder with a manifest, permissions granted once](0016-plugins.md) | Accepted |
| 0017 | [A settings screen, and settings sent to other machines](0017-settings-screen-and-sync.md) | Accepted |
| 0018 | [One command list for built-in commands, plugins and scripts](0018-command-list.md) | Accepted |
| 0019 | [One drive list for local drives and remote connections](0019-one-drive-list.md) | Accepted |
| 0020 | [Keeping the footprint small](0020-footprint.md) | Accepted |
| 0021 | [`nmc test` and a guide, so AI assistants can write scripts](0021-testing-scripts-for-ai.md) | Accepted |
| 0022 | [`nmc ai`: scripts written by Claude, checked by `nmc test`](0022-scripts-written-by-claude.md) | Accepted |
| 0023 | [Bookmarks with target folders for the other panel](0023-bookmark-targets.md) | Accepted |
| 0024 | [Multi-rename always previews, and refuses names that exist](0024-multi-rename-preview.md) | Accepted |
| 0025 | [Saved connections can log in with their own private key file](0025-connection-private-key.md) | Accepted |
| 0029 | [The product is renamed NemesisCommander, the program `nmc`](0029-rename-to-nemesiscommander.md) | Accepted |
| 0030 | [Remote connections are probed and reopened; jobs tune their parallelism; `nmc serve install`](0030-remote-resilience-and-parallelism.md) | Accepted |
| 0031 | [Server-to-server copies in the queue, S3 upload resume, content checks on servers](0031-remote-followups.md) | Accepted |
| 0032 | [Remote features tested against real servers, panel connection state](0032-tested-against-real-servers.md) | Accepted |
| 0036 | [Logins of connections: warn, ask every time or keep in ~/.netrc](0036-connection-logins.md) | Accepted; the `~/.netrc` saving is superseded by [0040](0040-keyring-logins.md) |
| 0037 | [A template plugin, and `nmc.writeBytes` for binary files](0037-first-plugin-template.md) | Accepted |
| 0039 | [New logo (blue band) and blue selection](0039-new-logo-and-blue-selection.md) | Accepted |
| 0040 | [Saved logins go to the system keyring, not ~/.netrc](0040-keyring-logins.md) | Accepted |
| 0043 | [Agent protocol experience and dialogs](0043-agent-protocol-and-dialogs.md) | Accepted |
| 0044 | [Adaptive transfer parallelism, job indicators, random file generator](0044-adaptive-transfer-parallelism.md) | Accepted |
| 0047 | [Dialogs: one row of keys, one order, one text for both interfaces](0047-dialogs.md) | Accepted |
| 0048 | [Default keys: no Alt, no key shared by two commands](0048-default-keys.md) | Accepted |
| 0049 | [Dialog sizing and the window at large fonts](0049-dialog-sizing.md) | Accepted |
| 0057 | [The product is renamed Pairnox, the program `pnxr`](0057-rename-to-pairnox.md) (formerly NemesisCommander / SuperCommander) | Accepted |
| 0058 | [The Pairnox logo: three SVG masters, every raster made by a script](0058-pairnox-logo-and-icons.md) | Accepted |
| 0059 | [Shortcut model: standard shortcuts and function keys with names](0059-shortcut-model.md) | Accepted |
| 0060 | [Finding similar pictures is a plugin](0060-similar-images-plugin.md) | Accepted |
| 0061 | [Ports and adapters for the interfaces](0061-ports-and-adapters.md) | Accepted |
| 0062 | [Plugin defaults for keys, and a list of results](0062-plugin-key-defaults-and-results.md) | Accepted |
| 0063 | [AI providers, the pnxr command and pictures in the terminal](0063-ai-providers-and-terminal-pictures.md) | Accepted |
| 0064 | [HEIC and RAW pictures](0064-heic-and-raw-pictures.md) | Accepted |

Template:

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD

## Context
## Decision
## Consequences
```
