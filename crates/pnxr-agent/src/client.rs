//! The controller side: an `agent://` (or `scs://`) [`Provider`] that talks
//! to a remote `pnxr agent` or `pnxr serve` over any byte stream.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use pnxr_proto::convert::{entry_from_pb, status_to_error, time_to_ns};
use pnxr_proto::pb::{self, agent_client::AgentClient, write_request::Part};
use pnxr_vfs::{
    Capabilities, Entry, HashAlgorithm, ListStream, Provider, ReadStream, Result, SetAttributes,
    VfsError, VfsPath, WriteMode, WriteOptions, WriteSink,
};
use tokio::io::AsyncWrite;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::io::StreamReader;
use tokio_util::sync::PollSender;
use tonic::transport::Channel;

/// Size of the data messages sent by writes.
pub const WRITE_CHUNK: usize = 256 * 1024;

/// A connected agent. Paths are `agent://<authority>/<absolute path>`
/// (started over SSH) or `scs://<host>:<port>/<absolute path>` (`pnxr serve`).
pub struct AgentProvider {
    client: AgentClient<Channel>,
    scheme: &'static str,
    hello: pb::HelloReply,
    /// Keeps the transport (for example the `ssh` process) alive.
    _keep_alive: Option<Box<dyn Send + Sync>>,
}

impl AgentProvider {
    /// Performs the handshake on an established connection.
    ///
    /// `keep_alive` is dropped together with the provider (pass the child
    /// process that carries the connection).
    pub async fn handshake(
        client: AgentClient<Channel>,
        keep_alive: Option<Box<dyn Send + Sync>>,
    ) -> std::result::Result<Self, String> {
        Self::handshake_with(client, "agent", None, keep_alive).await
    }

    /// Performs the handshake for paths of `scheme`, presenting `token` when
    /// this is a server contacting another one for a server-to-server copy.
    pub async fn handshake_with(
        mut client: AgentClient<Channel>,
        scheme: &'static str,
        token: Option<&str>,
        keep_alive: Option<Box<dyn Send + Sync>>,
    ) -> std::result::Result<Self, String> {
        let request = pb::HelloRequest {
            protocol: pnxr_proto::PROTOCOL_VERSION.into(),
            client_version: crate::VERSION.into(),
            token: token.unwrap_or_default().into(),
        };
        let hello = client.hello(request).await.map_err(|s| s.message().to_owned())?.into_inner();
        if !pnxr_proto::compatible(pnxr_proto::PROTOCOL_VERSION, &hello.protocol) {
            return Err(format!(
                "the agent speaks protocol {}, this program {}",
                hello.protocol,
                pnxr_proto::PROTOCOL_VERSION
            ));
        }
        Ok(AgentProvider { client, scheme, hello, _keep_alive: keep_alive })
    }

    /// The protocol client, for calls beyond the [`Provider`] interface
    /// (server-to-server transfers).
    pub fn raw_client(&self) -> AgentClient<Channel> {
        self.client.clone()
    }

    /// Keeps `value` (for example the process carrying the connection) alive
    /// as long as the provider.
    pub fn with_keep_alive(mut self, value: Box<dyn Send + Sync>) -> Self {
        self._keep_alive = Some(value);
        self
    }

    /// The agent's answer to the handshake (version, roots, read-only).
    pub fn info(&self) -> &pb::HelloReply {
        &self.hello
    }

    fn client(&self) -> AgentClient<Channel> {
        // Clients are cheap handles on the same connection.
        self.client.clone()
    }
}

impl std::fmt::Debug for AgentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentProvider")
            .field("scheme", &self.scheme)
            .field("agent_version", &self.hello.agent_version)
            .finish_non_exhaustive()
    }
}

/// The remote path of `path` (the agent only sees absolute paths).
fn remote(path: &VfsPath) -> String {
    path.path().to_owned()
}

fn request(path: &VfsPath) -> pb::PathRequest {
    pb::PathRequest { path: remote(path) }
}

#[async_trait]
impl Provider for AgentProvider {
    fn scheme(&self) -> &str {
        self.scheme
    }

    fn settings_dir(&self, on: &VfsPath) -> Option<VfsPath> {
        let dir = &self.hello.settings_dir;
        (!dir.is_empty()).then(|| on.with_path(dir))
    }

