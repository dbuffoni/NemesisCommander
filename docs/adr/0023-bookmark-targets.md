# 0023. Bookmarks with target folders for the other panel

- Status: Accepted
- Date: 2026-11-16

## Context

A bookmark only opened one folder in the active panel. The maintainer asked that a
bookmark can also remember the folder (or folders) the other panel should
show, and that a quick picker asks which one when there are several, in the
window and in the terminal, for local and remote places alike.

## Decision

- `Bookmark` gets `targets: Vec<String>`, VFS locations like `path`. The key
  is optional and left out when empty, so older settings files load and are
  written unchanged.
- New action `nav.add-bookmark-target` ("Bookmark this folder with the other
  panel's", Ctrl+Shift+B) bookmarks the active folder with the other panel's
  folder as a target. Run again on the same folder with another folder on
  the other side, it adds that one as a further target; a target already in
  the list is not added twice. Ctrl+Shift+D keeps bookmarking without
  targets.
- Opening a bookmark (Ctrl+D, Enter) navigates the active panel first, then:
  no target, nothing else; one target, the other panel opens it; several
  targets, the existing choose dialog lists them and Enter opens the chosen
  one in the other panel, while Esc leaves that panel as it is.
- Both panels resolve locations with `resolve_location`, the call used by
  Go to, drives and plain bookmarks, so remote and archive targets need no
  separate code. All of it lives in `nmc-frontend`, shared by TUI and GUI.
- Targets are edited by adding them with Ctrl+Shift+B or in `config.toml`;
  Delete in the list still removes the whole bookmark. A dedicated editor
  can come later if needed.

## Consequences

- The bookmark list shows `→ target` or `→ N targets` after the folder.
- Settings merge (import, `nmc config push/pull`) keeps working by name: a
  merged bookmark takes its targets from the incoming side.
