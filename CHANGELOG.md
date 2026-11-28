# Changelog

## 0.7.1

Rough edges found while using 0.7.0.

- **The plugin list no longer breaks on one new manifest.** A plugin whose
  `plugin.json` has a field this version does not know (such as `fkey` on a
  0.6 build) is listed with "requires newer Pairnox" and cannot be installed,
  instead of putting a raw error with a path in the list. Long descriptions
  wrap onto the next line in the terminal and the window.
- **Windows window:** secondary text (headers, footers, dates) is drawn with
  more contrast on Windows only; the window no longer opens a console beside
  it. (The Windows build prints nothing for `--help` or `--self-test`.)
- **`pnxr serve install HOST --with-settings`** also sends your settings to the
  server, the same as `pnxr config push scs://HOST`.
- **Similar pictures and the viewer read HEIC and RAW.** HEIC/HEIF photos
  (iPhone and others) are decoded with a decoder written in Rust, with no
  library to install (ADR 0064); RAW camera files (CR2, CR3, NEF, ARW, DNG,
  ORF, RW2, RAF and more) are read through the JPEG preview inside them, so a
  RAW and its JPEG are found as copies. Two more commands, *nearly identical
  only* and *looser match*, give fewer or more matches than the default.
- **AppImage:** the Wayland platform plugins are bundled, and the release
  build checks that the portal theme plugin and Wayland are inside it.
- **Message log** (Pairnox menu, F9): every message of the status line since
  start, errors included, newest first. `c` copies one, `a` copies all.
- **Compare** is now *Compare two files* (Ctrl+F3) and only compares two
  files; a folder under a cursor gets a clear message. *Compare folders* is its
  own command (F9; bind a key to it), and files missing from one side are
  shown in **light red** instead of the selection blue, in the terminal and the
  window, light and dark.
- **AI providers** (ADR 0063): Settings, *AI providers…* (also in the Pairnox
  menu) holds Anthropic, OpenRouter, OpenAI, a local server (Ollama, LM Studio)
  and any other server that speaks the OpenAI API, each with address, model
  and key, and a default one. `pnxr ai new|rename` take `--provider` and
  `--model`, `pnxr ai key --provider NAME` saves a key, `pnxr ai providers`
  lists them. Keys stay out of the settings file (variable or a private file).
- **AI key help.** When the default provider has no key, *Write a script with
  AI* and *Multi-rename with AI* open a dialog that says how to get a key and
  every way to give it, with a field to paste and save it there.
- **Pairnox menu:** *Install pnxr serve on a server…* moved from Tools to the
  Pairnox menu (the command keeps its id and shortcuts). New *Install the pnxr
  command…* puts a `pnxr` launcher in `~/.local/bin` (or a folder you choose;
  `%LOCALAPPDATA%\Pairnox\bin` on Windows), shows its status, removes it, and
  adds or removes the `pnxr shell-init` line in your shell's startup file.
  *Quit without changing the shell's folder* ends the terminal interface
  without the `cd` that `shell-init` does.
- **Terminal contrast.** Secondary text (footers, the info line under the
  panel, headers, the inactive panel, comments in the viewer) is the terminal's
  own text colour, dimmed, instead of "bright black" that was nearly invisible
  on many dark themes; the F-key labels are the terminal's colours swapped
  instead of black on bright cyan.
- **Terminal logo.** The About logo is drawn from its shape (ring, chevrons,
  white and black halves) at two sizes, with nothing drawn around the disc.
- **Sharp pictures in the terminal** (SC-38): the viewer and the quick view use
  kitty graphics (kitty, Ghostty), iTerm2 images (iTerm2, WezTerm) or sixel
  (foot, mlterm, Contour, or any terminal when forced) when the terminal is
  known to have them (not inside tmux). Other terminals get a sharper block
  drawing (Lanczos scaling, no smearing), or sextants. Setting: *Pictures in the
  terminal* (or `PNXR_IMAGES`).

## 0.7.0

Released.

- **Image duplicates plugin 1.1.0** (ADR 0062). *Find similar pictures* now shows
  a list in the window and the terminal, with the columns Group, Match, Pixels,
  Size, Date and File. `s` sorts by the next column, `r` reverses, `Space` marks,
  `a` marks all but the best of each group, `Delete` moves the marked pictures to
  the trash after a question that lists them, `m` moves them to
  `_similar-extras`. Hashes are remembered between runs (`image-hashes.tsv` in
  the data folder), so a second look at a big folder is quick. Scripts get
  `pnxr.showResults()` for such lists. Not done: a virtual panel with its own
  columns, RAW/HEIC pictures, a choice of the similarity threshold.
