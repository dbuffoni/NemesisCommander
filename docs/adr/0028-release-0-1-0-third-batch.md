# 0028. Choices made for the third batch of 0.1.0 features

- Status: Accepted
- Date: 2026-11-17

## Context

Nine cards joined release 0.1.0 after the second batch (ADR 0027): SC-10, 23,
24, 65, 66, 67, 68, 70 and a new card, "Disk analysis". They were built in one
branch.

## Decision

- **Keys (SC-70):** Ctrl+↓ swaps the panels and Ctrl+↑ opens this folder in the
  other panel. The old keys (Ctrl+U, Alt+O) stay as second shortcuts, so
  nobody's habits break.
- **Fonts (SC-68):** the font settings are a new kind of option (`Font`) that
  opens a list instead of a text field. The list of installed families belongs
  to the window: QML asks Qt (`Qt.fontFamilies()`) and hands it over when the
  application starts (`App::set_fonts`), so no font-scanning dependency is
  added. A terminal has no list, so Enter there only says the terminal uses its
  own font. The list is filtered by typing; "(system default)" is the first row.
- **Bookmarks and recent folders (SC-67):** the bookmarks list filters as you
  type (name or folder contains the text, case-insensitive), and under it the
  five most recent folders that match, taken from the visit history that was
  kept for the old "frequent folders" command. That command, its dialog and
  Alt+J are removed; the id `nav.jump` is read as `nav.bookmarks` so a saved
  key map still loads. The visit score (count times age weight) is no longer
  used, only the time of the last visit. The filter text is shown in the
  dialog's footer, so the window and the terminal draw the same list.
- **Folder sizes (SC-10):** Alt+S walks the selected folders (or all) in the
  background and keeps `(bytes, items)` in memory by path; the size column shows
  the bytes. A new listing of the folder forgets them. The item count is not
  shown in the column (too wide), only in the sum on the status line.
- **Disk usage (SC-24):** Alt+U measures the children of a folder from one
  walk (`usage::tally`) and shows a list with a bar relative to the biggest,
  the share and the size. Enter drills down, Backspace goes up. It uses the
  provider walk, so it works on remote locations.
- **Duplicates (SC-23):** group by size, then hash only the groups with
  `sc_ops::checksum` (BLAKE3; on an agent the hash is computed remotely). The
  oldest copy of each group is the one kept; extra copies go to the basket,
  never deleted by the command. Empty files are ignored.
- **Disk analysis (new card):** the list comes from the system's tools, not
  from system calls: `lsblk -J` on Linux (size, type, partition table, file
  system, label or model, used and free space, mount point), `df -Pk` on macOS
  and `Get-Volume` through PowerShell on Windows. The parsers are pure
  functions with tests. This avoids a dependency for `statvfs` and works
  without privileges; what a platform does not tell is shown as `-`.
- **Transfer indicator (SC-66):** a line above the status line only while
  transfers are active: a bar (done over total bytes), the number of jobs, the
  percentage, the speed over the last five seconds (`SpeedMeter`), the time
  left and the queued jobs. The window draws a 3 px line (clicking it opens the
  queue); the terminal draws `━` and `─`. The old `2 jobs 40%` text on the
  status line is replaced by this one.
- **Icons (SC-65):** 21 outline shapes drawn on a 16 × 16 grid (polylines kept
  in `icons.rs`), about 160 extensions in 21 groups, each with its own colour, plus well-known
  names (Makefile, README). The window draws them on a QML `Canvas` (no extra
  Qt module: `QtQuick.Shapes` and SVG support are separate packages on Linux);
  the shapes travel once per screen update and rows refer to them by name. The
  terminal shows one coloured character per kind. The setting `panels.icons`
  (on by default) turns them off.

## Consequences

- Disk analysis depends on `lsblk` being installed on Linux (it is part of
  util-linux); without it the command says why it failed.
- Folder sizes are not refreshed after a copy or delete until the folder is
  listed again, which every job does.
- Each row of the window has a small canvas; icons can be turned off if a very
  slow machine needs it.
