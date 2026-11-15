//! A [`WriteSink`] for providers that upload a file in one request (HTTP
//! PUT, an object store's multipart upload, ...).
//!
//! [`upload_sink`] starts the request with a stream of the written chunks
//! as its body. Writes wait while the request is slow (back-pressure), and
//! `shutdown()` returns the request's result, so a rejected upload is
//! reported to whoever wrote the data.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::{Bytes, BytesMut};
use futures::Stream;
use tokio::io::AsyncWrite;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::PollSender;

use crate::{Result, WriteSink};

/// Bytes collected before a chunk is handed to the request.
pub const UPLOAD_CHUNK: usize = 256 * 1024;

/// The written data, chunk by chunk, as seen by the request.
pub type UploadBody = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>;

/// Starts `request` with the body stream and returns the sink that feeds
/// it. The request ends when the sink is shut down (the stream then ends)
/// and is abandoned if the sink is dropped first.
pub fn upload_sink<F, Fut>(request: F) -> WriteSink
where
    F: FnOnce(UploadBody) -> Fut,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    // A few chunks in flight keep the connection busy without buffering
    // a large file in memory.
    let (sender, mut receiver) = mpsc::channel::<Bytes>(4);
    let body: UploadBody =
        Box::pin(futures::stream::poll_fn(move |cx| receiver.poll_recv(cx).map(|c| c.map(Ok))));
    let call = tokio::spawn(request(body));
    Box::pin(UploadSink {
        sender: Some(PollSender::new(sender)),
        buffer: BytesMut::new(),
        call: Some(call),
        failure: None,
    })
}

struct UploadSink {
    sender: Option<PollSender<Bytes>>,
    buffer: BytesMut,
    call: Option<JoinHandle<Result<()>>>,
    /// Why the request failed, repeated to every later call.
    failure: Option<(io::ErrorKind, String)>,
}

impl UploadSink {
    /// Hands the buffered bytes, if any, to the request.
    fn poll_send_buffer(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.buffer.is_empty() {
            return Poll::Ready(Ok(()));
        }
        let Some(sender) = self.sender.as_mut() else {
            return self.poll_finish(cx);
        };
        if ready!(sender.poll_reserve(cx)).is_err() {
            // The request ended early; finishing it reports why.
            return self.poll_finish(cx);
        }
        let chunk = self.buffer.split().freeze();
        if sender.send_item(chunk).is_err() {
            return self.poll_finish(cx);
        }
        Poll::Ready(Ok(()))
    }

    /// Ends the body and waits for the request's result.
    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.sender = None;
        if let Some(call) = self.call.as_mut() {
            let result = ready!(Pin::new(call).poll(cx));
            self.call = None;
            let failure = match result {
                Ok(Ok(())) if self.buffer.is_empty() => None,
                Ok(Ok(())) => Some(io::Error::other("the upload ended before all data was sent")),
                Ok(Err(error)) => Some(error.into_io()),
                Err(join) => Some(io::Error::other(join)),
            };
            self.failure = failure.map(|e| (e.kind(), e.to_string()));
        }
        Poll::Ready(match &self.failure {
            None => Ok(()),
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
        })
    }
}

impl AsyncWrite for UploadSink {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.buffer.len() >= UPLOAD_CHUNK {
            ready!(self.poll_send_buffer(cx))?;
        }
        let taken = buf.len().min(UPLOAD_CHUNK - self.buffer.len());
        self.buffer.extend_from_slice(&buf[..taken]);
        Poll::Ready(Ok(taken))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_send_buffer(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.poll_send_buffer(cx))?;
        self.poll_finish(cx)
    }
}

impl Drop for UploadSink {
    fn drop(&mut self) {
        // Dropped without shutdown: abandon the upload.
        if let Some(call) = self.call.take() {
            call.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{VfsError, VfsPath};
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn the_request_receives_everything_written() {
        let (done, received) = oneshot::channel();
        let mut sink = upload_sink(|mut body| async move {
            let mut all = Vec::new();
            while let Some(chunk) = body.next().await {
                all.extend_from_slice(&chunk.unwrap());
            }
            done.send(all).unwrap();
            Ok(())
        });
        let data: Vec<u8> = (0..1_000_000u32).map(|i| i as u8).collect();
        sink.write_all(&data).await.unwrap();
        sink.shutdown().await.unwrap();
        assert_eq!(received.await.unwrap(), data);
    }

    #[tokio::test]
    async fn a_rejected_upload_fails_with_the_request_error() {
        let mut sink = upload_sink(|_body| async {
            Err(VfsError::AlreadyExists(VfsPath::parse("/x").unwrap()))
        });
        // Writing may or may not notice before shutdown; shutdown must.
        let _ = sink.write_all(&[0u8; 2 * UPLOAD_CHUNK]).await;
        let error = sink.shutdown().await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
}
