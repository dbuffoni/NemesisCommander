//! The agent side: serves the local file system of the machine it runs on,
//! within configured limits, to a controller (or, under `nmc serve`, to
//! another server holding a transfer token).

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::StreamExt;
use nmc_proto::convert::{entry_to_pb, error_to_status, ns_to_time};
use nmc_proto::pb::{self, agent_server::Agent, write_request::Part};
use nmc_providers::local::LocalProvider;
use nmc_tls::{Grant, ServerIdentity, TokenError};
use nmc_vfs::{Provider, SetAttributes, VfsError, VfsPath, WriteMode, WriteOptions};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tonic::codegen::BoxStream;
use tonic::{Request, Response, Status, Streaming};

/// Size of the data chunks sent by `Read`.
pub const READ_CHUNK: usize = 256 * 1024;

/// What the agent allows, whatever the controller asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Limits {
    /// Only paths below these folders are served (empty: everything).
    pub roots: Vec<PathBuf>,
    /// Refuse every modification.
    pub read_only: bool,
    /// This machine's NemesisCommander settings folder, told to controllers
    /// so they can send settings here.
    pub settings_dir: Option<PathBuf>,
}

/// How an operation uses a path; decides how symbolic links are checked
/// against the roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Use {
    /// Works on the directory entry itself (stat, remove, rename): a link
    /// inside a root may point anywhere.
    Entry,
    /// Follows the path (read, write, list): the final target must be inside
    /// a root too.
    Content,
}

/// Who the service is talking to.
pub(crate) enum Session {
    /// The controller: `nmc agent --stdio` under SSH, or a connection to `nmc
    /// serve` presenting the controller's certificate. `serve` is set for
    /// the latter, which may also start server-to-server transfers.
    Controller {
        /// The `nmc serve` state.
        serve: Option<Arc<ServeState>>,
    },
    /// Another server under `nmc serve`: it must present a transfer token in
    /// `Hello`, and every path it uses must be covered by that token.
    Peer {
        /// The `nmc serve` state.
        serve: Arc<ServeState>,
        /// The token's grant, once `Hello` checked it.
        grant: Mutex<Option<Grant>>,
    },
}

/// What `nmc serve` shares between its connections.
pub struct ServeState {
    /// The server's certificates and the controller's token key.
    pub identity: ServerIdentity,
    /// Tokens already used, by id, with their expiry: a token opens one
    /// connection only.
    used: Mutex<std::collections::HashMap<String, u64>>,
}

impl ServeState {
    /// The state of a server with `identity`.
    pub fn new(identity: ServerIdentity) -> Self {
        ServeState { identity, used: Mutex::default() }
    }

    /// Checks `token` and marks it used.
    fn redeem(&self, token: &str) -> Result<Grant, TokenError> {
        let grant = self.identity.verify(token)?;
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        let now = nmc_tls::token::now();
        used.retain(|_, expires| *expires >= now);
        if used.insert(grant.id.clone(), grant.expires).is_some() {
            return Err(TokenError::Used);
        }
        Ok(grant)
    }
}

/// The gRPC service of `nmc agent`.
pub struct AgentService {
    local: LocalProvider,
    limits: Arc<Limits>,
    session: Session,
}

impl AgentService {
    /// A service for the controller, enforcing `limits`. Roots are resolved
    /// now, so they must exist.
    pub fn new(limits: Limits) -> io::Result<Self> {
        Ok(Self::with_session(
            Arc::new(resolve_roots(limits)?),
            Session::Controller { serve: None },
        ))
    }

    /// A service for one connection with `session`; `limits` must come from
    /// [`resolve_roots`].
    pub(crate) fn with_session(limits: Arc<Limits>, session: Session) -> Self {
        AgentService { local: LocalProvider::new(), limits, session }
    }

