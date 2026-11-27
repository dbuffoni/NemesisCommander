# 0059. Shortcut model: standard shortcuts and function keys with names

- Status: Accepted
- Date: 2026-11-27
- Supersedes the single key list of ADR 0048 (its rules stay: no Alt, no key shared by two commands).

## Context

A command had one list of chords, so function keys, Shift+F, Ctrl+F and plain
shortcuts were mixed. The bottom bar could not tell its own keys from shortcuts,
and the button names (Rename, View, Copy...) could not be changed.

## Decision

Every command has two independent bindings and a name:

- **Standard shortcuts** (`[keys]`): any number of chords that are not function
  keys (Ctrl+letter, symbols, Delete...).
- **One function key** (`[fkeys]`): F1 to F10, bare, with Shift or with Ctrl
  (30 slots; Alt and Ctrl+Shift are not slots). `""` means none.
- **A short name** (`[labels]`, at most 12 characters): the built-in name of the
  command travels with the key; a custom one is a secondary function (Ctrl+N in
  the function-key list) and an empty name restores the built-in one.
- Another command that has a chord loses it (the function-key list asks first).
- The command list (F9): Ctrl+K assigns a shortcut (refuses function keys),
  Ctrl+F assigns a function key (list of the 30 slots, Ctrl+N name, Delete
  removes, Ctrl+R restores the built-in key and name), Delete removes the
  shortcuts. Two columns show the shortcuts and the function key with its name.
- The bottom bar shows the names of the plain F1-F10; Shift or Ctrl switch it
  to their rows. Terminals do not report a held modifier, so the command
  "Bottom bar: next row" (Ctrl+F) cycles plain, Shift, Ctrl.
- `shortcut_model = 2` in the settings file marks the new shape. A file of the
  first model is converted when it is read: function-key chords move to
  `[fkeys]`, the others stay in `[keys]`, and a command whose old list had no
  function key keeps having none. Imported settings are converted the same way.
- Settings has "Reset all shortcuts" and "Reset all function keys and button names".

## Consequences

Defaults are split by the same rule (the function-key chord of the default list
is the function key). Plugin and script commands can have a function key and a
name through the same sections. Not done here: manifest fields for a default
key of plugin commands and the TypeScript API (cards for later).
