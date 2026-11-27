//! Running the protocol over a single byte stream.
//!
//! `pnxr agent --stdio` is started by SSH, so the agent has exactly one
//! connection: its standard input and output. HTTP/2 multiplexes every call
//! over that one stream. [`serve`] runs a service on such a stream and
//! returns when the other side hangs up; [`connect`] builds a client on the
//! controller's end (for example the stdin/stdout of the `ssh` child).

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::oneshot;
use tonic::transport::server::Connected;
use tonic::transport::{Channel, Endpoint, Server};

use crate::pb::agent_client::AgentClient;
use crate::pb::agent_server::{Agent, AgentServer};

/// Largest message accepted in either direction. Data chunks are much
/// smaller; this only bounds huge directory pages.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// Joins a reader and a writer into one bidirectional stream.
pub struct Duplex<R, W> {
    reader: R,
    writer: W,
}

impl<R, W> Duplex<R, W> {
    /// Combines `reader` and `writer`.
    pub fn new(reader: R, writer: W) -> Self {
        Duplex { reader, writer }
    }
}

impl<R: AsyncRead + Unpin, W: Unpin> AsyncRead for Duplex<R, W> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl<R: Unpin, W: AsyncWrite + Unpin> AsyncWrite for Duplex<R, W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

/// A stream that reports when the peer has closed it (read end of file or a
/// read error), so the server knows when to stop.
struct WatchClose<IO> {
    io: IO,
    closed: Option<oneshot::Sender<()>>,
}

impl<IO> WatchClose<IO> {
    fn close(&mut self) {
        if let Some(closed) = self.closed.take() {
            let _ = closed.send(());
        }
    }
}

impl<IO: AsyncRead + Unpin> AsyncRead for WatchClose<IO> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.io).poll_read(cx, buf);
        match &result {
            Poll::Ready(Ok(())) if buf.filled().len() == before && buf.remaining() > 0 => {
                self.close()
            }
            Poll::Ready(Err(_)) => self.close(),
            _ => {}
        }
        result
    }
}

impl<IO: AsyncWrite + Unpin> AsyncWrite for WatchClose<IO> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

impl<IO> Connected for WatchClose<IO> {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

/// Serves `agent` on `io` until the peer closes the stream.
pub async fn serve<A, IO>(agent: A, io: IO) -> Result<(), tonic::transport::Error>
where
    A: Agent,
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (closed, on_close) = oneshot::channel();
    let io = WatchClose { io, closed: Some(closed) };
    // Exactly one connection, and the server must not stop just because no
    // second one will ever arrive.
    let incoming = OnceThenPending { item: Some(io) };
    let service = AgentServer::new(agent)
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE);
    Server::builder()
        .add_service(service)
        .serve_with_incoming_shutdown(incoming, async {
            let _ = on_close.await;
        })
        .await
}

/// Yields one item, then stays pending forever (a "listener" with exactly
/// one connection).
struct OnceThenPending<T> {
    item: Option<T>,
}

impl<T> Unpin for OnceThenPending<T> {}

impl<T> tonic::codegen::tokio_stream::Stream for OnceThenPending<T> {
    type Item = io::Result<T>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.item.take() {
            Some(item) => Poll::Ready(Some(Ok(item))),
            None => Poll::Pending,
        }
    }
}