    fn capabilities(&self) -> Capabilities {
        let read = Capabilities::SEEK_READ | Capabilities::SERVER_HASH | Capabilities::SYMLINKS;
        if self.hello.read_only {
            return read;
        }
        read | Capabilities::WRITE
            | Capabilities::RESUME_WRITE
            | Capabilities::ATOMIC_RENAME
            | Capabilities::POSIX_PERMISSIONS
            | Capabilities::SET_MTIME
            | Capabilities::SERVER_COPY
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        let mut entry = self
            .client()
            .stat(request(path))
            .await
            .map(|r| entry_from_pb(r.into_inner()))
            .map_err(|s| status_to_error(&s, path))?;
        if path.is_root() {
            entry.name = "/".into();
        }
        Ok(entry)
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let pages = self
            .client()
            .list(request(path))
            .await
            .map_err(|s| status_to_error(&s, path))?
            .into_inner();
        let path = path.clone();
        Ok(Box::pin(pages.map(move |page| match page {
            Ok(page) => Ok(page.entries.into_iter().map(entry_from_pb).collect()),
            Err(s) => Err(status_to_error(&s, &path)),
        })))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        let chunks = self
            .client()
            .read(pb::ReadRequest { path: remote(path), offset })
            .await
            .map_err(|s| status_to_error(&s, path))?
            .into_inner();
        let path = path.clone();
        let bytes = chunks.map(move |chunk| match chunk {
            Ok(chunk) => Ok(chunk.data),
            Err(s) => Err(io::Error::other(status_to_error(&s, &path))),
        });
        Ok(Box::pin(StreamReader::new(bytes)))
    }

    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        let (mode, offset) = match options.mode {
            WriteMode::CreateNew => (pb::WriteMode::CreateNew, 0),
            WriteMode::Truncate => (pb::WriteMode::Truncate, 0),
            WriteMode::ResumeAt(offset) => (pb::WriteMode::Resume, offset),
        };
        let header = pb::WriteHeader {
            path: remote(path),
            mode: mode as i32,
            offset,
            size_hint: options.size_hint,
        };
        // A few chunks in flight keep the link busy without buffering much.
        let (sender, receiver) = mpsc::channel::<pb::WriteRequest>(4);
        sender
            .send(pb::WriteRequest { part: Some(Part::Header(header)) })
            .await
            .map_err(|_| VfsError::Other("write channel closed".into()))?;
        let mut client = self.client();
        let error_path = path.clone();
        let call = tokio::spawn(async move {
            let stream = tokio_stream_from(receiver);
            client
                .write(stream)
                .await
                .map(|_| ())
                .map_err(|s| io::Error::other(status_to_error(&s, &error_path)))
        });
        Ok(Box::pin(AgentWriter {
            sender: Some(PollSender::new(sender)),
            buffer: BytesMut::with_capacity(WRITE_CHUNK),
            call: Some(call),
        }))
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        self.client().create_dir(request(path)).await.map_err(|s| status_to_error(&s, path))?;
        Ok(())
    }

    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        self.client().remove_file(request(path)).await.map_err(|s| status_to_error(&s, path))?;
        Ok(())
    }

    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        self.client().remove_dir(request(path)).await.map_err(|s| status_to_error(&s, path))?;
        Ok(())
    }

    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let paths = pb::TwoPaths { from: remote(from), to: remote(to) };
        self.client().rename(paths).await.map_err(|s| status_to_error(&s, from))?;
        Ok(())
    }

    async fn set_attributes(&self, path: &VfsPath, attributes: &SetAttributes) -> Result<()> {
        let request = pb::SetAttributesRequest {
            path: remote(path),
            permissions: attributes.permissions,
            modified_ns: attributes.modified.map(time_to_ns),
        };
        self.client().set_attributes(request).await.map_err(|s| status_to_error(&s, path))?;
        Ok(())
    }

    async fn copy_within(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let paths = pb::TwoPaths { from: remote(from), to: remote(to) };
        self.client().copy_within(paths).await.map_err(|s| status_to_error(&s, to))?;
        Ok(())
    }

    async fn checksum(&self, path: &VfsPath, algorithm: HashAlgorithm) -> Result<String> {
        let request = pb::ChecksumRequest {
            path: remote(path),
            algorithm: pnxr_hash::algorithm_name(algorithm).into(),
        };
        let reply = self.client().checksum(request).await.map_err(|s| status_to_error(&s, path))?;
        Ok(reply.into_inner().hex)
    }

    async fn create_symlink(&self, path: &VfsPath, target: &str) -> Result<()> {
        let request = pb::SymlinkRequest { path: remote(path), target: target.into() };
        self.client().create_symlink(request).await.map_err(|s| status_to_error(&s, path))?;
        Ok(())
    }
}

/// Turns a channel receiver into a stream of messages.
fn tokio_stream_from<T: Send + 'static>(
    receiver: mpsc::Receiver<T>,
) -> impl futures::Stream<Item = T> + Send + 'static {
    futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    })
}

/// Sends written bytes as `Write` messages. `shutdown` ends the message
/// stream and waits for the agent to confirm the file was completed, so an
/// error on the remote side is reported to whoever wrote the data.
struct AgentWriter {
    sender: Option<PollSender<pb::WriteRequest>>,
    buffer: BytesMut,
    call: Option<JoinHandle<io::Result<()>>>,
}

impl AgentWriter {
    /// Sends the buffered bytes, if any.
    fn poll_send_buffer(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.buffer.is_empty() {
            return Poll::Ready(Ok(()));
        }
        let Some(sender) = self.sender.as_mut() else {
            return Poll::Ready(Err(io::Error::other("write already finished")));
        };
        if ready!(sender.poll_reserve(cx)).is_err() {
            // The call ended early: its result explains why.
            return Poll::Ready(Err(io::Error::other("the agent stopped receiving")));
        }
        let data: Bytes = self.buffer.split().freeze();
        sender
            .send_item(pb::WriteRequest { part: Some(Part::Data(data)) })
            .map_err(|_| io::Error::other("the agent stopped receiving"))?;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for AgentWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.buffer.len() >= WRITE_CHUNK {
            ready!(self.poll_send_buffer(cx))?;
        }
        let room = WRITE_CHUNK - self.buffer.len();
        let taken = buf.len().min(room);
        self.buffer.extend_from_slice(&buf[..taken]);
        Poll::Ready(Ok(taken))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_send_buffer(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_send_buffer(cx))?;
        // Closing the channel ends the request stream.
        self.sender = None;
        let Some(call) = self.call.as_mut() else { return Poll::Ready(Ok(())) };
        let result = ready!(Pin::new(call).poll(cx));
        self.call = None;
        Poll::Ready(match result {
            Ok(result) => result,
            Err(join) => Err(io::Error::other(join)),
        })
    }
}

impl Drop for AgentWriter {
    fn drop(&mut self) {
        // Dropped without shutdown: abandon the upload. The agent sees the
        // stream end; the caller treats the file as incomplete.
        if let Some(call) = self.call.take() {
            call.abort();
        }
    }
}