    /// Checks `path` against the limits (and, for another server, its
    /// token) and turns it into a local path.
    fn check(&self, path: &str, use_: Use, modifies: bool) -> Result<VfsPath, Status> {
        if modifies && self.limits.read_only {
            return Err(Status::permission_denied(format!("{path}: the agent is read-only")));
        }
        if !path.starts_with('/') {
            return Err(Status::invalid_argument(format!("{path}: paths must be absolute")));
        }
        let lexical = normalize(Path::new(path));
        if let Session::Peer { grant, .. } = &self.session {
            let grant = grant.lock().unwrap_or_else(|e| e.into_inner());
            let allowed = grant
                .as_ref()
                .is_some_and(|grant| grant.allows(&lexical.to_string_lossy(), modifies));
            if !allowed {
                return Err(Status::permission_denied(format!(
                    "{path}: not covered by the transfer token"
                )));
            }
        }
        if !self.limits.roots.is_empty() {
            let resolved = resolve(&lexical, use_);
            if !self.limits.roots.iter().any(|root| resolved.starts_with(root)) {
                return Err(Status::permission_denied(format!(
                    "{path}: outside the folders this agent serves"
                )));
            }
        }
        VfsPath::from_local(&lexical).map_err(|e| error_to_status(&e))
    }

    fn hello_reply(&self) -> pb::HelloReply {
        pb::HelloReply {
            protocol: nmc_proto::PROTOCOL_VERSION.into(),
            agent_version: crate::VERSION.into(),
            roots: self.limits.roots.iter().map(|r| r.display().to_string()).collect(),
            read_only: self.limits.read_only,
            hash_algorithms: vec!["blake3".into(), "xxh3".into(), "sha256".into()],
            settings_dir: self
                .limits
                .settings_dir
                .as_ref()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
        }
    }
}

/// Resolves the roots of `limits` (they must exist), so links and `..` in
/// them do not matter later.
pub(crate) fn resolve_roots(mut limits: Limits) -> io::Result<Limits> {
    for root in &mut limits.roots {
        *root = std::fs::canonicalize(&*root)?;
    }
    Ok(limits)
}

/// Removes `.` and resolves `..` without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
            _ => {}
        }
    }
    out
}

/// The real location of `path` as far as it exists: symbolic links in the
/// existing part are resolved, the missing rest is appended as is.
fn resolve(path: &Path, use_: Use) -> PathBuf {
    let (base, tail) = match use_ {
        Use::Content => (path, None),
        // The entry itself is not followed, only its folder.
        Use::Entry => match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => (parent, Some(name)),
            _ => (path, None),
        },
    };
    let mut existing = base.to_path_buf();
    let mut missing = Vec::new();
    let resolved = loop {
        match std::fs::canonicalize(&existing) {
            Ok(real) => break real,
            Err(_) => match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    missing.push(name.to_owned());
                    existing = parent.to_path_buf();
                }
                _ => break existing,
            },
        }
    };
    let mut result = resolved;
    for part in missing.into_iter().rev() {
        result.push(part);
    }
    if let Some(name) = tail {
        result.push(name);
    }
    result
}

fn status(error: VfsError) -> Status {
    error_to_status(&error)
}

type Reply<T> = Result<Response<T>, Status>;

#[tonic::async_trait]
impl Agent for AgentService {
    async fn hello(&self, request: Request<pb::HelloRequest>) -> Reply<pb::HelloReply> {
        if let Session::Peer { serve, grant } = &self.session {
            let token = request.into_inner().token;
            if token.is_empty() {
                return Err(Status::permission_denied(
                    "servers need a transfer token from the controller",
                ));
            }
            let redeemed =
                serve.redeem(&token).map_err(|e| Status::permission_denied(e.to_string()))?;
            *grant.lock().unwrap_or_else(|e| e.into_inner()) = Some(redeemed);
        }
        Ok(Response::new(self.hello_reply()))
    }

    async fn stat(&self, request: Request<pb::PathRequest>) -> Reply<pb::Entry> {
        let path = self.check(&request.into_inner().path, Use::Entry, false)?;
        let entry = self.local.stat(&path).await.map_err(status)?;
        Ok(Response::new(entry_to_pb(&entry)))
    }

