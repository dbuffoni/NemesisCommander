//! The local file system provider (`file://`).
//!
//! Directory listings run on a blocking thread and are streamed in pages of
//! [`LIST_PAGE_SIZE`] entries. Local-to-local copies go through
//! [`Provider::copy_within`], which uses a copy-on-write reflink when the file
//! system supports it and the kernel's in-kernel copy (`copy_file_range` on
//! Linux) otherwise, so the data never passes through user space.

use std::fs::Metadata;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use sc_vfs::{
    Capabilities, Entry, EntryKind, HashAlgorithm, ListStream, Provider, ProviderFactory,
    ProviderRegistry, ReadStream, Result, SetAttributes, VfsError, VfsPath, WriteMode,
    WriteOptions, WriteSink,
};
use tokio::io::{AsyncSeekExt, BufReader, BufWriter};
use tokio::sync::mpsc;

/// Number of entries per page sent by [`LocalProvider::list`].
pub const LIST_PAGE_SIZE: usize = 512;

/// Buffer size for reading and writing files through streams.
const IO_BUFFER: usize = 256 * 1024;

/// Serves `file://` paths from the local disk.
#[derive(Debug, Default, Clone)]
pub struct LocalProvider;

impl LocalProvider {
    /// Creates the provider. It holds no state.
    pub fn new() -> Self {
        LocalProvider
    }
}

/// Registers [`LocalProvider`] for the `file` scheme.
pub struct LocalFactory;

#[async_trait]
impl ProviderFactory for LocalFactory {
    fn scheme(&self) -> &str {
        "file"
    }
    async fn create(&self, _: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        Ok(Arc::new(LocalProvider))
    }
}

fn native(path: &VfsPath) -> Result<PathBuf> {
    path.to_local_path().ok_or_else(|| VfsError::InvalidPath {
        input: path.to_string(),
        reason: "the local provider only serves file:// paths",
    })
}

/// Builds an [`Entry`] from `lstat` metadata. For symlinks, `target_meta`
/// is the metadata of what the link points to (if it resolves).
fn entry_from_metadata(
    name: String,
    meta: &Metadata,
    link: Option<(String, Option<Metadata>)>,
) -> Entry {
    let file_type = meta.file_type();
    let kind = if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let mut entry = Entry::new(name, kind);
    entry.size = if kind == EntryKind::Directory { 0 } else { meta.len() };
    entry.modified = meta.modified().ok();
    if let Some((target, target_meta)) = link {
        entry.symlink_target = Some(target);
        entry.link_is_dir = target_meta.as_ref().is_some_and(Metadata::is_dir);
        if let Some(target_meta) = target_meta.filter(Metadata::is_file) {
            entry.size = target_meta.len();
        }
    }
    unix_fields(&mut entry, meta);
    entry
}

#[cfg(unix)]
fn unix_fields(entry: &mut Entry, meta: &Metadata) {
    use std::os::unix::fs::MetadataExt;
    entry.permissions = Some(meta.mode() & 0o7777);
    entry.uid = Some(meta.uid());
    entry.gid = Some(meta.gid());
}

#[cfg(not(unix))]
fn unix_fields(_: &mut Entry, _: &Metadata) {}

/// `lstat` + symlink resolution for one path, blocking.
fn stat_blocking(path: &Path, name: String) -> std::io::Result<Entry> {
    let meta = std::fs::symlink_metadata(path)?;
    let link = if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path)?.to_string_lossy().into_owned();
        Some((target, std::fs::metadata(path).ok()))
    } else {
        None
    };
    Ok(entry_from_metadata(name, &meta, link))
}

/// Runs blocking file system work on tokio's blocking pool and maps errors.
async fn blocking<T, F>(path: &VfsPath, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| VfsError::Other(format!("blocking task failed: {e}")))?
        .map_err(|e| VfsError::from_io(path, e))
}

