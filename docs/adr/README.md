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

Template:

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD

## Context
## Decision
## Consequences
```
