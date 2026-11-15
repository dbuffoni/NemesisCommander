# 0015. TypeScript scripts: deno_core, one `sc` global, no access by default

- Status: Accepted
- Date: 2026-11-15

## Context

Phase 4 adds automation in TypeScript: `sc run script.ts` and, later,
plugins. The architecture document fixes the engine (V8 through
`deno_core`, no Node), asks for permissions per script and limits of memory
and time, and requires the server build of `sc` to stay free of V8.

## Decision

- **Crate.** `sc-script` holds everything: transpiling (`deno_ast`, types
  are stripped, not checked), the runtime, the `sc` API and permissions.
  `sc-cli` uses it behind the `scripting` feature, on by default and off in
  `--no-default-features` (the agent build), so `sc serve`/`sc agent` never
  link V8.
- **One global.** A script sees `sc` (declarations in `js/sc.d.ts`, printed
  by `sc run --types`). Its functions are Rust ops over the same provider
  registry and copy engine as the panels: TypeScript orchestrates, Rust
  moves the bytes. `sc` is frozen and `Deno` is removed after the SDK
  captured the ops, so scripts cannot reach the runtime's internals.
- **Single file.** A script is one ES module; it cannot import others
  (`deno_core`'s default loader refuses). This keeps permissions meaningful
  and leaves packaging for plugins.
- **Permissions.** A script starts with none. `--allow-read LOCATION`,
  `--allow-write LOCATION` (write implies read) and `--allow-all` grant
  locations of any scheme. Paths are normalized before the check, so `..`
  cannot leave a granted folder; inside an archive, the archive file is the
  location checked, and writes into archives are refused. Refusals are
  `PermissionDenied` errors a script can catch.
- **Limits.** The V8 heap has a limit (`--memory`, 512 MiB by default);
  near it the script is terminated instead of the process aborting. An
  optional time limit (`--timeout`) stops busy JavaScript through a
  watchdog thread and long awaits through a timeout around the event loop.
- **Thread.** `deno_core` needs a current-thread runtime. `sc_script::run`
  starts a thread with its own runtime per script (an isolate per run) and
  returns a `Send` future, so the CLI, the TUI and the GUI can all await it.

## Consequences

- The desktop `sc` and the AppImage grow by the size of V8 (about 35 MB);
  server builds do not.
- Symbolic links inside a granted folder are followed, as by the panels: a
  grant means "what the user can reach from here". Users should grant the
  narrowest folder.
- Type errors are not reported (no `tsc`); editors get them from
  `sc run --types > sc.d.ts`.
- Plugins (a manifest declaring permissions, loaded by the front ends) build
  on the same runtime and permissions.
