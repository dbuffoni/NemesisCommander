# Security policy

Pairnox runs network services (`pnxr serve` with mutual TLS), stores
logins in the system keyring and runs plugins and scripts in a sandbox, so
security reports are welcome.

Please report a vulnerability privately through GitHub's "Report a
vulnerability" button on the Security tab of this repository, not in a public
issue. Include the version (`pnxr --version`), what you did and what you
expected. You will get an answer as soon as possible, and the fix is
announced in the release notes.

Only the latest release is supported.

Things worth knowing:

- `pnxr serve` listens on all interfaces by default (`--listen 0.0.0.0:7443`);
  pass `--listen 127.0.0.1:7443` to keep it local. Clients must present a
  certificate from the server's own authority.
- Plugins and scripts get no access until you grant it.
