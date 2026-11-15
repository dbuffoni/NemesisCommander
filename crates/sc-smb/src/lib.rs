//! # sc-smb
//!
//! The `smb://[user@]host[:port]/share/path` provider for Windows file
//! sharing, Samba and NAS devices, using the pure-Rust `smb2` client
//! (SMB 2.0.2 to 3.1.1, signing and encryption).
//!
//! The first path segment is the share. Logins come from the user's
//! `~/.netrc` like WebDAV ([`sc_vfs::netrc`]); a user name in the location
//! picks the entry, and without one the session is a guest session.
//! `user` may be `DOMAIN\user` or `user@domain` style as the server expects
//! (`%5C` for the backslash in a URI).
//!
//! The listing of `smb://host/` (no share) shows the server's shares.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{StreamExt, stream};
use sc_vfs::{
    Capabilities, Entry, EntryKind, ListStream, Provider, ProviderFactory, ProviderRegistry,
    ReadStream, Result, SetAttributes, VfsError, VfsPath, WriteMode, WriteOptions, WriteSink,
    netrc, upload_sink,
};
use smb2::{ClientConfig, ErrorKind, FileTimes, RenameOptions, SmbClient, Tree};
use tokio::sync::Mutex;
use tokio_util::io::StreamReader;

/// Bytes read per request while streaming a file.
const READ_CHUNK: u64 = 1024 * 1024;

/// A connection to one SMB server, with the shares opened so far.
pub struct SmbProvider {
    /// The client is used by one request at a time.
    client: Mutex<SmbClient>,
    /// Connected shares by name (lowercase: share names ignore case).
    shares: Mutex<Vec<(String, Tree)>>,
}

impl SmbProvider {
    /// Connects to `addr` (`host:port`) and signs in.
    pub async fn connect(addr: &str, user: &str, password: &str) -> Result<Self> {
        let config = ClientConfig {
            addr: addr.to_owned(),
            timeout: Duration::from_secs(30),
            username: user.to_owned(),
            password: password.to_owned(),
            auto_reconnect: true,
            ..Default::default()
        };
        let client = SmbClient::connect(config)
            .await
            .map_err(|e| VfsError::Other(format!("smb://{addr}: {e}")))?;
        Ok(SmbProvider { client: Mutex::new(client), shares: Mutex::new(Vec::new()) })
    }

    /// Connects for an `smb://` authority, with the login from netrc.
    pub async fn for_authority(authority: &str) -> Result<Self> {
        let (user, host) = match authority.rsplit_once('@') {
            Some((user, host)) => (Some(user), host),
            None => (None, authority),
        };
        if host.is_empty() {
            return Err(VfsError::Other("an SMB location needs a server name".into()));
        }
        let host_name = host.rsplit_once(':').map_or(host, |(name, _)| name);
        let addr = if host.contains(':') { host.to_owned() } else { format!("{host}:445") };
        let login = netrc::lookup(host_name, user);
        let (user, password) = match (login, user) {
            (Some(login), _) => (login.user, login.password),
            (None, Some(user)) => (user.to_owned(), String::new()),
            (None, None) => (String::new(), String::new()),
        };
        Self::connect(&addr, &user, &password).await
    }

    /// The share of the connection for `share`, connecting on first use.
    async fn tree(&self, share: &str, path: &VfsPath) -> Result<Tree> {
        let key = share.to_lowercase();
        let mut shares = self.shares.lock().await;
        if let Some((_, tree)) = shares.iter().find(|(name, _)| *name == key) {
            return Ok(tree.clone());
        }
        let tree =
            self.client.lock().await.connect_share(share).await.map_err(|e| error(e, path))?;
        shares.push((key, tree.clone()));
        Ok(tree)
    }

    /// The share and the path inside it (`""` for the share's root).
    fn split(path: &VfsPath) -> Option<(&str, &str)> {
        let inner = path.path().trim_start_matches('/');
        if inner.is_empty() {
            return None;
        }
        Some(match inner.split_once('/') {
            Some((share, rest)) => (share, rest.trim_end_matches('/')),
            None => (inner, ""),
        })
    }

