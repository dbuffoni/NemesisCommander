# 0047. Dialogs: one row of keys, one order, one text for both interfaces

- Status: Accepted
- Date: 2026-11-23

## Context

By release 0.2.0 every dialog had grown its own key hint: different verbs
for Enter (`OK`, `choose`, `use`, `run`), different words for the same key
(`Del` and `Delete`), Esc in the middle of the row or missing, a different
separator in the terminal and in the window, and some dialogs with no row at
all. The terminal drew dialogs from the application state and the window from
the shared screen description, so the two texts drifted apart.

## Decision

- **One row at the bottom of every dialog.** Parts are separated by four
  spaces and each reads `Key: verb`, with the verb in lower case (`OK` is the
  exception). A part may start with a status without a key (`11 found`,
  `Working…`).
- **One order.** The main action (`Enter: OK / open / choose / run / yes`);
  the keys the dialog adds; `Shift+Tab: back`, only when a question of a series
  has a previous one; `F1: help`, when the dialog has help; and last Esc:
  `Esc: cancel` for questions, confirmations and steps of a question,
  `Esc: close` for lists, messages and viewers.
- **Same names for the same keys.** `Delete` (not `Del`), `Backspace`,
  `←/→`, `PgUp/PgDn`, `Ctrl+U: clear`, `Ctrl+T: test`, `Ctrl+O: more options`,
  `Ctrl+K: set key`. A key that means one thing in the panels does not mean
  something else in a dialog unless the row says so.
- **What cannot be undone does not default to yes.** For a permanent delete
  Enter answers no and only `y` deletes (`Enter: no    y: yes    Esc: cancel`).
- **One source of text.** `nmc-frontend::hints` builds every row and holds the
  fixed ones (help, transfer queue, copy conflict). The terminal and the window
  both take the text from the shared view model, so they cannot differ; the
  terminal only shortens a long row to its width, dropping parts from the middle
  and keeping the main action and Esc.
- **A test keeps it so.** `every_dialog_has_a_key_row_in_one_convention` opens
  the dialogs, checks that the row ends with Esc, uses the allowed names and
  order, and that the terminal shows it.

## Consequences

- A new dialog picks its row from `hints` instead of writing text; the manual
  section "The keys of a dialog" describes the convention for users.
- The size and placement rules of the dialog box itself (width, height,
  margins) are not part of this record and still differ by kind of dialog.
- Releases before 0.3.0 showed other texts; nothing reads them, so there is no
  migration.
