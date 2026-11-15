//! # sc-proto
//!
//! The protocol between a SuperCommander controller and `sc agent`:
//!
//! * [`pb`]: the gRPC messages and services generated from
//!   `proto/sc/agent/v1/agent.proto`.
//! * [`convert`]: [`sc_vfs::Entry`], times and errors to and from the wire.
//! * [`transport`]: running the protocol over any byte stream, such as the
//!   standard input/output of `ssh host sc agent --stdio`.
//!
//! See ADR 0008 for why the agent speaks gRPC over a single stream.

pub mod convert;
pub mod transport;

/// Generated protocol types (`sc.agent.v1`).
#[allow(missing_docs, clippy::all, clippy::pedantic)]
pub mod pb {
    tonic::include_proto!("sc.agent.v1");
}

/// Version of the protocol in this build. The major number must match on
/// both sides; minor versions only add fields or calls (1.1: transfer
/// tokens and `Transfer`, for `sc serve`).
pub const PROTOCOL_VERSION: &str = "1.1";

/// Whether two protocol versions can talk to each other.
pub fn compatible(ours: &str, theirs: &str) -> bool {
    let major = |v: &str| v.split('.').next().unwrap_or("").to_owned();
    !major(ours).is_empty() && major(ours) == major(theirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_versions_must_match() {
        assert!(compatible("1.0", "1.3"));
        assert!(!compatible("1.0", "2.0"));
        assert!(!compatible("", ""));
    }
}