/// How often an idle connection is probed.
const KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);
/// How long an answer to a probe may take.
const KEEPALIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// A client talking to an agent over `io`.
///
/// The connection cannot be re-established: if it breaks, calls fail with
/// "unavailable" and the controller starts a new agent.
pub async fn connect<IO>(io: IO) -> Result<AgentClient<Channel>, tonic::transport::Error>
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let slot = Arc::new(Mutex::new(Some(io)));
    // HTTP/2 PING frames tell a silent dead link from an idle one: after
    // 20 s without an answer the connection is closed and calls fail.
    let channel = Endpoint::from_static("http://pnxr-agent")
        .http2_keep_alive_interval(KEEPALIVE_INTERVAL)
        .keep_alive_timeout(KEEPALIVE_TIMEOUT)
        .keep_alive_while_idle(true)
        .connect_with_connector(tower::service_fn(move |_: tonic::codegen::http::Uri| {
            let io = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            async move {
                let io = io.ok_or_else(|| io::Error::other("the agent connection was closed"))?;
                Ok::<_, io::Error>(TokioIo::new(io))
            }
        }))
        .await?;
    Ok(AgentClient::new(channel)
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pb;
    use tonic::{Request, Response, Status};

    /// An agent that only answers Hello.
    struct HelloOnly;

    #[tonic::async_trait]
    impl Agent for HelloOnly {
        async fn hello(
            &self,
            request: Request<pb::HelloRequest>,
        ) -> Result<Response<pb::HelloReply>, Status> {
            Ok(Response::new(pb::HelloReply {
                protocol: crate::PROTOCOL_VERSION.into(),
                agent_version: format!("echo {}", request.into_inner().client_version),
                ..Default::default()
            }))
        }
        async fn stat(&self, _: Request<pb::PathRequest>) -> Result<Response<pb::Entry>, Status> {
            Err(Status::unimplemented("stat"))
        }
        type ListStream = tonic::codegen::BoxStream<pb::EntryPage>;
        async fn list(
            &self,
            _: Request<pb::PathRequest>,
        ) -> Result<Response<Self::ListStream>, Status> {
            Err(Status::unimplemented("list"))
        }
        type ReadStream = tonic::codegen::BoxStream<pb::Chunk>;
        async fn read(
            &self,
            _: Request<pb::ReadRequest>,
        ) -> Result<Response<Self::ReadStream>, Status> {
            Err(Status::unimplemented("read"))
        }
        async fn write(
            &self,
            _: Request<tonic::Streaming<pb::WriteRequest>>,
        ) -> Result<Response<pb::WriteReply>, Status> {
            Err(Status::unimplemented("write"))
        }
        async fn create_dir(
            &self,
            _: Request<pb::PathRequest>,
        ) -> Result<Response<pb::Empty>, Status> {
            Err(Status::unimplemented(""))
        }
        async fn remove_file(
            &self,
            _: Request<pb::PathRequest>,
        ) -> Result<Response<pb::Empty>, Status> {
            Err(Status::unimplemented(""))
        }
        async fn remove_dir(
            &self,
            _: Request<pb::PathRequest>,
        ) -> Result<Response<pb::Empty>, Status> {
            Err(Status::unimplemented(""))
        }
        async fn rename(&self, _: Request<pb::TwoPaths>) -> Result<Response<pb::Empty>, Status> {
            Err(Status::unimplemented(""))
        }
        async fn copy_within(
            &self,
            _: Request<pb::TwoPaths>,
        ) -> Result<Response<pb::Empty>, Status> {
            Err(Status::unimplemented(""))
        }
        async fn set_attributes(
            &self,
            _: Request<pb::SetAttributesRequest>,
        ) -> Result<Response<pb::Empty>, Status> {
            Err(Status::unimplemented(""))
        }
        async fn checksum(
            &self,
            _: Request<pb::ChecksumRequest>,
        ) -> Result<Response<pb::ChecksumReply>, Status> {
            Err(Status::unimplemented(""))
        }
        async fn create_symlink(
            &self,
            _: Request<pb::SymlinkRequest>,
        ) -> Result<Response<pb::Empty>, Status> {
            Err(Status::unimplemented(""))
        }
        type TransferStream = tonic::codegen::BoxStream<pb::TransferProgress>;
        async fn transfer(
            &self,
            _: Request<pb::TransferRequest>,
        ) -> Result<Response<Self::TransferStream>, Status> {
            Err(Status::unimplemented(""))
        }
    }

    #[tokio::test]
    async fn client_and_server_talk_over_one_stream_and_stop_on_hang_up() {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(serve(HelloOnly, server_side));
        let mut client = connect(client_side).await.unwrap();
        let reply = client
            .hello(pb::HelloRequest {
                protocol: "1.0".into(),
                client_version: "t".into(),
                token: String::new(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(reply.agent_version, "echo t");
        let error = client.stat(pb::PathRequest { path: "/".into() }).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::Unimplemented);

        drop(client);
        let finished = tokio::time::timeout(std::time::Duration::from_secs(10), server).await;
        assert!(finished.is_ok(), "the server stops when the controller hangs up");
    }
}
