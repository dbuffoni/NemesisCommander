//! An in-memory file system (`mem://name/...`).
//!
//! Every authority (`mem://a/`, `mem://b/`) is an independent file system.
//! The capabilities are configurable so tests can simulate weaker backends,
//! for example one that cannot rename atomically or resume writes.

use std::collections::BTreeMap;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::SystemTime;

use async_trait::async_trait;
use sc_vfs::{
    Capabilities, Entry, EntryKind, ListStream, Provider, ProviderFactory, ProviderRegistry,
    ReadStream, Result, SetAttributes, VfsError, VfsPath, WriteMode, WriteOptions, WriteSink,
};
use tokio::io::AsyncWrite;

#[derive(Debug, Clone)]
enum Node {
    File { data: Vec<u8>, modified: SystemTime, mode: u32 },
    Dir { modified: SystemTime, mode: u32 },
}

type Tree = BTreeMap<String, Node>;

/// An in-memory file system. Cloning shares the same contents.
#[derive(Clone)]
pub struct MemoryProvider {
    tree: Arc<Mutex<Tree>>,
    capabilities: Capabilities,
}

impl Default for MemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryProvider {
    /// An empty file system with every capability a memory store can offer.
    pub fn new() -> Self {
        Self::with_capabilities(
            Capabilities::WRITE
                | Capabilities::SEEK_READ
                | Capabilities::RESUME_WRITE
                | Capabilities::ATOMIC_RENAME
                | Capabilities::POSIX_PERMISSIONS
                | Capabilities::SET_MTIME,
        )
    }

    /// An empty file system advertising only `capabilities`. Operations
    /// that are not advertised fail with [`VfsError::Unsupported`].
    pub fn with_capabilities(capabilities: Capabilities) -> Self {
        let mut tree = Tree::new();
        tree.insert("/".into(), Node::Dir { modified: SystemTime::now(), mode: 0o755 });
        MemoryProvider { tree: Arc::new(Mutex::new(tree)), capabilities }
    }

    /// Test helper: creates a file and any missing parent directories.
    pub fn put_file(&self, path: &str, data: impl Into<Vec<u8>>) {
        let path = VfsPath::parse(&format!("mem://x{path}")).expect("valid path");
        let mut tree = self.lock();
        let mut dir = path.parent();
        while let Some(d) = dir {
            tree.entry(d.path().to_owned())
                .or_insert(Node::Dir { modified: SystemTime::now(), mode: 0o755 });
            dir = d.parent();
        }
        tree.insert(
            path.path().to_owned(),
            Node::File { data: data.into(), modified: SystemTime::now(), mode: 0o644 },
        );
    }

    /// Test helper: the content of a file, if it exists.
    pub fn get_file(&self, path: &str) -> Option<Vec<u8>> {
        match self.lock().get(path) {
            Some(Node::File { data, .. }) => Some(data.clone()),
            _ => None,
        }
    }

