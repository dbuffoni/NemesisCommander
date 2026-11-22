# 0040. Saved logins go to the system keyring, not ~/.netrc

- Status: Accepted
- Date: 2026-11-21
- Supersedes: the `~/.netrc` saving of [0036](0036-connection-logins.md)

## Context

ADR 0036 let a connection's password be asked every time or saved to
`~/.netrc` (mode 600). Daniele, on Fedora, asked for the GNOME keyring
instead: a plain-text file is a weak place for a password when the desktop
already has a locked, encrypted store.

## Decision

- **Where it is saved.** The login question's second row now saves the
  password in the Secret Service (GNOME Keyring, KWallet, KeePassXC), the
  store of the desktop session. Nothing is written to `~/.netrc` any more;
  `nmc_vfs::netrc::save` is gone.
- **How.** `nmc_vfs::keyring` runs `secret-tool` (libsecret): `store`, `lookup`,
  `search`, `clear`. A login has the attributes `service nemesiscommander`,
  `host`, `user` (host lower-cased) and the label `NemesisCommander:
  user@host`, so Seahorse and KWalletManager list and delete it. The password
  goes through the standard input, never a command line. A command line tool
  rather than a D-Bus crate keeps the dependencies and the static agent
  builds as they are, and follows what the SSH provider already does with
  `ssh`. `NMC_SECRET_TOOL` (or `keyring::set_command`) names another command,
  for tests.
- **Lookup order** (`netrc::lookup`, used by WebDAV, SMB and the SSH
  password helper): the login typed for this session, then the keyring, then
  the `~/.netrc` file. Existing `.netrc` entries keep working through the
  third step, and the "keep as it is" row still says so. They are not
  migrated or rewritten: the file may be shared with curl and git, and
  deleting a password from it is the user's call.
- **No Secret Service.** Without `secret-tool` the row is labelled "needs
  secret-tool: not found". Choosing it anyway, or a keyring that refuses (no
  service on the bus, locked and dismissed), shows the error and suggests
  *ask every time*; nothing is stored. Ask every time is unchanged and is
  what Windows and macOS use for now (the Secret Service does not exist
  there).
- **S3.** Keys still go to `~/.aws/credentials`, the file the AWS tools use;
  this decision does not touch them.

## Consequences

The settings file and `~/.netrc` are no longer written with passwords. The
keyring may ask to be unlocked the first time a saved connection is opened.
Each lookup starts one short `secret-tool` process. Native Windows and macOS
stores are a possible next step.
