# 0021. `sc test` and a guide, so AI assistants can write scripts

- Status: Accepted
- Date: 2026-11-16

## Context

The maintainer asked whether AI could write and test scripts and plugins. Nothing
was planned. Two ways: an AI built into SuperCommander (a model, a key, a
network client, a chat), or making the existing tools easy for the AI
coding assistants people already use (Claude Code and others). The first
grows the program and needs choices about providers and keys; the second
needs only a safe way to try a script and a clear report.

## Decision

- `sc test TARGET` runs a script, or each command of a plugin folder, in a
  new temporary folder (`panel/` with sample files or `--fixture`,
  `other/` empty) and reports checks (manifest, syntax, types with `tsc`
  when installed, else skipped) and, per run, the error, the printed lines
  and the files created, removed and changed. `--json` for programs; exit
  status 0 only when everything passed.
- A script may change only the test folder. A plugin gets its manifest's
  `$PANEL`, `$OTHER` and `$SELECTION` filled in with the test folders;
  `$HOME`, absolute locations and `all` are not granted and are reported as
  notes, so a test never touches real files.
- `sc run --guide` prints a guide (in `sc-script/js/GUIDE.md`) written for
  AI assistants: the loop, the runtime's rules, the manifest. Users save it
  as `AGENTS.md` next to `sc.d.ts`.
- Types are checked with the user's `tsc` instead of bundling a checker:
  bundling TypeScript would add tens of MB (ADR 0020).
- Lives in `sc` only, behind `scripting`: agent builds are unchanged.

## Consequences

- Any assistant that can run a command can write and test scripts. The
  built-in `sc ai` (ADR 0022) is the same loop on top of this.
- The report compares file contents before and after, so tests suit
  sample-sized folders, not large fixtures.
