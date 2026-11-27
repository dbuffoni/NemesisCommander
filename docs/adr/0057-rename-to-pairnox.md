# 0057. The product is renamed Pairnox, the program `pnxr`

- Status: Accepted
- Date: 2026-11-27

## Context

The maintainer renamed the product from NemesisCommander to Pairnox (formerly
SuperCommander, see ADR 0029). As in 0029 he asked for a clean rename: no
backward compatibility and no migration of existing installs. No trademark or
package-name check was done for `Pairnox` or `pnxr`; this is not legal advice.

## Decision

- Product `Pairnox`, lowercase id `pairnox`, short prefix `pnxr`.
- Crates `pnxr-*`, binaries `pnxr` and `pnxr-gui`, environment variables
  `PNXR_*`, proto package `pnxr.agent.v1`, partial files `.pnxr-part`, script
  global `pnxr`, configuration folders `Pairnox` / `pairnox`.
- Old settings, certificates (run `tls init` again), scripts and plugins are not
  read. Old and new agents do not talk to each other.
- ADRs written before this one keep the old names: they are history. A CI step
  fails when the old name appears anywhere else.
- The GitHub repository is renamed by the maintainer after the merge.

## Consequences

Users start fresh on the first run after the update. The logo files were renamed
but still show the old picture; a new logo is out of scope.
