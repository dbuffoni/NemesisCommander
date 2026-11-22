# 0034. Release 0.1.0, fifth batch: chooser, flat view, archive search, highlighting, branding

- Status: Accepted
- Date: 2026-11-19

## Context

Six cards of release 0.1.0 (SC-3, SC-22, SC-32, SC-69, SC-91, SC-100) were
analysed with the maintainer and implemented in one pull request. The choices that
were not obvious are below.

## Decision

- **`nmc pick` (SC-3).** The terminal interface is the file chooser: it is
  drawn on standard error and the answer goes to standard output, so
  `$(nmc pick)` works. Enter chooses the file under the cursor, **Alt+Y**
  (`app.choose`) the marked files or, with `--dir`, the current folder; Esc
  quits with exit code 1. `-0` ends names with NUL. All other commands stay
  available, as the chooser is the normal interface in a mode (`PickMode`),
  not a second one. It needs a terminal on standard error.
- **Flat view (SC-22).** **Alt+V** (`nav.flat-view`) lists the files below the
  folder in the same panel and the same row model, with names relative to the
  folder (`sub/deep/x.txt`) and a *Folder* column. Selection, sorting and
  filters keep working because names are the keys. The walk is iterative,
  skips unreadable subfolders, and stops at 100 000 files (the footer says
  so). Backspace or Alt+V leaves it.
- **Archive search (SC-32).** The search filter gained the word `archives`
  (and `nmc find --archives`). `nmc-index` defines a small `ArchiveOpener`
  trait and does not depend on `nmc-archive`; `nmc-app` implements it with
  the provider registry. Archives inside archives are searched to depth 3,
  four archives at a time, so a bomb of nested archives cannot run away.
- **Command list height (SC-69).** The list has a fixed height (24 rows in the
  terminal, 16 in the window, less on a small screen) and always reserves the
  help line, so it no longer jumps while typing.
- **Syntax colours (SC-91).** A hand-written lexer in `nmc-preview::syntax`
  (about 600 lines, no dependency) colours bash, YAML, JSON, XML, HTML, SQL,
  TypeScript/JavaScript, Rust and Python. It knows comments, strings, numbers,
  keywords and a few kinds of names, with the state carried between lines
  (block comments, triple-quoted strings, templates, tags). It is not a
  parser: nested comments and regex literals are not understood. A syntax
  highlighting library (syntect, tree-sitter) was left out because of the
  footprint budget of ADR 0020. The tones are style words (`keyword`,
  `string`, `number`, `comment`, `name`) on the existing styled-span model, so
  the terminal and the window colour them with the same data. Files over
  50 000 lines are not coloured. In a comparison only unchanged lines get
  colours: red and green keep showing the change.
- **Branding (SC-100).** The logo was redrawn as an SVG (white arch, purple
  gradient band) by `scripts/make-icons.py`, which also writes the PNG sizes
  into `assets/branding/`, together with a reduced copy of the original
  artwork. The AppImage installs the SVG and every PNG size under `hicolor`.
  The About command (`app.about`, no default key) shows the logo, version,
  commit (taken at build time by `git describe`, or from `NMC_COMMIT`), the
  MIT license, the repository and the third-party notice (Qt, unRAR). The logo
  is in the binary (`include_bytes!`, 256 px, about 16 KB); the terminal draws
  it with half blocks, the window from a file in the temporary folder.
  The window icon is set from C++ (`gui/src/icon.cpp`, one call, because
  cxx-qt-lib has no `QIcon`) together with the desktop file name. The About
  window shows the N without a tile, in a dark-arch and a white-arch variant
  chosen by the theme.

## Consequences

- The binary grows by the logo (16 KB) and the lexer; no new dependency.
- Highlighting is approximate by design; wrong colour on a rare construct is a
  cosmetic bug, never a change of the text (a test checks the text is intact).
- `nmc pick` and the flat view are new default keys (Alt+Y, Alt+V); they can
  be changed like any other.