    /// The share and the inner path of a path that must be inside a share.
    async fn locate<'a>(&self, path: &'a VfsPath) -> Result<(Tree, &'a str)> {
        let Some((share, inner)) = Self::split(path) else {
            return Err(VfsError::PermissionDenied(path.clone()));
        };
        Ok((self.tree(share, path).await?, inner))
    }

    /// Like [`locate`](Self::locate), for operations on an entry inside a
    /// share (not the share itself).
    async fn locate_entry<'a>(&self, path: &'a VfsPath) -> Result<(Tree, &'a str)> {
        let (tree, inner) = self.locate(path).await?;
        if inner.is_empty() {
            return Err(VfsError::PermissionDenied(path.clone()));
        }
        Ok((tree, inner))
    }
}

/// Maps an SMB error for `path`.
fn error(error: smb2::Error, path: &VfsPath) -> VfsError {
    match error.kind() {
        ErrorKind::NotFound => VfsError::NotFound(path.clone()),
        ErrorKind::AlreadyExists => VfsError::AlreadyExists(path.clone()),
        ErrorKind::AccessDenied | ErrorKind::AuthRequired => {
            VfsError::PermissionDenied(path.clone())
        }
        ErrorKind::IsADirectory => VfsError::IsADirectory(path.clone()),
        ErrorKind::NotADirectory => VfsError::NotADirectory(path.clone()),
        ErrorKind::Cancelled => VfsError::Cancelled,
        _ if error.to_string().contains("DIRECTORY_NOT_EMPTY") => {
            VfsError::DirectoryNotEmpty(path.clone())
        }
        _ => VfsError::Other(format!("{path}: {error}")),
    }
}

fn time(file_time: smb2::pack::FileTime) -> Option<SystemTime> {
    file_time.to_system_time()
}

/// Share names ignore case.
fn same_share(a: &VfsPath, b: &VfsPath) -> bool {
    let share = |p| SmbProvider::split(p).map(|(s, _)| s.to_lowercase());
    share(a) == share(b)
}

fn name_of(inner: &str) -> String {
    inner.rsplit('/').next().unwrap_or(inner).to_owned()
}