    /// Test helper: every path in the file system, sorted.
    pub fn paths(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tree> {
        self.tree.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn require(&self, cap: Capabilities, operation: &'static str) -> Result<()> {
        if self.capabilities.contains(cap) {
            Ok(())
        } else {
            Err(VfsError::unsupported(operation, "mem"))
        }
    }
}

fn parent_key(path: &VfsPath) -> Option<String> {
    path.parent().map(|p| p.path().to_owned())
}

fn ensure_parent_dir(tree: &Tree, path: &VfsPath) -> Result<()> {
    match parent_key(path).and_then(|k| tree.get(&k).cloned()) {
        Some(Node::Dir { .. }) => Ok(()),
        Some(Node::File { .. }) => Err(VfsError::NotADirectory(path.parent().unwrap())),
        None => Err(VfsError::NotFound(path.parent().unwrap_or_else(|| path.clone()))),
    }
}

fn to_entry(name: &str, node: &Node) -> Entry {
    match node {
        Node::File { data, modified, mode } => {
            let mut e = Entry::new(name, EntryKind::File)
                .with_size(data.len() as u64)
                .with_modified(*modified);
            e.permissions = Some(*mode);
            e
        }
        Node::Dir { modified, mode } => {
            let mut e = Entry::new(name, EntryKind::Directory).with_modified(*modified);
            e.permissions = Some(*mode);
            e
        }
    }
}

/// Is `child` a direct child of `dir` (both normalized keys)?
fn is_direct_child(dir: &str, child: &str) -> bool {
    let prefix = if dir == "/" { "/".to_owned() } else { format!("{dir}/") };
    child.len() > prefix.len() && child.starts_with(&prefix) && !child[prefix.len()..].contains('/')
}

#[async_trait]
impl Provider for MemoryProvider {
    fn scheme(&self) -> &str {
        "mem"
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        let tree = self.lock();
        let node = tree.get(path.path()).ok_or_else(|| VfsError::NotFound(path.clone()))?;
        Ok(to_entry(path.file_name().unwrap_or("/"), node))
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let tree = self.lock();
        match tree.get(path.path()) {
            Some(Node::Dir { .. }) => {}
            Some(Node::File { .. }) => return Err(VfsError::NotADirectory(path.clone())),
            None => return Err(VfsError::NotFound(path.clone())),
        }
        let entries: Vec<Entry> = tree
            .iter()
            .filter(|(key, _)| is_direct_child(path.path(), key))
            .map(|(key, node)| to_entry(key.rsplit('/').next().unwrap_or_default(), node))
            .collect();
        Ok(Box::pin(futures::stream::iter([Ok(entries)])))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        if offset > 0 {
            self.require(Capabilities::SEEK_READ, "seek")?;
        }
        let tree = self.lock();
        match tree.get(path.path()) {
            Some(Node::File { data, .. }) => {
                let start = (offset as usize).min(data.len());
                Ok(Box::pin(io::Cursor::new(data[start..].to_vec())))
            }
            Some(Node::Dir { .. }) => Err(VfsError::IsADirectory(path.clone())),
            None => Err(VfsError::NotFound(path.clone())),
        }
    }

    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        self.require(Capabilities::WRITE, "write")?;
        let mut tree = self.lock();
        ensure_parent_dir(&tree, path)?;
        let existing = tree.get(path.path()).cloned();
        let data = match (options.mode, existing) {
            (_, Some(Node::Dir { .. })) => return Err(VfsError::IsADirectory(path.clone())),
            (WriteMode::CreateNew, Some(_)) => return Err(VfsError::AlreadyExists(path.clone())),
            (WriteMode::ResumeAt(offset), Some(Node::File { mut data, .. })) => {
                self.require(Capabilities::RESUME_WRITE, "resume")?;
                data.truncate(offset as usize);
                data
            }
            (WriteMode::ResumeAt(_), None) => return Err(VfsError::NotFound(path.clone())),
            _ => Vec::new(),
        };
        tree.insert(
            path.path().to_owned(),
            Node::File { data, modified: SystemTime::now(), mode: 0o644 },
        );
        Ok(Box::pin(MemoryWriter { tree: self.tree.clone(), key: path.path().to_owned() }))
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        self.require(Capabilities::WRITE, "create directory")?;
        let mut tree = self.lock();
        if tree.contains_key(path.path()) {
            return Err(VfsError::AlreadyExists(path.clone()));
        }
        ensure_parent_dir(&tree, path)?;
        tree.insert(path.path().to_owned(), Node::Dir { modified: SystemTime::now(), mode: 0o755 });
        Ok(())
    }

    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        self.require(Capabilities::WRITE, "delete")?;
        let mut tree = self.lock();
        match tree.get(path.path()) {
            Some(Node::File { .. }) => {
                tree.remove(path.path());
                Ok(())
            }
            Some(Node::Dir { .. }) => Err(VfsError::IsADirectory(path.clone())),
            None => Err(VfsError::NotFound(path.clone())),
        }
    }

    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        self.require(Capabilities::WRITE, "delete")?;
        let mut tree = self.lock();
        match tree.get(path.path()) {
            Some(Node::Dir { .. }) if path.is_root() => {
                Err(VfsError::PermissionDenied(path.clone()))
            }
            Some(Node::Dir { .. }) => {
                if tree.keys().any(|k| is_direct_child(path.path(), k)) {
                    return Err(VfsError::DirectoryNotEmpty(path.clone()));
                }
                tree.remove(path.path());
                Ok(())
            }
            Some(Node::File { .. }) => Err(VfsError::NotADirectory(path.clone())),
            None => Err(VfsError::NotFound(path.clone())),
        }
    }

    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        self.require(Capabilities::WRITE, "rename")?;
        let mut tree = self.lock();
        if !tree.contains_key(from.path()) {
            return Err(VfsError::NotFound(from.clone()));
        }
        ensure_parent_dir(&tree, to)?;
        if tree.contains_key(to.path()) && !self.capabilities.contains(Capabilities::ATOMIC_RENAME)
        {
            return Err(VfsError::AlreadyExists(to.clone()));
        }
        // Move the node and, for directories, everything below it.
        let prefix = format!("{}/", from.path());
        let moved: Vec<String> =
            tree.keys().filter(|k| *k == from.path() || k.starts_with(&prefix)).cloned().collect();
        for key in moved {
            let node = tree.remove(&key).expect("key listed above");
            let new_key = format!("{}{}", to.path(), &key[from.path().len()..]);
            tree.insert(new_key, node);
        }
        Ok(())
    }

    async fn set_attributes(&self, path: &VfsPath, attributes: &SetAttributes) -> Result<()> {
        let mut tree = self.lock();
        let node = tree.get_mut(path.path()).ok_or_else(|| VfsError::NotFound(path.clone()))?;
        let (modified, mode) = match node {
            Node::File { modified, mode, .. } | Node::Dir { modified, mode } => (modified, mode),
        };
        if let Some(m) = attributes.modified {
            self.require(Capabilities::SET_MTIME, "set modification time")?;
            *modified = m;
        }
        if let Some(p) = attributes.permissions {
            self.require(Capabilities::POSIX_PERMISSIONS, "set permissions")?;
            *mode = p;
        }
        Ok(())
    }
}

