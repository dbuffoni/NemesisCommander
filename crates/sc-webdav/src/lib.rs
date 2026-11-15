//! # sc-webdav
//!
//! The WebDAV provider: `davs://[user@]host[:port]/path` over HTTPS and
//! `dav://...` over plain HTTP (for a local network or a test server).
//! The path is the server path, so a Nextcloud folder is for example
//! `davs://alice@cloud.example.com/remote.php/dav/files/alice/`.
//!
//! Logins come from the user's `~/.netrc` (see [`netrc`]); SuperCommander
//! does not store passwords. Basic authentication is used, sent only once
//! the server asks for it.
//!
//! WebDAV has no partial writes and no way to set modification times or
//! permissions, so the provider supports neither resume nor attribute
//! changes. Renames and copies happen on the server (`MOVE`, `COPY`).

pub use sc_vfs::netrc;
mod xml;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt, stream};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use reqwest::header::{CONTENT_TYPE, HeaderValue};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use sc_vfs::{
    Capabilities, Entry, EntryKind, ListStream, Provider, ProviderFactory, ProviderRegistry,
    ReadStream, Result, VfsError, VfsPath, WriteMode, WriteOptions, WriteSink, upload_sink,
};
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

pub use xml::{Resource, parse_multistatus};

/// Entries per page delivered to the user interface.
const PAGE: usize = 512;

/// Characters escaped in a path segment.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// A WebDAV server.
pub struct WebDavProvider {
    scheme: String,
    client: reqwest::Client,
    /// `https://host:port`, without a trailing slash.
    origin: String,
    login: Option<netrc::Login>,
    /// The server asked for the login once: send it with every request.
    wants_login: AtomicBool,
}

impl WebDavProvider {
    /// A provider for `authority` (`[user@]host[:port]`). `secure` selects
    /// HTTPS. The login is looked up in the netrc file.
    pub fn new(authority: &str, secure: bool) -> Result<Self> {
        let (user, host) = match authority.rsplit_once('@') {
            Some((user, host)) => (Some(user), host),
            None => (None, authority),
        };
        if host.is_empty() {
            return Err(VfsError::Other("a WebDAV location needs a host name".into()));
        }
        let host_name = host.rsplit_once(':').map_or(host, |(name, _)| name);
        let login = netrc::lookup(host_name, user).or_else(|| {
            user.map(|user| netrc::Login { user: user.to_owned(), password: String::new() })
        });
        let http = if secure { "https" } else { "http" };
        Self::with_login(&format!("{http}://{host}"), if secure { "davs" } else { "dav" }, login)
    }

    /// A provider for the server at `origin` (`http[s]://host[:port]`),
    /// serving `scheme`, with an explicit login.
    pub fn with_login(origin: &str, scheme: &str, login: Option<netrc::Login>) -> Result<Self> {
        // reqwest is built without a default crypto provider; use ring.
        // Installing fails harmlessly when one is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(concat!("SuperCommander/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| VfsError::Other(format!("cannot start the HTTP client: {e}")))?;
        Ok(WebDavProvider {
            scheme: scheme.to_owned(),
            client,
            origin: origin.trim_end_matches('/').to_owned(),
            login,
            wants_login: AtomicBool::new(false),
        })
    }

    /// The URL of `path`; collections get a trailing slash, which many
    /// servers require.
    fn url(&self, path: &VfsPath, collection: bool) -> String {
        let mut url = self.origin.clone();
        for segment in path.path().split('/').filter(|s| !s.is_empty()) {
            url.push('/');
            url.extend(utf8_percent_encode(segment, SEGMENT));
        }
        if collection || path.is_root() {
            url.push('/');
        }
        url
    }

    /// Sends a request, adding the login when the server asks for one.
    async fn send(
        &self,
        method: Method,
        url: &str,
        setup: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> std::result::Result<Response, reqwest::Error> {
        if self.wants_login.load(Ordering::Relaxed) {
            return setup(self.authorize(self.client.request(method, url))).send().await;
        }
        let response = setup(self.client.request(method.clone(), url)).send().await?;
        if response.status() == StatusCode::UNAUTHORIZED && self.login.is_some() {
            self.wants_login.store(true, Ordering::Relaxed);
            return setup(self.authorize(self.client.request(method, url))).send().await;
        }
        Ok(response)
    }

    /// Adds the login. Uploads always send it up front, because their body
    /// can be sent only once.
    fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.login {
            Some(login) => request.basic_auth(&login.user, Some(&login.password)),
            None => request,
        }
    }

