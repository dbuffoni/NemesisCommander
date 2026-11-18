# 0030. Remote connections are probed and reopened; jobs tune their parallelism; `nmc serve install`

- Status: Accepted
- Date: 2026-11-18

## Context

A review of how remote connections and transfers behave (agent over SSH,
`nmc serve`, SFTP, S3, SMB) found: no keepalive anywhere, so a silently dead
link could hang for minutes (SFTP waited up to 120 s per request, S3 had no
read timeout and no retry); a lost connection was never reopened (the dead
provider stayed in the registry, so the job retry failed again at once);
the number of files copied at once was fixed when a job started; and
`nmc serve` had to be set up by hand on the server.

## Decision

- **Keepalive.** Every `ssh` child (agent and SFTP) gets `ConnectTimeout=30`
  and `ServerAliveInterval=15` / `ServerAliveCountMax=3`; the agent's gRPC
  channel sends HTTP/2 PING frames every 15 s and gives up after 20 s
  (`nmc-ssh`, `nmc-proto`). A dead link now fails in about 45 s.
- **Retry with reopening.** `nmc-ops` recognises lost connections by their
  message (`is_connection_error`). Before retrying such a step the executor
  forgets the providers of the step, so the registry opens a new agent or
  SFTP session, and waits at least 2 s (doubling, at most 30 s). It reports
  `OpEvent::Retrying`, shown in the status line (`JobEvent::Retrying`). The
  existing `RetryPolicy` (setting `transfers.retries`) still bounds attempts.
- **S3.** The HTTP client has a 60 s read timeout, and every request is
  tried again (400 ms, doubling) after network errors, timeouts, 5xx and
  429. Bodies are `Bytes`, so a request can be resent. Resumable multipart
  uploads are not part of this change.
- **Parallelism of a job.** `ExecuteOptions::live_parallelism` is an atomic
  read before each file starts, so the level can change while a job runs.
  `TransferManager::set_parallelism` / `set_auto_parallelism` change it (and
  persist it in the journal); `JobEvent::ParallelismChanged` tells the
  interfaces. In the job monitor `]` and `[` change the level of the selected
  job (turning the automatic mode off for it) and `a` switches the automatic
  mode. The setting `transfers.auto_parallelism` (on by default, in the
  settings screen) is the starting mode of new jobs; `transfers.parallel_files`
  is the starting level. `nmc cp --parallel N` keeps N.
- **Automatic mode.** `nmc-transfer::Tuner` is a hill climber that runs every
  3 s on jobs with at least two files: more files at once while the
  throughput rises by more than 10 %, one fewer when it falls or when steps
  are retried, between 1 and 16, with a pause before probing upward again.
- **`nmc serve install HOST`** (`nmc-agent::provision`, also the palette
  command "Install nmc serve on a server…") copies `nmc` to
  `~/.local/bin`, issues and copies the server certificate to
  `~/.config/nmc/serve-tls` (key 0600), writes and starts a systemd *user*
  unit with lingering, and checks the connection. It is idempotent and
  `nmc serve uninstall HOST` removes the unit and the certificates.
- The drive list shows how a remote location is reached (`agent over SSH`,
  `nmc serve, TLS`, `SFTP over SSH`, ...), and new-connection prompts list
  `scs://`.

## Consequences

- A broken connection is noticed in under a minute and a job can continue on
  a new one; panels still show an error until the folder is opened again.
- Not done, tracked on the board: server-to-server copies from the GUI,
  resumable S3 uploads, SMB share handles after a reconnect, a stronger
  default verification, a connection state indicator in the panel title.
