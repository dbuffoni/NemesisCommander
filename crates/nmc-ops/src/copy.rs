//! Copying a single file between any two providers.
//!
//! [`copy_file`] picks the cheapest strategy the providers allow:
//!
//! 1. **Server-side copy**: both paths on the same provider instance with
//!    [`Capabilities::SERVER_COPY`] (for the local disk: reflink or
//!    `copy_file_range`). No bytes pass through NemesisCommander.
//! 2. **Streaming pipeline**: a reader stage and a writer stage connected
//!    by a bounded channel of [`Bytes`] chunks. The bound gives
//!    back-pressure, so memory stays constant even for huge files, and
//!    reading overlaps with writing (important for network providers).
//!
//! While streaming, data goes to a temporary `.<name>.nmc-part` file when the
//! destination can rename atomically, so a half-written file never appears
//! under the real name. If the copy is interrupted, the part file is kept and
//! a later attempt with `resume` continues from where it stopped.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use nmc_hash::Hasher;
use nmc_vfs::{
    Capabilities, HashAlgorithm, Provider, Result, VfsError, VfsPath, WriteMode, WriteOptions,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Chunk size of the streaming pipeline.
pub const CHUNK_SIZE: usize = 256 * 1024;

/// Number of chunks that may be in flight between reader and writer.
const PIPELINE_DEPTH: usize = 8;

/// Limits throughput, for example to respect a bandwidth cap.
/// `nmc-transfer` provides a token-bucket implementation.
#[async_trait]
pub trait Throttle: Send + Sync {
    /// Waits until `bytes` may be sent.
    async fn acquire(&self, bytes: usize);

    /// Whether a limit is set now. A copy done by the disk or the server
    /// itself would ignore it, so a limited copy streams instead.
    fn limited(&self) -> bool {
        false
    }
}

/// Options for [`copy_file`].
#[derive(Clone, Default)]
pub struct CopyFileOptions {
    /// Replace the destination if it exists. When `false` the copy fails with
    /// [`VfsError::AlreadyExists`] instead.
    pub overwrite: bool,
    /// Continue a previous interrupted attempt if a part file exists.
    pub resume: bool,
    /// Compute a checksum of the source while copying (for verification).
    pub hash: Option<HashAlgorithm>,
    /// Never use a server-side copy, always stream (for benchmarks and tests).
    pub force_streaming: bool,
    /// Called with the number of bytes written after each chunk.
    pub progress: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    /// Optional bandwidth limiter.
    pub throttle: Option<Arc<dyn Throttle>>,
    /// Cancels the copy at the next chunk boundary.
    pub cancel: CancellationToken,
}

/// What [`copy_file`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CopyOutcome {
    /// Bytes written in this attempt (excludes resumed bytes).
    pub bytes_written: u64,
    /// Bytes skipped because they were already present in a part file.
    pub bytes_resumed: u64,
    /// Whether the provider copied the file itself.
    pub server_side: bool,
    /// Hex checksum of the *source* data, if [`CopyFileOptions::hash`] was set
    /// and the copy was streamed from the start.
    pub source_hash: Option<String>,
}

/// The name of the temporary file used while streaming into `destination`.
pub fn part_path(destination: &VfsPath) -> VfsPath {
    let name = destination.file_name().unwrap_or("file");
    destination.parent().unwrap_or_else(|| destination.clone()).join(&format!(".{name}.nmc-part"))
}

/// Copies one regular file. See the [module documentation](self).
pub async fn copy_file(
    source_provider: &dyn Provider,
    source: &VfsPath,
    destination_provider: &dyn Provider,
    destination: &VfsPath,
    options: &CopyFileOptions,
) -> Result<CopyOutcome> {
    if options.cancel.is_cancelled() {
        return Err(VfsError::Cancelled);
    }
    let dst_caps = destination_provider.capabilities();
    if dst_caps.is_read_only() {
        return Err(VfsError::unsupported("write", destination_provider.scheme()));
    }
    if !options.overwrite && destination_exists(destination_provider, destination).await? {
        return Err(VfsError::AlreadyExists(destination.clone()));
    }

    let same_provider = source.provider_key() == destination.provider_key();
    if same_provider
        && dst_caps.contains(Capabilities::SERVER_COPY)
        && !options.force_streaming
        && options.hash.is_none()
        && !options.throttle.as_ref().is_some_and(|t| t.limited())
    {
        let size = source_provider.stat(source).await?.size;
        destination_provider.copy_within(source, destination).await?;
        if let Some(progress) = &options.progress {
            progress(size);
        }
        return Ok(CopyOutcome { bytes_written: size, server_side: true, ..Default::default() });
    }

    stream_copy(source_provider, source, destination_provider, destination, options).await
}

