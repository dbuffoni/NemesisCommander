# NemesisCommander manual

NemesisCommander is a dual-pane file manager for local and remote files. It
comes as:

- **`nmc-gui`**, a desktop window (Qt 6), shipped as an AppImage on Linux;
- **`nmc`**, one program that is the terminal interface, a command line for
  scripts, and the agent that runs on servers.

Both interfaces work the same way and share every shortcut, so what you learn
in one works in the other. There are no toolbars or icons: every command is
in the command list (**F9**), where you find it by typing or by category,
and on the keyboard. **F1 shows the main shortcuts** at any time.

The pictures in this manual come from `scripts/screenshots.sh`, which drives
the real window on a demo folder.

## Contents

1. [Getting started](#getting-started)
2. [Commands (F9)](#commands-f9)
3. [The panels](#the-panels)
4. [Working with files](#working-with-files)
5. [Selecting files](#selecting-files)
6. [The viewer](#the-viewer)
7. [Quick view](#quick-view)
8. [Searching and comparing](#searching-and-comparing)
9. [Synchronizing folders](#synchronizing-folders)
10. [Archives](#archives)
11. [Remote locations](#remote-locations)
12. [Servers: the agent and `nmc serve`](#servers-the-agent-and-nmc-serve)
13. [The transfer queue](#the-transfer-queue)
14. [Bookmarks, tabs and history](#bookmarks-tabs-and-history)
15. [Plugins and scripts](#plugins-and-scripts)
16. [Assistants: `nmc mcp`](#assistants-nmc-mcp)
17. [The command line](#the-command-line)
18. [Settings](#settings)
19. [All shortcuts](#all-shortcuts)

## Getting started

Start the window with `nmc-gui` (or the AppImage), or the terminal interface
with `nmc`. Both take up to two locations, one for each panel:

```sh
nmc-gui ~/Projects sftp://nas/backup
nmc ~/Downloads s3://photos/2026
```

Without locations both panels open the current folder. The window also
takes `--size=WIDTHxHEIGHT` to choose its starting size.

![The window at 1280 × 800](images/panels-1280x800.png)

The window adapts to its size; here it is at 800 × 500 and at 1920 × 1080:

| 800 × 500 | 1920 × 1080 |
|---|---|
| ![Small window](images/panels-800x500.png) | ![Large window](images/panels-1920x1080.png) |

## Commands (F9)

**F9** opens the command list: every command of NemesisCommander, of your
plugins and of your scripts, one per row as `Category/Command`, with its
shortcuts on the right.

![The command list](images/commands.png)

- **Type** to find a command: the list keeps the rows that contain every
  word you typed, in the command or in its category (`files check` finds
  Files/Checksum), the names starting with what you typed first.
  Backspace erases a letter, Ctrl+U or Esc empties the filter.
- **Categories**: the buttons on top (All, Files, Select, Navigate, View,
  Tools, NemesisCommander, and Plugins and Scripts when you have some) show
  one category only. Left and Right (or Tab) move between them; in the
  window you can also click them.
- **Up/Down**, Page Up/Down, Home/End move; **Enter** (or a click in the
  window) runs the command.
- **Ctrl+K** gives the highlighted command a shortcut: press the new key
  (Esc cancels). If another command had that key, it loses it and the status
  line says which one. **Delete** removes the command's shortcuts. The
  change is saved at once, as on the settings screen.
- **F1** explains the highlighted command in a sentence or two; F1 or Esc
  hides the explanation.

  ![F1 explains a command](images/commands-help.png)
- **Esc** empties the filter, or closes the list.

![One category](images/commands-category.png)

![Finding a command](images/commands-find.png)

In the window there is no menu bar: this list is the menu. Clicking **F9
Commands** in the bar at the bottom opens it too.

The command **App/About NemesisCommander** (no shortcut; give it one in the
settings if you like) shows the logo, the version and the commit this build
was made from, the license and where the source is.

## The panels

Each panel lists one folder: an icon, name, size and modification date. The title of
the panel shows the location (your home folder is written `~`); the column
titles show the sort order with an arrow. Folders are bold and end in `/`;
`..` goes up.

- **Tab** switches between the panels. The active panel has a coloured
  border and a strong cursor bar; the other keeps a faint one.
- **Enter** opens a folder (or an archive), **Backspace** goes to the parent.
- **Ctrl+L** goes to any location: a path, `~/…`, or a URI such as
  `sftp://host/dir`.
- **Ctrl+F3 / Ctrl+F4 / Ctrl+F5** sort by name, extension or date; pressing
  the same one again reverses the order. (There is no "sort by size": use
  the disk usage list, **Alt+U**, below.)
- **Ctrl+H** shows or hides hidden files, **Ctrl+R** reloads.
- **Ctrl+S** filters the listing as you type (`*.pdf`, `report`). While a
  filter is on, the line under the panel says `filter: … (Esc clears)`.
- **Esc** clears the filter; with no filter it clears the selection (and,
  right after **Alt+C** compared two folders, the differences it selected in
  both panels).
- **Ctrl+↓** (or **Ctrl+U**) swaps the panels, **Ctrl+↑** (or **Alt+O**) opens
  this folder in the other one.
  **Ctrl+←** and **Ctrl+→** open the folder under the cursor in the left or
  right panel (on a file, this panel's folder). With "← goes up, → enters a
  folder" on in the settings, **←** goes to the parent folder and **→** enters
  the folder under the cursor.

### File type icons

Every entry starts with a small icon for its kind: a bold outline in one
colour with no fill, so it stays quiet next to the name. There are icons for
folders and for more than thirty kinds of files, in groups: office documents
(PDF red, Word-like documents blue, spreadsheets green, presentations orange),
text and Markdown, source code, scripts, data and settings files, pictures,
audio, video, archives, 3D models and drawings, fonts, programs and
installers, disc images, keys and certificates, and e-books. The kind comes
from the name's extension and the entry's type, so files on remote locations
have icons too. The terminal interface shows one coloured character per kind
(`▸` folder, `≡` text, `▤` document, `▲` picture…). The setting **Icons for
the kinds of files** (Panels) turns them off.

### Folder sizes and disk usage

A folder shows `<DIR>` instead of a size because counting is slow on big
trees. To see what takes the space, use the disk usage list below.
Files are drawn on the same scale as the folders, so you see at a glance
what takes the space.

**Alt+U** (Disk usage) lists the items of this folder by size, biggest first,
with a bar relative to the biggest, its share of the total and its size. Enter
looks into a folder, Backspace goes up, Esc closes. It works on any location,
including remote ones.

### Disks and partitions

**Alt+K** (Disks and partitions) shows the disks of this computer with their
partitions: name, type (disk, partition, LVM, removable…), size, partition table
(GPT or MBR), file system, a bar with the space used, the free space, where each
one is mounted and its label or model. Enter opens the mount point in the
panel. The list comes from the system's own tools (`lsblk` on Linux, `df` on
macOS, PowerShell on Windows), so nothing needs special rights; what the system
does not tell is shown as `-`.

The line under the panels describes the entry under the cursor (size, date,
permissions, link target) or shows the last message. The bottom row lists
what F1–F10 do, and it follows the keys you hold: with **Shift** down it shows
what Shift+F1…F10 do (`Drives` at F1, `Search` at F7, `Pack` at F5, `Unpack` at F9),
with **Ctrl** or a combination with Shift likewise (Alt does not change the bar), and nothing for the keys that are not bound. In the
window this follows the keyboard live (clicking a label with a modifier held
runs that combination). In the terminal it works only where the terminal
reports Ctrl, Alt and Shift on their own (terminals with the "kitty keyboard
protocol" turned on by the program; NemesisCommander does not turn it on).

The Alt+F keys are not used: desktops such as GNOME keep Alt+F1, Alt+F5, Alt+F7 and
Alt+F8 for themselves, so the window would never receive them. The extra
commands are on **Shift+F** instead.

### The mouse

Everything works from the keyboard, and the mouse works too, in the window
and in the terminal (if your terminal passes mouse events to programs):
click a row to put the cursor there, double-click to open it, turn the wheel
to scroll. **Drag a row onto the other panel** to copy it (or the selection,
if the row is part of it): you are asked where to copy to, as with F5.
In the window you can also drag files **to and from other programs**
(local files only): drop them on a panel to copy them into its folder, or
drag a row out to your desktop or another application.

### The flat view

**Alt+V** lists every file below the current folder in one list, with the
subfolder of each file in a *Folder* column, so you can see, sort, select and
copy files from a whole tree at once. Press it again, or Backspace, to come
back to the folder. Hidden files follow the usual setting, folders are not
listed, and listings stop at 100 000 files (the footer says so).

### Git marks

Inside a git repository the panel shows the branch in its title (`⎇ main ↑1 ↓2`:
one commit to push, two to pull) and a mark before each name: **M** changed or
staged, **?** untracked, **!** ignored. A folder carries the mark of what is
inside it. The marks are not the result of **Alt+C** (Compare): they are what
`git status` says now, read again every time the folder is listed, so they
disappear when the file is committed or ignored. With the cursor on a marked
entry, the line under the panel says what the mark means (for example
`git M: changed since the last commit`). It uses the `git` program of your system, only for local folders,
and can be switched off in the settings ("Git marks and branch in
repositories").

## Working with files

Commands act on the selected entries, or on the entry under the cursor when
nothing is selected. Copy and move go to the folder of the other panel by
default; the destination can be edited before you confirm.

| Key | Command |
|---|---|
| F3 | View the file ([the viewer](#the-viewer)) |
| F4 | Edit the file in your editor |
| F5 | Copy |
| F6 | Move |
| Shift+F6 or F2 | Rename |
| Ctrl+M | Rename many files with a template ([below](#renaming-many-files)) |
| F9, Files/Multi-rename with AI… | Describe the new names; Claude writes the rule ([below](#renaming-many-files)) |
| F7 | New folder |
| F8 or Delete | Delete (to the recycle bin when there is one) |
| Shift+F8 | Delete permanently |
| Shift+F5 / Shift+F9 | Pack into an archive / unpack one |
| Alt+Enter | Properties, and change the dates and permissions |
| F9, Files/Change dates and permissions… | Set the date and time and the permissions of the selection (words: `2026-11-17`, `10:30`, `now`, `mode=644`, `-r` for the contents of folders) |
| Ctrl+Z | Undo the last rename, new folder or move (up to 50 steps, until you quit) |
| Ctrl+K | Checksum (BLAKE3, XXH3 or SHA-256) |

| Copy | New folder | Delete |
|---|---|---|
| ![Copy](images/copy.png) | ![New folder](images/mkdir.png) | ![Delete](images/delete.png) |

When a file already exists at the destination, NemesisCommander asks whether
to overwrite, skip, keep both (renaming the new one) or overwrite only if
newer, for this file or for all of them. Every copy and move becomes a job
in the [transfer queue](#the-transfer-queue), so you can keep working while
it runs.

Properties show the name, location, type, size, date and permissions; on
systems with Unix permissions, Enter changes them (octal mode).

![Properties](images/properties.png)

### Typing and pasting

Every question (new folder, go to, a connection's address, a password…) is a
one-line field: Left, Right, Home, End, Backspace and Delete edit, **Ctrl+U**
empties it. **Ctrl+V** (or Shift+Insert) pastes the system clipboard into the
field, the filter of a list or the command search; in a terminal the terminal's
own paste does the same. Only the first line of the text is used, and pasting
into the panels does nothing, so a paste can never run a command.

### Renaming many files

**Ctrl+M** renames the selected files (or the one under the cursor) with a
template. While you type it, the dialog previews the first new names as
`old → new`, or says why the template cannot be used. The template is
copied for each file, with these placeholders:

| Placeholder | Becomes |
|---|---|
| `[N]` | the name without the extension |
| `[N2-5]`, `[N3-]` | characters 2 to 5 of the name, or from the 3rd to the end |
| `[E]` / `[X]` | the extension, without / with the dot |
| `[C]` | a counter: 1, 2, 3… in the order of the panel |
| `[P]` | the name of the folder the file is in |
| `[[`, `]]` | a literal `[` or `]` |

So `photo_[C][X]` turns `a.jpg`, `b.jpg` into `photo_1.jpg`, `photo_2.jpg`,
and `[P] - [N][X]` puts the folder name in front.

Enter shows the full preview: every name that changes (the first 12, and
how many more) and asks for confirmation; nothing is renamed before that.
If a new name belongs to a file that is not part of the rename, the preview
marks it `(already exists)` and nothing is renamed at all, so a rename never
replaces a file or stops halfway. Swapping names (`a` ↔ `b`) and changing
only the letter case are fine.

**F9, Files/Multi-rename with AI…** asks how the names should change, in
your words ("date first, as YYYY-MM-DD"). Claude writes the rule (template,
find and replace or regular expression, letter case, counter) from the file
names only, using your Anthropic API key; the same preview then shows the
rule and the new names before anything is renamed.

On the command line, `nmc rename` has every option of the rule (`--find`,
`--replace`, `--regex`, `--case`, `--start`, `--step`, `--digits`);
`--preview` (or `-n`) prints the new names without renaming. `nmc ai rename
"description" FILES...` prints the rule Claude wrote as an `nmc rename`
command, with the new names.

### Renaming in your editor

**Rename in the editor** (F9) writes the names of the selected items to a
file, one per line, and opens it in your editor (the one set in Settings, or
`$VISUAL` / `$EDITOR` in a terminal). Change the names, keep the lines in
order, save and close. You then see the old and new names, and nothing is
renamed until you confirm; names that already belong to other files stop the
whole thing, as in multi-rename. In the window, set an editor that waits until
it is closed (for example `code --wait`) in Settings first.

### Links

**Alt+L** makes a symbolic link (relative or absolute) or a hard link, in the
other panel's folder, to the selected items, with the same names. Only local
folders can hold links; an existing name is never replaced.

### Copying paths, exporting a listing

**Alt+P** copies to the system clipboard the full path, the name, the URI or
the folder of the selected items (one per line). In a terminal this uses the
OSC 52 escape sequence, which most terminals accept, also over SSH (tmux needs
`set -g set-clipboard on`).

![Copy path](images/copy-path.png)

**Export listing** (F9) writes the selected items, or the whole folder when
nothing is selected, as CSV, JSON, Markdown or HTML (name, path, type, size,
date). Give a file name to write it (an existing file is never overwritten), or
leave the name empty to copy it to the clipboard.

### The basket

The basket collects items from several folders and drives to act on together.
**Alt+B** adds the selection; **Ctrl+B** opens the basket. In it, **Enter**
shows an item in the panel, **F5** copies, **F6** moves, **F8** deletes,
**Shift+F5** packs (into `basket.zip` unless you type another name) and **Ctrl+M** renames everything in the basket, **Delete**
removes one item from the list (the file stays) and **Shift+Delete** empties it.
Moved and deleted items leave the basket by themselves. The basket is kept
until you quit.

![Basket](images/basket.png)

### A text for the folder

A file called `nmc_folder.md` (or `sc_folder.md`) in a folder is shown, as
Markdown, at the top of its panel in the terminal interface (up to a third of
the panel; files over 64 KiB are ignored): notes on what the folder is for.

## Selecting files

- **Insert** or **Space** selects the entry under the cursor and moves down.
- **Shift+Up/Down**, **Shift+PgUp/PgDn** and **Shift+Home/End** select a
  range as the cursor moves (Ctrl+Shift works too); moving back shrinks the
  range, and what was selected before stays. With the quick view open these
  keys scroll it instead.
- **+** selects by mask (`*.jpg;*.png`), **-** unselects by mask,
  **\*** inverts the selection, **Ctrl+A** selects everything. A mask that
  starts with `re:` is a regular expression (`re:^img-\d+\.jpe?g$`); the
  same works in the search, the quick filter and `nmc find --name`.

Selected entries are tinted, bold and marked at the left edge, so they stay
visible under the cursor. The panel footer counts the selected files,
folders and bytes.

## The viewer

F3 opens the viewer on the file under the cursor. It recognises the kind of
file and shows it the best way it can; **F4** always switches to the raw
bytes (hex) and back. The header lists the keys of the current kind of file
and the footer gives details (pages, picture size, zoom). **Esc** closes.

**Text** is shown with tabs expanded; F2 wraps long lines. Files that are not
text open as hex. Very large files are read only in part.

**Source and data files** are coloured: keywords, strings, numbers, comments
and names (keys, types, tags, variables). Shell scripts, YAML, JSON, XML and
HTML, SQL, TypeScript and JavaScript, Rust and Python are recognised by the
file name, or by the `#!` line when the name does not say. The language shows
in the footer. The same colours appear in the quick view and, for unchanged
lines, in a comparison (Alt+C). Files over 50 000 lines stay plain.

![Text](images/viewer-text.png)

**Markdown** is shown formatted: headings, emphasis, lists, quotes, code
blocks and tables.

![Markdown](images/viewer-markdown.png)

**Office documents** are shown as formatted text: Word (`.docx`) and
OpenDocument text (`.odt`) with headings, bold and italic, lists and tables;
spreadsheets (`.xlsx`, `.xls`, `.ods`) as one table per sheet; presentations
(`.pptx`, `.odp`) as the text of each slide.

| Word | Spreadsheet | Slides |
|---|---|---|
| ![Word](images/viewer-docx.png) | ![Spreadsheet](images/viewer-xlsx.png) | ![Slides](images/viewer-pptx.png) |

**E-books** are read as formatted text: EPUB (`.epub`, chapters in reading
order, with title and author on top) and FictionBook (`.fb2`, UTF-8 or
Windows-1251). Headings, emphasis, lists and quotes are kept; pictures, fonts
and styles are not. Up to 400 sections of a book are shown.

**Audio and video** files are not played, but described from their headers:
for audio (`.mp3`, `.flac`, `.ogg`, `.opus`, `.wav`, `.m4a`, `.aac`, `.aiff`,
`.ape`, `.wv`, `.mpc`) the tags (title, artist, album, year, genre), the
duration, the bit rate, the sample rate and the channels; for video (`.mp4`,
`.m4v`, `.mov`, `.3gp`, `.mkv`, `.webm`, `.avi`) the container, the duration
and each track (codec, picture size, frames per second, language). Only the
first 32 MiB of a file are read: an MP4 whose header is at the end of a large
file says so.

**PDF** pages are drawn by a built-in renderer (no external program).
PgDn/Space and PgUp turn the pages, Home and End go to the first and last;
**+**/**-** zoom and the arrows move around a zoomed page; **0** fits the page
again.

| Page 1 | Page 2 |
|---|---|
| ![PDF](images/viewer-pdf.png) | ![PDF, second page](images/viewer-pdf-page2.png) |

**3D models** (STL, OBJ, PLY, glTF and GLB) are drawn shaded. The arrows
turn the model, **+**/**-** zoom, **0** goes back to the first view; the footer
shows the number of triangles and the angles.

| Model | Turned and zoomed |
|---|---|
| ![Model](images/viewer-model.png) | ![Turned model](images/viewer-model-turned.png) |

**Pictures** (PNG, JPEG, GIF, WebP, BMP, ICO, TIFF) fit the window; **+**
and **-** zoom by steps up to 16×, the arrows move around, **0** fits again.

| Fit | Zoomed |
|---|---|
| ![Picture](images/viewer-image.png) | ![Zoomed picture](images/viewer-image-zoom.png) |

The terminal interface shows the same things: formatted documents in
colour, and pages, models and pictures with coloured half-block characters.

The viewer works on every location, remote ones and archives included.

## Quick view

**Ctrl+Q** turns the other panel into a preview of the entry under the
cursor, and keeps following the cursor as you move. It shows everything the
viewer shows; Ctrl+Q again closes it.

Plain **↑ ↓ PgUp PgDn** keep moving the cursor of the panel, and the preview
follows. To read the preview itself use **Shift+↑ / ↓** (a line),
**Shift+PgUp / PgDn** (a page; the pages of a PDF) and **Shift+Home / End**;
the mouse wheel over the preview scrolls it too. **Ctrl+U** (swap panels)
exchanges the file panel and the preview, and the focus goes with the file
panel.

An **archive** (zip, 7z, tar…) shows its contents: the number of files and
folders, the unpacked size and a list with size, date and name (the first
5000 entries), instead of the bytes of the file; **F4** in the viewer still
gives the hex dump.

| PDF | Model | Markdown |
|---|---|---|
| ![Quick view of a PDF](images/quick-view-pdf.png) | ![Quick view of a model](images/quick-view-model.png) | ![Quick view of Markdown](images/quick-view-markdown.png) |

## Searching and comparing

**Shift+F7** searches below the current folder by name mask (`*.rs;*.toml`,
with `!` to exclude) and, optionally, by text the files contain (start either with `re:` for a regular
expression, e.g. `re:^report-\d{2}`). Results
appear in a list as they are found; Enter jumps to the chosen file in the
panel. On the command line, `nmc find` also searches by regular expression,
size and age.

![Search](images/search.png)

The search asks one more thing in its conditions: the word `archives` makes
it look inside ZIP, 7-Zip, tar and RAR archives it meets (and in archives
inside archives, up to three levels). Found files are listed with the archive
in their path. On the command line it is
`nmc find --archives`.

**Alt+C** compares: two files (one under the cursor in each panel) by
content, or two folders, selecting the entries that differ. Two different
text files open side by side, with the file of the panel you are in on the
left: `-` marks lines only on the left, `+` lines only on the right and `~`
lines that differ (red on the left, green on the right). **n** and **p** jump to the
next and previous change, **Esc** closes. Files that are identical or not text just
report that.

The search asks three things: the names, the text inside the files, and
**conditions** on size and age, as words: `>10M` (bigger than 10 MiB), `<1G`,
`newer:7d` (changed in the last 7 days), `older:2w`; units for ages are `h`,
`d`, `w`, `m` (30 days) and `y`. Leave it empty for no conditions.

In the list of results, **Alt+B** puts all of them in the [basket](#the-basket),
so you can copy, pack, rename or delete them together.

### Saved searches

**Save the last search** (F9) keeps the search you just ran under a name, in
your settings file (`[[searches]]`). **Saved searches** lists them (Enter runs one
in the folder you are in, Delete forgets it), and each one is also in the
command list as `Tools/Search: NAME`, where **Ctrl+K** gives it a shortcut.

### Comparing by content

**Compare folders by content** (F9) hashes the files with the same name and
size on both sides, including those in subfolders, and selects what differs or
is only on one side. When a side is an agent connection (`agent://`), the agent
computes the hashes where the files are, so nothing is downloaded: this is the
way to compare two servers.

### Duplicate files

**Find duplicate files** (F9) looks at the files in both panels' folders and
below, groups the ones with the same size, and hashes only those groups
(BLAKE3; on an agent connection the hashes are computed where the files are, so
nothing is downloaded). Each group keeps its oldest copy. Enter on a group
shows the copy that is kept; the first row puts every extra copy in the basket
(Ctrl+B), ready to delete or move. Nothing is deleted by the command itself,
and empty files are ignored.

### Similar pictures

**Find similar pictures** (F9) looks at the pictures in this folder and below
and groups those that are resized or re-saved copies of each other (a
perceptual hash of each picture, so different formats and sizes match). Each
group names the best copy (most pixels, then the largest file); Enter on a
group shows it, and the first row puts every other copy in the basket, ready to
delete.

## Synchronizing folders

**Synchronize folders** (F9) makes the folder of the active panel and the other
panel's agree, on any two locations. Choose how:

- **Mirror** makes the other folder a copy of this one, deleting what only it
  has.
- **Update** copies new files and files that are newer here, and deletes nothing.
- **Two-way** copies the newest version of each file in both directions.
  Files that differ with no newer one are listed as not decided and left alone.

You first see the whole plan, colour coded: green for new files, blue for files
that replace an older one, red for deletions. Nothing has changed yet: **Enter**
runs it (copies are queued in the transfer queue and can be undone only by
hand), **Esc** cancels. Files count as equal when size and time (to the second,
two on FAT) match.

![Synchronize plan](images/sync.png)

## Archives

Archives open like folders: press Enter on one, browse it, view and copy
files out of it. Supported: ZIP, 7-Zip, tar (plain, gz, bz2, xz, zst),
single compressed files (`.gz`, `.xz`, …) and RAR (reading only). Shift+F5
packs the selection into a new archive, choosing the format from the name;
Shift+F9 unpacks.

To protect an archive with a password, use the Pack question's **more
options** (the key row says so: Ctrl+O, "format, level, password"): press **Ctrl+O** in it (the typed name,
such as `site.7z`, picks the format to start on). It asks, one step at a time, for the format, the compression level (fastest, fast, normal,
smallest; formats that have none skip it), a **password** and where to write
the archive. The password question appears for ZIP and 7z only: both use
AES-256, and 7z hides the file names too. It is typed twice, shown as dots and
never stored. Press Enter on an empty password for none. The name you type gets
the extension of the chosen format when it lacks it.

![Advanced pack](images/pack-advanced.png)

Opening an **encrypted** archive asks for its password: for a 7z with hidden
names as you enter it, for a ZIP as soon as it is opened (the names show, the
contents need the password). A wrong password asks again; Esc leaves it
locked until you leave the archive and enter it again. The password stays in
memory only, for as long as the program runs. `nmc pack` and `nmc unpack` read
it from the environment variable `NMC_ARCHIVE_PASSWORD` (never from the command
line, where `ps` would show it).

![Inside an archive](images/archive.png)

## Remote locations

Every command works the same on remote locations, written as URIs:

| Location | Example |
|---|---|
| SFTP (through your OpenSSH setup and keys) | `sftp://user@host/dir` |
| SMB / Windows shares | `smb://host/share/dir` |
| WebDAV (`dav` or `davs` for HTTPS) | `davs://cloud.example.org/remote.php/dav/files/me` |
| Amazon S3 and compatible services | `s3://bucket/dir` |
| A server running the NemesisCommander agent over SSH | `agent://host/dir` |
| A server running `nmc serve` | `scs://host:7443/dir` |
| Inside an archive | `zip:///home/me/site.zip!/css` |

### One list for drives and connections

**D**, **Shift+F1** or **Ctrl+N** open the drive list (in the window, clicking the
drive name above a panel does it for that panel; the name shows where the
panel is: `Local`, a drive letter, a connection's name): the drives of this computer
and your saved connections together. The first column is the type of each row,
and rows are grouped as disks, servers and archives:

- **Disk**: your home folder, the root and the disks and network shares
  mounted on this computer (drive letters on Windows, `/Volumes` on macOS).
- **SFTP**, **Agent**, **Serve** (`nmc serve`), **SMB**, **WebDAV**, **S3**:
  saved connections, one type each.
- **Archive**: a saved location inside an archive file.

Enter opens the highlighted row in the active panel, the same way for every
category: a server then behaves like a local drive, with the same keys,
copy and move between any two panels, the viewer, search and the transfer
queue.

![The drive list](images/drives.png)

The last row, **New connection…**, asks for a location (for example
`sftp://me@nas:22/srv`), then for the name to save it under (the host is
suggested), opens it and keeps it in the list. **Delete** removes a saved
connection; the drives of this computer stay.

The location question starts empty and shows one example per line (every kind,
with user and port). As you type, the examples that fit the first characters
stay: `s` keeps SFTP, SMB, S3 and `nmc serve`, `sm` only SMB. Nothing is called
wrong at the start: only three characters that fit no protocol (and are not a
path such as `/srv` or `~/docs`) show `✗ Invalid address`. Then the line under
the field says what it understood (`✓ SFTP over SSH · host nas · port 22 ·
user me`) or what is wrong (`✗ Port 99999 is not between 1 and 65535`, a
missing host or share, a bad bucket name). Enter on a wrong address keeps the
question open with the reason. If you type a password in the address
(`dav://anna:secret@host`), a `⚠` line warns that it would be saved as plain
text; the password is taken out of the address and you are offered the safe
ways below.

**F4** edits a saved connection (the location, then the name, in place).
**Ctrl+T** tests it: in the list, and also while you create or edit one, at
every step (location, name, the login list, user, password, S3 keys), before
anything is saved; the key is always in the key row at the bottom. A
log window shows each step with its time (address, connection, reading the
folder, number of entries), and a failure says where it stopped with a hint
(wrong credentials, closed port, unknown host). Enter in the log goes back to
the question. Connections are also kept in `[[connections]]` in the settings
file, so they travel with exported and shared settings.

### The login of a connection

After the name, a list asks where the login is kept. It appears for WebDAV,
SMB, SFTP, agent and S3 (not for `nmc serve`, which uses a certificate):

- **Ask every time**: the connection is saved with the user only (S3: no keys);
  the password (S3: access key ID and secret access key, both asked) is asked,
  hidden, the first time you open or test it, and kept in memory until you
  quit.
- **Save it**: WebDAV, SMB, SFTP and agent passwords go to the system keyring
  (GNOME Keyring, KWallet or any Secret Service), which asks you to unlock it
  when needed; this uses the `secret-tool` command (package `libsecret`, or
  `libsecret-tools` on Debian and Ubuntu). Without it, or without a running
  keyring, the list says so and *ask every time* still works. S3 keys go to the
  profile in `~/.aws/credentials`, the file the AWS tools use.
- **Keep it as it is**: nothing is asked. SFTP and agent use your SSH keys,
  ssh-agent and `~/.ssh/config`; S3 uses `~/.aws` and the environment; WebDAV
  and SMB use the login in the keyring or `~/.netrc`, if either has it.

If the address has no user, it is asked. NemesisCommander never writes a
password in its own settings.

SFTP and agent with a password run `ssh` with `SSH_ASKPASS`, which hands the
password to `ssh` through a small helper script and the process environment:
keys are tried first and the password once. The server's host key must already
be known (`ssh user@host` once), as without a password.

![A new connection](images/drives-new.png)

### Naming connections

Every new connection (SFTP, WebDAV, SMB, S3, agent…) asks for a **name** right
after its location; it is what the drive list and the drive label above the
panel show. It starts as the host name: type something you will recognise, such
as `Office NAS`. To change it later, select the connection in the drive list and
press **F2**.

### Private keys

There is nothing to set: SFTP and agent connections use the `ssh` of your
system, which finds keys the way it does in a terminal (`~/.ssh/config`, the
default key files, the SSH agent). For a `.pem` file, add it to
`~/.ssh/config` (`IdentityFile ~/.ssh/aws-web.pem`) or load it with
`ssh-add`. A connection no longer asks for a key file (older versions did);
an `identity_file` left in an old settings file is read and ignored.

## Servers: the agent and `nmc serve`

On a Linux server, the `nmc` agent (a single static program, no
dependencies) makes remote work much faster than SFTP: listings, searches,
checksums and copies between folders of that server run on the server.

- **Over SSH**, with nothing to set up: copy `nmc` to the server and use
  `agent://host/path`. NemesisCommander starts `nmc agent --stdio` through SSH.
- **Permanently**, with `nmc serve`, for long jobs and for copies directly
  between two servers. On your machine, `nmc tls init` creates a private
  certificate authority once; `nmc tls server NAME --out DIR` issues a bundle
  for each server, which runs `nmc serve --tls DIR`. Connections use mutual
  TLS; `--root` limits what is served and `--read-only` refuses changes.
- **Installed for you**: `nmc serve install me@host` does all of that over
  SSH: it creates the authority if needed, copies `nmc` to `~/.local/bin`,
  issues and copies the server certificate, starts a systemd user service
  (it keeps running after you log out) and checks the connection. Options:
  `--name`, `--port`, `--root DIR`, `--read-only`, `--renew`. Run it again to
  update; `nmc serve uninstall me@host` removes it. In the window or the
  terminal use **Install nmc serve on a server…** in the commands list (F9):
  the dialog checks what you type, shows examples and completes the options;
  **F1** explains what will be done, **Ctrl+T** tests the server without
  installing anything (it only reads), and the install ends with its log;
  you can give it a key with Ctrl+K.

The drive list (Shift+F1) shows how each remote place is reached, for example
`[agent over SSH]` or `[nmc serve, TLS]`.

Copies between two `scs://` servers (from the queue as well as `nmc cp`) go
directly from one to the other when they can reach each other (each authorised by a one-time token that expires
after five minutes), otherwise through your machine. `nmc cp` prints the
route taken; `--route` chooses one.

## The transfer queue

Copies and moves run in the background as jobs. **Ctrl+J** shows the queue:
progress, speed and errors of each job, with keys to pause (`p`), resume or
retry (`r`), cancel (`c`), change priority (`+`/`-`), change how many files
are copied at once (`[`/`]`), switch the automatic choice of that number on or
off for the job (`a`) and clear finished jobs (`x`).

### How many files at once

By default the number of files copied at the same time is automatic: a copy
starts with two, adds files while they make it clearly faster, goes back when
they do not help and uses fewer when the speed drops or the server returns
errors. If a server keeps failing, the copy waits (6 s, then longer) and tries
again by itself; other queued jobs for that server wait (`waiting for the server`) while jobs for other servers keep going, and the first one to start after the wait tests the server. Under **Settings → Transfers**: *Parallelism of transfers*
(automatic or fixed) and *Automatic profile*: **balanced** (up to 8 files),
**max speed** (up to 16) or **discreet** (up to 3, leaves room for other work).
A server that refuses sessions gets a limit saved on its saved connection
(`max_connections`, editable there). Each job in the queue says how many files
it uses and why the number last changed. **F1** on the settings screen explains
all of this.

What works on each server (files at once, limit learned from errors) is kept in
a small file, `connection-stats.json`, so a known server starts well. It cleans
itself: connections you delete, rename or point elsewhere, and a new connection
with the name of an old one, drop the old numbers; entries unused for months go.
**Settings → Transfers → Remember what works for each server** turns it off
(the file is deleted); *Forget what is remembered about servers* at the end of
the settings screen empties it.

The command **Generate random text files…** (Tools, F9) makes files of random
words in the current folder, for trying transfers: name, how many (a counter is
added to the name when there are several) and the size of each.

Each job in the queue shows where it comes from and where it goes, with the
system of each (`from Local: ~/docs  →  to nas: /backup`).

While jobs run, a thin line above the status line shows their progress and,
next to it, the numbers: `Copying 2 jobs · 1.2 GiB of 3.0 GiB (40%) · 12.4 MiB/s · 0:42 left · 4 files at once (auto) · 3 queued`
(the speed is measured over the last few seconds, so the time left appears
after the first one; the line goes when the jobs end). In the window, a click on
the line opens the queue.

![Transfer queue](images/jobs.png)

Jobs are written to a journal, so a copy interrupted by a crash or a lost
connection can be resumed. After copying, files are checked (by size by
default, or by a full XXH3 or BLAKE3 hash); settings choose the number of
parallel jobs and files, retries, and a bandwidth limit.

Each job shows `[4 at once, auto]`. With the automatic mode (on by default,
a setting), the number of files copied at once starts from the setting and is
adjusted every few seconds from the measured speed: more files while that
makes the copy faster, fewer when it does not or when errors appear. Choosing
a number by hand with `[` and `]` turns it off for that job.

Connections over SSH are probed every 15 seconds, so a dead link is noticed
in about 45 seconds. When a step fails because the connection was lost, the
job opens it again (a new agent or SFTP session) and retries after a pause,
up to the number of retries in the settings; the part of the file already
copied is kept. S3 requests are retried the same way, and a large S3 upload that was
interrupted continues with the parts already stored when the same file is
copied again in the same session. When a check "by size" is chosen and the
destination is an agent or `nmc serve` server, the content is checked
instead (hashed on the server, no extra traffic); a setting turns that off.

A panel on a remote place shows how it is reached above the list. When a
listing fails because the connection was lost it adds "connection lost", and
"reconnecting…" while the next listing (any key that reloads the folder, such
as Ctrl+R) opens the connection again; both disappear when a listing works.
An SMB connection that the server dropped is signed in again on the next call.

## Filters forgive typos

The filters of the command list (F9) and of the settings screen accept small
spelling mistakes: `settngs` finds Settings, `renmae` finds Rename. If
something fits exactly as typed, only that is listed.

## The launcher and its icon (Linux)

`nmc desktop install` writes the launcher (applications menu, dock) for this
user, with an icon named by its content, so GNOME cannot keep showing an old
one after an update; the window brings that launcher up to date when it
starts. `nmc desktop remove` takes it away. From the AppImage:
`NemesisCommander.AppImage nmc desktop install`.

## Bookmarks, tabs and history

- **Ctrl+Shift+D** bookmarks the current folder, **Ctrl+D** lists the
  bookmarks and, below them, the recent folders (Enter opens one; Delete
  removes a bookmark). While the current folder is not a bookmark, the line
  at the bottom of the list offers **Ctrl+D: bookmark this folder**; once it is
  one, the command goes away. Move with the arrows, or just type: only the
  bookmarks whose name or folder contains the text (case does not matter)
  stay, and the five most recent folders that match are listed below them.
- **Ctrl+Alt+D** bookmarks the current folder together with the folder
  shown in the other panel, its *target*. Opening the bookmark then sets
  both panels at once. Press Ctrl+Alt+D again on the same folder with
  another folder on the other side to add more targets: opening a bookmark
  with several targets shows a quick list to pick the one for the other
  panel (Esc leaves the other panel as it is). The list of bookmarks shows
  `→ target`, or `→ 2 targets`. Folders and targets can be any location:
  local, archives and remote sources (SFTP, SMB, WebDAV, S3, agent).
  In `config.toml` a bookmark looks like this:

  ```toml
  [[bookmarks]]
  name = "site"
  path = "~/projects/site"
  targets = ["sftp://me@web01/var/www", "sftp://me@web02/var/www"]
  ```
- **Ctrl+T** opens a tab, **Ctrl+W** closes it, **Ctrl+Tab** and
  **Ctrl+Shift+Tab** move between tabs. Each panel has its own tabs.
- **Alt+Left** and **Alt+Right** go back and forward in the folders visited.

![Bookmarks](images/bookmarks.png)

### Recent folders

Every folder you open is remembered, local or remote. The bookmarks list
(Ctrl+D) shows the most recent ones under the bookmarks, five at a time, so a
folder you use often is a few keys away without bookmarking it. The old
"frequent folders" command (Alt+J) is gone: its shortcut, if you set one, now
opens the bookmarks list.

## Plugins and scripts

NemesisCommander runs extensions written in TypeScript (or JavaScript) with a
small `nmc` API to list, read, write, copy and search files on any location.
Scripts run in a sandbox: they can only touch the paths they were allowed.

- `nmc run script.ts` runs a script from the command line, with
  `--allow-read` and `--allow-write` for the paths it may use.
- Plugins are folders with a `plugin.json` that names their commands and the
  permissions they need (for example `$SELECTION` or `$OTHER`). Install one
  with `nmc plugin install DIR`; the first run asks you to allow its
  permissions.
- A plugin can also come from **Git**: `nmc plugin install
  https://github.com/owner/repo` (or `github:owner/repo`, or
  `git@host:owner/repo`) clones it with the system's `git` and installs the
  plugin at the top of the repository (`--subdir FOLDER` when it is in a
  folder of it; `#branch-or-tag` at the end of the address picks a
  version). Where it came from and the commit installed are kept in
  `plugin-lock.json` in the data folder, and `nmc plugin list` shows them.
  `nmc plugin update` fetches the latest version of every plugin installed
  from Git (or of the ones you name) and replaces those that changed; a
  plugin that now asks for more permissions asks again before it runs.
  Installing from a folder, or removing the plugin, drops its entry.
- Plugin commands are in the command list (F9) as `Category/Command`: in
  the category their manifest names (`"category": "Files"`), or in
  **Plugins**. Each can have a shortcut (Ctrl+K in the list).
- Your own scripts go in the `scripts` folder of the data folder
  (`~/.local/share/nemesiscommander/scripts` on Linux). Each `.ts` or `.js`
  file there is a command in the **Scripts** category, which can have a
  shortcut too. It runs on the active panel like a script from a panel, with
  no question, since you put it there.
- **Alt+X** in either interface lists the plugin commands, and offers to run
  the script under the cursor, after a confirmation.
- The window has no script engine of its own: it runs scripts and plugins
  with the `nmc` program installed next to it (the AppImage has both), so
  they behave exactly as from the command line.

- `nmc test SCRIPT_OR_PLUGIN` tries it on sample files in a throwaway
  folder and lists what it printed and which files it created, removed and
  changed, so you (or an AI assistant writing it for you) can check it
  before running it on real files. `nmc run --guide` prints a guide for
  AI assistants.
- **Tools/Write a script with AI…** (F9) asks what the script should do;
  Claude writes it with your Anthropic API key, `nmc test` checks it, and
  Claude fixes it until it passes. It then appears under **Scripts**. From
  the command line: `nmc ai new "description"` (key: `nmc ai key` or
  `ANTHROPIC_API_KEY`).

- **Panel columns and viewers from plugins.** A manifest can declare
  `"columns": [{"id": "pages", "title": "Pages", "match": ["*.pdf"],
  "width": 6}]` and `"previewers": [{"id": "csv", "title": "CSV table",
  "match": ["*.csv"]}]`. A column runs the script as the command
  `column:pages` with the matching files of the folder in
  `nmc.context.selection` (at most 300 at a time) and prints one line per
  file, in that order: the text of the column. A previewer runs as
  `preview:csv` with the file in `nmc.context.selection` and prints Markdown,
  which F3 and the quick view show. Both run only for plugins you have
  already allowed (run one of their commands, or answer the question of
  `nmc plugin list`); a plugin may have only columns and previewers. The
  terminal interface shows the columns before the size; the window shows the
  viewers.

See [docs/scripting.md](../scripting.md) for the API and the examples in
`examples/`.

## Assistants: `nmc mcp`

`nmc mcp` makes NemesisCommander a Model Context Protocol server, so an AI
assistant can look at and organise your files. It talks over standard input
and output; the assistant starts it. For Claude Code:

```
claude mcp add nmc -- nmc mcp --read ~/Documents --write ~/Documents/sorted
```

and the same command line in the configuration of Claude Desktop. Nothing is
allowed by default: `--read LOCATION` lets it read, `--write LOCATION` read and
change (repeat them; locations can be `sftp://`, `s3://` or `scs://`), or put
them in a JSON file `{"read": [...], "write": [...]}` and pass `--grants FILE`.
`..` and symbolic links cannot lead out of a granted folder. The tools are
`list`, `stat`, `read` (text, 1 MiB at a time, never `.env`, keys or
passwords), `search`, `hash`, `mkdir`, `copy`, `move`, `rename` and `trash`.
Every change accepts `dry_run`; `copy` and `move` never replace a file unless
asked; deleting only moves to the trash. `--log FILE` keeps one line per
call. The text of your files is data for the assistant, never instructions:
the tools say so.

## The command line

`nmc` with a command works without the interface, for scripts and quick
jobs. Every command accepts the same locations as the panels.

| Command | What it does |
|---|---|
| `nmc ls LOCATION` | List a folder (`--json` for scripts) |
| `nmc cp SRC... DEST` | Copy, with `--conflict`, `--verify`, `--parallel`, `--limit`, `--queue`, `--route` |
| `nmc mv SRC... DEST` | Move or rename |
| `nmc rm PATH...` | Delete |
| `nmc mkdir PATH...` | Create folders |
| `nmc cat FILE` | Print a file |
| `nmc hash FILE...` | Checksums (`--algorithm blake3`, `xxh3` or `sha256`) |
| `nmc find DIR` | Search by `--name`, `--content` (`--regex`), size and age; `--archives` looks inside archives |
| `nmc pick [LOCATION]` | Use the terminal interface as a file chooser: the choice is printed on standard output (`--dir`, `--multiple`, `-0`) |
| `nmc pack` / `nmc unpack` | Create and extract archives |
| `nmc rename FILE...` | Rename many files with a template (`[N]`, `[E]`, `[C]` counter, find and replace); `--preview` shows the new names only |
| `nmc jobs` | Show and control the transfer queue |
| `nmc config path / show / export / import` | Locate, print, back up and restore the settings |
| `nmc config push LOCATION...` / `pull LOCATION` | Send settings to other machines, or fetch theirs |
| `nmc keys` | Print the main shortcuts |
| `nmc shell-init bash\|zsh\|fish\|pwsh` | Print the shell function that leaves the shell in the folder of the active panel when the terminal interface ends |
| `nmc mcp` | Serve file tools to an AI assistant over MCP (see Assistants) |
| `nmc run` / `nmc plugin` | Scripts and plugins (`nmc run --guide`: the guide for AI assistants) |
| `nmc test SCRIPT_OR_PLUGIN` | Try a script or plugin on sample files and report what it changed (`--json`) |
| `nmc ai new` / `nmc ai rename` / `nmc ai key` | Scripts, plugins and multi-rename rules written by Claude; save the API key |
| `nmc agent --stdio`, `nmc serve`, `nmc tls` | The server side (Linux) |

**`nmc pick`** turns the terminal interface into a file chooser for scripts:
the interface is drawn on the terminal (standard error) and what you choose
is printed on standard output, one path per line (`-0` ends each with a NUL
byte for names with newlines). Enter chooses the file under the cursor, **Alt+Y**
chooses the marked files, or the current folder with `--dir`; **Esc** quits
with exit code 1 and prints nothing. Every other command still works while
choosing.

```sh
file=$(nmc pick ~/Documents) && echo "chose $file"
cd "$(nmc pick --dir ~/Projects)"
```

**Leaving the shell in the folder.** Put `eval "$(nmc shell-init bash)"` in
`~/.bashrc` (zsh: `~/.zshrc`; fish: `nmc shell-init fish | source` in
`config.fish`; PowerShell: `nmc shell-init pwsh | Out-String |
Invoke-Expression` in the profile). From then on `nmc` is a function: when the
terminal interface ends, the shell is in the folder of the active panel.
Commands (`nmc ls`, `nmc cp`…) are not affected. **Open terminal here**
(F9, Files) does the same from the interface: it ends and leaves you in the
folder; in the window it opens a terminal window there (the command is set in
Settings, `{dir}` is the folder), and on a server reached with SSH it opens an
SSH session in that folder.

For example:

```sh
nmc cp --verify full ~/Pictures/2026 sftp://nas/backup/pictures/
nmc find ~/Projects --name "*.rs" --content "TODO" --json
nmc rename --template "holiday-[C].[E]" --digits 3 *.jpg --preview
```

## Settings

**Ctrl+,** opens the settings screen. It lists every option and the
commands to share settings. **Type** to find a setting (`font`, `retries`):
only the rows whose section or name contain every word are left, under their
headers; Backspace erases a letter and **Esc** empties the filter before it
closes the screen. Shortcuts are set in the command list (**F9**,
then **Ctrl+K**). Changes are saved
and applied at once; values that differ from the built-in ones are marked
with a dot.

![Settings](images/settings.png)

![Finding a setting by typing](images/settings-find.png)

- **Options**: Enter or ← → change on/off and choice options; numbers and
  text are typed after Enter (← → also step numbers). **Delete** puts the
  default back.
  - General: confirm before deleting, use the recycle bin, the editor.
  - Panels: hidden files, sort order, human-readable sizes, ← and → to move
    between folders, git marks, icons for the kinds of files.
  - Transfers: jobs at the same time (1 runs them one after the other, in
    priority order), files at the same time in a job, the check after
    copying, what to do when a file exists, the bandwidth limit, retries.
  - Viewer: wrapping, tab width, when to open files as hex.
  - Window fonts: the font and its size for all text, and the fixed-width
    font of the viewers and lists (empty or 0: the system's). Only the
    window uses them; the terminal interface uses the terminal's font. Enter
    on a font opens the list of the installed fonts (type to narrow it;
    "(system default)" is the first row); there is no free text to get wrong.

The same settings serve the window and the terminal interface: a shortcut
changed in one works in the other.

### Sharing settings with other machines

The last rows of the screen, and `nmc config`, move settings between
machines. Nothing is synchronized in the background: settings move only
when you send or fetch them.

- **Send these settings to another machine…** (`nmc config push LOCATION`)
  writes your settings there. For a server running the agent, the machine
  is enough (`agent://host`, `scs://host:7443`): the agent says where its
  settings live. Anywhere else, give the settings file or its folder, for
  example `sftp://host/home/me/.config/nemesiscommander`. The previous file
  is kept as `config.toml.bak`; `--merge` keeps the other machine's
  bookmarks, connections and shortcuts.
- **Get the settings of another machine…** (`nmc config pull LOCATION`)
  reads them, then asks whether to replace yours or merge them.
- **Export / Import** write and read a settings file anywhere, and **Edit
  the settings file** opens the TOML file in your editor; it is read again
  when you close the editor.

`nmc config path` shows where the settings file is.

## All shortcuts

F1 shows the main ones; this is the complete list. Every one can be changed
on the settings screen (Ctrl+,).

![Main shortcuts (F1)](images/help.png)

| Files | |
|---|---|
| F3 | View file |
| F4 | Edit file |
| F5 | Copy |
| F6 | Move |
| Shift+F6, F2 | Rename |
| Ctrl+M | Multi-rename (with a preview) |
| (F9 only) | Multi-rename with AI |
| F7 | New folder |
| F8, Delete | Delete (to trash) |
| Shift+F8, Shift+Delete | Delete permanently |
| Shift+F5 | Pack into archive |
| Ctrl+O (in the Pack question) | More options: format, level, password |
| Shift+F9 | Unpack archive (asks the password of an encrypted one) |
| Alt+Enter | Properties, dates and permissions |
| Ctrl+K | Checksum |
| Alt+P | Copy path to clipboard |
| Alt+L | Create symlink or hard link |
| Alt+B | Add to basket |
| (F9 only) | Export listing, Rename in the editor |

| Select | |
|---|---|
| Insert, Space | Select / unselect |
| + | Select by mask |
| - | Unselect by mask |
| * | Invert selection |
| Ctrl+A | Select all |

| Navigate | |
|---|---|
| Enter | Open / enter |
| Backspace | Parent folder |
| Tab | Other panel |
| Ctrl+L | Go to location |
| Alt+Left, Alt+Right | Back, forward |
| D, Shift+F1, Ctrl+N | Drives and connections (on a connection: F2 rename) |
| Ctrl+D | Bookmarks and recent folders (type to filter) |
| Ctrl+Shift+D | Bookmark this folder |
| Ctrl+Alt+D | Bookmark this folder with the other panel's |
| Ctrl+T, Ctrl+W | New tab, close tab |
| Ctrl+Tab, Ctrl+PageDown | Next tab |
| Ctrl+Shift+Tab, Ctrl+PageUp | Previous tab |
| Ctrl+↑, Alt+O | Open this folder in the other panel |
| Ctrl+↓, Ctrl+U | Swap panels |
| Ctrl+B | Basket |
| Ctrl+←, Ctrl+→ | Open the folder under the cursor in the left / right panel |

| View | |
|---|---|
| Ctrl+R | Refresh |
| Ctrl+H | Show hidden files |
| Ctrl+S | Quick filter |
| Ctrl+F3, Ctrl+F4, Ctrl+F5 | Sort by name, extension, date |
| Ctrl+Q | Quick view in the other panel |

| Tools | |
|---|---|
| Shift+F7 | Search |
| Alt+C | Compare files or folders |
| Alt+U | Disk usage of this folder |
| Alt+K | Disks and partitions |
| (F9 only) | Compare folders by content, Synchronize folders, Find duplicate files, Find similar pictures, Saved searches, Save the last search |
| Ctrl+J | Transfer queue |
| Alt+X | Plugins and scripts |
| (F9 only) | Write a script with AI |

| NemesisCommander | |
|---|---|
| F1 | Show main shortcuts |
| F9 | Commands: find and run any command, give it a shortcut |
| Ctrl+, | Settings (options and shortcuts) |
| (F9 only) | Export / import settings to a file |
| F10 | Quit |

Every command, with all its shortcuts, is in the [command list (F9)](#commands-f9).