    /// `PROPFIND` with the given depth.
    async fn propfind(&self, path: &VfsPath, depth: &str) -> Result<Vec<Resource>> {
        let method = Method::from_bytes(b"PROPFIND").expect("valid method");
        let url = self.url(path, depth != "0");
        let response = self
            .send(method, &url, |r| {
                r.header("Depth", depth)
                    .header(CONTENT_TYPE, "application/xml; charset=utf-8")
                    .body(xml::PROPFIND_BODY)
            })
            .await
            .map_err(|e| network(e, path))?;
        let response = check(response, path)?;
        let text = response.text().await.map_err(|e| network(e, path))?;
        parse_multistatus(&text).map_err(|e| VfsError::Other(format!("{path}: {e}")))
    }

    /// The server path of a `href`, without a trailing slash.
    fn href_path(&self, href: &str) -> String {
        let path = match href.find("://") {
            Some(start) => {
                let rest = &href[start + 3..];
                rest.find('/').map_or("/", |slash| &rest[slash..])
            }
            None => href,
        };
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() { "/".into() } else { trimmed.into() }
    }

    /// `MOVE` or `COPY` to `to`, replacing a file there. A folder at `to` is
    /// never replaced: WebDAV would delete it with all its contents.
    async fn transfer(&self, method: &[u8], from: &VfsPath, to: &VfsPath) -> Result<()> {
        if let Ok(existing) = self.stat(to).await
            && existing.is_dir()
        {
            return Err(VfsError::AlreadyExists(to.clone()));
        }
        let source = self.stat(from).await?;
        let method = Method::from_bytes(method).expect("valid method");
        let destination = self.url(to, source.is_dir());
        let response = self
            .send(method, &self.url(from, source.is_dir()), |r| {
                r.header("Destination", &destination).header("Overwrite", "T")
            })
            .await
            .map_err(|e| network(e, from))?;
        match response.status() {
            // 409: the destination's folder is missing.
            StatusCode::CONFLICT => Err(VfsError::NotFound(to.clone())),
            _ => check(response, from).map(drop),
        }
    }
}

/// Maps an unsuccessful HTTP status for `path` to an error.
fn check(response: Response, path: &VfsPath) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(match status {
        StatusCode::NOT_FOUND | StatusCode::GONE => VfsError::NotFound(path.clone()),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::LOCKED => {
            VfsError::PermissionDenied(path.clone())
        }
        StatusCode::PRECONDITION_FAILED => VfsError::AlreadyExists(path.clone()),
        StatusCode::INSUFFICIENT_STORAGE => {
            VfsError::Other(format!("{path}: the server is out of space"))
        }
        _ => VfsError::Other(format!("{path}: the server answered {status}")),
    })
}

fn network(error: reqwest::Error, path: &VfsPath) -> VfsError {
    VfsError::Other(format!("{path}: {error}"))
}

fn entry(name: String, resource: &Resource) -> Entry {
    let kind = if resource.is_collection { EntryKind::Directory } else { EntryKind::File };
    let mut entry = Entry::new(name, kind).with_size(resource.length.unwrap_or(0));
    entry.modified = resource.modified;
    entry
}

fn name_of(path: &str) -> String {
    path.rsplit('/').find(|p| !p.is_empty()).unwrap_or("/").to_owned()
}

