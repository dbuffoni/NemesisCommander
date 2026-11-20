# 0036. Logins of connections: warn, ask every time or keep in ~/.netrc

- Status: Accepted
- Date: 2026-11-20

## Context

The new-connection question of ADR 0035 listed its examples on one long line,
carried a note about passwords among the examples and refused a password in the
address. Feedback: one example per line, the note only as a warning when a
password is really typed, the connection test while creating (not only
editing), and a safe way to keep the login or to type it every time.

## Decision

- **Layout.** The examples are one per line under the question, shown as soon as
  it opens; the note about passwords is gone from them.
- **Warning, not refusal.** `check` takes `user:password@host` apart: the
  returned address has no password, a `⚠` line warns that it would be plain
  text, and the password is carried (as `Secret`, which never prints in debug
  output) to the login question. It is never put in the settings (ADR 0009).
- **Where the login lives** (WebDAV and SMB, the kinds that use passwords; SFTP,
  the agent and `nmc serve` use SSH keys, S3 its own keys): after the name a list
  offers *ask every time*, *save in ~/.netrc* and *keep as it is*. Asking every
  time saves the option `password = "ask"` in the connection; opening or testing
  it prompts (hidden) unless a login is already in memory. Saving writes
  `machine H login U password P` to the netrc file, mode 600, replacing the entry
  of the same host and user. Both providers already read `nmc-vfs::netrc`, which
  now looks first in a per-process login map (`set_session`), so a typed password
  works without touching the disk. A user name is asked when the address has none.
- **Test while creating.** Ctrl+T works in the address and in the name
  questions (new and edit), on the address as typed; a password found in it
  is used for the test through the session map. Enter in the log returns to the
  question.
- **Test hook.** `netrc::set_file` overrides the file for one process, because
  the workspace forbids `unsafe` and so `std::env::set_var` in tests.

## Consequences

The settings file still never holds a password. `~/.netrc` is plain text, but
it is the file curl and git share and it is created readable only by the user.
A password typed for one session stays in memory until the program ends.
