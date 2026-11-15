# Architecture Decision Records

Each file records one decision: the context, what was decided, and its
consequences. Records are immutable once accepted; a later record can
supersede an earlier one. The reference design is the document
"SuperCommander — Architettura v0.1"; these records cover the decisions
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

Template:

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD

## Context
## Decision
## Consequences
```
