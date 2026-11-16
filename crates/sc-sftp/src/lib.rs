//! # sc-sftp
//!
//! The `sftp://[user@]host[:port]/path` provider. The SFTP protocol is
//! spoken by `russh-sftp` over the standard streams of
//! `ssh -s host sftp`, so the user's OpenSSH setup (config, keys, agent,
//! known hosts, jump hosts) applies unchanged; see ADR 0009.
//!
//! Paths are absolute on the server; `sftp://host/~/notes` starts in the
//! login folder.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::stream;
use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags, StatusCode};
use sc_vfs::{
    Capabilities, Entry, EntryKind, ListStream, Provider, ProviderFactory, ProviderRegistry,
    ReadStream, Result, SetAttributes, VfsError, VfsPath, WriteMode, WriteOptions, WriteSink,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::process::Child;

/// Environment variable that replaces `ssh -s host sftp` with another
/// command speaking SFTP on its standard streams (`{host}` is replaced).
/// Tests point it at OpenSSH's `sftp-server`.
pub const SFTP_COMMAND_VAR: &str = "SC_SFTP_COMMAND";

/// Entries per page delivered to the user interface.
const PAGE: usize = 512;

/// A connected SFTP session.
pub struct SftpProvider {
    session: SftpSession,
    /// The login folder, for `~` paths.
    home: String,
    /// The process carrying the connection (`ssh`).
    _child: Option<Child>,
}

impl SftpProvider {
    /// Starts the SFTP session on top of an established byte stream.
    pub async fn over<S>(stream: S, child: Option<Child>) -> std::result::Result<Self, String>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let session = SftpSession::new(stream).await.map_err(|e| e.to_string())?;
        // Slow links and large folders need more than the default 10 s.
        session.set_timeout(120);
        let home = session.canonicalize(".").await.map_err(|e| e.to_string())?;
        Ok(SftpProvider { session, home, _child: child })
    }

    /// Connects to `authority` with `ssh` (or `$SC_SFTP_COMMAND`).
    pub async fn connect(authority: &str) -> std::result::Result<Self, String> {
        let (program, args) = match sc_ssh::command_override(SFTP_COMMAND_VAR, authority) {
            Some(command) => command,
            None => {
                let target = sc_ssh::Target::parse(authority)?;
                let args = sc_ssh::ssh_args(&target, sc_ssh::Remote::Subsystem("sftp"))
                    .map_err(|e| format!("sftp://{authority}: {e}"))?;
                ("ssh".into(), args)
            }
        };
        let mut child =
            sc_ssh::spawn(&program, &args).map_err(|e| format!("cannot start {program}: {e}"))?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let stream = tokio::io::join(stdout, stdin);
        match tokio::time::timeout(Duration::from_secs(60), Self::over(stream, None)).await {
            Ok(Ok(mut provider)) => {
                // Drain ssh's messages so it never blocks on a full pipe.
                tokio::spawn(async move {
                    let mut sink = Vec::new();
                    let _ = stderr.read_to_end(&mut sink).await;
                });
                provider._child = Some(child);
                Ok(provider)
            }
            failure => {
                let mut message = String::new();
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    stderr.read_to_string(&mut message),
                )
                .await;
                let reason = match failure {
                    Ok(Err(e)) => e,
                    _ => "timed out".into(),
                };
                let detail = message.trim();
                Err(format!(
                    "sftp://{authority}: {}",
                    if detail.is_empty() { reason.as_str() } else { detail }
                ))
            }
        }
    }

    /// The server path of `path`, with `~` expanded.
    fn remote(&self, path: &VfsPath) -> String {
        let inner = path.path();
        if inner == "/~" {
            return self.home.clone();
        }
        match inner.strip_prefix("/~/") {
            Some(rest) => format!("{}/{rest}", self.home.trim_end_matches('/')),
            None => inner.to_owned(),
        }
    }

    async fn exists(&self, path: &str) -> bool {
        self.session.symlink_metadata(path).await.is_ok()
    }
}

