# 0019. One drive list for local drives and remote connections

- Status: Accepted
- Date: 2026-11-16

## Context

Remote locations were already opened like folders (ADR 0003: every place is
a `VfsPath`, and every command works on any provider), but they were reached
through a separate "Connect" list (Ctrl+N) that showed only the saved
connections, while there was no list of the computer's own drives at all.
Daniele asked that, in the window and in the terminal, local drives and
configured external sources (SFTP, S3, ...) be chosen from one list and
navigated the same way, with a category to tell them apart. Saving a new
connection meant editing the settings file.

## Decision

- One action, `nav.drives` ("Drives and connections…", Alt+F1 and Ctrl+N),
  replaces `tools.connect`. The old id is still read from `[keys]`, so a
  shortcut the user had given to Connect keeps working.
- `sc_frontend::drives` builds the list: the home folder ("~"), the
  computer's drives (Linux: `/` and the disks and network shares in
  `/proc/self/mounts`, leaving out system mounts such as `/boot`, `/proc`
  and snaps; macOS: `/` and `/Volumes/*`; Windows: the drive letters), then
  `config.connections` in their order.
- The category is derived from the location alone (`Category::of`): an
  archive scheme is Archive, any other scheme except `file` is Remote, a
  plain path is Local. It is shown as the first column, and the list is
  sorted by category with a stable sort, so rows keep their order inside a
  group. No category is stored in the settings.
- Choosing a row navigates the active panel through `resolve_location`, the
  same call used by Go to and bookmarks: there is no separate code path for
  remote places.
- The last row, "New connection…", asks for the URI and then for a name
  (the host or bucket is suggested), saves it in `config.connections` and
  opens it. Delete removes a saved connection; the computer's drives cannot
  be removed. Bookmarks keep their own list (Ctrl+D).
- The list is the existing choose dialog, so both interfaces draw it without
  new screen code; the window now draws choose lists in the fixed-width
  font so their columns line up.

## Consequences

- One shortcut and one list for every place; a remote source behaves like a
  local drive from the moment it is chosen.
- Mounts are read when the list opens, so a USB stick plugged in later
  appears the next time, with no watcher.
- Credentials still never go into the settings (ADR 0009 to 0012): a new
  connection stores only its name and URI.
- Options of a connection (`options`) are still edited in the settings
  file. The private key file (`identity_file`) has its own question since
  ADR 0025.
