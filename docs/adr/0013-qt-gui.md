# 0013. The Qt window draws a screen description; QML stays thin

- Status: Accepted
- Date: 2026-11-15

## Context

Phase 2 adds the desktop interface: Qt 6 with QML, no icons, every command
in the menus and on the keyboard, F1 for the main shortcuts. It has to
behave exactly like the terminal interface (ADR 0007) and stay testable in
CI machines without a display. Rust and Qt need a binding; the options were
cxx-qt (KDAB, maintained, QML elements written in Rust), qmetaobject-rs
(less active) and a C++ shell around a Rust library.

## Decision

- The application model moved from `nmc-tui` to `nmc-frontend`: `App`, the
  dialogs, the viewer and key presses. Both interfaces drive the same `App`.
- `nmc_frontend::view::Screen` describes everything to draw (visible panel
  rows, status line, F-key bar, the open dialog, a copy conflict) as plain
  serializable data. The terminal interface shares its text helpers.
- `nmc-gui` (in `gui/`, a separate crate because it needs Qt to build) uses
  cxx-qt 0.10. One QML object, `Commander`, owns the `App` and a tokio
  runtime. QML sends it every key press (`key`), menu choice (`perform`)
  and its size in rows (`resize`); after each change it publishes the
  `Screen` as JSON in its `screen` property, which the QML files only draw.
  Background results reach the Qt thread through cxx-qt's thread queue, in
  batches at most about sixty times a second.
- The menu bar is built from the action table, with each command's
  shortcut. Dialogs are drawn over the panels and never take the keyboard,
  so keys mean the same in both interfaces.
- The QML files are compiled into the binary; there is nothing to install
  beside it. The bridge declaration lives alone in `ffi.rs`, the only
  module allowed `unsafe` (generated glue).
- "Edit" in the window opens the configured editor, else the system's
  (`xdg-open`, `open -W -t`, `notepad`), never `$EDITOR`, which usually
  needs a terminal.
- `nmc-gui --self-test --keys="F1 Esc" --screenshot=file.png` presses keys
  once the listings are loaded, prints the screen as JSON, saves a picture
  and quits. With `QT_QPA_PLATFORM=offscreen` the tests open every dialog
  this way and fail on any QML warning.
- The workspace's `default-members` leave `nmc-gui` out, so plain `cargo
  build` and `cargo test` need no Qt; CI has a separate job with Qt.
- The AppImage contains `nmc`, `nmc-gui` and Qt (linuxdeploy's Qt plugin).
  It opens the window; `NemesisCommander.AppImage nmc ...` runs `nmc`.

## Consequences

- A new dialog or field is added once in `nmc-frontend` and drawn in QML;
  its behavior is tested with the terminal-free `App` tests.
- The whole screen is serialized after every change. A screen is a few
  kilobytes (only visible rows), so this is cheap, and it keeps QML free of
  state; a list model per panel can replace it if profiling asks for it.
- The mouse is not used yet beyond the menu bar: clicking rows, dragging
  and context menus need `App` commands for "put the cursor on row N".
- Windows and macOS builds of the window are not packaged yet; the code is
  portable, but installers (and Qt deployment there) are still to do.
