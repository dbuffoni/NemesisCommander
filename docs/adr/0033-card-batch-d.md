# 0033. Release 0.1.0, fourth batch: small fixes and the key bar

- Status: Accepted
- Date: 2026-11-18

## Context

Daniele added ten more cards to release 0.1.0 (SC-74 to SC-77, SC-90, SC-94
to SC-98). Each is small; the choices that were not obvious are below.

## Decision

- **No flash while scrolling (SC-74).** The panel's QML `Repeater` got the
  array of rows as its model, so each cursor move built every row again (and
  repainted every icon `Canvas`). Its model is now the row count; each
  delegate reads `rows[index]`, so rows are updated in place.
- **Quick view (SC-75).** Plain keys keep moving the panel's cursor, as the
  preview follows it. Shift+arrows, Shift+PgUp/PgDn and Shift+Home/End scroll
  the preview (they turn PDF pages, rotate nothing: models keep their arrows
  in the full viewer); the wheel over the preview scrolls it instead of
  focusing the hidden panel. Swap panels with the quick view open also moves
  the focus, so the file panel and the preview trade places.
- **No "Sort by size" (SC-76).** The command and its Ctrl+F6 key are gone;
  `SortKey::Size` stays for the CLI and for settings. After "Count folder
  sizes" every row of that panel gets a bar against the biggest item: a
  rectangle in the window, a block character in the terminal, both coloured
  from green to red (`view::bar_color`). Panels that never counted show none.
- **Filter hint (SC-77).** The footer reads `filter: x (Esc clears)`.
- **Settings (SC-90).** Typing filters the settings (section and label, all
  words); reset moved from `r` to Delete; Esc clears the filter, then closes.
  Space still toggles when the filter is empty.
- **Private key removed (SC-94).** ssh uses what the system gives it. The
  key prompt, F4 in the drive list, the Browse button and `nmc-ssh`'s
  identity code are gone, with the `qml6-module-qtquick-dialogs` dependency.
  `Connection.identity_file` stays as a read-only field, so settings files
  that have it still load (`deny_unknown_fields`); it is never written back.
- **Help in the command list (SC-96).** F1 shows one or two sentences about
  the highlighted command (`Action::help`, one per command, a test checks all
  have one); plugins and scripts get a generic line.
- **Function key bar (SC-97).** `App::set_modifiers` stores the held
  modifiers and the bar looks up `Ctrl/Alt/Shift+F1..F10`. The window sends
  press and release of the modifier keys (and forgets them when it loses the
  focus). The terminal can only do so when the terminal reports modifier keys
  on their own; the application does not turn that protocol on, because that
  changes how every other key arrives and could not be tried on real
  terminals here.
- **Compare leaves marks (SC-98).** Esc clears them in both panels (a flag
  set by Compare); with no flag Esc clears the active panel's selection.
- **Alt+F5 in the basket (SC-95).** Could not be reproduced in the terminal
  UI tests (the pack dialog opens and packs). The pack name for basket items
  now is `basket.zip`, where it used to be the name of the active folder.

## Consequences

- The key bar does not follow the modifiers in a terminal that does not
  report them.
- Old settings with `identity_file` keep that line until the file is edited.
