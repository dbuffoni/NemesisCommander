# 0007. Front ends share one model; the TUI is an event-driven state machine

- Status: Accepted
- Date: 2026-11-15

## Context

SuperCommander has three interactive front ends (terminal UI, Qt GUI, and
the command line for scripts) that must behave the same. Remote listings
can be slow, copies take minutes, and the interface must never freeze.
The code should also be testable without a real terminal or window.

## Decision

- `sc-app` is the composition root: it loads settings, the key map and a
  provider registry with every provider of the build, and creates the
  transfer manager. Front ends never assemble these themselves.
- `sc-panel` holds the panel model (location, sorted and filtered entries,
  cursor, selection, history, tabs). It never does I/O: navigating returns a
  `LoadRequest`, the front end lists the folder asynchronously and hands the
  result back. Request ids are unique in the whole process, so a listing is
  offered to every panel and only the one that asked accepts it; stale
  results are dropped.
- Commands are the `Action` table of `sc-config`. Keys, the F9 menu, the F1
  help and (later) the GUI menu bar are all generated from it, so a new
  command appears everywhere at once and every shortcut can be changed in
  the settings.
- The TUI's `App` is a state machine: key presses and background results
  (`AppEvent`) go in, state comes out. Slow work runs in tokio tasks that
  send an event when done. Drawing (`ui.rs`) is a pure function of the
  state, apart from remembering scroll offsets.
- Copy and move go through the persistent transfer queue, also from the
  TUI, so they survive a crash and show up in `sc jobs`. When the conflict
  policy is "ask", the job asks through a resolver that posts the question
  to the interface and waits for the answer.
- Editing hands the terminal to an external editor (`$VISUAL`, `$EDITOR` or
  the configured one); only local files can be edited for now.

## Consequences

- The TUI test suite drives `App` with key presses against real temporary
  folders and renders into ratatui's test backend, covering every dialog
  without a terminal.
- The GUI can reuse `sc-app`, `sc-panel` and the action table, and only
  needs its own drawing and event loop.
- Remote files cannot be edited in place yet; doing so needs a
  download/watch/upload cycle and is left for later.
