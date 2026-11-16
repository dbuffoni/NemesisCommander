# Scripts and plugins

SuperCommander runs TypeScript (and JavaScript) on a built-in V8 engine:
nothing else to install. Scripts see one global, `sc`, whose functions use
the same locations, providers and copy engine as the panels, so a script
works the same on local folders, SFTP, S3, WebDAV, SMB, agents and inside
archives.

The server build of `sc` (for `sc agent` and `sc serve`) does not include
scripts.

## Scripts: `sc run`

```sh
sc run --types > sc.d.ts          # declarations for your editor
sc run tidy.ts --allow-write ~/Downloads -- ~/Downloads --dry-run
```

A script starts with **no access** to files. Grant it locations:

| Flag | Grants |
| --- | --- |
| `--allow-read LOCATION` | reading below the location (repeatable) |
| `--allow-write LOCATION` | reading and changing below it (repeatable) |
| `--allow-all` | everything you can reach |
| `--memory MIB` | memory limit (default 512) |
| `--timeout SECONDS` | time limit (default none) |

Arguments after `--` are `sc.args`. A refused access throws a
`PermissionDenied` error the script can catch.

```ts
for (const entry of await sc.list(sc.args[0])) {
  if (entry.kind === "file" && entry.size === 0) {
    await sc.remove(entry.path);
    sc.print(`removed ${entry.name}`);
  }
}
```

The API (see `sc run --types`): `print`, `list`, `stat`, `exists`,
`readText`, `writeText`, `mkdir`, `rename`, `remove`, `copy` (with a
conflict policy, through the copy engine), `join`, `basename`, `dirname`,
`args`, `version` and, in plugins, `context`.

## Plugins

A plugin is a folder with a `plugin.json` and a script. See
[`examples/plugins/sort-by-type`](../examples/plugins/sort-by-type).

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
sc plugin install examples/plugins/sort-by-type
sc plugin list
sc plugin allow sort-by-type      # or answer the question at first use
sc plugin run sort-by-type sort --panel ~/Downloads ~/Downloads/*.jpg
```

Each command is in the command list (F9) as `Category/Title`. `category`
is optional: one of the built-in categories (`Files`, `Select`, `Navigate`,
`View`, `Tools`, `SuperCommander`) puts it among those commands; without it,
or with another name, it goes under `Plugins`. Any command can get a
shortcut there (Ctrl+K); it is saved in `[keys]` as
`"plugin.NAME.COMMAND" = ["Alt+T"]`.

Scripts you put in the `scripts` folder of the data folder
(`~/.local/share/supercommander/scripts` on Linux, or `$SC_DATA_DIR/scripts`)
are commands of the `Scripts` category, with shortcuts stored as
`"script.FILE" = [...]`. They run like a script from a panel: they may change
the active panel's folder and read the other one.

In the terminal UI and the window, **Alt+X** (Tools, "Plugins and
scripts…") lists the plugin commands, and offers to run the `.ts` or `.js`
file under the cursor (it may change the folder it is in, after a
confirmation). The first time a plugin runs you are asked to allow what its
manifest requests; an update asking for more asks again.

In a plugin, `sc.context` holds the command id, both panels' folders and
the selection.
