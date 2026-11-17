# 0026. Choices made for the 0.1.0 features

- Status: Accepted
- Date: 2026-11-17

## Context

The 0.1.0 cards of the kanban (SC-15, 18, 20, 34, 37, 58-63) were built in one
branch. Several left room for choice; the ones worth remembering are here.

## Decision

- **Menu bar (SC-58):** removed from the window. The Commands list (F9) is the
  menu; the F-key bar is clickable, so F9 is one click away.
- **Settings and shortcuts (SC-60):** shortcuts are set only in the Commands
  list (Ctrl+K); `[keys]` in the file is unchanged.
- **Navigation (SC-61):** Ctrl+Left/Right open the folder under the cursor in
  the left/right panel. Plain Left/Right (parent / enter) are off by default
  (`panels.arrow_navigation`).
- **Fonts (SC-62):** `[appearance]` with family, size and fixed-width family,
  empty/0 = system. Window only; the terminal keeps its own font.
- **Drives (SC-63):** the drive name sits above each panel (clickable) and the
  `d` key opens the existing drive list for the active panel.
- **Dates and permissions (SC-34):** one action, `files.attributes`, typed as
  words (`2026-11-17 10:30`, `now`, `mode=644`, `-r`); also reachable from the
  Properties window. Provider `set_attributes` does the work.
- **Undo (SC-18):** in memory, 50 steps, for renames, new folders and finished
  move jobs. It is not built on the transfer journal, so it ends with the
  session, and deleting to the recycle bin cannot be undone yet.
- **Git marks (SC-15):** read by running the system `git` (`status
  --porcelain=v1 -z`) in the background, for local folders only; no library is
  linked. Option `panels.git_status` (on by default).
- **Diff (SC-20):** Alt+C on two text files shows a side-by-side comparison
  in the viewer (crate `similar`), as formatted lines; Blue is left-only, green
  right-only. Binary or larger than 4 MiB files keep the old identical/different
  message.
- **Mouse (SC-37):** shared logic in `sc-frontend` (`click_row`, `drag_rows`,
  `drop_files`). Terminal: click, double-click, wheel, drag between panels.
  Window: the same plus dragging local files to and from other programs;
  remote files cannot be dragged out.

## Consequences

- New dependency: `similar` (pure Rust, small).
- Dragging out of the window and the Qt QML changes were checked by building
  and running the GUI tests, not by hand with a real desktop.
