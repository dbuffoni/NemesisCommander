# 0044. Adaptive transfer parallelism, job indicators, random file generator

- Status: Accepted
- Date: 2026-11-22

## Context

ADR 0030 added a hill climber that raised or lowered the number of files a
job copies at once. It worked per job, compared two noisy 3 s samples, knew
nothing about servers (two jobs to one server fought each other, a server
that refuses sessions was only handled by retries) and could never pause by
itself. The goal now: as automatic as possible for the user, with few settings.

## Decision

**What the user sees**

- Settings > Transfers: *Parallelism of transfers* = automatic (default) or
  fixed; *Files at the same time when Parallelism is fixed* (the old
  `parallel_files`); *Automatic profile* = balanced (default), max speed,
  discreet (`transfers.profile`); the bandwidth limit. `parallel_jobs` stays in
  the file (default 2) but is no longer on the screen.
- A saved connection may carry `max_connections` in its options. It is
  suggested automatically: when errors keep coming with several files at once
  the controller saves the lowered number there and says so in the status line.
- Job queue (Ctrl+J): each job shows the files at once, `auto` or `fixed`, and
  why the automatic mode last changed it (`raised to 4: +18% faster`,
  `lowered to 3: 2 errors`, `kept at 4: more files did not help`, `server not
  answering, waiting 6 s`). `[` `]` force a level, `a` returns to automatic,
  `p` pauses; a manual pause is never undone by the controller.
- F1 on the settings screen explains all this (`settings::help_lines`).

**The controller** (`nmc-transfer/src/tuning.rs`, every 3 s per job)

- Throughput is a moving average (about 6 s), not two samples.
- Slow start: from 2 files (1 for discreet, or the level that worked last time
  on the same servers) it doubles while each doubling is faster than the
  profile's threshold (8 % balanced, 4 % max speed, 15 % discreet).
- Probing: at a steady level it tries one file more every few intervals and
  *goes back* when the speed did not rise, so an extra file never stays for
  free. A drop of 30 % that is not ours lowers the level by one.
- Multiplicative decrease on errors (retries): the level is multiplied by
  0.7 (0.75 / 0.5 by profile). At one file, the job *waits for the server*
  (level 0: no new file starts, running files finish): 6 s, 12 s, ... 96 s,
  then tries one file again; any success resets the wait. The executor treats
  level 0 as "wait", never as "done".
- Ceiling per profile: balanced 8, max speed 16, discreet 3 (manual up to 16).
- Per-server limits (`endpoint_key` = `scheme://authority`; archives count as
  the server holding them): configured `max_connections`, or learned from
  repeated errors at two or more files. The limit is shared by the running
  jobs that use the server. The last good level per server is remembered while
  the program runs.

**Server memory file** (`connection-stats.json` in the data folder, small
JSON; `nmc-transfer/src/stats.rs`): per server (`scheme://user@host`) the level
a copy ended with, the learned limit, the number of copies and when it was last
used; a server saved as a connection also records that connection's name and
address. New copies start from it, and the learned limit applies at once.
Settings > Transfers > *Remember what works for each server* turns it on (the
default) or off (off forgets everything and deletes the file); *Forget what is
remembered about servers* in the commands at the end of the settings screen
empties it. It tidies itself:
- every time the connections are saved, entries whose connection was deleted,
  renamed or moved to another address are removed;
- creating a connection first drops what an older connection of the same name
  left, so a new connection never inherits old numbers;
- entries unused for 180 days (saved connections) or 30 days (others) go, at
  most 200 are kept, and this is checked at start-up and on every write;
- the file is written through a temporary file, a broken file counts as empty,
  and an empty store has no file.

**Queued jobs wait for a down server.** When a job's controller starts waiting
for its server, the server is marked as not answering for that time. Queued
jobs that use it stay queued (shown as `[waiting for the server]`, and counted
in the status line) while jobs for other servers go on; when the time is up the
queue wakes, starts one, and its first file is the probe: if it works the mark
is cleared, if it fails the wait grows (6 s ... 96 s). The good level per
server is kept across runs by the memory file.

**Not done**: parallelism inside one big file (range requests / multipart).

**Job indicators** (status bar): `Copying 2 jobs · 1.2 GiB of 3.0 GiB (40%) ·
12.4 MiB/s · 0:42 left · 4 files at once (auto) · 1 queued · 1 paused · 1
failed`; `waiting for the server` replaces the speed when every running job
waits; paused jobs alone still show `1 paused`. The sizes of a running job are
read as soon as it is planned (they showed `…` before).

**Random text files**: command *Generate random text files…* (Tools, no key):
asks the name, how many and the size (`10k`, `2.5M`, `1G`). With more than one
file a counter before the extension is added, as wide as the count needs
(`sample-07.txt`). Files are made in the active panel's folder, even on a
remote one, and never replace existing files.

## Consequences

- The tuner is unit-tested against a simulated link (`tuning::tests`) and the
  executor's wait against a real run (`level_zero_waits_and_then_continues`).
- The window (QML) shows the same strings (they come from `nmc-frontend`); it
  could not be built in the cloud, so the indicators were checked as terminal
  screens (`NMC_SCREENS=dir cargo test -p nmc-tui --test app`).
