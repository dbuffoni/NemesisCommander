# 0005. Transfer journal in SQLite, planning at start time

- Status: Accepted
- Date: 2026-11-15

## Context

The architecture asks for a persistent queue in SQLite (WAL) with pause,
priority, cancellation, retry and bandwidth limits.

## Decision

- The journal has two tables: `jobs` (the request as JSON, state, priority)
  and `steps` (one row per plan step with its outcome). Every step outcome
  is its own small transaction (`synchronous = NORMAL`).
- A job is planned when it **starts**, not when it is submitted, so
  submitting is instant even for huge trees.
- On startup, jobs left in `running` are put back in `queued`; finished
  steps are skipped.
- Pause and cancel are written to the journal *before* the running job is
  signalled; the runner only writes a final state if the job is still
  `running`. A pause can never be overwritten by a finishing job.
- Background jobs cannot show dialogs: a job with the `Ask` conflict policy
  behaves as `Skip` unless the front end installed a resolver.
- Bandwidth limiting uses a token bucket (one second of burst). A global
  bucket is shared by all jobs, and a job can add its own limit.
- Resuming a `failed` job clears the failed step outcomes so only those
  steps run again.

## Consequences

- A crash loses at most the progress inside the files being copied, and
  that is recovered from part files.
- The schema version is kept in SQLite's `user_version` for future
  migrations.
