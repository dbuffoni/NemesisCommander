# 0054. Plugin repositories

Status: accepted (0.4.0, SC-160)

## Context

Plugins were installed one at a time from a folder or a Git URL. There was no
way to see what exists, no official source, and a plugin could not run
programs (ADR 0016 left that for later).

## Decision

- A repository is a Git repository with a `plugins/` folder, one folder per
  plugin with its `plugin.json`. The catalogue is built from those manifests; an
  optional `plugins/index.json` lists plugins that live in other repositories.
- The official repository is the `plugins/` folder of this repository
  (`examples/plugins` moved there). It is built in, cannot be removed and
  can be switched off. Extra repositories are the `[plugins]` section of the
  settings; adding one asks for a confirmation naming it, and checks that it
  has plugins.
- Catalogues are cached in the data folder and fetched at most once a day, so
  browsing works offline. Fetching uses the system `git`, as before.
  `plugin-lock.json` keeps the repository and folder, so `plugin update`
  fetches from the right place.
- `nmc plugin available | repo add|list|remove|official | install NAME[@REPO]`
  and, in both interfaces, Ctrl+X, "Browse and install plugins…" with a list
  of repositories (F7). The list marks plugins Official or Unofficial,
  Installed and update available.
- Permission to run programs: `permissions.run` lists bare program names;
  `nmc.run(program, args, {cwd})` starts one without a shell, the arguments
  reach it unchanged, a program not in the list is refused, and `all` does not
  include programs. The list is part of the grant, so changing it asks again.

## Not done

A hosted marketplace, signatures, pinned commits for repositories, plugins that
add file systems or archive formats.
