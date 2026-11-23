# 0048. Default keys: no Alt, no key shared by two commands

- Status: Accepted
- Date: 2026-11-23

## Context

Several secondary commands were on Alt (properties, copy path, link, basket,
compare, disk usage, disks, plugins, back and forward, flat view). Alt is the
weakest modifier: desktops keep Alt+F keys (GNOME takes Alt+F1, F5, F7, F8),
macOS terminals type characters with Option, and the Qt window cannot match
`Alt+P` when Option types `π`. Some defaults also needed keys that legacy
terminals cannot send (Ctrl+M is Enter, Ctrl+H is Backspace).

## Decision

- **No built-in command is bound to Alt.** A user can still bind Alt keys
  (F9, then Ctrl+K); the keymap parses them.
- **No two commands share a default key.** A test goes through every default
  and fails on an Alt key or a duplicate.
- **Secondary commands use Shift+F, Ctrl+F or Ctrl+letter keys** that a
  terminal delivers: properties Ctrl+E, copy path Ctrl+P, link Shift+F4, add to
  the basket Ctrl+G, back and forward Ctrl+O and Ctrl+Y, flat view Ctrl+F6,
  compare Shift+F3, disk usage Ctrl+F7, disks Ctrl+F8, plugins and scripts
  Ctrl+X, choose (`nmc pick`) Ctrl+F10, multi-rename Shift+F2, hidden files
  Ctrl+F2, settings Ctrl+, and Ctrl+F1. Bookmark with the other panel's folder
  is Ctrl+Shift+B.
- **Going back in a series of questions is Shift+Tab**, which also frees Alt+←.
- Saved keymaps keep working: the identifiers of the commands did not change,
  and a key the user set always wins over a default.

## Consequences

- Someone used to the Alt keys of 0.2.0 has to learn the new ones; the manual's
  table of shortcuts and the F1 help show them.
- Ctrl+Z (undo), Ctrl+K (checksum), Ctrl+U (swap panels) and other keys mean
  something else inside a dialog (see ADR 0047); the row of the dialog says
  which.
