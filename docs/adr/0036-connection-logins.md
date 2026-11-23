# 0036. Logins of connections: warn, ask every time or keep in ~/.netrc

- Status: Accepted; the `~/.netrc` saving is superseded by [0040](0040-keyring-logins.md)
- Date: 2026-11-20

## Context

The new-connection question of of an earlier release listed its examples on one long line,
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
- **Where the login lives** (WebDAV, SMB, SFTP, agent and S3; not scs, which
  uses a certificate): after the name a list offers *ask every time*, *save* and
  *keep as it is*. Asking every time saves the option `password = "ask"` in the
  connection; opening or testing it prompts (hidden) unless a login is already in
  memory. Saving a password writes `machine H login U password P` to the netrc
  file, mode 600, replacing the entry of the same host and user; S3 keys go to
  the profile in `~/.aws/credentials`, other keys and profiles kept. WebDAV and
  SMB already read `nmc-vfs::netrc`, which now looks first in a per-process login
  map (`set_session`); S3 has the same map for keys (`settings::set_session`). A
  user name is asked when the address has none.
- **SSH passwords.** `nmc-ssh` looks up a login for the target's host and user
  (session map, then netrc). If there is one, `ssh` runs without batch mode,
  with `NumberOfPasswordPrompts=1`, `SSH_ASKPASS_REQUIRE=force` and
  `SSH_ASKPASS` pointing to a small script in the temp folder that prints
  `$NMC_SSH_PASSWORD`; the password is only in the environment of that `ssh`
  process. Keys and the agent are still tried first. Without a login nothing
  changes (batch mode). An unknown host key still fails, since the helper would
  answer the yes/no question with the password.
- **Test while creating.** Ctrl+T works in the address and in the name
  questions (new and edit), on the address as typed; a password found in it
  is used for the test through the session map. Enter in the log returns to the
  question.
- **Test hook.** `netrc::set_file` and `settings::set_credentials_file` override
  the files for one process, because the workspace forbids `unsafe` and so
  `std::env::set_var` in tests.

## Consequences

The settings file still never holds a password. `~/.netrc` is plain text, but
it is the file curl and git share and it is created readable only by the user.
A password typed for one session stays in memory until the program ends.
