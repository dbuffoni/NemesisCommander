//! Archives as read-only folders.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use sc_vfs::{
    Capabilities, Entry, EntryKind, ListStream, Provider, ProviderFactory, ProviderRegistry,
    ReadStream, Result, VfsError, VfsPath,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::index::ArchiveIndex;
use crate::pipe::stream_from_blocking;
use crate::{ArchiveFormat, Compression, readers};

/// Serves the contents of one archive file.
pub struct ArchiveProvider {
    scheme: &'static str,
    format: ArchiveFormat,
    /// The archive on the local disk (the original, or a downloaded copy).
    local_file: PathBuf,
    /// Keeps a downloaded copy alive as long as the provider exists.
    _download: Option<tempfile::TempPath>,
    index: Arc<ArchiveIndex>,
}

impl ArchiveProvider {
    /// Opens the archive at `local_file`.
    pub async fn open_local(format: ArchiveFormat, local_file: PathBuf) -> Result<Self> {
        Self::open(format, local_file, None).await
    }

    async fn open(
        format: ArchiveFormat,
        local_file: PathBuf,
        download: Option<tempfile::TempPath>,
    ) -> Result<Self> {
        let file = local_file.clone();
        let index = tokio::task::spawn_blocking(move || readers::build_index(&file, format))
            .await
            .map_err(|e| VfsError::Other(e.to_string()))?
            .map_err(|e| {
                VfsError::Other(format!("cannot read archive {}: {e}", local_file.display()))
            })?;
        Ok(ArchiveProvider {
            scheme: format.scheme(),
            format,
            local_file,
            _download: download,
            index: Arc::new(index),
        })
    }

    /// The format being served.
    pub fn format(&self) -> ArchiveFormat {
        self.format
    }

    fn item(&self, path: &VfsPath) -> Result<Entry> {
        if path.is_root() {
            return Ok(Entry::new("/", EntryKind::Directory));
        }
        self.index
            .get(path.path())
            .map(|i| i.entry.clone())
            .ok_or_else(|| VfsError::NotFound(path.clone()))
    }
}

#[async_trait]
impl Provider for ArchiveProvider {
    fn scheme(&self) -> &str {
        self.scheme
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::SYMLINKS
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        self.item(path)
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let entry = self.item(path)?;
        if !entry.is_dir() {
            return Err(VfsError::NotADirectory(path.clone()));
        }
        let children = self.index.children(path.path());
        Ok(Box::pin(futures::stream::iter([Ok(children)])))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        let item =
            self.index.get(path.path()).cloned().ok_or_else(|| VfsError::NotFound(path.clone()))?;
        if item.entry.is_dir() {
            return Err(VfsError::IsADirectory(path.clone()));
        }
        let (file, format) = (self.local_file.clone(), self.format);
        let mut stream =
            stream_from_blocking(move |out| readers::extract(&file, format, &item, out));
        if offset > 0 {
            // Archives are not seekable streams: skip by reading.
            let skipped = tokio::io::copy(&mut (&mut stream).take(offset), &mut tokio::io::sink())
                .await
                .map_err(|e| VfsError::from_io(path, e))?;
            if skipped < offset {
                return Err(VfsError::Other(format!("{path}: offset beyond end of entry")));
            }
        }
        Ok(stream)
    }
}

/// Creates [`ArchiveProvider`]s for one archive scheme.
pub struct ArchiveFactory {
    scheme: &'static str,
}

impl ArchiveFactory {
    /// A factory for `scheme` (`zip`, `tar`, `7z`, `rar`, `gz`, `xz`, `bz2`, `zst`).
    pub fn new(scheme: &'static str) -> Self {
        ArchiveFactory { scheme }
    }
}

#[async_trait]
impl ProviderFactory for ArchiveFactory {
    fn scheme(&self) -> &str {
        self.scheme
    }

    async fn create(
        &self,
        path: &VfsPath,
        registry: &ProviderRegistry,
    ) -> Result<Arc<dyn Provider>> {
        let container = path.container().ok_or_else(|| VfsError::InvalidPath {
            input: path.to_string(),
            reason: "archive paths need a container",
        })?;
        let format = match self.scheme {
            "zip" => ArchiveFormat::Zip,
            "tar" => ArchiveFormat::Tar(Compression::None), // compression is sniffed when reading
            "7z" => ArchiveFormat::SevenZ,
            "rar" => ArchiveFormat::Rar,
            "gz" => ArchiveFormat::Compressed(Compression::Gzip),
            "xz" => ArchiveFormat::Compressed(Compression::Xz),
            "bz2" => ArchiveFormat::Compressed(Compression::Bzip2),
            "zst" => ArchiveFormat::Compressed(Compression::Zstd),
            other => return Err(VfsError::UnknownScheme(other.to_owned())),
        };
        if let Some(local) = container.to_local_path() {
            return Ok(Arc::new(ArchiveProvider::open(format, local, None).await?));
        }
        // Remote or nested archive: archive libraries need random access, so
        // fetch a local copy first.
        let source = registry.provider_for(container).await?;
        let temp = tempfile::NamedTempFile::new().map_err(|e| VfsError::Other(e.to_string()))?;
        let mut out = tokio::fs::File::create(temp.path())
            .await
            .map_err(|e| VfsError::from_io(container, e))?;
        let mut input = source.read(container, 0).await?;
        tokio::io::copy(&mut input, &mut out).await.map_err(|e| VfsError::from_io(container, e))?;
        out.shutdown().await.map_err(|e| VfsError::from_io(container, e))?;
        let temp = temp.into_temp_path();
        let local = temp.to_path_buf();
        Ok(Arc::new(ArchiveProvider::open(format, local, Some(temp)).await?))
    }
}

/// Registers a factory for every archive scheme.
pub fn register_archive_providers(registry: &mut ProviderRegistry) {
    for scheme in sc_vfs::ARCHIVE_SCHEMES {
        registry.register(ArchiveFactory::new(scheme));
    }
}