/// Maps an SFTP error for `path`. SFTP version 3 reports many conditions
/// as a generic "failure"; callers refine those where they know more.
fn error(error: SftpError, path: &VfsPath) -> VfsError {
    match &error {
        SftpError::Status(status) => match status.status_code {
            StatusCode::NoSuchFile => VfsError::NotFound(path.clone()),
            StatusCode::PermissionDenied => VfsError::PermissionDenied(path.clone()),
            StatusCode::OpUnsupported => {
                VfsError::Unsupported { operation: "this operation", scheme: "sftp".into() }
            }
            _ => VfsError::Other(format!("{path}: {}", status.error_message)),
        },
        _ => VfsError::Other(format!("{path}: {error}")),
    }
}

fn is_failure(error: &SftpError) -> bool {
    matches!(error, SftpError::Status(s) if s.status_code == StatusCode::Failure)
}

fn entry(name: String, attributes: &FileAttributes) -> Entry {
    let kind = match attributes.file_type() {
        FileType::Dir => EntryKind::Directory,
        FileType::File => EntryKind::File,
        FileType::Symlink => EntryKind::Symlink,
        FileType::Other => EntryKind::Other,
    };
    let mut entry = Entry::new(name, kind).with_size(attributes.size.unwrap_or(0));
    entry.modified = attributes.mtime.map(|t| UNIX_EPOCH + Duration::from_secs(u64::from(t)));
    entry.permissions = attributes.permissions.map(|p| p & 0o7777);
    entry.uid = attributes.uid;
    entry.gid = attributes.gid;
    entry
}

fn name_of(path: &str) -> String {
    path.rsplit('/').find(|p| !p.is_empty()).unwrap_or("/").to_owned()
}

#[async_trait]
impl Provider for SftpProvider {
    fn scheme(&self) -> &str {
        "sftp"
    }

