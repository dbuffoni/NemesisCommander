# 0029. The product is renamed NemesisCommander, the program `nmc`

- Status: Accepted
- Date: 2026-11-18

## Context

The maintainer renamed the product from SuperCommander to NemesisCommander . He asked for a clean rename: no backward compatibility and no
migration of existing installs. `sc` also collided with `sc.exe` (Windows
Service Control). He chose the short name `nmc` and accepted the trademark risk
of the name (live NEMESIS marks exist in class 9; no official register search
was done; this is not legal advice).

## Decision

One prefix, `nmc`, replaces `sc` everywhere:

- crates `sc-*` become `nmc-*` (`nmc-gui` for the window); binaries are `nmc`
  and `nmc-gui`; the AppImage is `NemesisCommander-*.AppImage`; agent assets
  are `nmc-agent-linux-<arch>`;
- environment variables `SC_*` become `NMC_*`;
- the protocol package `sc.agent.v1` becomes `nmc.agent.v1` (the gRPC method
  paths change, so old agents and controllers cannot talk to new ones; the
  first call, `Hello`, already fails with `UNIMPLEMENTED`, so raising the
  protocol major version would add nothing and it stays 1.1);
- partial transfer files `.name.sc-part` become `.name.nmc-part`;
- the TypeScript global `sc` of scripts and plugins becomes `nmc`
  (`nmc.d.ts`);
- the settings folder follows the product name (`org.NemesisCommander`,
  `nemesiscommander`); QML module, application name, desktop file and
  certificate names use `NemesisCommander`.

Nothing from the old name is read: no settings migration, no old environment
variables, no old binary name, no old certificates.

## Consequences

- Existing users start fresh: settings, bookmarks, connections, plugins, scripts
  and certificates (`nmc tls init` again) of SuperCommander are not picked up,
  and scripts must use `nmc.*`.
- Earlier ADRs were rewritten to the new names in release 0.3.0; only this
  record keeps the old ones, because it describes the change.
- The GitHub repository and the project pages are renamed outside the code.