#[async_trait]
impl Provider for SmbProvider {
    fn scheme(&self) -> &str {
        "smb"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::WRITE
            | Capabilities::SEEK_READ
            | Capabilities::RESUME_WRITE
            | Capabilities::ATOMIC_RENAME
            | Capabilities::SET_MTIME
            | Capabilities::SERVER_COPY
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        let Some((share, inner)) = Self::split(path) else {
            return Ok(Entry::new("/", EntryKind::Directory));
        };
        let mut tree = self.tree(share, path).await?;
        if inner.is_empty() {
            return Ok(Entry::new(share, EntryKind::Directory));
        }
        let info =
            self.client.lock().await.stat(&mut tree, inner).await.map_err(|e| error(e, path))?;
        let kind = if info.is_directory { EntryKind::Directory } else { EntryKind::File };
        let mut entry = Entry::new(name_of(inner), kind).with_size(info.size);
        entry.modified = time(info.modified);
        Ok(entry)
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let Some((share, inner)) = Self::split(path) else {
            // The server's shares; administrative ones (C$, IPC$) are hidden.
            let shares =
                self.client.lock().await.list_shares().await.map_err(|e| error(e, path))?;
            let entries: Vec<Entry> = shares
                .into_iter()
                .filter(|s| !s.name.ends_with('$'))
                .map(|s| Entry::new(s.name, EntryKind::Directory))
                .collect();
            return Ok(Box::pin(stream::iter([Ok(entries)])));
        };
        let mut tree = self.tree(share, path).await?;
        let listing = self.client.lock().await.list_directory(&mut tree, inner).await;
        let entries: Vec<Entry> = listing
            .map_err(|e| error(e, path))?
            .into_iter()
            .filter(|e| e.name != "." && e.name != "..")
            .map(|e| {
                let kind = if e.is_directory { EntryKind::Directory } else { EntryKind::File };
                let mut entry = Entry::new(e.name, kind).with_size(e.size);
                entry.modified = time(e.modified);
                entry
            })
            .collect();
        Ok(Box::pin(stream::iter([Ok(entries)])))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        let (tree, inner) = self.locate_entry(path).await?;
        let reader = self
            .client
            .lock()
            .await
            .open_file_reader(&tree, inner)
            .await
            .map_err(|e| error(e, path))?;
        let size = reader.size();
        let reader = Arc::new(reader);
        let chunks = stream::unfold(offset, move |position| {
            let reader = reader.clone();
            async move {
                if position >= size {
                    return None;
                }
                let length = READ_CHUNK.min(size - position);
                match reader.read_at(position, length).await {
                    Ok(data) if data.is_empty() => None,
                    Ok(data) => {
                        let next = position + data.len() as u64;
                        Some((Ok(Bytes::from(data)), next))
                    }
                    Err(e) => Some((Err(std::io::Error::other(e)), size)),
                }
            }
        });
        Ok(Box::pin(StreamReader::new(chunks)))
    }

    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        let (tree, inner) = self.locate_entry(path).await?;
        let writer = {
            let mut client = self.client.lock().await;
            match options.mode {
                WriteMode::CreateNew => client.create_file_writer_exclusive(&tree, inner).await,
                WriteMode::Truncate => client.create_file_writer(&tree, inner).await,
                WriteMode::ResumeAt(offset) => {
                    client.create_file_writer_at(&tree, inner, offset).await
                }
            }
        }
        .map_err(|e| error(e, path))?;
        let path = path.clone();
        Ok(upload_sink(move |mut body| async move {
            let mut writer = writer;
            while let Some(chunk) = body.next().await {
                let chunk = chunk.map_err(|e| VfsError::from_io(&path, e))?;
                writer.write_chunk(&chunk).await.map_err(|e| error(e, &path))?;
            }
            writer.finish().await.map_err(|e| error(e, &path)).map(drop)
        }))
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        let (tree, inner) = self.locate_entry(path).await?;
        let result = self.client.lock().await.create_directory(&tree, inner).await;
        result.map_err(|e| error(e, path))
    }

    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        let (tree, inner) = self.locate_entry(path).await?;
        let result = self.client.lock().await.delete_file(&tree, inner).await;
        result.map_err(|e| error(e, path))
    }

    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        let (tree, inner) = self.locate_entry(path).await?;
        let result = self.client.lock().await.delete_directory(&tree, inner).await;
        match result.map_err(|e| error(e, path)) {
            // Some servers report a non-empty folder only as a generic
            // failure: check what is there.
            Err(VfsError::Other(message)) => {
                let mut listing = self.list(path).await?;
                while let Some(page) = listing.next().await {
                    if !page?.is_empty() {
                        return Err(VfsError::DirectoryNotEmpty(path.clone()));
                    }
                }
                Err(VfsError::Other(message))
            }
            other => other,
        }
    }

    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let (tree, source) = self.locate_entry(from).await?;
        let (_, target) = self.locate_entry(to).await?;
        if !same_share(from, to) {
            return Err(VfsError::unsupported("rename between shares", "smb"));
        }
        let options = RenameOptions::new().replace_if_exists(true);
        let result = self.client.lock().await.rename_with(&tree, source, target, options).await;
        result.map_err(|e| error(e, from))
    }

    async fn set_attributes(&self, path: &VfsPath, attributes: &SetAttributes) -> Result<()> {
        let Some(modified) = attributes.modified else { return Ok(()) };
        let (tree, inner) = self.locate_entry(path).await?;
        let times = FileTimes::new().set_modified(modified);
        let result = self.client.lock().await.set_times(&tree, inner, times).await;
        result.map_err(|e| error(e, path))
    }

    async fn copy_within(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let (tree, source) = self.locate_entry(from).await?;
        let (_, target) = self.locate_entry(to).await?;
        if !same_share(from, to) {
            return Err(VfsError::unsupported("server-side copy between shares", "smb"));
        }
        let result = self.client.lock().await.server_side_copy_file(&tree, source, target).await;
        result.map_err(|e| error(e, from)).map(drop)
    }
}

/// Creates [`SmbProvider`]s for `smb://` paths.
pub struct SmbFactory;

#[async_trait]
impl ProviderFactory for SmbFactory {
    fn scheme(&self) -> &str {
        "smb"
    }

    async fn create(&self, path: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        Ok(Arc::new(SmbProvider::for_authority(path.authority()).await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_segment_is_the_share() {
        let split = |uri: &str| {
            let path = VfsPath::parse(uri).unwrap();
            SmbProvider::split(&path).map(|(s, i)| (s.to_owned(), i.to_owned()))
        };
        assert_eq!(split("smb://nas/"), None);
        assert_eq!(split("smb://nas/media"), Some(("media".into(), "".into())));
        assert_eq!(
            split("smb://nas/media/films/a b.mkv"),
            Some(("media".into(), "films/a b.mkv".into()))
        );
        assert_eq!(name_of("films/a b.mkv"), "a b.mkv");
    }
}
