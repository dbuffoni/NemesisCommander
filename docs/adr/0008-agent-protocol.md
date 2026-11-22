# 0008. The agent speaks gRPC over the SSH session's standard streams

- Status: Accepted
- Date: 2026-11-15

## Context

The architecture document asks for an agent that is the same `sc` binary,
started on demand through SSH (`sc agent --stdio`, no open port), with a
gRPC protocol whose control calls are not slowed down by large transfers,
a version/capability handshake, and limits enforced by the agent itself.
The agent runs on Linux only.

## Decision

- `sc-proto` defines the protocol in `proto/sc/agent/v1/agent.proto`
  (package `sc.agent.v1`). The code is generated with tonic/prost. The
  `.proto` file is compiled by `protox`, a Rust implementation of `protoc`,
  so contributors and CI need no extra tool.
- The calls mirror the `Provider` trait (stat, list, read, write, mkdir,
  remove, rename, copy, attributes, checksum, symlink) plus `Hello`. Data
  moves in 256 KiB chunks: `Read` is a server stream, `Write` a client
  stream whose first message is a header. `List` streams pages.
- The whole protocol runs over **one** byte stream: HTTP/2 multiplexes every
  call as its own stream with its own flow control, so a large transfer and
  directory browsing share the SSH channel without blocking each other. This
  meets the "separate control and data" requirement without a second
  connection.
- Errors are gRPC status codes; kinds that share a code (not a directory,
  directory not empty, ...) carry an `sc-error-kind` metadata entry, so the
  controller gets the same `VfsError` variant the agent saw.
- The handshake returns the protocol version (major must match), the agent
  version, its roots and whether it is read-only. Fields are only added.
- `sc agent --stdio [--root DIR]... [--read-only]` enforces its limits on
  every call. Paths are normalized lexically, then resolved through existing
  symbolic links: operations that follow a path (read, write, list) check
  the final target, operations on the entry itself (stat, remove, rename)
  check its folder, so a link inside a root cannot be used to read outside.
- The controller starts `ssh -T -o BatchMode=yes host -- sh -c '<script>'`.
  The script runs `~/.cache/sc/sc-<version>` or an installed `sc` of the
  same version; otherwise it reports "missing", the controller checks
  `uname -sm` and uploads a Linux `sc` (`$SC_AGENT_BINARY`, a
  `sc-agent-linux-<arch>` file next to the program, or itself) and retries.
- `SC_AGENT_COMMAND` replaces the SSH command line (`{host}` is
  substituted). Tests use it to run a local agent; it also works for
  `docker exec` and similar.

## Consequences

- The agent provider passes the same copy-engine tests as local files, in
  memory (duplex stream) and as a real child process.
- Batch mode means password logins are not supported: users need keys or an
  SSH agent. The controller shows ssh's error message when login fails.
- `sc serve` (phase 3) can reuse the same service over TLS with several
  connections.
