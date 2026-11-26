//! The [`Provider`] trait implemented by every backend.

use std::pin::Pin;
use std::time::SystemTime;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{Capabilities, Entry, Result, VfsError, VfsPath};

/// A stream of file contents.
pub type ReadStream = Pin<Box<dyn AsyncRead + Send>>;

/// A sink for file contents. Callers must `shutdown()` it to make sure all
/// data is flushed and the file is finalized.
pub type WriteSink = Pin<Box<dyn AsyncWrite + Send>>;

/// A directory listing, delivered in pages so a UI can draw the first rows
/// of a huge directory while the rest is still arriving.
pub type ListStream = Pin<Box<dyn Stream<Item = Result<Vec<Entry>>> + Send>>;

/// How [`Provider::write`] treats an existing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteMode {
    /// Fail with [`VfsError::AlreadyExists`] if the file exists.
    CreateNew,
    /// Replace any existing content.
    Truncate,
    /// Keep the first `n` bytes and continue writing from there. Only valid
    /// when the provider has [`Capabilities::RESUME_WRITE`].
    ResumeAt(u64),
}

/// Options for [`Provider::write`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteOptions {
    /// What to do with an existing file.
    pub mode: WriteMode,
    /// Expected final size, if known. Providers may use it to preallocate.
    pub size_hint: Option<u64>,
}

impl WriteOptions {
    /// Create a new file, failing if it exists.
    pub fn create_new() -> Self {
        WriteOptions { mode: WriteMode::CreateNew, size_hint: None }
    }

    /// Create or replace a file.
    pub fn truncate() -> Self {
        WriteOptions { mode: WriteMode::Truncate, size_hint: None }
    }
}

/// Attributes that [`Provider::set_attributes`] can change. `None` fields
/// are left untouched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetAttributes {
    /// New POSIX permission bits.
    pub permissions: Option<u32>,
    /// New modification time.
    pub modified: Option<SystemTime>,
    /// New Windows attributes (`entry::os_flags` bits).
    #[serde(default)]
    pub os_flags: Option<u32>,
}

/// Checksum algorithms understood across the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HashAlgorithm {
    /// BLAKE3: the default for verification (fast and cryptographically strong).
    Blake3,
    /// XXH3-64: very fast, non-cryptographic; for quick comparisons.
    Xxh3,
    /// SHA-256, for interoperability with published checksums.
    Sha256,
}

/// A backend that can list, read, and (optionally) modify files.
///
/// One provider instance serves every path with the same
/// [`VfsPath::provider_key`]: for example one SFTP connection, or one
/// opened archive. Implementations must be cheap to call concurrently.
///
/// Optional operations have default implementations that return
/// [`VfsError::Unsupported`]; a provider advertises the ones it overrides
/// through [`Provider::capabilities`].
#[async_trait]
pub trait Provider: Send + Sync + 'static {
    /// The URI scheme this provider serves (`"file"`, `"sftp"`, ...).
    fn scheme(&self) -> &str;

    /// What the provider supports natively.
    fn capabilities(&self) -> Capabilities;

    /// The settings folder of the NemesisCommander running at the other end
    /// of `on` (a path of this provider), when the provider talks to one
    /// (the agent does). Used to send settings to other machines.
    fn settings_dir(&self, on: &VfsPath) -> Option<VfsPath> {
        let _ = on;
        None
    }

    /// Metadata of a single path. Symlinks are *not* followed: a link is
    /// reported as [`EntryKind::Symlink`](crate::EntryKind::Symlink).
    async fn stat(&self, path: &VfsPath) -> Result<Entry>;

    /// Lists a directory. Entries `.` and `..` are never included.
    async fn list(&self, path: &VfsPath) -> Result<ListStream>;

    /// Opens a file for reading, starting at `offset` bytes.
    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream>;

    /// Opens a file for writing.
    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        let _ = options;
        Err(self.unsupported_on(path, "write"))
    }

    /// Creates a single directory (its parent must exist).
    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        Err(self.unsupported_on(path, "create directory"))
    }

    /// Removes a file or a symbolic link.
    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        Err(self.unsupported_on(path, "delete"))
    }

    /// Removes an empty directory.
    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        Err(self.unsupported_on(path, "delete"))
    }

    /// Renames or moves a path within this provider.
    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let _ = to;
        Err(self.unsupported_on(from, "rename"))
    }

    /// Extended attributes of `path` (macOS and Linux file systems), name and
    /// value. Only providers with [`Capabilities::XATTRS`] have any.
    async fn read_xattrs(&self, path: &VfsPath) -> Result<Vec<(String, Vec<u8>)>> {
        let _ = path;
        Ok(Vec::new())
    }

    /// Sets extended attributes on `path`; the ones the file system refuses
    /// are skipped. Only providers with [`Capabilities::XATTRS`] do anything.
    async fn write_xattrs(&self, path: &VfsPath, attributes: &[(String, Vec<u8>)]) -> Result<()> {
        let _ = (path, attributes);
        Ok(())
    }

    /// Changes permissions and/or modification time.
    async fn set_attributes(&self, path: &VfsPath, attributes: &SetAttributes) -> Result<()> {
        let _ = attributes;
        Err(self.unsupported_on(path, "set attributes"))
    }

    /// Copies a file to another path of the same provider without sending
    /// the data through the client. See [`Capabilities::SERVER_COPY`].
    async fn copy_within(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let _ = to;
        Err(self.unsupported_on(from, "server-side copy"))
    }

    /// Computes a checksum on the provider side. See
    /// [`Capabilities::SERVER_HASH`]. Returns the lowercase hex digest.
    async fn checksum(&self, path: &VfsPath, algorithm: HashAlgorithm) -> Result<String> {
        let _ = algorithm;
        Err(self.unsupported_on(path, "server-side checksum"))
    }

    /// Creates a symbolic link at `path` pointing to `target`.
    /// See [`Capabilities::SYMLINKS`].
    async fn create_symlink(&self, path: &VfsPath, target: &str) -> Result<()> {
        let _ = target;
        Err(self.unsupported_on(path, "create symlink"))
    }

    /// Moves a path to the recycle bin. See [`Capabilities::TRASH`].
    async fn trash(&self, path: &VfsPath) -> Result<()> {
        Err(self.unsupported_on(path, "trash"))
    }

    /// Helper for default method implementations.
    #[doc(hidden)]
    fn unsupported_on(&self, _path: &VfsPath, operation: &'static str) -> VfsError {
        VfsError::unsupported(operation, self.scheme())
    }
}

/// Reads a whole listing into memory. Convenient for small directories,
/// tests, and the CLI; UIs should consume the pages as they arrive.
pub async fn collect_listing(provider: &dyn Provider, path: &VfsPath) -> Result<Vec<Entry>> {
    let mut stream = provider.list(path).await?;
    let mut all = Vec::new();
    while let Some(page) = stream.next().await {
        all.extend(page?);
    }
    Ok(all)
}
