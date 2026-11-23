# 0014. `nmc serve`: mutual TLS from a private authority, signed one-time tokens

- Status: Accepted
- Date: 2026-11-15

## Context

Phase 3 adds the permanent agent (`nmc serve`, its own port, for direct
transfers and long jobs) and server-to-server copies in which the data
does not pass through the desktop. The architecture document asks for
mutual TLS and for one-time tokens, with an expiry, signed by the
controller, that authorize only one job on given paths, and for the route
(direct, reverse, relay, through the controller) to be visible.

## Decision

- **Authority.** `nmc tls init` creates, in the controller's configuration
  folder, a private CA, the controller's client certificate and an Ed25519
  token key (`nmc-tls`, generated with `rcgen`, ECDSA P-256, crypto by
  `ring` so the static musl agent still builds). `nmc tls server NAME...
  --out DIR` issues a bundle per server: its certificate (server and client
  use, valid for the names given), the CA, the controller's certificate and
  the token public key. Private keys are written mode 600.
- **Roles.** `nmc serve` requires a client certificate from the CA. A
  connection presenting exactly the controller's certificate has the same
  rights as `nmc agent --stdio`; any other certificate of the CA (another
  server) has none until it presents a token. The roots and read-only
  limits apply to everyone.
- **Tokens.** A token is a JSON grant (random id, audience server name,
  expiry, read paths, write paths) signed with the controller's Ed25519
  key, base64url encoded. The other server sends it in `Hello`; the agent
  checks signature, expiry and audience, binds the grant to that connection
  and refuses the same id on any other connection (single use). Every path
  the connection touches must be below a granted path (write grants
  include reading). Tokens live five minutes: enough to open the connection.
- **Transfer call.** Protocol 1.1 adds `Hello.token` and `Transfer`, which
  only the controller may call: the server connects to the other server
  with its own certificate and the token, and runs the ordinary copy engine
  (`nmc-ops`) between its files and the other server, streaming progress.
  The controller's disconnect cancels it. Conflicts cannot be asked about:
  the policy is fixed up front ("ask" is refused).
- **Routes.** `nmc cp` between two `scs://` servers tries direct (the
  destination pulls with a read token for the source), then reverse (the
  source pushes with a write token for the destination), then copies through
  the controller. A route is abandoned only when the worker cannot reach the
  other server (`UNAVAILABLE`); other errors stop. `--route` forces one, and
  the route taken is always printed. An explicit relay is not implemented.
- `scs://host[:port]/path` (default port 7443) is the provider for `nmc
  serve`; the names in URLs must be names in the server's certificate.

## Consequences

- Revoking a server means issuing new bundles from a new authority; there
  is no revocation list. Losing the controller folder means re-issuing all
  bundles. Both are acceptable for a personal tool and keep the model small.
- One controller per server bundle. Several controllers would need several
  controller certificates and token keys in the bundle (a later, compatible
  change).
- The direct route needs the source's name, as the controller writes it, to
  resolve from the destination. A per-server "address seen by peers" can be
  added if networks differ.
- Server-to-server copies run in the CLI; the queue, the TUI and the window
  still copy through the controller. Moving them onto `route` is a matter of
  calling the same function from the transfer manager.
