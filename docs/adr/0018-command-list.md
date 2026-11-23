# 0018. One command list for built-in commands, plugins and scripts

## Context

F9 opened a menu bar copy: one menu per category, left and right to move
between menus. The maintainer asked for a command palette in the spirit of VS
Code's: type to filter, buttons to show one category, every row named
`category/command` with its shortcuts visible, any command bindable to a
key, and plugins and scripts in the same list, in a category of their own or
in an existing one. He also asked to call it "Commands" rather than "Menu".

## Decision

- F9 (`app.commands`) opens the command list (`nmc_frontend::commands`).
  It is built when it opens from the action table, the installed plugins'
  manifests and the user's scripts folder (`$NMC_DATA_DIR/scripts`), plus
  "Run <file>" when the cursor is on a script.
- Rows read `Category/Command` with every shortcut. Typing keeps the rows
  containing every typed word (category, name or id). Category buttons
  (All first, then each category that has commands) are chosen with
  Left/Right or Tab, or a click in the window.
- Ctrl+K records a shortcut for the highlighted row, Delete removes its
  shortcuts. Shortcuts of plugin commands and scripts live in `[keys]` like
  the others, under `plugin.NAME.COMMAND` and `script.FILE`; the key map
  keeps them apart from the built-in actions and dispatches them when no
  action owns the chord. A key given to one command is taken from any other.
- A plugin command may name a built-in category in its manifest
  (`"category": "Files"`); otherwise it goes under `Plugins`. Scripts are
  under `Scripts`. Scripts from the scripts folder run without the
  confirmation asked for a script in a panel, since the user put them there.
- The GUI's menu bar is built from the same list, one menu per category.

## Consequences

- One place to find and bind every command, in both interfaces, with the
  same keys; the GUI adds clicks through `Commander::pick`.
- A plugin or script removed later leaves its `[keys]` entry behind; the
  key then reports "no such plugin command or script" instead of failing
  silently.
- `app.menu` was renamed `app.commands`: a `[keys]` override of the old id
  is reported as unknown.
