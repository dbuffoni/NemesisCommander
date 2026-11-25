# Contributing to NemesisCommander

Thanks for helping. This is a small project; keeping changes small and
tested makes them easy to merge.

## Getting started

```sh
cargo build                 # nmc: terminal interface, CLI, agent
cargo test --workspace      # unit and integration tests
cargo fmt --all && cargo clippy --workspace --all-targets
```

The Qt window (`gui/`, crate `nmc-gui`) needs Qt 6.8 or later and `lld`;
build it with its own command, `cargo build -p nmc-gui` (ADR 0020 says why).
The terminal interface is the quickest way to try a change: most behaviour
lives in `nmc-frontend` and is shared by both interfaces.

## Pull requests

- Open pull requests against `main`.
- One topic per pull request; describe what changed and why.
- Add or update tests, and the manual (`docs/manual/README.md`) when
  behaviour the user sees changes.
- A decision that is hard to reverse (a file format, a protocol, a new
  dependency with a large footprint) gets a short record in `docs/adr`.
- Keep the footprint in mind: `scripts/footprint.py` has budgets that CI
  enforces on releases.
- Features and bugs are tracked as GitHub issues.

## Style

`cargo fmt` and no `clippy` warnings. Public items have doc comments.
User-visible texts are plain, short English; dialogs and key hints follow
the wording of the existing ones.

By contributing you agree that your work is released under the MIT license.
No CLA or sign-off is required.
