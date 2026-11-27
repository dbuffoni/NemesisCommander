# Changelog

## 0.5.0

Not published yet.

- Function keys: Shift+F is the advanced variant of the plain key (Shift+F3
  quick view, Shift+F4 properties, Shift+F6 unpack, Shift+F7 link) and
  Ctrl+F holds the tools (Ctrl+F2 search, Ctrl+F3 compare, Ctrl+F4
  synchronize, Ctrl+F5 duplicates, Ctrl+F6 checksum, Ctrl+F9 plugins). The
  duplicate rename on Shift+F6 is gone. Moved: hidden files Ctrl+H, sorting
  Ctrl+1/2/3, flat view Ctrl+Shift+F. Letter shortcuts stay as aliases; keys
  you set yourself are not touched.
- Open terminal: the system's default terminal is found with
  `xdg-terminal-exec` first (Ptyxis and GNOME Console on Fedora Atomic), every
  terminal is told the folder with its own option and starts in it, the
  environment of the AppImage is not passed on (it broke other AppImages such as
  Warp), a terminal that fails shows what it said, and the setting accepts
  quotes and `{dir_url}`.
- Properties is one dialog (the separate "Change dates and permissions" is
  gone): it opens read-only, `e` or F2 turns on editing with a check box for
  each permission, the octal number kept in step and a checked date.
- Checksums: Enter in the Checksums window, or Ctrl+K on a selection or a
  folder, writes checksum files (`SHA256SUMS`, `B3SUMS`, `name.sha256`,
  `name.b3`) that `sha256sum -c` and `b3sum --check` accept, and a checksum
  file can be checked; `nmc hash --write / --per-file / --check`.
- Settings: separate editors for the window and the terminal, a list of open
  source editors (installed ones first), a Git section, the help of the
  selected setting under the list (F1 for all of it), and Find also matches
  section titles and help.
- Select by mask: the question shows how many files match while you type,
  F2 switches between select, unselect and keep only, Ctrl+O lists ready-made
  masks, F1 explains the syntax, `has:text` is a new mask, and the last mask
  is remembered.
- Multi-rename: options after the template (letter case, replace text or
  regular expression, counter start/step/width), a count of the names that
  change, ready-made rules on Ctrl+O and a help on F1.
- The window starts with JetBrainsMono Nerd Font Mono at 11 pt; where the font
  is not installed the system fonts are used.

## 0.4.1

The first release to be published since 0.2.0; it includes everything in
0.4.0 below.

- Desktop window: the folder text is shown formatted; the main shortcuts,
  long messages and long yes or no questions scroll with the keys, and the
  key row says so; table rows stay readable at large fonts.
- The plugin list says that it is loading and why it failed.
- Install nmc serve: the option tables fit 80 columns.
- Copies record each file in the transfer journal as soon as it ends, so
  an interrupted copy repeats less; overwriting many files reads the
  destination folder less often.
- Building the window needs Qt 6.4 or later.
- macOS: the app in the disk image uses the Qt bundled with it.

## 0.4.0

Not published.

- Plugin repositories: an official catalogue and your own Git repositories,
  browsed and installed from the program; a plugin may run the programs it
  asks permission for.
- Short names for every command and plugin command; the bottom bar buttons of
  the window are equally wide.
- Removing a saved connection, bookmark or saved search, and emptying the
  basket, ask first.
- Saved logins use the Windows Credential Manager and the macOS Keychain.
- Windows paths: network shares, long paths, reserved names and folder links.
  Copies keep or drop attributes and extended attributes across systems, and
  names that clash on the destination are recognised.
- RAR archives are read with libarchive; no unRAR code is included.
- Installers: a Windows zip and a macOS dmg (signed only when signing keys
  are configured).
- Small windows and large fonts: long dialog texts scroll with the keys, the
  function key bar stays readable, the hex view fits, dialog tables keep their
  columns.
- Faster sorting and copying.
- Copyright holder, licence of the assets and third-party notices.
- CI in two tiers, with nightly tests on Linux, Windows and macOS and against
  real servers.

## 0.3.0

A consolidation release: no new features.

- Code review fixes: a mirror no longer deletes on an unreadable side, no
  panic on provider names that leave the folder, a failed job start no longer
  leaves its controls behind, the selection summary follows the quick filter.
- Traces of earlier releases and of the old name removed from code, scripts,
  workflows and documents.
- More tests, a check of keys and dialogs, documentation and open source files.

## 0.2.0

Shell exit into the last folder (`nmc shell-init`), terminal launcher, folder
text, archive quick view, plugin columns and viewers, MCP server (`nmc mcp`),
copiable log, `localhost` without SSH, bottom bar with Shift and Ctrl keys,
F1 help that fits every screen size.

## 0.1.0

First release: twin panels, local files, SFTP, SMB, WebDAV, S3 and archives,
viewer, transfer queue with resume, multi-rename, agent, plugins and scripts,
terminal interface and Qt window.
