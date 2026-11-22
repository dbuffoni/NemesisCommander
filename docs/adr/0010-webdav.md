# 0010. WebDAV over reqwest, logins from .netrc

- Status: Accepted
- Date: 2026-11-15

## Context

WebDAV is in the base scope (Nextcloud, ownCloud, Synology, Apache
mod_dav, IIS). It is HTTP with a few extra methods, so it needs an HTTP
client with TLS, an XML reader for `PROPFIND` answers, and somewhere to
find the user's login. ADR 0009 rules out storing secrets.

## Decision

- `sc-webdav` uses `reqwest` with `rustls` and the `ring` crypto provider.
  Both build from Rust sources plus a little C and assembly that
  cross-compiles with the musl/zig toolchain of the static agent build; no
  OpenSSL, no cmake. The system's root certificates are used through
  `rustls-platform-verifier`.
- Schemes: `davs://` is HTTPS and `dav://` plain HTTP. The URI path is the
  server path, so any server layout works without extra settings.
- Logins come from `~/.netrc` (or the file in `$NETRC`), which curl, git
  and many tools already read. A user name in the URI picks the matching
  entry. Basic authentication is sent once the server asks for it, and up
  front for uploads (their body can be sent only once).
- Only `resourcetype`, `getcontentlength` and `getlastmodified` are
  requested. XML elements are matched by local name.
- Mapping to the `Provider` operations:
  - `rename` and `copy_within` are server-side `MOVE`/`COPY` with
    `Overwrite: T`, so the provider claims `ATOMIC_RENAME` and
    `SERVER_COPY`. A *folder* at the destination is never overwritten,
    because WebDAV would delete it with its contents.
  - `remove_file` and `remove_dir` check the kind (and emptiness) first,
    because `DELETE` on a collection is recursive.
  - Uploads are a single streamed `PUT` (`If-None-Match: *` for new
    files). There is no partial write, so no resume, and no way to set
    modification times or permissions.
- The streaming upload sink is generic (`sc_vfs::upload_sink`), so other
  request-based providers (S3) reuse it.

## Consequences

- Digest and NTLM/Kerberos authentication are not supported yet; Basic
  over HTTPS covers the common servers.
- Copies to WebDAV restart interrupted files from the beginning.
- Interactive password entry can be added later as another source of
  logins without changing the provider.
