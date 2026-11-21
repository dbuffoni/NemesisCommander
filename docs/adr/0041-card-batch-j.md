# 0041. Window size remembered, bookmark list can add the folder, dialogs wrap their key row

- Status: Accepted
- Date: 2026-11-21

## Context

Three cards joined the keyring work: SC-109 (save the window size on close and
restore it), SC-110 (the bookmark list should have the command to add the
current folder), SC-112 (with a larger font, dialogs were too narrow to show
the whole key row; grow up to 90% of the window, then wrap).

## Decision

- **SC-109.** The desktop window writes `WIDTHxHEIGHT` to `window-size` in the
  data folder when a normal (not maximized or full-screen) window closes, and
  reads it at start. `--size` still wins and is never saved; self tests do not
  save. The file is per machine, so settings sharing does not carry it. Sizes
  below 320×200 or above 20000 are ignored (`AppPaths::save_window_size` and
  `load_window_size`). Window position and the maximized state are not kept.
- **SC-110.** The bookmark list shows a row `+ Bookmark this folder: PATH` after
  the bookmarks (only while nothing is typed and the folder is not bookmarked);
  Enter on it adds the folder. The list also opens when there are no bookmarks,
  so the command is reachable. The key `Ctrl+Shift+D` is unchanged.
- **SC-112.** Dialogs (`Box.qml`) grow with their content up to 90% of the
  window's width and height; a key row longer than that wraps onto more
  lines. The command list is capped at 90% too. The terminal interface already
  fits dialogs to the screen and is not changed.

## Consequences

The Qt part could not be built in the cloud session (no Qt there); the
desktop CI job checks it, and the sizes deserve a look with a large font.
