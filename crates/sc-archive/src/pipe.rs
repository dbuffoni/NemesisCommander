//! Bridges between blocking `std::io` code and async streams.

use std::io::{self, Read, Write};

use bytes::Bytes;
use sc_vfs::ReadStream;
use tokio::sync::mpsc;

/// Chunks buffered between a blocking producer and an async consumer.
const DEPTH: usize = 8;
const CHUNK: usize = 256 * 1024;

/// A `std::io::Write` that forwards chunks to an async receiver.
pub(crate) struct ChannelWriter {
    tx: mpsc::Sender<io::Result<Bytes>>,
    buffer: Vec<u8>,
}

impl ChannelWriter {
    fn send(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let chunk = Bytes::from(std::mem::replace(&mut self.buffer, Vec::with_capacity(CHUNK)));
        self.tx
            .blocking_send(Ok(chunk))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "reader went away"))
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        if self.buffer.len() >= CHUNK {
            self.send()?;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.send()
    }
}

/// Runs `produce` on a blocking thread and returns its output as an async
/// stream. An error returned by `produce` surfaces as a read error.
pub(crate) fn stream_from_blocking<F>(produce: F) -> ReadStream
where
    F: FnOnce(&mut ChannelWriter) -> io::Result<()> + Send + 'static,
{
    let (tx, rx) = mpsc::channel(DEPTH);
    tokio::task::spawn_blocking(move || {
        let mut writer = ChannelWriter { tx: tx.clone(), buffer: Vec::with_capacity(CHUNK) };
        let result = produce(&mut writer).and_then(|()| writer.flush());
        if let Err(e) = result {
            // BrokenPipe means the consumer stopped reading on purpose.
            if e.kind() != io::ErrorKind::BrokenPipe {
                let _ = tx.blocking_send(Err(e));
            }
        }
    });
    let stream =
        futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|item| (item, rx)) });
    Box::pin(tokio_util::io::StreamReader::new(stream))
}

/// A `std::io::Read` fed by an async producer through a channel.
pub(crate) struct ChannelReader {
    rx: mpsc::Receiver<Bytes>,
    current: Bytes,
}

impl ChannelReader {
    pub fn new(rx: mpsc::Receiver<Bytes>) -> Self {
        ChannelReader { rx, current: Bytes::new() }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.current.is_empty() {
            match self.rx.blocking_recv() {
                Some(chunk) => self.current = chunk,
                None => return Ok(0), // producer finished
            }
        }
        let n = buf.len().min(self.current.len());
        buf[..n].copy_from_slice(&self.current[..n]);
        self.current = self.current.slice(n..);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn blocking_producer_to_async_reader() {
        let mut stream = stream_from_blocking(|w| {
            for i in 0..1000u32 {
                w.write_all(&i.to_le_bytes())?;
            }
            Ok(())
        });
        let mut out = Vec::new();
        stream.read_to_end(&mut out).await.unwrap();
        assert_eq!(out.len(), 4000);
        assert_eq!(&out[4..8], &1u32.to_le_bytes());
    }

    #[tokio::test]
    async fn producer_errors_become_read_errors() {
        let mut stream = stream_from_blocking(|w| {
            w.write_all(b"partial")?;
            w.flush()?;
            Err(io::Error::other("corrupt archive"))
        });
        let mut out = Vec::new();
        let err = stream.read_to_end(&mut out).await.unwrap_err();
        assert_eq!(err.to_string(), "corrupt archive");
    }

    #[tokio::test]
    async fn async_producer_to_blocking_reader() {
        let (tx, rx) = mpsc::channel(2);
        let reader = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            ChannelReader::new(rx).read_to_end(&mut out).unwrap();
            out
        });
        for chunk in [&b"ab"[..], b"", b"cde"] {
            tx.send(Bytes::copy_from_slice(chunk)).await.unwrap();
        }
        drop(tx);
        assert_eq!(reader.await.unwrap(), b"abcde");
    }
}
