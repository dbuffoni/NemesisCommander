# 0062. Plugin defaults for keys, and a list of results

- Status: Accepted
- Date: 2026-11-27

## Context

0.6.0 gave every command, plugins included, a shortcut, a function key and a
button name that the user can set (ADR 0059), but a plugin could not suggest
any. The image-duplicates plugin (ADR 0060) also needed what the window and the
terminal had no general form for: a list of files with columns to sort, marks
and actions.

## Decision

- `plugin.json` commands may declare `shortcut`, `fkey` and `label` (`short`).
  The front end gives them to the keymap as defaults below everything the user
  sets (`Config::plugin_defaults`, filled from the installed plugins, never
  written to the settings file). A chord that a built-in command or an
  earlier plugin has is not taken and the settings screen warns.
- A script prints a list with `pnxr.showResults(...)` when
  `pnxr.context.results` is true. The call prints one line, a control
  character and JSON, that the front end finds in the output of the command
  (the programs it starts set `PNXR_RESULTS=1`; `pnxr run` does not, so the
  plugin prints a text report there). The front end shows a list that
  sorts by each column (`s`, `r`), marks (`Space`, `a`, `u`), trashes the marked
  files through the normal delete question (`Delete`) and runs the plugin's
  declared actions with the marked files as the selection.
- Hashes of pictures are cached between runs (`image-hashes.tsv` in the data
  folder, keyed by location, size and modification time), inside
  `pnxr.imageHash`.
- Shown as an ordinary choose list, the results need no new window code in
  the terminal or the QML.

## Consequences

A plugin with a list of results gets sort, marks and trash for free and the
same behaviour in the window and the terminal. Not done, and kept as cards:
a true virtual panel with its own columns, RAW/HEIC decoding, a choice of the
similarity threshold.
