# Writing Pairnox scripts and plugins

This guide is for people and for AI coding assistants (Claude Code and
others). Save it next to your scripts as `AGENTS.md` or `CLAUDE.md`:

```sh
pnxr run --guide > AGENTS.md
pnxr run --types > pnxr.d.ts
```

## The loop

1. Read `pnxr.d.ts`: it is the whole API.
2. Write the script (`.ts`) or the plugin (a folder with `plugin.json`).
3. Run `pnxr test TARGET --json` and read the report.
4. Fix and repeat until `"passed": true`. Then try it on real files.

`pnxr test` never touches your files. It makes a temporary folder with
`panel/` (the active panel, filled with sample files or a copy of
`--fixture DIR`) and `other/` (the other panel, empty), runs the script
there and reports:

- `checks`: `manifest` (plugins), `syntax`, and `types` (with `tsc`, when
  installed; otherwise `skipped`).
- `runs`: one per script, or per plugin command: `passed`, `error`,
  `output` (what `pnxr.print` printed), and the files `created`, `removed`
  and `changed`, relative to the test folder (`panel/jpg/photo.JPG`).
- `notes`: plugin permissions the test did not grant.

Useful options: `--fixture DIR` (your own sample files), `--command ID`
(one plugin command), `--select NAME` (only some entries are selected;
default: all of `panel/`), `--keep` (keep the folder to look at it),
`-- ARGS` (a script's `pnxr.args`; `$PANEL` and `$OTHER` are the test
folders; default: `$PANEL`). The exit status is 0 when everything passed.

Check the report against what the script should do: a passing run that
created nothing, or changed files it should not, is still wrong.

## Rules of the runtime

- One file, no `import`: there are no modules, no Node or Deno APIs, no
  `fetch`, no network and no processes. Only the standard JavaScript
  library (ES2022) and the global `pnxr`.
- Print with `pnxr.print(...)`, not `console.log`.
- Top-level `await` works. Every file function of `pnxr` returns a promise.
- Locations are strings: local paths (relative to the active panel's
  folder) or URIs (`sftp://host/dir`, `s3://bucket/key`,
  `zip:///a.zip!/inside`). Build them with `pnxr.join`, `pnxr.basename`,
  `pnxr.dirname`; entries from `pnxr.list` carry a ready `path`.
- A script may only touch the locations it was granted. A refusal throws
  an error named `PermissionDenied`.
- `pnxr.remove` removes a file or an empty folder: remove what is inside a
  folder first. `pnxr.rename` works within one location; across locations
  use `pnxr.copy`, then `pnxr.remove`.
- Throw an `Error` to fail with a message.

## Scripts

Run with `pnxr run script.ts --allow-write LOCATION -- ARGS`. Read arguments
from `pnxr.args`. In `pnxr test` the script may change everything in the test
folder.

```ts
const [folder = "."] = pnxr.args;
for (const entry of await pnxr.list(folder)) {
  if (entry.kind === "file" && entry.size === 0) {
    await pnxr.remove(entry.path);
    pnxr.print(`removed ${entry.name}`);
  }
}
```

## Plugins

A folder with `plugin.json` and the script (`main`, default `main.ts`):

```json
{
  "name": "sort-by-type",
  "version": "1.0.0",
  "description": "Moves the selected files into folders named after their extension",
  "main": "main.ts",
  "commands": [
    { "id": "sort", "title": "Sort selected files into folders by type", "short": "SortType", "category": "Files" }
  ],
  "permissions": { "write": ["$PANEL"] },
  "limits": { "memory": 64, "timeout": 120 }
}
```

- `name`: lowercase letters, digits and dashes. Command ids are unique.
- `short`: a name of 1 to 8 characters for the bottom bar's function-key
  button. Give every command one; without it the first word of the title is
  used and `pnxr test` warns.
- `category` (optional): `Files`, `Select`, `Navigate`, `View`, `Tools`,
  `Pairnox`; anything else, or none, means `Plugins`.
- `shortcut`, `fkey` (optional): default keys of the command: a standard
  shortcut such as `Ctrl+Shift+I`, and a function key (`F1` to `F10`, with
  `Shift+` or `Ctrl+`). `label` is another name for `short`. They are only
  defaults: whatever the user sets in the settings wins, a chord that a
  built-in command or an earlier plugin already has is not taken (the settings
  screen reports it), and an empty `[fkeys]` entry turns the default off.
- `permissions`: `read` and `write` lists of `$PANEL`, `$OTHER`,
  `$SELECTION`, `$HOME`, absolute paths or URIs, or `"all": true`. Ask
  for as little as possible: `$PANEL` or `$SELECTION` is usually enough.
  `pnxr test` grants only `$PANEL`, `$OTHER` and `$SELECTION`.
- The script reads `pnxr.context`: `command` (the id), `panel`, `other`
  (folders) and `selection` (selected entries). `pnxr.args` is empty.

A command that finds files can hand them over as a list with
`pnxr.showResults({ title, columns, rows, actions })` when `pnxr.context.results`
is true (the window and the terminal; print a text report otherwise). The list
is sorted and marked by the user (`a` marks all rows but those with
`keep: true`); Delete moves the marked files to the trash after the usual
question, and each action runs one of the plugin's own commands with the
marked files as `pnxr.context.selection`. `image-duplicates` is the example.

Install and use: `pnxr plugin install FOLDER`, then Ctrl+X or F9 in the
interface, or `pnxr plugin run NAME COMMAND --panel DIR FILES...`.
