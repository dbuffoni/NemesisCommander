# 0038. Plugins from Git, the logo redrawn, password hint in Pack

- Status: Accepted
- Date: 2026-11-21

## Context

Cards SC-31 (install plugins from Git and update them), SC-106 (branding is
still broken: no window icon, a bad logo in About) and SC-107 (an archive is
not protected when asked). Also: CI takes too long on macOS and Windows.

## Decision

- **SC-31.** `nmc plugin install SOURCE` accepts a Git address (URL,
  `git@host:path`, `github:owner/repo`, `#branch-or-tag` suffix, `--subdir`)
  besides a folder. The system's `git` clones it (`--depth 1`, no terminal
  prompts), the plugin is copied without `.git`, and `plugin-lock.json` (in
  the data folder, next to the plugins) records url, reference, subdir,
  commit and version. `nmc plugin update [NAME…]` clones again and replaces
  the plugin when the commit differs. Permissions are unchanged in spirit: an
  update that asks for more must be allowed again (the grant holds the exact
  request). Nothing in the repository runs while fetching. A curated index
  (the card's "optionally") is left out. `nmc-script::source`, no new
  dependency apart from `tempfile`.
- **SC-106.** The earlier icon was a rough approximation (a pill across the
  arch). `scripts/make-icons.py` now holds the paths traced from the artwork
  and renders the SVG and every PNG with Chromium; the About logos are the
  same shapes without the tile. The window gets every size (16–256) instead
  of the 256 only.
- **SC-107.** The archive libraries do encrypt (AES-256 for zip and 7z, tested
  end to end), but only the advanced Pack asked for a password, and its key
  was not obvious. The key row of the Pack question now says
  "Ctrl+O: more options (format, level, password)" and the manual says so.
  Needs Daniele's check: if the reported flow was another one, the card must
  be reopened with the steps.
- **CI.** The macOS and Windows jobs (`test`, `gui-other`) run only when the
  repository variable `CI_ALL_OS` is `true`; set it to turn them back on.
