# 0042. Card batch K: tolerant filters, e-book/audio/video viewers, launcher icon, install dialog

- Status: Accepted
- Date: 2026-11-21

## Context

Eight 0.1.0 cards (SC-113 to SC-120).

## Decision

- **SC-113.** `fuzzy.rs`: every typed word must be found with a few spelling
  mistakes (1 from 3 letters, 2 from 7; swapped neighbours count once), by
  approximate substring matching. When something fits exactly, only exact
  matches are listed; mistakes are forgiven only when nothing else fits. Used
  by the command list and the settings screen; file and bookmark filters stay
  exact.
- **SC-114.** "Count folder sizes" (Alt+S) and its size bars are removed;
  Disk usage (Alt+U) replaces them. Settings files that still bind
  `view.folder-sizes` are silently accepted (`RETIRED` in the key map).
- **SC-115.** `nmc desktop install|remove` writes the launcher with an
  absolute icon path named by a hash of the picture, so no desktop cache can
  show an old icon; the window refreshes an existing launcher of its own at
  start. Not testable without GNOME: to be confirmed on the real desktop.
- **SC-116.** `nmc-preview` gets `ebook` (EPUB, FB2 incl. Windows-1251) and
  `media` (audio tags/properties with `lofty`; MP4/MOV, Matroska/WebM and AVI
  headers parsed by hand). Audio and video are described, not played; only
  the first 32 MiB are read.
- **SC-117.** The M/?/! marks are git's, not Compare's. They are dropped when
  the folder is listed again, and the line of the entry under the cursor says
  what the mark means.
- **SC-118.** Queue rows show `from SYSTEM: folder  →  to SYSTEM: folder`
  (second line in the terminal).
- **SC-119.** The bookmark list's "add this folder" is no longer a row but
  `Ctrl+D: bookmark this folder` in the key line, only while the folder is not
  bookmarked; the list opens even when empty.
- **SC-120.** "Install nmc serve" checks the address while typed, shows
  samples and completes options, F1 explains, Ctrl+T tests without
  installing (`provision::check_serve`, read-only on the server), and the
  install shows its timed log.

## Consequences

The Qt changes (bars removed from `PanelView.qml`, launcher refresh in
`nmc-gui`) were not built in the cloud session; the desktop CI job checks them.
The install test cannot be run against a real server here.