    fn capabilities(&self) -> Capabilities {
        // Plain SFTP rename never replaces an existing file, so renames are
        // not advertised as atomic; the copy engine then writes in place.
        Capabilities::WRITE
            | Capabilities::SEEK_READ
            | Capabilities::RESUME_WRITE
            | Capabilities::POSIX_PERMISSIONS
            | Capabilities::SET_MTIME
            | Capabilities::SYMLINKS
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        let remote = self.remote(path);
        let attributes =
            self.session.symlink_metadata(remote.as_str()).await.map_err(|e| error(e, path))?;
        let mut result = entry(name_of(&remote), &attributes);
        if result.kind == EntryKind::Symlink {
            result.symlink_target = self.session.read_link(remote.as_str()).await.ok();
            result.link_is_dir =
                self.session.metadata(remote.as_str()).await.is_ok_and(|a| a.is_dir());
        }
        Ok(result)
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let remote = self.remote(path);
        let listing = self.session.read_dir(remote.as_str()).await.map_err(|e| error(e, path))?;
        let mut entries = Vec::new();
        for item in listing {
            let mut row = entry(item.file_name(), &item.metadata());
            if row.kind == EntryKind::Symlink {
                let full = format!("{}/{}", remote.trim_end_matches('/'), row.name);
                row.symlink_target = self.session.read_link(full.as_str()).await.ok();
                row.link_is_dir = self.session.metadata(full).await.is_ok_and(|a| a.is_dir());
            }
            entries.push(row);
        }
        let pages: Vec<Result<Vec<Entry>>> =
            entries.chunks(PAGE).map(|page| Ok(page.to_vec())).collect();
        Ok(Box::pin(stream::iter(pages)))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        let remote = self.remote(path);
        if self.session.metadata(remote.as_str()).await.map_err(|e| error(e, path))?.is_dir() {
            return Err(VfsError::IsADirectory(path.clone()));
        }
        let mut file = self.session.open(remote).await.map_err(|e| error(e, path))?;
        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| VfsError::from_io(path, e))?;
        }
        Ok(Box::pin(file))
    }

    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        let remote = self.remote(path);
        let flags = match options.mode {
            WriteMode::CreateNew => OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
            WriteMode::Truncate => OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
            WriteMode::ResumeAt(_) => OpenFlags::WRITE | OpenFlags::CREATE,
        };
        let opened = self.session.open_with_flags(remote.as_str(), flags).await;
        let mut file = match opened {
            Ok(file) => file,
            Err(e) if is_failure(&e) && self.exists(&remote).await => {
                return Err(VfsError::AlreadyExists(path.clone()));
            }
            Err(e) => return Err(error(e, path)),
        };
        if let WriteMode::ResumeAt(offset) = options.mode {
            // Drop anything after the part that is kept, then continue.
            let attributes = FileAttributes { size: Some(offset), ..FileAttributes::empty() };
            file.set_metadata(attributes).await.map_err(|e| error(e, path))?;
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| VfsError::from_io(path, e))?;
        }
        Ok(Box::pin(file))
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        let remote = self.remote(path);
        match self.session.create_dir(remote.as_str()).await {
            Ok(()) => Ok(()),
            Err(e) if is_failure(&e) && self.exists(&remote).await => {
                Err(VfsError::AlreadyExists(path.clone()))
            }
            Err(e) => Err(error(e, path)),
        }
    }

    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        let remote = self.remote(path);
        self.session.remove_file(remote).await.map_err(|e| error(e, path))
    }

    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        let remote = self.remote(path);
        match self.session.remove_dir(remote.as_str()).await {
            Ok(()) => Ok(()),
            Err(e) if is_failure(&e) => {
                let not_empty = self
                    .session
                    .read_dir(remote.as_str())
                    .await
                    .is_ok_and(|mut listing| listing.next().is_some());
                Err(if not_empty {
                    VfsError::DirectoryNotEmpty(path.clone())
                } else {
                    error(e, path)
                })
            }
            Err(e) => Err(error(e, path)),
        }
    }

    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        let (source, target) = (self.remote(from), self.remote(to));
        match self.session.rename(source, target.as_str()).await {
            Ok(()) => Ok(()),
            Err(e) if is_failure(&e) && self.exists(&target).await => {
                Err(VfsError::AlreadyExists(to.clone()))
            }
            Err(e) => Err(error(e, from)),
        }
    }

    async fn set_attributes(&self, path: &VfsPath, attributes: &SetAttributes) -> Result<()> {
        let remote = self.remote(path);
        let seconds = |t: SystemTime| {
            t.duration_since(UNIX_EPOCH).map(|d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
        };
        let mtime = attributes.modified.and_then(|t| seconds(t).ok());
        let changes = FileAttributes {
            permissions: attributes.permissions,
            // SFTP sets both times together.
            atime: mtime,
            mtime,
            ..FileAttributes::empty()
        };
        self.session.set_metadata(remote, changes).await.map_err(|e| error(e, path))
    }

    async fn create_symlink(&self, path: &VfsPath, target: &str) -> Result<()> {
        let remote = self.remote(path);
        // OpenSSH's server reads the two fields of SSH_FXP_SYMLINK in the
        // opposite order from the draft standard (target first), and the
        // library sends them in the standard order. Nearly every server
        // follows OpenSSH, so pass them swapped: `ln -s target remote`.
        self.session.symlink(target, remote).await.map_err(|e| error(e, path))
    }
}

/// Creates [`SftpProvider`]s for `sftp://` paths.
pub struct SftpFactory;

#[async_trait]
impl ProviderFactory for SftpFactory {
    fn scheme(&self) -> &str {
        "sftp"
    }

    async fn create(&self, path: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        let provider = SftpProvider::connect(path.authority()).await.map_err(VfsError::Other)?;
        Ok(Arc::new(provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_paths() {
        assert_eq!(name_of("/srv/www/"), "www");
        assert_eq!(name_of("/"), "/");
    }

    #[test]
    fn attributes_become_entries() {
        let mut attributes = FileAttributes::empty();
        attributes.size = Some(10);
        attributes.permissions = Some(0o100644);
        attributes.mtime = Some(1_000);
        let e = entry("f".into(), &attributes);
        assert_eq!((e.kind, e.size, e.permissions), (EntryKind::File, 10, Some(0o644)));
        assert_eq!(e.modified, Some(UNIX_EPOCH + Duration::from_secs(1_000)));
        attributes.permissions = Some(0o040755);
        assert_eq!(entry("d".into(), &attributes).kind, EntryKind::Directory);
    }
}