/// Appends written bytes straight into the shared tree, so a partially
/// written file is visible (and resumable) even if the writer is dropped.
struct MemoryWriter {
    tree: Arc<Mutex<Tree>>,
    key: String,
}

impl AsyncWrite for MemoryWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut tree = self.tree.lock().unwrap_or_else(|e| e.into_inner());
        match tree.get_mut(&self.key) {
            Some(Node::File { data, modified, .. }) => {
                data.extend_from_slice(buf);
                *modified = SystemTime::now();
                Poll::Ready(Ok(buf.len()))
            }
            _ => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotFound,
                "file removed while writing",
            ))),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Factory for `mem://` paths. Each authority gets its own empty store.
#[derive(Default)]
pub struct MemoryFactory;

#[async_trait]
impl ProviderFactory for MemoryFactory {
    fn scheme(&self) -> &str {
        "mem"
    }
    async fn create(&self, _: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        Ok(Arc::new(MemoryProvider::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_vfs::collect_listing;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn p(s: &str) -> VfsPath {
        VfsPath::parse(&format!("mem://t{s}")).unwrap()
    }

    #[tokio::test]
    async fn put_list_read() {
        let m = MemoryProvider::new();
        m.put_file("/a/b/c.txt", "hello");
        m.put_file("/a/d.txt", "x");
        let names: Vec<String> =
            collect_listing(&m, &p("/a")).await.unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["b", "d.txt"]);
        let mut out = String::new();
        m.read(&p("/a/b/c.txt"), 1).await.unwrap().read_to_string(&mut out).await.unwrap();
        assert_eq!(out, "ello");
    }

    #[tokio::test]
    async fn rename_moves_subtrees() {
        let m = MemoryProvider::new();
        m.put_file("/a/b/c.txt", "hello");
        m.rename(&p("/a"), &p("/z")).await.unwrap();
        assert_eq!(m.paths(), ["/", "/z", "/z/b", "/z/b/c.txt"]);
    }

    #[tokio::test]
    async fn partial_writes_are_visible_and_resumable() {
        let m = MemoryProvider::new();
        let f = p("/f");
        let mut w = m.write(&f, WriteOptions::create_new()).await.unwrap();
        w.write_all(b"abcXX").await.unwrap();
        drop(w);
        assert_eq!(m.get_file("/f").unwrap(), b"abcXX");
        let mut w = m
            .write(&f, WriteOptions { mode: WriteMode::ResumeAt(3), size_hint: None })
            .await
            .unwrap();
        w.write_all(b"def").await.unwrap();
        assert_eq!(m.get_file("/f").unwrap(), b"abcdef");
    }

    #[tokio::test]
    async fn missing_capabilities_are_enforced() {
        let m = MemoryProvider::with_capabilities(Capabilities::WRITE);
        m.put_file("/f", "abc");
        assert!(matches!(m.read(&p("/f"), 1).await.err().unwrap(), VfsError::Unsupported { .. }));
        let resume = WriteOptions { mode: WriteMode::ResumeAt(1), size_hint: None };
        assert!(matches!(
            m.write(&p("/f"), resume).await.err().unwrap(),
            VfsError::Unsupported { .. }
        ));
        m.put_file("/g", "x");
        assert!(matches!(
            m.rename(&p("/f"), &p("/g")).await.unwrap_err(),
            VfsError::AlreadyExists(_)
        ));
    }

    #[tokio::test]
    async fn directory_rules() {
        let m = MemoryProvider::new();
        m.create_dir(&p("/d")).await.unwrap();
        assert!(matches!(m.create_dir(&p("/d")).await.unwrap_err(), VfsError::AlreadyExists(_)));
        assert!(m.create_dir(&p("/x/y")).await.unwrap_err().is_not_found());
        m.put_file("/d/f", "1");
        assert!(matches!(
            m.remove_dir(&p("/d")).await.unwrap_err(),
            VfsError::DirectoryNotEmpty(_)
        ));
        m.remove_file(&p("/d/f")).await.unwrap();
        m.remove_dir(&p("/d")).await.unwrap();
        assert!(matches!(m.remove_dir(&p("/")).await.unwrap_err(), VfsError::PermissionDenied(_)));
    }
}
