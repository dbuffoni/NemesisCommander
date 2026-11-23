# 0004. Operations are plans of idempotent steps

- Status: Accepted
- Date: 2026-11-15

## Context

The transfer engine must survive crashes and resume jobs. Operations must
behave the same whether started from the GUI, the TUI, the CLI or an agent.

## Decision

- Copy, move and delete are first turned into a `Plan`: a flat, serializable
  list of `Step`s (`CreateDir`, `CopyFile`, `CopySymlink`, `SetAttributes`,
  `Move`, `Remove`). Each step is idempotent: running it twice has the same
  effect as running it once (creating an existing directory succeeds,
  removing a missing file succeeds, a file copy restarts or resumes).
- Copy plans list all directories first, then all files, then directory
  attributes deepest-first. Files therefore run in parallel batches, and
  directory modification times are restored after their contents are
  written.
- `Move` within one provider is a rename; if the rename fails for any reason
  other than "not found" or "permission denied" (for example crossing
  devices) it falls back to copy-then-delete, deleting the source only if
  every copy succeeded.
- File data is streamed through a bounded reader/writer pipeline into a
  hidden `.<name>.nmc-part` file when the destination can rename atomically.
  A partial file is never visible under its final name, and an interrupted
  copy resumes from the part file when both sides support seeking/resuming.
- Same-provider copies use `Provider::copy_within` when advertised
  (reflink / `copy_file_range` locally), so the data never enters user space.

## Consequences

- Plans can be previewed, stored, and resumed step by step.
- Planning walks the whole source tree first; for very large remote trees
  planning takes time before the first byte is copied. A streaming planner
  is a possible later optimization.
- Hard links are copied as independent files (not preserved).
