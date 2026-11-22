# 0017. A settings screen, and settings sent to other machines

- Status: Accepted
- Date: 2026-11-16

## Context

Settings were a TOML file edited in an external editor (Ctrl+,), with
export and import to a file. The architecture document ruled out syncing
settings between machines. The maintainer then asked for a screen to change
shortcuts and options inside the program, and for a way to send the
settings to remote instances when needed.

## Decision

- **One screen, shared by both interfaces.** `sc-frontend::settings`
  describes every option (section, label, how it changes: toggle, choice,
  number, text) and builds the rows: options, then every command's
  shortcuts by category, then the commands that share settings. The window
  and the terminal draw `view::settings_view`; keys are handled once in
  `app/settings_screen.rs`.
- **Changes apply at once.** Each change is saved and applied: the key
  map, the number of transfer jobs at once (the manager can now change it
  while running) and the bandwidth limit. There is no "OK/Cancel": `r`
  puts a row's default back.
- **Shortcuts are recorded.** Enter waits for the next key; `a` adds a key
  instead of replacing; Delete leaves the command without keys. A key
  taken from another command is removed there, and the status line says
  so. Only keys that differ from the built-in ones are written to `[keys]`.
- **Sending settings is explicit, not a sync.** "Send these settings to
  another machine" (also `sc config push`) writes this machine's
  settings to another one; "Get the settings of another machine" (`sc
  config pull`) reads them, then asks replace or merge. Nothing runs in
  the background and nothing is compared.
  - Through the agent (`agent://host`, `scs://host`) the location of the
    machine is enough: the handshake now tells the controller where the
    agent's settings folder is (`HelloReply.settings_dir`, a compatible
    protocol addition; `Provider::settings_dir` exposes it).
  - Any other location names the settings file or its folder.
  - The previous file is kept as `config.toml.bak`; `--merge` keeps the
    other machine's bookmarks, connections and shortcuts.

## Consequences

- The "no sync between machines" decision becomes "no automatic sync":
  settings move only when the user sends or fetches them.
- An option added to `Config` must be added to `settings::Field` to appear
  on the screen (the file still accepts it otherwise).
- Remote machines without the agent need the path of their settings file.
- Update (SC-60): the settings screen no longer lists shortcuts. Keys are
  set in the Commands list (F9, Ctrl+K), which does the same; `[keys]` in the
  file is unchanged.
