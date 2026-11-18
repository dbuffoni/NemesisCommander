# 0031. Server-to-server copies in the queue, S3 upload resume, content checks on servers

- Status: Accepted
- Date: 2026-11-18

## Context

ADR 0030 left four items open: copies between two `nmc serve` servers only
ran direct from the command line, S3 uploads restarted from zero after a
network error, the default check ("same size") cannot see corruption, and
SMB errors after a lost connection were not recognised as such.

## Decision

- **Server-to-server in the queue.** `nmc-transfer` has a `ServerCopier`
  trait (no dependency on the agent); `nmc-app` implements it with
  `nmc_agent::route` and gives it to the manager. A copy job whose sources
  are on one `scs://` server and whose destination is on another asks the
  copier first: direct, then reverse. The data then never passes through
  this machine, progress comes from the servers' `TransferProgress`, and the
  route is shown in the status line (`JobEvent::Route`). Without
  certificates or when the servers cannot reach each other the job copies
  through this machine as before. A conflict policy of "ask" becomes "skip"
  there, as for any background job.
- **S3 resume.** An S3 provider remembers (at most 4) multipart uploads that
  were interrupted, with the SHA-256 and ETag of each stored part, recorded
  as soon as a part is stored (so it also holds when the copy is dropped,
  not only when it fails). The next upload of the same key reuses that
  upload id and skips the parts whose content is identical. A permanent
  error or a completed upload clears the entry. The memory is per process:
  after a restart, the old upload is simply replaced.
- **Content check on servers.** `transfers.verify_remote_fast` (on by
  default): when the check is "same size" and the destination is an agent or
  `nmc serve` server, XXH3 is used instead, hashed on the server, so no
  extra traffic. `nmc_app::effective_verify` decides, for the queue and for
  `nmc cp`.
- **SMB.** `ConnectionLost`, `TimedOut` and `SessionExpired` errors are
  worded "smb connection lost", which the executor treats like any lost
  connection (provider forgotten and opened again, shares included).
- **Connection kind and loss in the panels.** The panel header shows how the
  place is reached (`name · agent over SSH`); a listing that fails with a
  lost connection forgets the provider, so the next try opens a new one.

## Consequences

- The S3 resume only helps within one session; persisting upload ids in the
  journal would be a later step.
- A per-panel "reconnecting" state is not drawn; the status line carries the
  message.
