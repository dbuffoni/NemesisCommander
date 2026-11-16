# 0020. Keeping the footprint small

- Status: Accepted
- Date: 2026-11-16

## Context

Daniele asked whether viewers and archive formats are loaded only when
needed, and to keep the footprint under control. Measured on `dev`
(x86_64 release builds): viewers and archive formats are compiled in but
nothing costly starts with the program (archive factories are tiny, the
index is built on first open, previews run when a file is opened, V8 only
when a script runs). The weak points were elsewhere:

- `sc` and `sc-gui` each linked V8 (about 60 MB), so the AppImage carried
  it twice; `sc-gui` was 83 MB.
- Opened archives stayed in the provider cache for the whole session (a
  zip of 100,000 entries keeps about 50 MB of index).
- The viewer copied a PDF's bytes once more, and split every file into
  text lines, binary files and documents included.
- The agent build carried the SFTP, WebDAV, S3 and SMB clients.
- Nothing watched sizes or memory, and there were no benchmarks.

## Decision

Small, separate changes, each with the simplest logic that works:

- **V8 once.** `sc-script` has an `engine` feature (V8 and the `sc` API);
  without it, it only reads plugin manifests and grants. The window builds
  `sc-frontend` with `scripting` only: Alt+X runs the command in the `sc`
  program next to it (`SC_PROGRAM`, else the sibling binary, else `PATH`)
  with the same settings folders, and shows what it printed. `sc run`
  takes `--panel`, `--other` and `--selected` so a panel script keeps its
  `sc.context`. The TUI lives in `sc` and keeps running scripts in process
  (`script-engine`).
- **Archives are released.** After each listing the front end drops the
  archive providers no tab is inside and nobody else holds
  (`ProviderRegistry::release_archives`); never while transfers run.
- **The viewer keeps one copy.** The bytes are one `Arc`, shared with the
  PDF renderer; text lines are made only for plain text, or when a hex
  dump switches to text. Reading files in windows (true streaming) is left
  out for now: it would make scrolling asynchronous in both interfaces.
- **A slimmer agent.** SFTP, WebDAV, S3 and SMB are the `remote` feature
  of `sc-app` and `sc-cli`, on by default; the release agents are built
  with `--no-default-features` and leave them out (`agent://` and
  `scs://` stay).
- **A budget in CI.** `scripts/footprint.py` checks sizes and idle memory
  against budgets written in the release workflow and fails above them;
  it also times a few jobs (listing, hashing, copying, opening a large
  zip, packing) with their peak memory, for the job summary.

## Consequences

- `sc-gui` 83 → 27 MB, idle 82 → 71 MB; agent 18.2 → 14.8 MB.
- The window needs `sc` installed next to it (the AppImage has both) to
  run scripts and plugins.
- A budget is raised only on purpose, in the commit that needs it.
- `sc-gui` must be built on its own (`cargo build -p sc-gui`): built in the
  same command as `sc-cli`, Cargo unifies features and links V8 into it
  again. The first release run caught exactly that (79 MB).
- A server `sc` built without `remote` cannot open SFTP/WebDAV/S3/SMB
  locations itself; a full `sc` can still be installed there.
