# Continuous integration

One workflow, `.github/workflows/ci.yml`, with two tiers.

| Tier | When | What |
| --- | --- | --- |
| Simple | every push to `main`, every pull request | format, clippy, `cargo test` on Ubuntu, the Qt window tests |
| Full | every night at 01:00 Rome time, **Run workflow** (manual), and pull requests with the `full-ci` label | the simple tier, plus macOS and Windows, plus the heavy checks below |

The `plan` job decides. Cron runs in UTC without daylight saving, so two
schedules (`0 23` and `0 0`) are listed and `plan` lets through only the one
where `TZ=Europe/Rome date +%H` is `01`; GitHub may start a run a few minutes
late, so only the hour counts. Scheduled runs only fire on `main`.

A failed nightly opens (or comments on) the issue "Nightly CI failed".

## Heavy checks in the full tier

- **S3** against MinIO (`crates/pnxr-s3/tests/real_server.rs`): signing, folders,
  ranges, multipart upload, wrong key.
- **WebDAV** against Apache `mod_dav` and Nextcloud
  (`crates/pnxr-webdav/tests/real_server.rs`).
- **SMB** against Samba in three variants: SMB 2.1, SMB 3, SMB 3 with signing
  required; a file over 2 GiB; a wrong password.
- **SSH**: key with a passphrase from an agent; a changed host key is refused.
- **Network faults**: toxiproxy in front of MinIO (`crates/pnxr-s3/tests/faults.rs`):
  latency, a reset in the middle of an upload, a dead link.
- **All systems**: the whole test suite and the Qt window tests on macOS and
  Windows, and the tests that need the real system (see below).

A missing server fails the test instead of skipping it when
`PNXR_REQUIRE_SERVERS` asks for it: `1` for all servers, or a list such as
`sftp,smb,ssh` (the simple tier asks for the servers it installs itself).

## Run the heavy tests on your machine

Needs Docker (Compose), `openssl` and `curl`.

```sh
set -a; eval "$(scripts/heavy/up.sh)"; set +a      # NEXTCLOUD=1 adds Nextcloud
PNXR_REQUIRE_SERVERS=1 cargo test -p pnxr-s3 -p pnxr-webdav --test real_server
PNXR_REQUIRE_SERVERS=1 cargo test -p pnxr-s3 --test faults
docker compose -f scripts/heavy/compose.yml down -v
```

Samba and OpenSSH variants: `scripts/smb-test-server.sh` and
`scripts/ssh-test-server.sh` (their header says how; `SMB_MIN_PROTOCOL`,
`SMB_SIGNING`, `SSH_VARIANT`).

## Add a new heavy check

1. Write the test so it reads its server from environment variables and skips
   (or fails when `PNXR_REQUIRE_SERVERS` asks for it) without them.
2. Add a job to `ci.yml` with `needs: plan` and
   `if: needs.plan.outputs.full == 'true'`, and add it to `nightly-alert`'s
   `needs`.
3. Pin every container image version in `scripts/heavy/compose.yml`.

Fault tests use generous timeouts; only the proxy setup may be retried, never
an assertion.

## Tests that need Windows or macOS

A test that only makes sense on one system is gated with `#[cfg(windows)]` or
`#[cfg(target_os = "macos")]`. They run in the full tier only (the simple tier
is Linux only), so they cost nothing on pull requests. A test that touches the
real credential store or a real volume must use a throwaway name and clean up.

## OS-specific tests (SC-164)

The nightly tier runs the whole workspace on macOS and Windows. Tests that
need the real system sit in `crates/pnxr-frontend/tests/os_native.rs` (drive
letters, drive and disk lists, the path model, symlinks) and in the `cfg`-gated
modules of `pnxr-providers` (Windows attributes, long and UNC paths, xattr),
`pnxr-vfs` (Credential Manager, Keychain) and `pnxr-libarchive`. The agent and
CLI integration tests (`pnxr-agent`, `pnxr-cli`) run there too and act as the
smoke test of the binaries. Missing features are not tested; installers have
their own job (SC-168).

## Installers (Windows and macOS)

`.github/workflows/installers.yml` builds the Windows portable zip
(`packaging/windows/build-zip.ps1`, Qt bundled by `windeployqt`), the Windows
installer made from it (`packaging/windows/build-setup.ps1`, Inno Setup, per
user without administrator rights, Start menu entry, optional PATH, an entry to
remove it; the smoke test installs silently, runs `pnxr --version` and
uninstalls) and the macOS
disk image (`packaging/macos/build-dmg.sh`, an app bundle with Qt bundled by
`macdeployqt`), each with `LICENSE` and `THIRD-PARTY-NOTICES.md`, and tests
them: unpack or mount into a temporary place and run `pnxr --version` and
`pnxr-gui --version`. On macOS the copy of the app runs with the runner's Qt
hidden and, unsigned, opens its window off screen; any error of
`macdeployqt` fails the build. The Windows test still runs with Qt installed.
The full CI tier (nightly) runs it; the release workflow runs it too and
publishes the files on a `v*` tag.

Without signing secrets the files are named `-unsigned` (macOS: signed ad hoc,
so it runs on the Mac that built it; other Macs ask in Gatekeeper). Secrets,
all optional: `WINDOWS_CERT_PFX` (base64 .pfx) and `WINDOWS_CERT_PASSWORD`;
`MACOS_SIGN_IDENTITY`, `MACOS_CERT_P12` (base64) and `MACOS_CERT_PASSWORD`;
for notarization also `APPLE_ID`, `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD`.
Certificates and the Apple developer account are the owner's decision and cost.
