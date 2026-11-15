//! The controller's side of server-to-server copies: choosing the route
//! and starting the copy on the server that does the work.
//!
//! Between two `sc serve` servers the data should not pass through the
//! controller. The routes, tried in this order:
//!
//! 1. **direct**: the destination connects to the source and pulls;
//! 2. **reverse**: the source connects to the destination and pushes (when
//!    only the source can reach the destination, e.g. behind NAT);
//! 3. **controller**: the controller copies itself, like any other copy.
//!
//! The first two need a transfer token for the server being contacted; the
//! controller signs it for exactly the paths of this copy, valid for a few
//! minutes and one connection. The route is reported to the user, since it
//! changes bandwidth, cost and what is exposed.

use std::fmt;
use std::time::Duration;

use futures::StreamExt;
use sc_ops::ConflictPolicy;
use sc_proto::pb;
use sc_tls::{ControllerIdentity, Grant};
use sc_vfs::VfsPath;
use tonic::Code;

use crate::serve::{connect, host_and_port};

/// How long a transfer token stays valid: enough to open the connection.
const TOKEN_LIFETIME: Duration = Duration::from_secs(300);

/// A way data can travel between two servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The destination pulls from the source.
    Direct,
    /// The source pushes to the destination.
    Reverse,
    /// Through the controller (a normal copy).
    Controller,
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Route::Direct => "direct",
            Route::Reverse => "reverse",
            Route::Controller => "via controller",
        })
    }
}

/// A copy between two `sc serve` servers.
pub struct ServerCopy<'a> {
    /// Sources, all on one `scs://` server.
    pub sources: &'a [VfsPath],
    /// The destination folder, on another `scs://` server.
    pub destination: &'a VfsPath,
    /// What to do with existing files (not "ask": nobody can answer).
    pub conflict: ConflictPolicy,
}

impl ServerCopy<'_> {
    /// Whether this copy is between two different `sc serve` servers, so a
    /// direct route is possible.
    pub fn is_between_servers(&self) -> bool {
        let first = match self.sources.first() {
            Some(first) => first,
            None => return false,
        };
        self.destination.scheme() == "scs"
            && self
                .sources
                .iter()
                .all(|s| s.scheme() == "scs" && s.authority() == first.authority())
            && first.authority() != self.destination.authority()
    }
}

/// The outcome of a server-to-server copy.
#[derive(Debug)]
pub enum Outcome {
    /// The servers copied: the route taken and the final progress message
    /// (with any failures).
    Copied(Route, pb::TransferProgress),
    /// Neither server could reach the other (or `routes` allowed only the
    /// controller): the caller should copy through the controller. The
    /// messages say why each route failed.
    UseController(Vec<String>),
}

/// Tries `routes` in order (only [`Route::Direct`] and [`Route::Reverse`]
/// are attempted here), calling `progress` with each progress message.
///
/// A route is abandoned only when the server doing the work cannot reach
/// the other one; any other failure (a missing file, a refused token, the
/// controller not reaching the server) ends the copy with an error.
pub async fn copy_between_servers(
    identity: &ControllerIdentity,
    copy: &ServerCopy<'_>,
    routes: &[Route],
    mut progress: impl FnMut(&pb::TransferProgress),
) -> Result<Outcome, String> {
    if !copy.is_between_servers() {
        return Err("a server-to-server copy needs sources on one scs:// server and a destination on another".into());
    }
    let conflict = serde_json::to_value(copy.conflict)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let source_authority = copy.sources[0].authority();
    let (source_host, source_port) = host_and_port(source_authority)?;
    let (destination_host, destination_port) = host_and_port(copy.destination.authority())?;
    let source_paths: Vec<String> = copy.sources.iter().map(|s| s.path().to_owned()).collect();
    let destination_path = copy.destination.path().to_owned();
    let config = identity.client_config().map_err(|e| e.to_string())?;

    let mut unreachable = Vec::new();
    for &route in routes {
        let (worker, request) = match route {
            Route::Direct => {
                let grant =
                    Grant::new(&source_host, source_paths.clone(), Vec::new(), TOKEN_LIFETIME);
                let request = pb::TransferRequest {
                    peer: format!("{source_host}:{source_port}"),
                    token: identity.sign(&grant),
                    push: false,
                    sources: source_paths.clone(),
                    destination: destination_path.clone(),
                    conflict: conflict.clone(),
                };
                ((destination_host.as_str(), destination_port), request)
            }
            Route::Reverse => {
                let grant = Grant::new(
                    &destination_host,
                    Vec::new(),
                    vec![destination_path.clone()],
                    TOKEN_LIFETIME,
                );
                let request = pb::TransferRequest {
                    peer: format!("{destination_host}:{destination_port}"),
                    token: identity.sign(&grant),
                    push: true,
                    sources: source_paths.clone(),
                    destination: destination_path.clone(),
                    conflict: conflict.clone(),
                };
                ((source_host.as_str(), source_port), request)
            }
            Route::Controller => break,
        };
        let server = connect(config.clone(), worker.0, worker.1, None).await?;
        let mut stream = match server.raw_client().transfer(request).await {
            Ok(response) => response.into_inner(),
            Err(status) if status.code() == Code::Unavailable => {
                unreachable.push(format!("{route}: {}", status.message()));
                continue;
            }
            Err(status) => return Err(status.message().to_owned()),
        };
        while let Some(message) = stream.next().await {
            let message = message.map_err(|status| status.message().to_owned())?;
            progress(&message);
            if message.finished {
                return Ok(Outcome::Copied(route, message));
            }
        }
        return Err(format!("{}:{}: the copy ended without a result", worker.0, worker.1));
    }
    Ok(Outcome::UseController(unreachable))
}
