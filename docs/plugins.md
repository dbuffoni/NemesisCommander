# Plugins

A plugin adds commands, panel columns or viewers to NemesisCommander. It is a
folder with a `plugin.json` and a TypeScript script. This page explains how to
install plugins, how to write one, and how to publish your own repository of
plugins. The scripting API itself is in [scripting.md](scripting.md)
(`nmc run --types` prints it, `nmc run --guide` prints a guide for AI
assistants).

## Installing plugins

```sh
nmc plugin available                    # what the repositories offer
nmc plugin available --search rename    # filter by name or description
nmc plugin install random-file          # by name
nmc plugin install NAME@REPOSITORY      # when two repositories have NAME
nmc plugin install ./my-plugin          # from a folder
nmc plugin install github:me/tools --subdir plugins/tidy   # from Git
nmc plugin update                       # newer versions of what came from Git
nmc plugin remove NAME
```

`available` marks each plugin **Official** (from the official repository) or
**Unofficial** with the repository it comes from, and says whether it is
installed and whether a newer version is available. The first time a plugin
runs, you are asked to allow what its manifest requests; an update that asks
for more asks again.

### Repositories

The official repository (the `plugins/` folder of the NemesisCommander
repository) is built in. Your own are added from the command line:

```sh
nmc plugin repo add github:me/my-plugins          # asks to confirm
nmc plugin repo add https://example.com/x.git#v2  # a branch or tag
nmc plugin repo list
nmc plugin repo remove https://example.com/x.git#v2
nmc plugin repo official off                      # hide the official one
```

Adding a repository asks for a confirmation that names it, because its
plugins run code (still behind the permission you grant to each plugin).
Removing a repository does not uninstall its plugins. They are saved in the
settings file:

```toml
[plugins]
official = true

[[plugins.repositories]]
url = "https://github.com/me/my-plugins.git"
reference = "v2"        # optional
```

Lists are kept in the data folder (`plugin-catalog`), so browsing works
offline; they are fetched again at most once a day, or with `--refresh`. The
system's `git` does the fetching.

## Writing a plugin

Copy [`plugins/random-file`](../plugins/random-file), a template that explains
every part in comments, and try it with `nmc test`:

```sh
nmc test ./my-plugin --select photo.JPG --json
```

### plugin.json

```json
{
  "name": "sort-by-type",
  "version": "1.0.0",
  "description": "Moves the selected files into folders named after their extension",
  "author": "Your Name",
  "license": "MIT",
  "homepage": "https://example.com/sort-by-type",
  "min_nmc": "0.4.0",
  "main": "main.ts",
  "commands": [
    { "id": "sort", "title": "Sort selected files by type", "category": "Files" }
  ],
  "permissions": { "write": ["$PANEL"], "run": ["bsdtar"] },
  "limits": { "memory": 64, "timeout": 120 }
}
```

- `name`: lowercase letters, digits and dashes; unique among installed plugins.
- `version`, `description`: shown in the catalogue. `author`, `license`
  (an SPDX name), `homepage`, `min_nmc` are optional.
- `main`: the script, inside the plugin folder (default `main.ts`).
- `commands`: each has an `id` (given to the script as `nmc.context.command`),
  a `title` and an optional `category` (`Files`, `Select`, `Navigate`, `View`,
  `Tools`, `NemesisCommander`; anything else goes under `Plugins`).
- `columns` and `previewers`: panel columns and viewers for kinds of files
  (see the manual, "Panel columns and viewers from plugins").
- `limits`: memory in MiB and time in seconds.

### Permissions

A plugin starts with no access. `permissions` lists what it asks for, and you
allow it once:

| Field | Meaning |
| --- | --- |
| `read` | locations the plugin may read |
| `write` | locations it may read and change |
| `all` | every file you can reach |
| `run` | programs it may start (`nmc.run`) |

Locations are absolute paths or URIs, or placeholders filled in at each run:
`$PANEL` (the active panel's folder), `$OTHER` (the other panel's), `$SELECTION`
(each selected entry) and `$HOME`.

`run` is a list of program names, like `["bsdtar", "7z"]`: bare names, looked
up on your PATH, never paths and never a shell. The permission is shown in
words (“may run: bsdtar, 7z”) and a change of the list asks again. In the
script:

```ts
const result = await nmc.run("bsdtar", ["-tf", archive]);
if (result.code !== 0) nmc.print(result.stderr);
```

The arguments are a list and reach the program unchanged (spaces and quotes
included). A program that is not in the list is refused; one that is not
installed gives a clear error the script can show. The plugin's time limit
applies, and `cwd` (a folder the plugin may read) can be given as an option.

## Making a repository of plugins

Any Git repository can be a plugin repository. Put one folder per plugin
under `plugins/`:

```
plugins/
  tidy/
    plugin.json
    main.ts
    README.md        (optional)
  rename-photos/
    plugin.json
    main.ts
```

There is no index to maintain: the list is built from the `plugin.json` files.
Push it to GitHub and tell people to run:

```sh
nmc plugin repo add github:you/your-plugins
nmc plugin install tidy
```

A repository can also list plugins that live in other repositories, with an
optional `plugins/index.json`:

```json
{
  "plugins": [
    {
      "name": "far-away",
      "version": "2.0.0",
      "description": "Lives in another repository",
      "repository": "github:someone/far-away",
      "ref": "v2",
      "subdir": "plugins/far-away"
    }
  ]
}
```

To change a plugin, raise its `version` in `plugin.json` and push: users get it
with `nmc plugin update`, and the catalogue shows “update available”.