    type ListStream = BoxStream<pb::EntryPage>;

    async fn list(&self, request: Request<pb::PathRequest>) -> Reply<Self::ListStream> {
        let path = self.check(&request.into_inner().path, Use::Content, false)?;
        let pages = self.local.list(&path).await.map_err(status)?;
        let pages = pages.map(|page| match page {
            Ok(entries) => Ok(pb::EntryPage { entries: entries.iter().map(entry_to_pb).collect() }),
            Err(e) => Err(status(e)),
        });
        Ok(Response::new(Box::pin(pages)))
    }

    type ReadStream = BoxStream<pb::Chunk>;

    async fn read(&self, request: Request<pb::ReadRequest>) -> Reply<Self::ReadStream> {
        let request = request.into_inner();
        let path = self.check(&request.path, Use::Content, false)?;
        let reader = self.local.read(&path, request.offset).await.map_err(status)?;
        let chunks =
            ReaderStream::with_capacity(reader, READ_CHUNK).map(move |chunk| match chunk {
                Ok(data) => Ok(pb::Chunk { data }),
                Err(e) => Err(status(VfsError::from_io(&path, e))),
            });
        Ok(Response::new(Box::pin(chunks)))
    }

    async fn write(&self, request: Request<Streaming<pb::WriteRequest>>) -> Reply<pb::WriteReply> {
        let mut messages = request.into_inner();
        let header = match messages.message().await? {
            Some(pb::WriteRequest { part: Some(Part::Header(header)) }) => header,
            _ => return Err(Status::invalid_argument("a write starts with a header")),
        };
        let path = self.check(&header.path, Use::Content, true)?;
        let mode = match pb::WriteMode::try_from(header.mode) {
            Ok(pb::WriteMode::CreateNew) => WriteMode::CreateNew,
            Ok(pb::WriteMode::Truncate) => WriteMode::Truncate,
            Ok(pb::WriteMode::Resume) => WriteMode::ResumeAt(header.offset),
            _ => return Err(Status::invalid_argument("unknown write mode")),
        };
        let options = WriteOptions { mode, size_hint: header.size_hint };
        let mut sink = self.local.write(&path, options).await.map_err(status)?;
        let mut written = 0u64;
        while let Some(message) = messages.message().await? {
            let data: Bytes = match message.part {
                Some(Part::Data(data)) => data,
                _ => return Err(Status::invalid_argument("only data may follow the header")),
            };
            sink.write_all(&data).await.map_err(|e| status(VfsError::from_io(&path, e)))?;
            written += data.len() as u64;
        }
        // The controller closed its side: the file is complete.
        sink.shutdown().await.map_err(|e| status(VfsError::from_io(&path, e)))?;
        Ok(Response::new(pb::WriteReply { bytes_written: written }))
    }

