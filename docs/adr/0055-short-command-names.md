# 0055. Short names for commands

Status: accepted (0.4.0, SC-159, SC-161)

## Context

The function-key bar showed the first word of a command's long label, with two
hand-kept lists (one for the window, one for the narrow terminal bar). It gave
"Change" for "Change dates and permissions…", the same "Delete" for two
different commands, and a plugin command had no say at all. In the window the
buttons were as wide as their own text.

## Decision

- `Action::short()` is a column of the `actions!` table: at most 8
  characters, no spaces, no ellipsis, unique among all commands (a test).
  `Action::narrow()` gives at most 5 characters for the narrow terminal bar,
  unique inside each key layer (a test). The bar uses them for every layer and
  the two lists are gone. The terminal gives each label as many columns as fit
  (6 to 8), five below 91 columns.
- A plugin command has an optional `"short"` (1 to 8 characters, no control
  characters, checked when the manifest loads). Without it the first word of
  the title cut at 8 is used, and `nmc plugin install` and `nmc test` warn.
- The command list's help line (F1) says what the button says.
- Window: every function-key button is as wide as `F10` plus `Commands`;
  only a longer label stretches its own button. The priority rule that drops
  labels in a narrow window is unchanged.

## Consequences

Old manifests keep working. The widths of the window buttons are not covered by
the automated tests (they read the screen model, not the geometry): they are
checked by eye on the desktop at 10, 16 and 24 pt.
