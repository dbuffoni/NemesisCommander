# 0035. Release 0.1.0, sixth batch: paste, connection editing and testing, advanced pack

- Status: Accepted
- Date: 2026-11-20

## Context

Three cards of release 0.1.0 (SC-101, SC-102, SC-103) were implemented in one
pull request. The choices that were not obvious are below.

## Decision

- **Paste (SC-103).** Nothing took text from the clipboard: Ctrl+V reached the
  key map, found no command and did nothing. `App::paste(text)` types the first
  line of the text into the open one-line question, the filter of a list, or
  the command search (not while a shortcut is being recorded); with no such
  window it does nothing, so pasted text cannot run commands. The terminal
  interface turns on bracketed paste and hands `Event::Paste` over; the window
  reads the clipboard in QML (the hidden `TextEdit` that already does the copy
  calls `paste()`) on Ctrl+V and Shift+Insert, and calls `Commander.paste`. The
  self test has a `Ctrl+V` pseudo key that goes through the same QML code, so a
  window test copies a path and pastes it.
- **Connection address (SC-102).** `nmc-frontend::connections` holds a table of
  the kinds of server with a sample each (`sftp://user@host:22/path`…) and
  `check`, which parses the address with regular expressions (scheme, user,
  host or bucket, port 1-65535, SMB needs a share) and says what is wrong. The
  question shows the verdict live under the field (the existing `preview`
  lines); Enter on a wrong address keeps the question open. **A password is
  refused in the address**: the program never stores passwords (ADR 0009), and
  an address is saved in the settings in clear. The prompt says where each kind
  gets its password (SSH keys, `~/.netrc`, `~/.aws`).
- **Edit and test connection (SC-102).** F4 in the drive list asks the address
  and then the name, and changes the saved connection in place. Ctrl+T, in the
  list or in the address question, runs `connections::run_test` in the
  background: it forgets the cached provider (a cached one would hide a broken
  connection), opens the connection and reads the folder, each step with a
  30 second limit, and shows a log window (times, counts, error lines and a
  hint). From the question, Enter in the log goes back to it. The old F4 (key
  file) was already gone (ADR 0033).
- **Advanced pack (SC-101).** The options are part of Pack: **Ctrl+O** in
  the Pack question (Shift+F5) opens them (a first version had a separate
  command, removed at the maintainer's request, PR after #20). They ask the format, the level (skipped for tar and 7z, which
  have none to choose), the password (zip and 7z only, typed twice, shown as
  dots) and the destination, whose extension follows the chosen format.
  Encryption: zip uses AES-256 (the `aes-crypto` feature of the `zip` crate);
  7z uses AES-256 with the file names hidden (the `aes256` feature of
  `sevenz-rust2`). Folders and links inside a zip are not encrypted. Old
  ZipCrypto is read but never written. The other formats refuse a password.
  *Not done:* deleting the originals after packing (a mistake there loses data;
  the card does not ask for it), splitting into volumes.
- **Passwords of archives.** `nmc-archive::password` keeps them in memory
  only, by container location. An archive that is encrypted marks itself
  (zip: any encrypted member when it is opened; 7z: the library reports it),
  and a wrong password marks it as refused. After every event the application
  looks at the archives the panels are in: if one `needs_password`, a question
  with a hidden field opens (it replaces a viewer that could not read), unless
  the user already declined it (Esc; forgotten when the archive is left). The
  answer is stored and the panels reload. No new error type: the readers return
  an `io::Error` whose message says a password is needed, and the store tells
  the application what to ask. `nmc pack` and `nmc unpack` read the password
  from `NMC_ARCHIVE_PASSWORD`, never an argument.

## Consequences

- The window and the text interface share one paste rule; a context menu or a
  drop of text can use `App::paste` later.
- Two new dependencies features (AES in `zip` and `sevenz-rust2`) add a few
  small crates (`aes`, `hmac`, `pbkdf2`, `sha1`, `cbc`); the binary size was not
  re-measured against the budget of ADR 0020.
- RAR passwords are not handled (reading RAR is optional and read only).
- A 7z archive with an encrypted header only shows a password question; a
  wrong password on a ZipCrypto member can only be seen while reading, so it
  is reported when a file is read, not when the archive is opened.
