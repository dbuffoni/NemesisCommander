# 0051. Saved logins use each system's own secret store

- Status: Accepted
- Date: 2026-11-25
- Extends: [0040](0040-keyring-logins.md)

## Context

ADR 0040 saved logins in the Secret Service through `secret-tool`. On
Windows and macOS the app could only ask every time or read `~/.netrc`.

## Decision

`nmc_vfs::keyring` keeps its API (`available`, `store`, `lookup`, `clear`) and
gets one backend per system, chosen at compile time:

- **Linux and others**: `secret-tool`, as before.
- **macOS**: the Keychain through `/usr/bin/security`. A login is a generic
  password, service `nemesiscommander`, account `user@host`, comment `host`.
  The password must not be in the arguments of a process (`ps` shows them) and
  `add-generic-password -w` without a value asks on the terminal, so the
  command is sent to `security -i` on its standard input, with every argument
  double-quoted. A password with a line break is refused.
- **Windows**: the Credential Manager through `CredWriteW`, `CredReadW`,
  `CredDeleteW` and `CredEnumerateW`, as a generic credential named
  `nemesiscommander:HOST:USER` holding the user and the password in UTF-16,
  saved for this user on this machine. `cmdkey` cannot read a password back and
  takes it as an argument, so it is not used. The only new dependency is
  `windows-sys` limited to the credential API (already in the lock file for
  other crates), compiled on Windows only.

The login question names the store (`keyring::store_name()`) and what is
missing (`keyring::missing()`). A store that refuses (a locked keychain) shows
the error and offers "ask every time"; nothing is stored.

## Tests

Unit tests run a fake command on every Unix system (`NMC_SECRET_TOOL` /
`set_command`), so the macOS backend is tested on Linux too. The real stores
are tested only in the full CI tier, on the Windows runner and on the macOS
runner (which first makes a throwaway default keychain), with a throwaway host
name that the test removes.
