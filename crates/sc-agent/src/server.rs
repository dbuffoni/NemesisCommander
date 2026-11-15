//! The agent side: serves the local file system of the machine it runs on,
//! within configured limits, to one controller.

use std::io;
use std::path::{Component, Path, PathBuf};

use bytes::Bytes;
use futures::StreamExt;
use sc_proto::convert::{entry_to_pb, error_to_status, ns_to_time};
use sc_proto::pb::{self, agent_server::Agent, write_request::Part};
use sc_providers::local::LocalProvider;
use sc_vfs::{Provider, SetAttributes, VfsError, VfsPath, WriteMode, WriteOptions};
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

/// The gRPC service of `sc agent`.
pub struct AgentService {
    local: LocalProvider,
    limits: Limits,
}

impl AgentService {
    /// A service enforcing `limits`. Roots are resolved now, so they must
    /// exist.
    pub fn new(mut limits: Limits) -> io::Result<Self> {
        for root in &mut limits.roots {
            *root = std::fs::canonicalize(&*root)?;
        }
        Ok(AgentService { local: LocalProvider::new(), limits })
    }

    /// Checks `path` against the limits and turns it into a local path.
    fn check(&self, path: &str, use_: Use, modifies: bool) -> Result<VfsPath, Status> {
        if modifies && self.limits.read_only {
            return Err(Status::permission_denied(format!("{path}: the agent is read-only")));
        }
        if !path.starts_with('/') {
            return Err(Status::invalid_argument(format!("{path}: paths must be absolute")));
        }
        let lexical = normalize(Path::new(path));
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
            protocol: sc_proto::PROTOCOL_VERSION.into(),
            agent_version: crate::VERSION.into(),
            roots: self.limits.roots.iter().map(|r| r.display().to_string()).collect(),
            read_only: self.limits.read_only,
            hash_algorithms: vec!["blake3".into(), "xxh3".into(), "sha256".into()],
        }
    }
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
    async fn hello(&self, _: Request<pb::HelloRequest>) -> Reply<pb::HelloReply> {
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
        };
        self.local.set_attributes(&path, &attributes).await.map_err(status)?;
        Ok(Response::new(pb::Empty {}))
    }

    async fn checksum(&self, request: Request<pb::ChecksumRequest>) -> Reply<pb::ChecksumReply> {
        let request = request.into_inner();
        let path = self.check(&request.path, Use::Content, false)?;
        let algorithm = sc_hash::parse_algorithm(&request.algorithm).ok_or_else(|| {
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
}

/// Serves the agent protocol on standard input/output until the controller
/// hangs up. This is `sc agent --stdio`.
pub async fn serve_stdio(limits: Limits) -> io::Result<()> {
    let service = AgentService::new(limits)?;
    let io = sc_proto::transport::Duplex::new(tokio::io::stdin(), tokio::io::stdout());
    sc_proto::transport::serve(service, io).await.map_err(io::Error::other)
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
        let service =
            AgentService::new(Limits { roots: vec![root.clone()], read_only: false }).unwrap();
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
        let service = AgentService::new(Limits { roots: vec![], read_only: true }).unwrap();
        let error = service.check("/tmp/x", Use::Entry, true).unwrap_err();
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(service.check("/tmp/x", Use::Entry, false).is_ok());
    }
}
