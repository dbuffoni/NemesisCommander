# 0045. Shift+arrow selection, a calmer transfer queue, regular expressions

- Status: Accepted
- Date: 2026-11-22

## Context

Four cards (SC-121..124) asked for range selection with the keyboard, a less
wordy transfer queue with more statistics, a time left that does not flicker,
and regular expressions in search and masks.

## Decision

- **Selection.** Shift+Up/Down/PgUp/PgDn/Home/End (also with Ctrl) select
  from where the movement started to the cursor; moving back shrinks the
  range; what was selected before is kept; any other selection command or a
  new folder ends the range (`Panel::extend_selection`). Ctrl+Up/Down alone
  stay "open in other panel" / "swap panels" (existing commands), so the Ctrl
  half of the card is read as Ctrl+Shift. With the quick view open Shift+arrows
  still scroll it.
- **Queue.** The way data travels is a short label (`Local`, `Upload`,
  `Download`, `On the server`, `Server to server (direct)`, `Server to server
  (via this computer)`). A statistics line follows: items done, time waiting,
  time so far, average speed and time left while running; time taken and
  average speed when finished. The times come from what the application saw in
  this session (`JobClock`), so jobs from an earlier run show only the items.
- **Time left.** The status bar speed/time left and each job's statistics are
  recalculated at most every 3 seconds (`speed::Steady`), and in between the
  last text stays.
- **Regular expressions.** A mask starting with `re:` is a regular expression
  matched anywhere in the name, case-insensitively (anchor with `^`/`$`); the
  content text of the search dialog accepts `re:` too. It works wherever a mask
  is parsed: search, select/unselect, quick filter, `nmc find --name`.

## Consequences

- No new settings and no extra dialog fields: the prefix keeps the dialogs
  as they were.
- The GUI (Qt) shows the new queue text through the existing `detail` field
  (now two lines); it could not be built in the cloud, so CI checks it.