async fn destination_exists(provider: &dyn Provider, path: &VfsPath) -> Result<bool> {
    match provider.stat(path).await {
        Ok(_) => Ok(true),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(e),
    }
}

async fn stream_copy(
    source_provider: &dyn Provider,
    source: &VfsPath,
    destination_provider: &dyn Provider,
    destination: &VfsPath,
    options: &CopyFileOptions,
) -> Result<CopyOutcome> {
    let src_caps = source_provider.capabilities();
    let dst_caps = destination_provider.capabilities();
    let use_part_file = dst_caps.contains(Capabilities::ATOMIC_RENAME);
    let target = if use_part_file { part_path(destination) } else { destination.clone() };

    // Resume only when both sides can do it and a part file is there.
    let mut offset = 0;
    if options.resume
        && use_part_file
        && dst_caps.contains(Capabilities::RESUME_WRITE)
        && src_caps.contains(Capabilities::SEEK_READ)
        && let Ok(part) = destination_provider.stat(&target).await
    {
        let source_size = source_provider.stat(source).await?.size;
        if part.size <= source_size {
            offset = part.size;
        }
    }

    let mode = if offset > 0 {
        WriteMode::ResumeAt(offset)
    } else if options.overwrite || use_part_file {
        // A stale part file from an earlier attempt is simply replaced.
        WriteMode::Truncate
    } else {
        WriteMode::CreateNew
    };
    let mut reader = source_provider.read(source, offset).await?;
    let mut writer =
        destination_provider.write(&target, WriteOptions { mode, size_hint: None }).await?;

    // A hash of the source is only meaningful if we saw every byte.
    let mut hasher = options.hash.filter(|_| offset == 0).map(Hasher::new);
    let (tx, mut rx) = mpsc::channel::<std::io::Result<Bytes>>(PIPELINE_DEPTH);

    let read_stage = async move {
        loop {
            let mut buf = BytesMut::zeroed(CHUNK_SIZE);
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    buf.truncate(n);
                    if tx.send(Ok(buf.freeze())).await.is_err() {
                        break; // writer gave up
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
    };

    let write_stage = async {
        let mut written = 0u64;
        while let Some(chunk) = rx.recv().await {
            if options.cancel.is_cancelled() {
                return Err(VfsError::Cancelled);
            }
            let chunk = chunk.map_err(|e| VfsError::from_io(source, e))?;
            if let Some(throttle) = &options.throttle {
                throttle.acquire(chunk.len()).await;
            }
            writer.write_all(&chunk).await.map_err(|e| VfsError::from_io(&target, e))?;
            if let Some(h) = hasher.as_mut() {
                h.update(&chunk);
            }
            written += chunk.len() as u64;
            if let Some(progress) = &options.progress {
                progress(chunk.len() as u64);
            }
        }
        writer.shutdown().await.map_err(|e| VfsError::from_io(&target, e))?;
        Ok(written)
    };

    let ((), written) = tokio::join!(read_stage, write_stage);
    let written = written?;

    if use_part_file {
        destination_provider.rename(&target, destination).await?;
    }
    Ok(CopyOutcome {
        bytes_written: written,
        bytes_resumed: offset,
        server_side: false,
        source_hash: hasher.map(Hasher::finish_hex),
    })
}

/// How thoroughly a finished copy is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifyMode {
    /// No check.
    None,
    /// Sizes must match (cheap, catches truncation).
    #[default]
    Size,
    /// Full content comparison through a fast non-cryptographic hash (XXH3).
    Fast,
    /// Full content comparison through BLAKE3.
    Full,
}

impl VerifyMode {
    /// The hash algorithm this mode needs, if any.
    pub fn algorithm(self) -> Option<HashAlgorithm> {
        match self {
            VerifyMode::Fast => Some(HashAlgorithm::Xxh3),
            VerifyMode::Full => Some(HashAlgorithm::Blake3),
            VerifyMode::None | VerifyMode::Size => None,
        }
    }
}

/// Computes a checksum on the provider if it can, otherwise by streaming.
pub async fn checksum(
    provider: &dyn Provider,
    path: &VfsPath,
    algorithm: HashAlgorithm,
) -> Result<String> {
    if provider.capabilities().contains(Capabilities::SERVER_HASH) {
        match provider.checksum(path, algorithm).await {
            Err(VfsError::Unsupported { .. }) => {}
            other => return other,
        }
    }
    let mut reader = provider.read(path, 0).await?;
    nmc_hash::hash_reader(&mut reader, algorithm).await.map_err(|e| VfsError::from_io(path, e))
}

/// Checks that `destination` matches `source` according to `mode`.
///
/// `known_source_hash` avoids re-reading the source when the copy already
/// hashed it on the fly. Returns `Ok(false)` on a mismatch.
pub async fn verify_copy(
    source_provider: &dyn Provider,
    source: &VfsPath,
    destination_provider: &dyn Provider,
    destination: &VfsPath,
    mode: VerifyMode,
    known_source_hash: Option<&str>,
) -> Result<bool> {
    if mode == VerifyMode::None {
        return Ok(true);
    }
    let (src, dst) =
        (source_provider.stat(source).await?, destination_provider.stat(destination).await?);
    if src.size != dst.size {
        return Ok(false);
    }
    let Some(algorithm) = mode.algorithm() else {
        return Ok(true);
    };
    let source_hash = match known_source_hash {
        Some(h) => h.to_owned(),
        None => checksum(source_provider, source, algorithm).await?,
    };
    Ok(checksum(destination_provider, destination, algorithm).await? == source_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmc_providers::local::LocalProvider;
    use nmc_providers::memory::MemoryProvider;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn mem(path: &str) -> VfsPath {
        VfsPath::parse(&format!("mem://a{path}")).unwrap()
    }
    fn mem_b(path: &str) -> VfsPath {
        VfsPath::parse(&format!("mem://b{path}")).unwrap()
    }

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    #[tokio::test]
    async fn streams_between_providers_with_progress() {
        let (a, b) = (MemoryProvider::new(), MemoryProvider::new());
        a.put_file("/big", data(3 * CHUNK_SIZE + 5));
        let seen = Arc::new(AtomicU64::new(0));
        let seen2 = seen.clone();
        let options = CopyFileOptions {
            progress: Some(Arc::new(move |n| {
                seen2.fetch_add(n, Ordering::SeqCst);
            })),
            hash: Some(HashAlgorithm::Blake3),
            ..Default::default()
        };
        let out = copy_file(&a, &mem("/big"), &b, &mem_b("/copy"), &options).await.unwrap();
        assert_eq!(out.bytes_written, (3 * CHUNK_SIZE + 5) as u64);
        assert!(!out.server_side);
        assert_eq!(seen.load(Ordering::SeqCst), out.bytes_written);
        assert_eq!(b.get_file("/copy").unwrap(), a.get_file("/big").unwrap());
        // the part file was renamed away
        assert_eq!(b.paths(), ["/", "/copy"]);
        // verification agrees, using the hash computed on the fly
        let ok = verify_copy(
            &a,
            &mem("/big"),
            &b,
            &mem_b("/copy"),
            VerifyMode::Full,
            out.source_hash.as_deref(),
        )
        .await
        .unwrap();
        assert!(ok);
    }

    #[tokio::test]
    async fn refuses_to_overwrite_unless_asked() {
        let (a, b) = (MemoryProvider::new(), MemoryProvider::new());
        a.put_file("/f", "new");
        b.put_file("/f", "old");
        let err = copy_file(&a, &mem("/f"), &b, &mem_b("/f"), &CopyFileOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, VfsError::AlreadyExists(_)));
        let options = CopyFileOptions { overwrite: true, ..Default::default() };
        copy_file(&a, &mem("/f"), &b, &mem_b("/f"), &options).await.unwrap();
        assert_eq!(b.get_file("/f").unwrap(), b"new");
    }

    #[tokio::test]
    async fn resumes_from_part_file() {
        let (a, b) = (MemoryProvider::new(), MemoryProvider::new());
        let content = data(1000);
        a.put_file("/f", content.clone());
        b.put_file("/.f.nmc-part", content[..400].to_vec());
        let options = CopyFileOptions { resume: true, ..Default::default() };
        let out = copy_file(&a, &mem("/f"), &b, &mem_b("/f"), &options).await.unwrap();
        assert_eq!(out.bytes_resumed, 400);
        assert_eq!(out.bytes_written, 600);
        assert_eq!(out.source_hash, None);
        assert_eq!(b.get_file("/f").unwrap(), content);
    }

    #[tokio::test]
    async fn without_atomic_rename_writes_in_place() {
        let a = MemoryProvider::new();
        let b = MemoryProvider::with_capabilities(Capabilities::WRITE);
        a.put_file("/f", "abc");
        copy_file(&a, &mem("/f"), &b, &mem_b("/f"), &CopyFileOptions::default()).await.unwrap();
        assert_eq!(b.paths(), ["/", "/f"]);
    }

    #[tokio::test]
    async fn read_only_destination_is_rejected() {
        let a = MemoryProvider::new();
        let b = MemoryProvider::with_capabilities(Capabilities::SEEK_READ);
        a.put_file("/f", "abc");
        let err = copy_file(&a, &mem("/f"), &b, &mem_b("/f"), &CopyFileOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, VfsError::Unsupported { .. }));
    }

    #[tokio::test]
    async fn cancellation_keeps_part_file() {
        let (a, b) = (MemoryProvider::new(), MemoryProvider::new());
        a.put_file("/f", data(10 * CHUNK_SIZE));
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let options = CopyFileOptions {
            progress: Some(Arc::new(move |_| c2.cancel())),
            cancel,
            ..Default::default()
        };
        let err = copy_file(&a, &mem("/f"), &b, &mem_b("/f"), &options).await.unwrap_err();
        assert!(matches!(err, VfsError::Cancelled));
        assert!(b.get_file("/f").is_none());
        let part = b.get_file("/.f.nmc-part").unwrap();
        assert!(!part.is_empty() && part.len() < 10 * CHUNK_SIZE);
    }

    #[tokio::test]
    async fn local_same_provider_uses_server_copy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), data(5000)).unwrap();
        let p = LocalProvider::new();
        let a = VfsPath::from_local(dir.path().join("a")).unwrap();
        let b = VfsPath::from_local(dir.path().join("b")).unwrap();
        let out = copy_file(&p, &a, &p, &b, &CopyFileOptions::default()).await.unwrap();
        assert!(out.server_side);
        assert_eq!(std::fs::read(dir.path().join("b")).unwrap(), data(5000));
        let ok = verify_copy(&p, &a, &p, &b, VerifyMode::Fast, None).await.unwrap();
        assert!(ok);
    }

    struct Limit;

    #[async_trait]
    impl Throttle for Limit {
        async fn acquire(&self, _bytes: usize) {}
        fn limited(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn a_bandwidth_limit_makes_a_local_copy_stream() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), data(5000)).unwrap();
        let p = LocalProvider::new();
        let a = VfsPath::from_local(dir.path().join("a")).unwrap();
        let b = VfsPath::from_local(dir.path().join("b")).unwrap();
        let options = CopyFileOptions { throttle: Some(Arc::new(Limit)), ..Default::default() };
        let out = copy_file(&p, &a, &p, &b, &options).await.unwrap();
        assert!(!out.server_side, "the limit must apply, so the data goes through");
        assert_eq!(std::fs::read(dir.path().join("b")).unwrap(), data(5000));
    }

    #[tokio::test]
    async fn verify_detects_differences() {
        let (a, b) = (MemoryProvider::new(), MemoryProvider::new());
        a.put_file("/f", "abcd");
        b.put_file("/f", "abce");
        let (src, dst) = (mem("/f"), mem_b("/f"));
        assert!(verify_copy(&a, &src, &b, &dst, VerifyMode::Size, None).await.unwrap());
        assert!(!verify_copy(&a, &src, &b, &dst, VerifyMode::Fast, None).await.unwrap());
        assert!(!verify_copy(&a, &src, &b, &dst, VerifyMode::Full, None).await.unwrap());
        b.put_file("/f", "abc");
        assert!(!verify_copy(&a, &src, &b, &dst, VerifyMode::Size, None).await.unwrap());
    }
}
