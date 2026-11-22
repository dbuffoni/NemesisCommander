# 0016. Plugins: a folder with a manifest, permissions granted once

- Status: Accepted
- Date: 2026-11-15

## Context

After scripts (ADR 0015), the architecture document asks for plugins: a
package with a manifest, permissions declared there and granted by the user
at first use, no free access by default, and an isolate per plugin with
memory and time limits so a stuck plugin does not block the interface. It
lists providers, viewers, commands, columns and search filters as things
plugins add.

## Decision

- **Package.** A plugin is a folder with `plugin.json` (name, version,
  description, `main` script, commands, permissions, limits) and its
  script. `sc plugin install FOLDER` copies it to the data folder
  (`plugins/NAME`); `list`, `remove`, `allow`, `deny` and `run` manage it.
  Unknown manifest fields are refused, so a manifest asking for a
  permission that does not exist yet (network, processes) fails loudly
  instead of being ignored.
- **Commands first.** This version lets plugins add commands. A command
  runs the plugin's script with `sc.context` (command id, both panels'
  folders, the selection). Alt+X ("Plugins and scripts…", in the Tools
  menu of the TUI and the window) lists them, and also offers to run the
  `.ts`/`.js` file under the cursor. Providers, viewers, columns and search
  filters need a long-lived plugin with callbacks and are left for later.
- **Permissions.** The manifest names locations to read and to write:
  absolute paths, URIs, or the placeholders `$PANEL`, `$OTHER`,
  `$SELECTION` and `$HOME`, filled in at each run, so "change the folder
  I am in" is a permission without naming a folder. At first use the user
  sees the request in words and allows it; the grant (the exact request) is
  stored in `plugin-grants.json` in the configuration folder. An update
  that asks for something different must be allowed again. A script run
  from a panel may change its panel's folder and read the other one, after
  a confirmation each time.
- **Isolation.** Each run gets its own isolate on its own thread (ADR 0015)
  with the manifest's limits (256 MiB and 300 s by default). The interface
  only waits for an event; what the plugin printed is shown when it ends.

## Consequences

- Plugins are single scripts: no imports, no npm packages. A plugin that
  needs a library must bundle it into its script.
- Grants are per plugin name. Installing a different plugin under an
  allowed name with the same request would inherit the grant; signatures
  for published plugins are an open question.
- Long-lived plugins (providers, viewers, columns) will need a protocol of
  calls from Rust into a resident isolate; the manifest has room for it.
