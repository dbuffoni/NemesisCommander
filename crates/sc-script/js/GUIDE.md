# Writing SuperCommander scripts and plugins

This guide is for people and for AI coding assistants (Claude Code and
others). Save it next to your scripts as `AGENTS.md` or `CLAUDE.md`:

```sh
sc run --guide > AGENTS.md
sc run --types > sc.d.ts
```

## The loop

1. Read `sc.d.ts`: it is the whole API.
2. Write the script (`.ts`) or the plugin (a folder with `plugin.json`).
3. Run `sc test TARGET --json` and read the report.
4. Fix and repeat until `"passed": true`. Then try it on real files.

`sc test` never touches your files. It makes a temporary folder with
`panel/` (the active panel, filled with sample files or a copy of
`--fixture DIR`) and `other/` (the other panel, empty), runs the script
there and reports:

- `checks`: `manifest` (plugins), `syntax`, and `types` (with `tsc`, when
  installed; otherwise `skipped`).
- `runs`: one per script, or per plugin command: `passed`, `error`,
  `output` (what `sc.print` printed), and the files `created`, `removed`
  and `changed`, relative to the test folder (`panel/jpg/photo.JPG`).
- `notes`: plugin permissions the test did not grant.

Useful options: `--fixture DIR` (your own sample files), `--command ID`
(one plugin command), `--select NAME` (only some entries are selected;
default: all of `panel/`), `--keep` (keep the folder to look at it),
`-- ARGS` (a script's `sc.args`; `$PANEL` and `$OTHER` are the test
folders; default: `$PANEL`). The exit status is 0 when everything passed.

Check the report against what the script should do: a passing run that
created nothing, or changed files it should not, is still wrong.

## Rules of the runtime

- One file, no `import`: there are no modules, no Node or Deno APIs, no
  `fetch`, no network and no processes. Only the standard JavaScript
  library (ES2022) and the global `sc`.
- Print with `sc.print(...)`, not `console.log`.
- Top-level `await` works. Every file function of `sc` returns a promise.
- Locations are strings: local paths (relative to the active panel's
  folder) or URIs (`sftp://host/dir`, `s3://bucket/key`,
  `zip:///a.zip!/inside`). Build them with `sc.join`, `sc.basename`,
  `sc.dirname`; entries from `sc.list` carry a ready `path`.
- A script may only touch the locations it was granted. A refusal throws
  an error named `PermissionDenied`.
- `sc.remove` removes a file or an empty folder: remove what is inside a
  folder first. `sc.rename` works within one location; across locations
  use `sc.copy`, then `sc.remove`.
- Throw an `Error` to fail with a message.

## Scripts

Run with `sc run script.ts --allow-write LOCATION -- ARGS`. Read arguments
from `sc.args`. In `sc test` the script may change everything in the test
folder.

```ts
const [folder = "."] = sc.args;
for (const entry of await sc.list(folder)) {
  if (entry.kind === "file" && entry.size === 0) {
    await sc.remove(entry.path);
    sc.print(`removed ${entry.name}`);
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
    { "id": "sort", "title": "Sort selected files into folders by type", "category": "Files" }
  ],
  "permissions": { "write": ["$PANEL"] },
  "limits": { "memory": 64, "timeout": 120 }
}
```

- `name`: lowercase letters, digits and dashes. Command ids are unique.
- `category` (optional): `Files`, `Select`, `Navigate`, `View`, `Tools`,
  `SuperCommander`; anything else, or none, means `Plugins`.
- `permissions`: `read` and `write` lists of `$PANEL`, `$OTHER`,
  `$SELECTION`, `$HOME`, absolute paths or URIs, or `"all": true`. Ask
  for as little as possible: `$PANEL` or `$SELECTION` is usually enough.
  `sc test` grants only `$PANEL`, `$OTHER` and `$SELECTION`.
- The script reads `sc.context`: `command` (the id), `panel`, `other`
  (folders) and `selection` (selected entries). `sc.args` is empty.

Install and use: `sc plugin install FOLDER`, then Alt+X or F9 in the
interface, or `sc plugin run NAME COMMAND --panel DIR FILES...`.