#[async_trait]
impl Provider for LocalProvider {
    fn scheme(&self) -> &str {
        "file"
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::WRITE
            | Capabilities::SEEK_READ
            | Capabilities::RESUME_WRITE
            | Capabilities::ATOMIC_RENAME
            | Capabilities::SET_MTIME
            | Capabilities::SERVER_HASH
            | Capabilities::SERVER_COPY
            | Capabilities::SYMLINKS;
        if cfg!(unix) {
            caps |= Capabilities::POSIX_PERMISSIONS;
        }
        if cfg!(feature = "trash") {
            caps |= Capabilities::TRASH;
        }
        caps
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        let native = native(path)?;
        let name = path.file_name().unwrap_or("/").to_owned();
        blocking(path, move || stat_blocking(&native, name)).await
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let native = native(path)?;
        // Open the directory up front so "not found" and "not a directory"
        // are reported by `list` itself rather than as the first page.
        let reader = blocking(path, {
            let native = native.clone();
            move || std::fs::read_dir(native)
        })
        .await?;

        let (tx, rx) = mpsc::channel::<Result<Vec<Entry>>>(4);
        let dir_path = path.clone();
        tokio::task::spawn_blocking(move || {
            let mut page = Vec::with_capacity(LIST_PAGE_SIZE);
            for item in reader {
                let entry = item.and_then(|item| {
                    let name = item.file_name().to_string_lossy().into_owned();
                    stat_blocking(&item.path(), name)
                });
                match entry {
                    Ok(entry) => page.push(entry),
                    // An entry can disappear between readdir and lstat; skip it.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => {
                        let _ = tx.blocking_send(Err(VfsError::from_io(&dir_path, e)));
                        return;
                    }
                }
                if page.len() == LIST_PAGE_SIZE {
                    let full = std::mem::replace(&mut page, Vec::with_capacity(LIST_PAGE_SIZE));
                    if tx.blocking_send(Ok(full)).is_err() {
                        return; // the consumer stopped listening
                    }
                }
            }
            if !page.is_empty() {
                let _ = tx.blocking_send(Ok(page));
            }
        });
        Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|page| (page, rx))
        })))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        let native = native(path)?;
        let mut file =
            tokio::fs::File::open(&native).await.map_err(|e| VfsError::from_io(path, e))?;
        if file.metadata().await.map_err(|e| VfsError::from_io(path, e))?.is_dir() {
            return Err(VfsError::IsADirectory(path.clone()));
        }
        if offset > 0 {
            file.seek(SeekFrom::Start(offset)).await.map_err(|e| VfsError::from_io(path, e))?;
        }
        Ok(Box::pin(BufReader::with_capacity(IO_BUFFER, file)))
    }

    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        let native = native(path)?;
        let io_err = |e| VfsError::from_io(path, e);
        let mut open = tokio::fs::OpenOptions::new();
        open.write(true);
        let file = match options.mode {
            WriteMode::CreateNew => open.create_new(true).open(&native).await.map_err(io_err)?,
            WriteMode::Truncate => {
                open.create(true).truncate(true).open(&native).await.map_err(io_err)?
            }
            WriteMode::ResumeAt(offset) => {
                let mut file = open.open(&native).await.map_err(io_err)?;
                file.set_len(offset).await.map_err(io_err)?;
                file.seek(SeekFrom::Start(offset)).await.map_err(io_err)?;
                file
            }
        };
        Ok(Box::pin(BufWriter::with_capacity(IO_BUFFER, file)))
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        let native = native(path)?;
        tokio::fs::create_dir(native).await.map_err(|e| VfsError::from_io(path, e))
    }

    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        let native = native(path)?;
        tokio::fs::remove_file(native).await.map_err(|e| VfsError::from_io(path, e))
    }

    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        let native = native(path)?;
        tokio::fs::remove_dir(native).await.map_err(|e| VfsError::from_io(path, e))
    }

    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let (src, dst) = (native(from)?, native(to)?);
        tokio::fs::rename(src, dst).await.map_err(|e| VfsError::from_io(from, e))
    }

    async fn set_attributes(&self, path: &VfsPath, attributes: &SetAttributes) -> Result<()> {
        let native = native(path)?;
        let attributes = attributes.clone();
        blocking(path, move || {
            #[cfg(unix)]
            if let Some(mode) = attributes.permissions {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&native, std::fs::Permissions::from_mode(mode))?;
            }
            if let Some(modified) = attributes.modified {
                filetime::set_symlink_file_times(
                    &native,
                    filetime::FileTime::now(),
                    filetime::FileTime::from_system_time(modified),
                )?;
            }
            Ok(())
        })
        .await
    }

    async fn copy_within(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let (src, dst) = (native(from)?, native(to)?);
        // reflink_or_copy clones the extents when possible (btrfs, xfs, APFS)
        // and otherwise falls back to std::fs::copy, which uses
        // copy_file_range / sendfile on Linux.
        blocking(from, move || reflink_copy::reflink_or_copy(src, dst).map(|_| ())).await
    }

    async fn checksum(&self, path: &VfsPath, algorithm: HashAlgorithm) -> Result<String> {
        let native = native(path)?;
        blocking(path, move || sc_hash::hash_local_file(&native, algorithm)).await
    }

    async fn create_symlink(&self, path: &VfsPath, target: &str) -> Result<()> {
        let native = native(path)?;
        let target = PathBuf::from(target);
        blocking(path, move || {
            #[cfg(unix)]
            return std::os::unix::fs::symlink(target, native);
            #[cfg(windows)]
            return std::os::windows::fs::symlink_file(target, native);
        })
        .await
    }

    #[cfg(feature = "trash")]
    async fn trash(&self, path: &VfsPath) -> Result<()> {
        let native = native(path)?;
        blocking(path, move || trash::delete(&native).map_err(std::io::Error::other)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn vfs(path: &Path) -> VfsPath {
        VfsPath::from_local(path).unwrap()
    }

    async fn write_file(p: &LocalProvider, path: &VfsPath, data: &[u8], mode: WriteOptions) {
        let mut sink = p.write(path, mode).await.unwrap();
        sink.write_all(data).await.unwrap();
        sink.shutdown().await.unwrap();
    }

    async fn read_all(p: &LocalProvider, path: &VfsPath, offset: u64) -> Vec<u8> {
        let mut out = Vec::new();
        p.read(path, offset).await.unwrap().read_to_end(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn write_read_stat_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalProvider::new();
        let file = vfs(&dir.path().join("hello.txt"));
        write_file(&p, &file, b"hello world", WriteOptions::create_new()).await;

        let entry = p.stat(&file).await.unwrap();
        assert_eq!(entry.name, "hello.txt");
        assert_eq!(entry.kind, EntryKind::File);
        assert_eq!(entry.size, 11);
        assert!(entry.modified.is_some());
        assert_eq!(read_all(&p, &file, 6).await, b"world");
    }

    #[tokio::test]
    async fn create_new_refuses_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalProvider::new();
        let file = vfs(&dir.path().join("a"));
        write_file(&p, &file, b"1", WriteOptions::create_new()).await;
        let err = p.write(&file, WriteOptions::create_new()).await.err().unwrap();
        assert!(matches!(err, VfsError::AlreadyExists(_)));
    }

    #[tokio::test]
    async fn resume_keeps_prefix_and_drops_tail() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalProvider::new();
        let file = vfs(&dir.path().join("partial"));
        write_file(&p, &file, b"abcdXXXX", WriteOptions::truncate()).await;
        let resume = WriteOptions { mode: WriteMode::ResumeAt(4), size_hint: None };
        write_file(&p, &file, b"efgh", resume).await;
        assert_eq!(read_all(&p, &file, 0).await, b"abcdefgh");
    }

    #[tokio::test]
    async fn listing_streams_every_entry_in_pages() {
        let dir = tempfile::tempdir().unwrap();
        let count = LIST_PAGE_SIZE * 2 + 3;
        for i in 0..count {
            std::fs::write(dir.path().join(format!("f{i}")), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let p = LocalProvider::new();

        let mut stream = p.list(&vfs(dir.path())).await.unwrap();
        let mut pages = 0;
        let mut total = 0;
        while let Some(page) = futures::StreamExt::next(&mut stream).await {
            let page = page.unwrap();
            assert!(page.len() <= LIST_PAGE_SIZE);
            pages += 1;
            total += page.len();
        }
        assert_eq!(total, count + 1);
        assert_eq!(pages, 3);
    }

    #[tokio::test]
    async fn listing_errors_are_reported_up_front() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalProvider::new();
        let missing = vfs(&dir.path().join("missing"));
        assert!(p.list(&missing).await.err().unwrap().is_not_found());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_are_reported_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("real")).unwrap();
        std::os::unix::fs::symlink("real", dir.path().join("link")).unwrap();
        std::os::unix::fs::symlink("nowhere", dir.path().join("dangling")).unwrap();
        let p = LocalProvider::new();
        let entries = sc_vfs::collect_listing(&p, &vfs(dir.path())).await.unwrap();
        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
        assert!(link.link_is_dir);
        assert_eq!(link.symlink_target.as_deref(), Some("real"));
        let dangling = entries.iter().find(|e| e.name == "dangling").unwrap();
        assert!(!dangling.link_is_dir);
    }

    #[tokio::test]
    async fn rename_mkdir_remove() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalProvider::new();
        let sub = vfs(&dir.path().join("sub"));
        p.create_dir(&sub).await.unwrap();
        let a = sub.join("a");
        write_file(&p, &a, b"1", WriteOptions::create_new()).await;
        let b = sub.join("b");
        p.rename(&a, &b).await.unwrap();
        assert!(p.stat(&a).await.unwrap_err().is_not_found());
        assert!(matches!(p.remove_dir(&sub).await.unwrap_err(), VfsError::DirectoryNotEmpty(_)));
        p.remove_file(&b).await.unwrap();
        p.remove_dir(&sub).await.unwrap();
        assert!(p.stat(&sub).await.unwrap_err().is_not_found());
    }

    #[tokio::test]
    async fn copy_within_and_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalProvider::new();
        let a = vfs(&dir.path().join("a"));
        let b = vfs(&dir.path().join("b"));
        write_file(&p, &a, &vec![7u8; 100_000], WriteOptions::create_new()).await;
        p.copy_within(&a, &b).await.unwrap();
        let ha = p.checksum(&a, HashAlgorithm::Blake3).await.unwrap();
        let hb = p.checksum(&b, HashAlgorithm::Blake3).await.unwrap();
        assert_eq!(ha, hb);
    }

    #[tokio::test]
    async fn set_attributes_changes_mtime_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalProvider::new();
        let a = vfs(&dir.path().join("a"));
        write_file(&p, &a, b"x", WriteOptions::create_new()).await;
        let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        let attrs = SetAttributes { permissions: Some(0o600), modified: Some(when) };
        p.set_attributes(&a, &attrs).await.unwrap();
        let entry = p.stat(&a).await.unwrap();
        assert_eq!(entry.modified, Some(when));
        if cfg!(unix) {
            assert_eq!(entry.permissions, Some(0o600));
        }
    }

    #[tokio::test]
    async fn reading_a_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = LocalProvider.read(&vfs(dir.path()), 0).await.err().unwrap();
        assert!(matches!(err, VfsError::IsADirectory(_)));
    }

    #[tokio::test]
    async fn non_local_paths_are_rejected() {
        let err = LocalProvider.stat(&VfsPath::parse("sftp://h/x").unwrap()).await.unwrap_err();
        assert!(matches!(err, VfsError::InvalidPath { .. }));
    }
}