- **Plugin manifest defaults** (ADR 0062): a command in `plugin.json` can declare
  `shortcut`, `fkey` and `label` (a second name for `short`). They are defaults
  below everything the user sets; a chord that a built-in command or another
  plugin has is not taken. image-duplicates uses Shift+F9 and Ctrl+Shift+I.
- **A note after the shortcut settings were converted.** The first start after the
  conversion to the new shortcut model (0.6.0) says so on the status line once
  and keeps the old file as `config.toml.before-shortcut-model-2`.
- **Windows:** `pnxr-gui.exe` carries the Pairnox icon (window, taskbar, Alt+Tab,
  shortcuts); the installer smoke tests check the icon resource of the exe and
  the icon of the macOS bundle.
- **Fixes from the check on a GNOME desktop** (AppImage 0.6.1): the window follows
  the dark theme (the AppImage starts with the desktop portal as Qt's platform
  theme on GNOME unless you set `QT_QPA_PLATFORMTHEME`), `pnxr desktop install`
  no longer overwrites a `pairnox.desktop` written by another tool (`--force`
  does), and the group titles of the F1 help are readable on the light theme.
- The command list no longer repeats what is obvious in its hint line (the
  category arrows and "Type: find").
- **README:** the window and the terminal side by side with real screenshots, how
  settings are shared (`pnxr config push`/`pull`) and how servers are set up.
  `scripts/tui-screenshot.py` takes terminal screenshots of the real program.

## 0.6.1

Released 2026-11-27. This release also contains everything listed under 0.6.0
below: 0.6.0 was never published on its own.

Fixes from the visual check of the 0.6.0 window on the desktop.

- The command list shows the function key and button name of every command
  next to its shortcuts.
- The window draws no font ligatures: `rw-r--r--`, `->` and diffs show the
  characters they have.
- Checksums and Git output use the fixed-width font and a window wide enough
  for their lines; help tables shrink a little before they wrap. A missing file
  in a checksum check no longer shows its whole address, and a checksum file is
  titled "Check or write checksums".
- A long Git branch name no longer covers the drive button of a narrow panel.
- Lists longer than their window show a scroll bar.
- Settings: names without a value use the whole row. The editor list says what
  the default of each setting does and no longer repeats "(terminal)".
- Key rows: Properties no longer offers "Enter: choose" or two Enter entries,
  and Multi-rename names Ctrl+O and F1.
- Image duplicates plugin 1.0.1: the report shows paths from the panel's folder.

## 0.6.0

Not published on its own; included in 0.6.1.

- **Internal: ports and adapters** (ADR 0061). Saved logins moved to a new crate
  `pnxr-secrets`; the interfaces reach terminals, Git, disks, SSH, S3 keys and
  the server install through ports (`HostTools`, `Remotes`, `SecretStore`) with
  fakes for tests, and a test fails if a front-end crate depends on an adapter
  crate. No change in behaviour.

- **Find similar pictures is now a plugin**, `image-duplicates` (ADR 0060): it
  groups pictures by a perceptual hash, adds the panel columns Similar and
  Picture and a command that moves the extra copies aside. The built-in command,
  its dialog and its code are gone; a key you had given to it follows the
  plugin. New script function `pnxr.imageHash(path)`.

- **Shortcut model** (ADR 0059): every command has standard shortcuts and,
  independently, one function key (F, Shift+F or Ctrl+F) with a short name
  shown on the bottom bar. The command list has Ctrl+K (shortcut) and Ctrl+F
  (function key, name behind Ctrl+N); Ctrl+F in the main window switches the bar
  row in a terminal. Settings of earlier versions are converted when read.

- **New Pairnox logo** (ADR 0058): the round icon and the wordmark replace the old
  picture everywhere (window and taskbar icon, About, AppImage, macOS bundle,
  Windows `.ico`, README). All pictures are made from three SVG masters by
  `scripts/make-icons.py`.

## 0.5.0

Released 2026-11-27.

- **NemesisCommander is now Pairnox, the program `pnxr`** (`pnxr-gui` for the
  window): crates, environment variables (`PNXR_*`), script global, agent
  protocol, certificates, folders and installers use the new name. Nothing is
  migrated: settings, certificates (run `pnxr tls init`), scripts and plugins of
  older installs are not read. Agents of the old and the new name cannot talk
  to each other.
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
- F1 in the lists and questions explains the row under the cursor (Properties
  rows, link kinds, synchronize modes, import, checksums, ready-made masks and
  rules, bookmarks, basket, drives...) and goes back to the dialog.
- Git actions (Ctrl+F11): stage, unstage, diff, commit, discard (asks first),
  switch branch, log, pull (fast-forward only) and push, with the system's
  `git`, on the selected files.
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