    async fn create_dir(&self, request: Request<pb::PathRequest>) -> Reply<pb::Empty> {
        let path = self.check(&request.into_inner().path, Use::Entry, true)?;
        self.local.create_dir(&path).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn remove_file(&self, request: Request<pb::PathRequest>) -> Reply<pb::Empty> {
        let path = self.check(&request.into_inner().path, Use::Entry, true)?;
        self.local.remove_file(&path).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn remove_dir(&self, request: Request<pb::PathRequest>) -> Reply<pb::Empty> {
        let path = self.check(&request.into_inner().path, Use::Entry, true)?;
        self.local.remove_dir(&path).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn rename(&self, request: Request<pb::TwoPaths>) -> Reply<pb::Empty> {
        let request = request.into_inner();
        let from = self.check(&request.from, Use::Entry, true)?;
        let to = self.check(&request.to, Use::Entry, true)?;
        self.local.rename(&from, &to).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn copy_within(&self, request: Request<pb::TwoPaths>) -> Reply<pb::Empty> {
        let request = request.into_inner();
        let from = self.check(&request.from, Use::Content, false)?;
        let to = self.check(&request.to, Use::Content, true)?;
        self.local.copy_within(&from, &to).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn set_attributes(&self, request: Request<pb::SetAttributesRequest>) -> Reply<pb::Empty> {
        let request = request.into_inner();
        let path = self.check(&request.path, Use::Content, true)?;
        let attributes = SetAttributes {
            permissions: request.permissions,
            modified: request.modified_ns.map(ns_to_time),
            ..Default::default()
        };
        self.local.set_attributes(&path, &attributes).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn checksum(&self, request: Request<pb::ChecksumRequest>) -> Reply<pb::ChecksumReply> {
        let request = request.into_inner();
        let path = self.check(&request.path, Use::Content, false)?;
        let algorithm = nmc_hash::parse_algorithm(&request.algorithm).ok_or_else(|| {
            Status::invalid_argument(format!("unknown hash algorithm {:?}", request.algorithm))
        })?;
        let hex = self.local.checksum(&path, algorithm).await.map_err(status)?;
        Ok(Response::new(pb::ChecksumReply { hex }))
    }

    async fn create_symlink(&self, request: Request<pb::SymlinkRequest>) -> Reply<pb::Empty> {
        let request = request.into_inner();
        let path = self.check(&request.path, Use::Entry, true)?;
        self.local.create_symlink(&path, &request.target).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    type TransferStream = BoxStream<pb::TransferProgress>;

    async fn transfer(&self, request: Request<pb::TransferRequest>) -> Reply<Self::TransferStream> {
        let serve = match &self.session {
            Session::Controller { serve: Some(serve) } => serve.clone(),
            Session::Controller { serve: None } => {
                return Err(Status::unimplemented(
                    "server-to-server copies need `nmc serve` on both servers",
                ));
            }
            Session::Peer { .. } => {
                return Err(Status::permission_denied("only the controller starts transfers"));
            }
        };
        let request = request.into_inner();
        // The local side is checked here; the peer checks its side against
        // the token.
        let local: Vec<String> =
            if request.push { request.sources.clone() } else { vec![request.destination.clone()] };
        for path in &local {
            self.check(path, Use::Content, !request.push)?;
        }
        let progress = crate::transfer::run(serve, request).await?;
        Ok(Response::new(progress))
    }
}

/// Serves the agent protocol on standard input/output until the controller
/// hangs up. This is `nmc agent --stdio`.
pub async fn serve_stdio(limits: Limits) -> io::Result<()> {
    let service = AgentService::new(limits)?;
    let io = nmc_proto::transport::Duplex::new(tokio::io::stdin(), tokio::io::stdout());
    nmc_proto::transport::serve(service, io).await.map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_never_climbs_above_the_root() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("/../../etc")), PathBuf::from("/etc"));
    }

    #[cfg(unix)]
    #[test]
    fn roots_are_enforced_through_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(dir.path().join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret"), root.join("link")).unwrap();
        let service = AgentService::new(Limits {
            roots: vec![root.clone()],
            read_only: false,
            ..Default::default()
        })
        .unwrap();
        let root_text = std::fs::canonicalize(&root).unwrap().display().to_string();
        let inside = format!("{root_text}/new-file");
        assert!(service.check(&inside, Use::Content, true).is_ok(), "missing files are fine");
        let escape = format!("{root_text}/../secret");
        assert!(service.check(&escape, Use::Content, false).is_err());
        let link = format!("{root_text}/link");
        assert!(service.check(&link, Use::Entry, true).is_ok(), "the link itself is inside");
        assert!(service.check(&link, Use::Content, false).is_err(), "its target is not");
        assert!(service.check("relative", Use::Entry, false).is_err());
    }

    #[test]
    fn read_only_refuses_changes() {
        let service =
            AgentService::new(Limits { roots: vec![], read_only: true, ..Default::default() })
                .unwrap();
        let error = service.check("/tmp/x", Use::Entry, true).unwrap_err();
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(service.check("/tmp/x", Use::Entry, false).is_ok());
    }
}
