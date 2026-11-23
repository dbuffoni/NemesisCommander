# 0049. Dialog sizing and the window at large fonts

- Status: Accepted
- Date: 2026-11-23

## Context

ADR 0047 fixes the key row of every dialog. The size and placement of the
dialog box itself (the window's `Box.qml`) also differed by kind of dialog,
and at large fonts (16 and 24 pt) or in a small window (800×500) the window
showed half rows, cut table columns, labels broken in the middle of a word and
text that started below its top. A review of every dialog at 10, 16 and 24 pt in
two window sizes produced these rules.

## Decision

- **Width.** 60% of the window (at least 560 pixels), at most 90%. The wide
  dialogs (command list, settings, transfer queue) take 90% up to 1000 pixels.
  A table in the fixed font (disks, drives, disk usage) takes the width of its
  widest row, then shrinks its font down to 60% before a column is cut.
- **Height.** Follows the content, up to 90% of the window. A dialog whose
  content changes while it is open (command list, transfers, search results, a
  running log) keeps one height. Lists and the help show whole rows only, never
  half a row. Text taller than the dialog scrolls, from its top; only a log that
  is still running follows its end.
- **Key row.** At the bottom, in 90% of the font. When it is too long it breaks
  between parts, never inside `Key: verb`.
- **Examples and tables inside a question** (the connection and install
  examples) keep their columns: the text shrinks down to 60% before it is cut.
- **The window's bottom key bar** shrinks its text down to 50% so every label
  fits, then drops the label of the least important keys; F10 and F9 keep
  theirs longest. Panel columns give the name first claim on the width, then
  size, then date, then plugin columns; icons follow the font.
- **The terminal** fits its boxes to the screen; a row that does not fit drops
  its middle parts (ADR 0047). Terminal box widths are not unified with the
  window's.

## Consequences

- Dialogs read the same at every font size; what no longer fits is scaled or
  scrolled, not cut.
- Some text is smaller than the setting at the extremes (24 pt in 800×500).
  Settings still shows few rows there, and About scrolls.
