# 0060. Finding similar pictures is a plugin

- Status: Accepted
- Date: 2026-11-27

## Context

"Find similar pictures" was a built-in command with its own dialog, grouping
code and a hash of its own. It is used rarely and is easy to separate.

## Decision

- The command, its dialog (`ChoosePurpose::Similar`), the grouping code and the
  menu entry are removed from the program.
- A bundled plugin `image-duplicates` (`plugins/image-duplicates`, written in
  TypeScript like the other bundled plugins) does the work: grouping, the
  report, a command that moves the extra copies to `_similar-extras`, and two
  panel columns (group and likeness in percent; pixels).
- The only new core piece is the script function `pnxr.imageHash(path)`: a 64-bit
  difference hash with width, height and size, decoded with the `image` crate
  that the viewer already needs. It lives in `pnxr-script` (the `pnxr` program),
  not in the window.
- A key, function key or name given to `tools.similar-images` is read as given
  to `plugin.image-duplicates.find` (`MOVED_TO_PLUGIN` in the keymap), so it
  works as soon as the plugin is installed.
- The threshold is fixed at 6 of 64 bits (about 90 %).

## Consequences

The window no longer contains the code. The `image` crate stays in the program
because pictures are viewed. Not done (cards for later): a results panel with
its own columns, grouping and a selection rule ("all but the best"), trash and
link actions, a persistent hash cache, RAW and HEIC, and a default key in the
plugin manifest. Columns recompute hashes for the files of the folder each time.
