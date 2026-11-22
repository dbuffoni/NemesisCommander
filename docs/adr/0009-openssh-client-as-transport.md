# 0009. SSH connections go through the system's OpenSSH client

- Status: Accepted
- Date: 2026-11-15

## Context

Two providers need SSH: the agent (`agent://`, ADR 0008) and SFTP
(`sftp://`). Users already have an SSH setup that works: `~/.ssh/config`
host aliases, keys in an agent, hardware tokens, jump hosts, known hosts,
Kerberos. Reimplementing SSH in-process (for example with `russh`) would
mean re-reading and partly re-implementing all of that, and every gap would
show up as "works in my terminal but not in SuperCommander".

## Decision

- The `sc-ssh` crate builds `ssh` command lines and starts them with piped
  standard streams. Paths name the server as `[user@]host[:port]`.
  Connections run with `-T -o BatchMode=yes`: no terminal, and no password
  prompt that nobody could answer. Authentication must work without typing
  (keys, an agent, or a control master).
- The agent runs as `ssh host sh -c '<start script>'`; SFTP runs as the
  standard subsystem, `ssh -s host sftp`. The SFTP protocol is spoken by
  the pure-Rust `russh-sftp` client over those streams.
- An environment variable per provider replaces the whole command
  (`SC_AGENT_COMMAND`, `SC_SFTP_COMMAND`, with `{host}` substituted). Tests
  use it to run `sc agent --stdio` or OpenSSH's `sftp-server` locally, so
  everything except the SSH login is covered in CI.

## Consequences

- The behaviour matches the user's `ssh`; no keys or passwords are stored
  by SuperCommander.
- An `ssh` client must be installed (it is on Linux and macOS, and on
  current Windows). Interactive password logins are not supported yet; a
  later front-end feature can add an askpass helper.
- Plain SFTP rename does not replace an existing file, so the SFTP
  provider does not claim atomic rename; the copy engine then writes the
  target directly instead of through a part file, and removes an existing
  file before a rename replaces it.
- OpenSSH's server reads the two paths of the SYMLINK request in the
  opposite order from the draft standard. Like other clients we follow
  OpenSSH, which nearly every server does.
