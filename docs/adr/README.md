# Architecture Decision Records

Note: the product was called SuperCommander and its program `sc` until
ADR 0029; records 0001 to 0028 keep the old names.

Each file records one decision: the context, what was decided, and its
consequences. Records are immutable once accepted; a later record can
supersede an earlier one. The reference design is the document
"NemesisCommander — Architettura v0.1"; these records cover the decisions
taken while implementing it.

| #    | Title                                                        | Status   |
|------|--------------------------------------------------------------|----------|
| 0001 | [Record architecture decisions](0001-record-architecture-decisions.md) | Accepted |
| 0002 | [Workspace layout and crate boundaries](0002-workspace-layout.md) | Accepted |
| 0003 | [Paths are URIs with nested archive containers](0003-path-model.md) | Accepted |
| 0004 | [Operations are plans of idempotent steps](0004-plans-of-idempotent-steps.md) | Accepted |
| 0005 | [Transfer journal in SQLite, planning at start time](0005-transfer-journal.md) | Accepted |
| 0006 | [Archives are read-only providers; packing is separate](0006-archives.md) | Accepted |
| 0007 | [Front ends share one model; the TUI is an event-driven state machine](0007-front-ends-share-one-model.md) | Accepted |
| 0008 | [The agent speaks gRPC over the SSH session's standard streams](0008-agent-protocol.md) | Accepted |
| 0009 | [SSH connections go through the system's OpenSSH client](0009-openssh-client-as-transport.md) | Accepted |
| 0010 | [WebDAV over reqwest, logins from .netrc](0010-webdav.md) | Accepted |
| 0011 | [S3 with a small signed-request client, settings from ~/.aws](0011-s3.md) | Accepted |
| 0012 | [SMB with the pure-Rust smb2 client](0012-smb.md) | Accepted |
| 0013 | [The Qt window draws a screen description; QML stays thin](0013-qt-gui.md) | Accepted |
| 0014 | [`sc serve`: mutual TLS from a private authority, signed one-time tokens](0014-serve-mtls-and-tokens.md) | Accepted |
| 0015 | [TypeScript scripts: deno_core, one `sc` global, no access by default](0015-typescript-scripts.md) | Accepted |
| 0016 | [Plugins: a folder with a manifest, permissions granted once](0016-plugins.md) | Accepted |
| 0017 | [A settings screen, and settings sent to other machines](0017-settings-screen-and-sync.md) | Accepted |
| 0018 | [One command list for built-in commands, plugins and scripts](0018-command-list.md) | Accepted |
| 0019 | [One drive list for local drives and remote connections](0019-one-drive-list.md) | Accepted |
| 0020 | [Keeping the footprint small](0020-footprint.md) | Accepted |
| 0021 | [`sc test` and a guide, so AI assistants can write scripts](0021-testing-scripts-for-ai.md) | Accepted |
| 0022 | [`sc ai`: scripts written by Claude, checked by `sc test`](0022-scripts-written-by-claude.md) | Accepted |
| 0023 | [Bookmarks with target folders for the other panel](0023-bookmark-targets.md) | Accepted |
| 0024 | [Multi-rename always previews, and refuses names that exist](0024-multi-rename-preview.md) | Accepted |
| 0025 | [Saved connections can log in with their own private key file](0025-connection-private-key.md) | Accepted |
| 0029 | [The product is renamed NemesisCommander, the program `nmc`](0029-rename-to-nemesiscommander.md) | Accepted |
| 0030 | [Remote connections are probed and reopened; jobs tune their parallelism; `nmc serve install`](0030-remote-resilience-and-parallelism.md) | Accepted |
| 0031 | [Server-to-server copies in the queue, S3 upload resume, content checks on servers](0031-remote-followups.md) | Accepted |
| 0032 | [Remote features tested against real servers, panel connection state](0032-tested-against-real-servers.md) | Accepted |

Template:

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD

## Context
## Decision
## Consequences
```
