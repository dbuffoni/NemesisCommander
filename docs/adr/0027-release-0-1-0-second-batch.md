# 0027. Choices made for the second batch of 0.1.0 features

- Status: Accepted
- Date: 2026-11-17

## Context

Eleven more cards joined release 0.1.0 after the first batch (ADR 0026): SC-1,
7, 8, 9, 17, 19, 21, 33, 35, 55 and 64. They were built in one branch.

## Decision

- **F-keys (SC-64):** already done by SC-59 in the first batch: the key bar
  shows `F1`…`F10` in the terminal and in the window. No change.
- **Copy path (SC-17):** `files.copy-path` (Alt+P) lists the forms (full path,
  name, URI, folder). The clipboard is handed over by the application model
  (`App::take_clipboard`): the terminal writes an OSC 52 sequence, the window
  copies through a hidden QML `TextEdit`. No clipboard library.
- **Export (SC-33):** CSV, JSON, Markdown, HTML of the selection or the whole
  listing, to the clipboard or to a new local file (never overwrites).
- **Links (SC-35):** relative and absolute symlinks and hard links, local
  folders only, made in the other panel with the source's name.
- **Rename in editor (SC-9):** a temporary file with one name per line; after
  the editor ends the names are checked (same count, valid, unique) and go
  through the same preview and conflict check as multi-rename. In the window
  an editor that waits must be configured, because the default `xdg-open`
  returns at once.
- **Frecency jump (SC-1):** visits are kept in `frecency.json` in the data
  folder (URI, count, last time, at most 500), scored by count times an age
  weight (hour 4, day 2, week 0.5, older 0.25), like zoxide. Every word must be
  in the path and the last one in the folder name.
- **Saved searches (SC-8):** `[[searches]]` in the settings (name, mask,
  content, filters). The search dialog got a third question for size and age
  (`>10M`, `<1G`, `newer:7d`, `older:2w`). Each saved search is a command
  `search.NAME` in the command list, so it takes a key like plugin commands.
- **Basket and search results (SC-7):** the basket is a list in memory
  (`App::basket`), shown as a list window, not as a third panel; its commands
  reuse the panel commands by overriding their targets. Search results are
  added with Alt+B. Moved and deleted items leave the basket.
- **Similar pictures (SC-55):** a 64-bit difference hash of a 9×8 grey
  thumbnail, similar when at most 6 bits differ; flat pictures are skipped.
  The best copy has most pixels, then the biggest file. The extra copies go to
  the basket rather than being deleted.
- **Compare by content (SC-21):** walks both folders and hashes files of equal
  size with `sc_ops::checksum`, which asks a provider with `SERVER_HASH` (the
  agent) to hash where the files are. It selects the top-level entries that
  differ.
- **Folder sync (SC-19):** `sc_ops::plan_sync` builds the plan (mirror, update,
  two-way), shown in the viewer in colour (the rich-text style got a red
  `removed`, also used for the left side of a diff); Enter runs it: deletions
  first, then one copy job per target folder with overwrite. Equal means same
  size and times within two seconds. Whole folders that exist on one side only
  are one item. Two-way never resolves a real conflict.

- **Connection names (asked by the maintainer afterwards):** a new connection already
  asked for its name as the second question; the prompt now says what the name
  is for, and F2 in the drive list renames a saved connection (names stay
  unique).

## Consequences

- Undo does not cover sync, links or renames from the editor in the basket
  (renames from the editor are undoable).
- The basket is lost on quit.
- Hashing a very large tree for "compare by content" reads every equal-sized
  file once per side (on the agent for agent connections).
