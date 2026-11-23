# 0024. Multi-rename always previews, and refuses names that exist

- Status: Accepted
- Date: 2026-11-16

## Context

The maintainer asked that the multi-rename have a preview mode everywhere. Before
this record the window and the terminal showed the new names only after
Enter, in the confirmation; `nmc rename` had `--dry-run`; `nmc ai rename`
printed the names. None of them said when a new name belonged to a file
outside the rename. `apply_renames` refused to replace such a file, but only
when it reached it, so the files before it were already renamed.

## Decision

- `nmc_ops::rename_conflicts(registry, paths, names)` returns the new paths
  that already belong to another file. Files of the rename that move away
  themselves (swaps, chains) and letter-case-only changes do not count. It
  stats each new path through the provider, so it works on every location.
- `apply_renames` calls it first and renames nothing when there is a
  conflict: a multi-rename is never left half done by a name that exists.
- Ctrl+M (TUI and window, shared in `nmc-frontend`) previews the first new
  names under the template while it is typed (`InputDialog::preview`,
  `OverlayView::Input.preview`), or the reason the template cannot be used.
  Enter shows the full preview (`old → new` for the names that change) and
  asks for confirmation. With conflicts, the preview marks them
  `(already exists)`, lists them first, and only closes. Multi-rename with
  AI goes through the same confirmation.
- `nmc rename` prints the same preview (`old -> new`, conflicts marked) and
  then renames; `--preview` (alias of `--dry-run`, `-n`) stops there. With
  conflicts it renames nothing and exits with an error, also in preview, so
  scripts can test the exit status. `nmc ai rename` marks conflicts in its
  output and lists them as `conflicts` in `--json`.

## Consequences

- One stat per changed name before renaming. Cheap locally; on remote
  locations it is one round trip per file, acceptable for a rename of a
  folder's files.
- The live preview uses only the template (the dialog has one field); the
  existing-file check happens on Enter, because it needs the provider.
  Find/replace, case and counter options stay on the command line and in
  the rules Claude writes.
