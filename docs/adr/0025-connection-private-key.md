# 0025. Saved connections can log in with their own private key file

- Status: Accepted
- Date: 2026-11-16

## Context

The maintainer asked whether SFTP supports `.pem` keys. Through OpenSSH it did,
but only with `IdentityFile` in `~/.ssh/config` or `ssh-add`: the
`identity_file` field of a saved connection existed in the settings and
nothing read it, and no screen could set it.

## Decision

- `nmc-ssh` keeps a process-wide list of logins (user, host, port; no port
  means 22) and their key files, set with `set_identity_files`. `ssh_args`
  adds `-i FILE -o IdentitiesOnly=yes` for a listed login, so both SFTP and
  the agent (and the agent's install step) use the key. It now returns a
  `Result`: a key file that is missing, is not a file or, on Unix, can be
  read by others is reported before `ssh` starts, with the `chmod 600` fix,
  instead of ssh's own message after it silently skips the key.
- `nmc-app` fills that list from the SFTP and agent connections of the
  settings, when they are loaded and every time they are saved. Keyed by
  login rather than by connection, a path opened by hand on the same login
  (Go to, the command line) uses the same key.
- The front ends ask for the key after the name of a new SFTP or agent
  connection, and F4 in the drive list changes it for a saved one. The
  question is an ordinary input (`~/` allowed, empty = SSH agent); it is
  marked `browse`, and the window adds a Browse… button with Qt's file
  picker (QtQuick.Dialogs). The terminal types the path.
- A key that fails the check is still saved, with a warning, so a typo
  does not lose the new connection.

## Consequences

- No secrets in the settings: only the key's path. Keys with a passphrase
  still need the SSH agent, since a password prompt cannot run in batch
  mode.
- Two saved connections to the same login with different keys: the first
  one wins.
- The Linux CI and the AppImage need the QtQuick.Dialogs QML module.