#[async_trait]
impl Provider for WebDavProvider {
    fn scheme(&self) -> &str {
        &self.scheme
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::WRITE
            | Capabilities::SEEK_READ
            | Capabilities::ATOMIC_RENAME
            | Capabilities::SERVER_COPY
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        let resources = self.propfind(path, "0").await?;
        let resource = resources.first().ok_or_else(|| VfsError::NotFound(path.clone()))?;
        Ok(entry(name_of(path.path()), resource))
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let resources = self.propfind(path, "1").await?;
        let own = self.href_path(path.path());
        let mut entries = Vec::new();
        let mut is_collection = false;
        for resource in &resources {
            let href = self.href_path(&resource.href);
            if href == own {
                is_collection = resource.is_collection;
                continue;
            }
            entries.push(entry(name_of(&href), resource));
        }
        if !is_collection && entries.is_empty() && !resources.is_empty() {
            return Err(VfsError::NotADirectory(path.clone()));
        }
        let pages: Vec<Result<Vec<Entry>>> =
            entries.chunks(PAGE).map(|page| Ok(page.to_vec())).collect();
        Ok(Box::pin(stream::iter(pages)))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        let range = format!("bytes={offset}-");
        let response = self
            .send(Method::GET, &self.url(path, false), |r| {
                if offset > 0 { r.header("Range", &range) } else { r }
            })
            .await
            .map_err(|e| network(e, path))?;
        if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
            // Reading from the end of the file.
            return Ok(Box::pin(tokio::io::empty()));
        }
        let response = check(response, path)?;
        let ignores_range = offset > 0 && response.status() == StatusCode::OK;
        let body = response.bytes_stream().map_err(std::io::Error::other);
        let mut reader: ReadStream = Box::pin(StreamReader::new(body));
        if ignores_range {
            // The server sent the whole file: skip to the offset.
            let mut skip = (&mut reader).take(offset);
            tokio::io::copy(&mut skip, &mut tokio::io::sink())
                .await
                .map_err(|e| VfsError::from_io(path, e))?;
        }
        Ok(reader)
    }

    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        match options.mode {
            WriteMode::ResumeAt(_) => return Err(self.unsupported_on(path, "resume")),
            // Fail early; `If-None-Match` below closes the race.
            WriteMode::CreateNew => {
                if self.stat(path).await.is_ok() {
                    return Err(VfsError::AlreadyExists(path.clone()));
                }
            }
            WriteMode::Truncate => {}
        }
        let mut request = self.authorize(self.client.put(self.url(path, false)));
        if options.mode == WriteMode::CreateNew {
            request = request.header("If-None-Match", "*");
        }
        if let Some(size) = options.size_hint {
            request = request.header("Content-Length", HeaderValue::from(size));
        }
        let path = path.clone();
        Ok(upload_sink(move |body| async move {
            let response = request
                .body(reqwest::Body::wrap_stream(body))
                .send()
                .await
                .map_err(|e| network(e, &path))?;
            match response.status() {
                // 409: the folder is missing; 405: a folder is in the way.
                StatusCode::CONFLICT => Err(VfsError::NotFound(path)),
                StatusCode::METHOD_NOT_ALLOWED => Err(VfsError::IsADirectory(path)),
                _ => check(response, &path).map(drop),
            }
        }))
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        let method = Method::from_bytes(b"MKCOL").expect("valid method");
        let response =
            self.send(method, &self.url(path, true), |r| r).await.map_err(|e| network(e, path))?;
        match response.status() {
            // 405: something already exists there.
            StatusCode::METHOD_NOT_ALLOWED => Err(VfsError::AlreadyExists(path.clone())),
            // 409: the parent folder is missing.
            StatusCode::CONFLICT => Err(VfsError::NotFound(path.clone())),
            status if status.is_success() => Ok(()),
            // Some servers fail with a generic error when a file is in
            // the way: report that plainly.
            _ if self.stat(path).await.is_ok() => Err(VfsError::AlreadyExists(path.clone())),
            _ => check(response, path).map(drop),
        }
    }

    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        // DELETE removes folders with their contents: make sure it is a file.
        if self.stat(path).await?.is_dir() {
            return Err(VfsError::IsADirectory(path.clone()));
        }
        let response = self
            .send(Method::DELETE, &self.url(path, false), |r| r)
            .await
            .map_err(|e| network(e, path))?;
        check(response, path).map(drop)
    }

    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        let mut listing = self.list(path).await?;
        while let Some(page) = listing.next().await {
            if !page?.is_empty() {
                return Err(VfsError::DirectoryNotEmpty(path.clone()));
            }
        }
        let response = self
            .send(Method::DELETE, &self.url(path, true), |r| r)
            .await
            .map_err(|e| network(e, path))?;
        check(response, path).map(drop)
    }

    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        self.transfer(b"MOVE", from, to).await
    }

    async fn copy_within(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        self.transfer(b"COPY", from, to).await
    }
}

/// Creates [`WebDavProvider`]s for `dav://` (HTTP) or `davs://` (HTTPS).
pub struct WebDavFactory {
    secure: bool,
}

impl WebDavFactory {
    /// The factory for `davs://` paths.
    pub fn https() -> Self {
        WebDavFactory { secure: true }
    }

    /// The factory for `dav://` paths.
    pub fn http() -> Self {
        WebDavFactory { secure: false }
    }
}

#[async_trait]
impl ProviderFactory for WebDavFactory {
    fn scheme(&self) -> &str {
        if self.secure { "davs" } else { "dav" }
    }

    async fn create(&self, path: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        Ok(Arc::new(WebDavProvider::new(path.authority(), self.secure)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> WebDavProvider {
        WebDavProvider::with_login("https://host:8443/", "davs", None).unwrap()
    }

    #[test]
    fn urls_escape_each_segment() {
        let p = provider();
        let path = VfsPath::parse("davs://host:8443/My Docs/50% #1.txt").unwrap();
        assert_eq!(p.url(&path, false), "https://host:8443/My%20Docs/50%25%20%231.txt");
        assert_eq!(p.url(&path.parent().unwrap(), true), "https://host:8443/My%20Docs/");
        assert_eq!(
            p.url(&VfsPath::parse("davs://host:8443/").unwrap(), false),
            "https://host:8443/"
        );
    }

    #[test]
    fn hrefs_become_paths() {
        let p = provider();
        assert_eq!(p.href_path("/a/b/"), "/a/b");
        assert_eq!(p.href_path("https://host:8443/a/b"), "/a/b");
        assert_eq!(p.href_path("https://host:8443"), "/");
        assert_eq!(p.href_path("/"), "/");
    }
}
