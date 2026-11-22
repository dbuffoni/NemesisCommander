# Scripts and plugins

NemesisCommander runs TypeScript (and JavaScript) on a built-in V8 engine:
nothing else to install. Scripts see one global, `nmc`, whose functions use
the same locations, providers and copy engine as the panels, so a script
works the same on local folders, SFTP, S3, WebDAV, SMB, agents and inside
archives.

The server build of `nmc` (for `nmc agent` and `nmc serve`) does not include
scripts.

## Scripts: `nmc run`

```sh
nmc run --types > nmc.d.ts          # declarations for your editor
nmc run tidy.ts --allow-write ~/Downloads -- ~/Downloads --dry-run
```

A script starts with **no access** to files. Grant it locations:

| Flag | Grants |
| --- | --- |
| `--allow-read LOCATION` | reading below the location (repeatable) |
| `--allow-write LOCATION` | reading and changing below it (repeatable) |
| `--allow-all` | everything you can reach |
| `--memory MIB` | memory limit (default 512) |
| `--timeout SECONDS` | time limit (default none) |

Arguments after `--` are `nmc.args`. A refused access throws a
`PermissionDenied` error the script can catch.

```ts
for (const entry of await nmc.list(nmc.args[0])) {
  if (entry.kind === "file" && entry.size === 0) {
    await nmc.remove(entry.path);
    nmc.print(`removed ${entry.name}`);
  }
}
```

The API (see `nmc run --types`): `print`, `list`, `stat`, `exists`,
`readText`, `writeText`, `writeBytes`, `mkdir`, `rename`, `remove`, `copy` (with a
conflict policy, through the copy engine), `join`, `basename`, `dirname`,
`args`, `version` and, in plugins, `context`.

## Plugins

A plugin is a folder with a `plugin.json` and a script. See
[`examples/plugins/sort-by-type`](../examples/plugins/sort-by-type).
To start your own, copy [`examples/plugins/random-file`](../examples/plugins/random-file):
a template that creates a file with random content (json, xml, txt or png)
and explains every part in comments, with `TEMPLATE` marking what to replace.

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

Permissions list locations to `read` and to `write` (or `"all": true`).
Besides absolute paths and URIs, these placeholders are filled in at each
run: `$PANEL` (the active panel's folder), `$OTHER` (the other panel's
folder), `$SELECTION` (each selected entry) and `$HOME`.

```sh
nmc plugin install examples/plugins/sort-by-type
nmc plugin list
nmc plugin allow sort-by-type      # or answer the question at first use
nmc plugin run sort-by-type sort --panel ~/Downloads ~/Downloads/*.jpg
```

Each command is in the command list (F9) as `Category/Title`. `category`
is optional: one of the built-in categories (`Files`, `Select`, `Navigate`,
`View`, `Tools`, `NemesisCommander`) puts it among those commands; without it,
or with another name, it goes under `Plugins`. Any command can get a
shortcut there (Ctrl+K); it is saved in `[keys]` as
`"plugin.NAME.COMMAND" = ["Alt+T"]`.

Scripts you put in the `scripts` folder of the data folder
(`~/.local/share/nemesiscommander/scripts` on Linux, or `$NMC_DATA_DIR/scripts`)
are commands of the `Scripts` category, with shortcuts stored as
`"script.FILE" = [...]`. They run like a script from a panel: they may change
the active panel's folder and read the other one.

In the terminal UI and the window, **Ctrl+X** (Tools, "Plugins and
scripts…") lists the plugin commands, and offers to run the `.ts` or `.js`
file under the cursor (it may change the folder it is in, after a
confirmation). The first time a plugin runs you are asked to allow what its
manifest requests; an update asking for more asks again.

In a plugin, `nmc.context` holds the command id, both panels' folders and
the selection.

## Testing, and writing with an AI assistant

`nmc test` tries a script or a plugin without touching your files. It makes
a temporary folder with `panel/` (sample files, or a copy of
`--fixture DIR`) and `other/` (empty), runs the script there, or each
plugin command, and reports:

- checks: the manifest, the syntax and, when the TypeScript compiler
  (`tsc`) is installed, the types against `nmc.d.ts`;
- for each run: whether it passed, the error, what it printed, and the
  files it created, removed and changed.

```sh
nmc test tidy.ts                                  # nmc.args = [the test panel]
nmc test tidy.ts --fixture ./samples -- '$PANEL' --dry-run
nmc test examples/plugins/sort-by-type --select photo.JPG --json
```

A script may change anything in the test folder. A plugin gets what its
manifest asks for `$PANEL`, `$OTHER` and `$SELECTION` (filled in with the
test folders and `--select`, default all of `panel/`); other locations are
not granted and are listed as notes. `--json` prints the report for
programs, `--keep` keeps the folder, and the exit status is 0 only when
everything passed.

This is what lets an AI coding assistant (Claude Code, for example) write
scripts and plugins on its own: give it the API and the guide,

```sh
nmc run --types > nmc.d.ts
nmc run --guide > AGENTS.md     # or CLAUDE.md
```

ask for the script, and it writes it, runs `nmc test --json`, reads what
went wrong or what changed, and fixes it until it passes (ADR 0021).

## `nmc ai`: scripts written by Claude

NemesisCommander can also do this loop itself, with your Anthropic API key:

```sh
echo "$KEY" | nmc ai key        # once; or set ANTHROPIC_API_KEY
nmc ai new "move files older than 30 days into a folder named old"
nmc ai new --plugin "count the lines of the selected text files"
```

`nmc ai new` sends the description, the guide and the API to Claude
(`claude-opus-5-5` unless `--model` says otherwise), tests the answer with
`nmc test`, and sends the report back when it fails, up to `--tries` times
(default 3). Each try is one API request, billed to your key.

- A script that passes is saved in your scripts folder, so it is a command
  of the `Scripts` category (F9). A plugin is installed; like any plugin it
  asks for its permissions the first time it runs.
- `--out PATH` saves it elsewhere instead, `--fixture DIR` tests it on your
  own sample files, `--force` replaces one with the same name.
- If no try passes, nothing is saved and the drafts are kept in a
  temporary folder, named in the error.
- In the terminal UI and the window: F9, **Tools/Write a script with AI…**
  asks what the script should do and runs `nmc ai new` with it.

`nmc ai rename "date first, as YYYY-MM-DD" FILES...` does the same for the
multi-rename: Claude writes the rule (template, find/replace or regex, case,
counter) from the description and the file names (only the names are
sent), NemesisCommander checks it on the files and sends errors back, and
prints the rule as an `nmc rename` command line with the new names (names
that already belong to other files are marked; `--json` lists them as
`conflicts`). It renames nothing. In the interfaces, F9, **Files/Multi-rename
with AI…** shows the rule and the same preview as Ctrl+M, and renames after
a confirmation (ADR 0024).

A passing test means the script ran without errors on sample files and the
report shows what it changed; read the script before running it on files
that matter. Server builds leave `nmc ai` out (ADR 0022).
