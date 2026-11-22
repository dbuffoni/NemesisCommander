# 0032. Remote features tested against real servers, panel connection state

- Status: Accepted
- Date: 2026-11-18

## Context

ADR 0030 and 0031 added `nmc serve install`, SMB reconnection and panel
connection indicators, but only tried them against fakes. Two gaps were open:
a panel showed no sign of a lost connection, and nobody had checked that the
SMB provider recovers after the server drops the connection.

## Decision

- **Panel state.** `App.connection_lost[side]` is set when a listing fails with
  a lost connection and cleared by the next listing that works. The drive
  text of a remote panel then ends with "connection lost", or "reconnecting…"
  while the next listing runs. Same text in the TUI and the GUI.
- **SMB.** A test puts a TCP relay between the provider and a real Samba and
  cuts every connection. It showed that the library's own reconnect leaves
  the cached share handles stale ("handle belongs to a session that has
  ended"). The provider now remembers the lost connection and, on the next
  call, signs in again and drops the cached shares.
- **`nmc serve install`.** `scripts/ssh-test-server.sh` starts a private
  OpenSSH server as the current user; `crates/nmc-cli/tests/install.rs` runs
  install, a second install (certificates kept), a connection over mutual
  TLS, and uninstall. Where there is no systemd user manager (containers, CI
  runners) the script puts a stand-in `systemctl` in the PATH of the
  sessions, which starts the unit's `ExecStart` in the background. CI runs
  both servers.

## Consequences

- The real `systemctl --user` path (units, linger, restart on failure) is
  still not exercised; only the commands the installer sends are.
- The SMB provider still uses one mutex for all calls and has no server-side
  hash (`SERVER_HASH`); not changed here.
