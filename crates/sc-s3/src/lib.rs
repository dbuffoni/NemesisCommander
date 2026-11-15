//! # sc-s3
//!
//! The `s3://[profile@]bucket/key` provider, for Amazon S3 and the many
//! services that speak the same API (MinIO, Cloudflare R2, Backblaze B2,
//! Wasabi, Ceph, ...). Keys, region and endpoint come from the usual AWS
//! environment variables and `~/.aws` files; see [`settings`].
//!
//! S3 has objects, not folders. As every S3 browser does, keys are shown
//! as a tree split at `/`: a "folder" is a common prefix, and creating one
//! stores an empty `name/` marker object. Renaming a folder copies and
//! deletes every object under it, so it is not atomic.
//!
//! Requests are signed with Signature Version 4 ([`sigv4`]) and sent with
//! reqwest; ADR 0011 explains why no S3 library is used.

pub mod settings;
pub mod sigv4;
mod xml;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::{StreamExt, TryStreamExt, stream};
use reqwest::{Method, Response, StatusCode};
use sc_vfs::{
    Capabilities, Entry, EntryKind, ListStream, Provider, ProviderFactory, ProviderRegistry,
    ReadStream, Result, VfsError, VfsPath, WriteMode, WriteOptions, WriteSink, upload_sink,
};
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

pub use settings::Settings;
use xml::ListPage;

/// Size of the parts of a multipart upload. Files smaller than this are
/// uploaded with one request. S3 allows at most 10 000 parts, so this
/// reaches 80 GB.
const PART_SIZE: usize = 8 * 1024 * 1024;

/// A bucket.
pub struct S3Provider {
    client: reqwest::Client,
    settings: Settings,
    bucket: String,
    /// `https://host[:port]` the requests go to.
    origin: String,
    /// The `Host` header (and the host of `origin`).
    host: String,
    /// `/bucket` with path-style addressing, empty with virtual hosts.
    path_prefix: String,
}

/// One S3 request being built.
struct Call<'a> {
    method: Method,
    key: &'a str,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Option<reqwest::Body>,
}

impl<'a> Call<'a> {
    fn new(method: Method, key: &'a str) -> Self {
        Call { method, key, query: Vec::new(), headers: Vec::new(), body: None }
    }

    fn query(mut self, name: &str, value: impl Into<String>) -> Self {
        self.query.push((name.to_owned(), value.into()));
        self
    }

    /// Adds a header; the name must be lowercase (it is signed).
    fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    fn body(mut self, body: impl Into<reqwest::Body>) -> Self {
        self.body = Some(body.into());
        self
    }
}

impl S3Provider {
    /// A provider for `bucket` with explicit settings.
    pub fn new(bucket: &str, settings: Settings) -> Result<Self> {
        if bucket.is_empty() {
            return Err(VfsError::Other("an S3 location needs a bucket: s3://bucket/".into()));
        }
        // reqwest is built without a default crypto provider; use ring.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(concat!("SuperCommander/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| VfsError::Other(format!("cannot start the HTTP client: {e}")))?;
        // Custom endpoints get path-style requests, which every
        // S3-compatible server understands. AWS gets virtual hosts, except
        // for bucket names with dots (they break the TLS certificate).
        let (origin, path_prefix) = match &settings.endpoint {
            Some(endpoint) => (endpoint.clone(), format!("/{bucket}")),
            None if bucket.contains('.') => {
                (format!("https://s3.{}.amazonaws.com", settings.region), format!("/{bucket}"))
            }
            None => {
                (format!("https://{bucket}.s3.{}.amazonaws.com", settings.region), String::new())
            }
        };
        let host = origin.split_once("://").map_or(origin.as_str(), |(_, h)| h);
        let host = host.split('/').next().unwrap_or(host).to_owned();
        Ok(S3Provider { client, settings, bucket: bucket.to_owned(), origin, host, path_prefix })
    }

    /// The object key of `path` (no leading slash).
    fn key(path: &VfsPath) -> String {
        path.path().trim_start_matches('/').to_owned()
    }

    /// The listing prefix for the folder `path` (`""` or ending with `/`).
    fn folder_prefix(path: &VfsPath) -> String {
        let key = Self::key(path);
        let key = key.trim_end_matches('/');
        if key.is_empty() { String::new() } else { format!("{key}/") }
    }

    /// Signs and sends a request.
    async fn send(&self, call: Call<'_>) -> std::result::Result<Response, reqwest::Error> {
        let path = format!("{}/{}", self.path_prefix, sigv4::encode_key(call.key));
        let mut url = format!("{}{path}", self.origin);
        if !call.query.is_empty() {
            let query: Vec<String> = call
                .query
                .iter()
                .map(|(k, v)| format!("{}={}", sigv4::encode(k), sigv4::encode(v)))
                .collect();
            url = format!("{url}?{}", query.join("&"));
        }
        let mut headers = call.headers.clone();
        if let Some(credentials) = &self.settings.credentials {
            let time = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%SZ").to_string();
            let request = sigv4::Request {
                method: call.method.as_str(),
                host: &self.host,
                path: &path,
                query: &call.query,
                headers: &call.headers,
            };
            headers.extend(sigv4::sign(&request, credentials, &self.settings.region, &time));
        }
        let mut request = self.client.request(call.method, url);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        if let Some(body) = call.body {
            request = request.body(body);
        }
        request.send().await
    }

    /// Sends a request and maps a failure for `path` to an error.
    async fn call(&self, call: Call<'_>, path: &VfsPath) -> Result<Response> {
        let response = self.send(call).await.map_err(|e| network(e, path))?;
        check(response, path).await
    }

    /// `HEAD` of an object: `None` when there is none.
    async fn head(&self, key: &str, path: &VfsPath) -> Result<Option<Response>> {
        let response =
            self.send(Call::new(Method::HEAD, key)).await.map_err(|e| network(e, path))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        check(response, path).await.map(Some)
    }

    /// One page of `ListObjectsV2`.
    async fn list_page(
        &self,
        prefix: &str,
        delimiter: bool,
        token: Option<String>,
        max: u32,
        path: &VfsPath,
    ) -> Result<ListPage> {
        let mut call = Call::new(Method::GET, "")
            .query("list-type", "2")
            .query("prefix", prefix)
            .query("max-keys", max.to_string());
        if delimiter {
            call = call.query("delimiter", "/");
        }
        if let Some(token) = token {
            call = call.query("continuation-token", token);
        }
        let text = self.call(call, path).await?.text().await.map_err(|e| network(e, path))?;
        xml::parse_list(&text).map_err(|e| VfsError::Other(format!("{path}: {e}")))
    }

    /// `true` if some key starts with the folder prefix (`name/`); on S3
    /// that includes the marker object of an empty folder.
    async fn folder_exists(&self, prefix: &str, path: &VfsPath) -> Result<bool> {
        let page = self.list_page(prefix, false, None, 1, path).await?;
        Ok(!page.objects.is_empty() || !page.prefixes.is_empty())
    }

    /// Every key under `prefix`, recursively.
    async fn all_keys(&self, prefix: &str, path: &VfsPath) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        let mut token = None;
        loop {
            let page = self.list_page(prefix, false, token, 1000, path).await?;
            keys.extend(page.objects.into_iter().map(|o| o.key));
            match page.next {
                Some(next) => token = Some(next),
                None => return Ok(keys),
            }
        }
    }

    async fn copy_object(&self, from: &str, to: &str, path: &VfsPath) -> Result<()> {
        let source = format!("/{}/{}", self.bucket, sigv4::encode_key(from));
        let response =
            self.call(Call::new(Method::PUT, to).header("x-amz-copy-source", source), path);
        let response = response.await?;
        // A copy can fail after the 200 status line: the body says so.
        let text = response.text().await.map_err(|e| network(e, path))?;
        match xml::parse_error(&text) {
            Some((code, message)) => Err(VfsError::Other(format!("{path}: {code}: {message}"))),
            None => Ok(()),
        }
    }

    async fn delete_object(&self, key: &str, path: &VfsPath) -> Result<()> {
        self.call(Call::new(Method::DELETE, key), path).await.map(drop)
    }

    /// Uploads `body` to `key`: one request for small files, a multipart
    /// upload for larger ones.
    async fn upload(
        self: Arc<Self>,
        key: String,
        path: VfsPath,
        create_new: bool,
        mut body: sc_vfs::UploadBody,
    ) -> Result<()> {
        let mut buffer = BytesMut::new();
        let mut upload: Option<(String, Vec<String>)> = None;
        let result = async {
            while let Some(chunk) = body.next().await {
                buffer.extend_from_slice(&chunk.map_err(|e| VfsError::from_io(&path, e))?);
                if buffer.len() >= PART_SIZE {
                    if upload.is_none() {
                        upload = Some((self.start_multipart(&key, &path).await?, Vec::new()));
                    }
                    let (id, etags) = upload.as_mut().expect("started above");
                    let part = buffer.split_to(PART_SIZE).freeze();
                    etags.push(self.upload_part(&key, id, etags.len() + 1, part, &path).await?);
                }
            }
            match upload.as_mut() {
                None => {
                    let mut call = Call::new(Method::PUT, &key)
                        .header("content-length", buffer.len().to_string());
                    if create_new {
                        call = call.header("if-none-match", "*");
                    }
                    self.call(call.body(buffer.split().freeze()), &path).await.map(drop)
                }
                Some((id, etags)) => {
                    if !buffer.is_empty() {
                        let part = buffer.split().freeze();
                        etags.push(self.upload_part(&key, id, etags.len() + 1, part, &path).await?);
                    }
                    let call = Call::new(Method::POST, &key)
                        .query("uploadId", id.clone())
                        .body(xml::complete_body(etags));
                    let response = self.call(call, &path).await?;
                    let text = response.text().await.map_err(|e| network(e, &path))?;
                    match xml::parse_error(&text) {
                        Some((code, message)) => {
                            Err(VfsError::Other(format!("{path}: {code}: {message}")))
                        }
                        None => Ok(()),
                    }
                }
            }
        }
        .await;
        if result.is_err()
            && let Some((id, _)) = upload
        {
            // Abandoned parts would be billed until deleted.
            let _ = self.send(Call::new(Method::DELETE, &key).query("uploadId", id)).await;
        }
        result
    }

    async fn start_multipart(&self, key: &str, path: &VfsPath) -> Result<String> {
        let call = Call::new(Method::POST, key).query("uploads", "");
        let text = self.call(call, path).await?.text().await.map_err(|e| network(e, path))?;
        xml::parse_upload_id(&text)
            .ok_or_else(|| VfsError::Other(format!("{path}: the server did not start the upload")))
    }

    async fn upload_part(
        &self,
        key: &str,
        id: &str,
        number: usize,
        data: Bytes,
        path: &VfsPath,
    ) -> Result<String> {
        let call = Call::new(Method::PUT, key)
            .query("partNumber", number.to_string())
            .query("uploadId", id)
            .header("content-length", data.len().to_string())
            .body(data);
        let response = self.call(call, path).await?;
        let etag = response.headers().get("etag").and_then(|v| v.to_str().ok());
        etag.map(str::to_owned)
            .ok_or_else(|| VfsError::Other(format!("{path}: the server sent no ETag for a part")))
    }
}

/// Maps an unsuccessful answer for `path` to an error, using the S3 error
/// code when the body has one.
async fn check(response: Response, path: &VfsPath) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let text = response.text().await.unwrap_or_default();
    let (code, message) = xml::parse_error(&text).unwrap_or_default();
    Err(match (status, code.as_str()) {
        (_, "NoSuchKey" | "NoSuchBucket" | "NoSuchUpload") | (StatusCode::NOT_FOUND, _) => {
            VfsError::NotFound(path.clone())
        }
        (_, "PreconditionFailed") | (StatusCode::PRECONDITION_FAILED, _) => {
            VfsError::AlreadyExists(path.clone())
        }
        (StatusCode::FORBIDDEN, "AccessDenied" | "") => VfsError::PermissionDenied(path.clone()),
        _ if !code.is_empty() => VfsError::Other(format!("{path}: {code}: {message}")),
        _ => VfsError::Other(format!("{path}: the server answered {status}")),
    })
}

fn network(error: reqwest::Error, path: &VfsPath) -> VfsError {
    VfsError::Other(format!("{path}: {error}"))
}

/// The last name of a key or prefix.
fn name_of(key: &str) -> String {
    key.trim_end_matches('/').rsplit('/').next().unwrap_or(key).to_owned()
}

/// The entries of a listing page: folders, then files. Keys ending with
/// `/` are folder markers (of the listed folder itself or of an empty
/// subfolder that also appears as a prefix) and are not shown as files.
fn page_entries(page: ListPage) -> Vec<Entry> {
    let folders = page.prefixes.into_iter().map(|p| folder(name_of(&p)));
    let files = page.objects.into_iter().filter(|o| !o.key.ends_with('/')).map(|o| {
        let mut entry = Entry::new(name_of(&o.key), EntryKind::File).with_size(o.size);
        entry.modified = o.modified;
        entry
    });
    folders.chain(files).collect()
}

fn folder(name: String) -> Entry {
    Entry::new(name, EntryKind::Directory)
}

#[async_trait]
impl Provider for S3Provider {
    fn scheme(&self) -> &str {
        "s3"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::WRITE | Capabilities::SEEK_READ | Capabilities::SERVER_COPY
    }

    async fn stat(&self, path: &VfsPath) -> Result<Entry> {
        let key = Self::key(path);
        let key = key.trim_end_matches('/');
        if key.is_empty() {
            // The bucket itself; a listing reports a missing bucket.
            self.list_page("", true, None, 1, path).await?;
            return Ok(folder("/".into()));
        }
        if let Some(response) = self.head(key, path).await? {
            let header = |name: &str| {
                response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
            };
            let size = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut entry = Entry::new(name_of(key), EntryKind::File).with_size(size);
            entry.modified =
                header("last-modified").and_then(|v| httpdate::parse_http_date(&v).ok());
            return Ok(entry);
        }
        if self.folder_exists(&format!("{key}/"), path).await? {
            return Ok(folder(name_of(key)));
        }
        Err(VfsError::NotFound(path.clone()))
    }

    async fn list(&self, path: &VfsPath) -> Result<ListStream> {
        let prefix = Self::folder_prefix(path);
        let first = self.list_page(&prefix, true, None, 1000, path).await?;
        if first.objects.is_empty() && first.prefixes.is_empty() && !prefix.is_empty() {
            // Nothing under the prefix: a file, or nothing at all.
            return Err(match self.stat(path).await {
                Ok(_) => VfsError::NotADirectory(path.clone()),
                Err(error) => error,
            });
        }
        let first_entries = page_entries(first.clone());
        // Later pages are fetched as the consumer reads them.
        let this = self.clone_handle();
        let path = path.clone();
        let rest = stream::try_unfold(first.next, move |token| {
            let (this, path, prefix) = (this.clone(), path.clone(), prefix.clone());
            async move {
                let Some(token) = token else { return Ok(None) };
                let page = this.list_page(&prefix, true, Some(token), 1000, &path).await?;
                let next = page.next.clone();
                Ok(Some((page_entries(page), next)))
            }
        });
        Ok(Box::pin(stream::once(async { Ok(first_entries) }).chain(rest)))
    }

    async fn read(&self, path: &VfsPath, offset: u64) -> Result<ReadStream> {
        let key = Self::key(path);
        let mut call = Call::new(Method::GET, &key);
        if offset > 0 {
            call = call.header("range", format!("bytes={offset}-"));
        }
        let response = self.send(call).await.map_err(|e| network(e, path))?;
        if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(Box::pin(tokio::io::empty()));
        }
        let response = check(response, path).await?;
        let ignores_range = offset > 0 && response.status() == StatusCode::OK;
        let body = response.bytes_stream().map_err(std::io::Error::other);
        let mut reader: ReadStream = Box::pin(StreamReader::new(body));
        if ignores_range {
            let mut skip = (&mut reader).take(offset);
            tokio::io::copy(&mut skip, &mut tokio::io::sink())
                .await
                .map_err(|e| VfsError::from_io(path, e))?;
        }
        Ok(reader)
    }

    async fn write(&self, path: &VfsPath, options: WriteOptions) -> Result<WriteSink> {
        let key = Self::key(path);
        if key.is_empty() || key.ends_with('/') {
            return Err(VfsError::IsADirectory(path.clone()));
        }
        match options.mode {
            WriteMode::ResumeAt(_) => return Err(self.unsupported_on(path, "resume")),
            WriteMode::CreateNew => {
                if self.head(&key, path).await?.is_some() {
                    return Err(VfsError::AlreadyExists(path.clone()));
                }
            }
            WriteMode::Truncate => {}
        }
        let this = self.clone_handle();
        let path = path.clone();
        let create_new = options.mode == WriteMode::CreateNew;
        Ok(upload_sink(move |body| this.upload(key, path, create_new, body)))
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<()> {
        if self.stat(path).await.is_ok() {
            return Err(VfsError::AlreadyExists(path.clone()));
        }
        // There is no hierarchy to check: `a/b/` makes `a` appear too.
        let marker = Self::folder_prefix(path);
        let call = Call::new(Method::PUT, &marker).header("content-length", "0").body(Bytes::new());
        self.call(call, path).await.map(drop)
    }

    async fn remove_file(&self, path: &VfsPath) -> Result<()> {
        let key = Self::key(path);
        // Deleting a missing key succeeds on S3: check first.
        if self.head(&key, path).await?.is_none() {
            return Err(match self.stat(path).await {
                Ok(_) => VfsError::IsADirectory(path.clone()),
                Err(error) => error,
            });
        }
        self.delete_object(&key, path).await
    }

    async fn remove_dir(&self, path: &VfsPath) -> Result<()> {
        let prefix = Self::folder_prefix(path);
        let page = self.list_page(&prefix, true, None, 2, path).await?;
        let contents =
            page.objects.iter().filter(|o| o.key != prefix).count() + page.prefixes.len();
        if contents > 0 {
            return Err(VfsError::DirectoryNotEmpty(path.clone()));
        }
        if self.head(prefix.trim_end_matches('/'), path).await?.is_some() {
            return Err(VfsError::NotADirectory(path.clone()));
        }
        // A folder that only existed through its contents disappears with
        // its last file; deleting its (missing) marker then succeeds.
        self.delete_object(&prefix, path).await
    }

    async fn rename(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        if self.stat(from).await?.is_dir() {
            if self.stat(to).await.is_ok() {
                return Err(VfsError::AlreadyExists(to.clone()));
            }
            // Copy everything first, then delete: an interruption leaves
            // both copies rather than losing objects.
            let source = Self::folder_prefix(from);
            let target = Self::folder_prefix(to);
            let keys = self.all_keys(&source, from).await?;
            for key in &keys {
                let renamed = format!("{target}{}", &key[source.len()..]);
                self.copy_object(key, &renamed, from).await?;
            }
            for key in &keys {
                self.delete_object(key, from).await?;
            }
            return Ok(());
        }
        let key = Self::key(from);
        self.copy_object(&key, &Self::key(to), from).await?;
        self.delete_object(&key, from).await
    }

    async fn copy_within(&self, from: &VfsPath, to: &VfsPath) -> Result<()> {
        self.copy_object(&Self::key(from), &Self::key(to), from).await
    }
}

impl S3Provider {
    /// Where requests go: the custom endpoint, or the AWS host.
    pub fn endpoint(&self) -> &str {
        &self.origin
    }

    /// A second handle on the same connection pool and settings, for work
    /// that outlives `&self` (later listing pages, uploads).
    fn clone_handle(&self) -> Arc<Self> {
        Arc::new(S3Provider {
            client: self.client.clone(),
            settings: self.settings.clone(),
            bucket: self.bucket.clone(),
            origin: self.origin.clone(),
            host: self.host.clone(),
            path_prefix: self.path_prefix.clone(),
        })
    }
}

/// Creates [`S3Provider`]s for `s3://` paths.
pub struct S3Factory;

#[async_trait]
impl ProviderFactory for S3Factory {
    fn scheme(&self) -> &str {
        "s3"
    }

    async fn create(&self, path: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        let (profile, bucket) = match path.authority().rsplit_once('@') {
            Some((profile, bucket)) => (Some(profile), bucket),
            None => (None, path.authority()),
        };
        Ok(Arc::new(S3Provider::new(bucket, settings::resolve(profile))?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(endpoint: Option<&str>) -> Settings {
        Settings {
            credentials: None,
            region: "eu-west-1".into(),
            endpoint: endpoint.map(str::to_owned),
        }
    }

    #[test]
    fn addressing_depends_on_the_endpoint() {
        let aws = S3Provider::new("photos", settings(None)).unwrap();
        assert_eq!(aws.origin, "https://photos.s3.eu-west-1.amazonaws.com");
        assert_eq!(
            (aws.host.as_str(), aws.path_prefix.as_str()),
            ("photos.s3.eu-west-1.amazonaws.com", "")
        );

        let dotted = S3Provider::new("my.photos", settings(None)).unwrap();
        assert_eq!(dotted.path_prefix, "/my.photos");

        let minio = S3Provider::new("photos", settings(Some("http://nas:9000"))).unwrap();
        assert_eq!((minio.host.as_str(), minio.path_prefix.as_str()), ("nas:9000", "/photos"));
        assert!(S3Provider::new("", settings(None)).is_err());
    }

    #[test]
    fn keys_and_prefixes() {
        let path = VfsPath::parse("s3://b/a/b/").unwrap();
        assert_eq!(S3Provider::folder_prefix(&path), "a/b/");
        assert_eq!(S3Provider::folder_prefix(&VfsPath::parse("s3://b/").unwrap()), "");
        assert_eq!(S3Provider::key(&VfsPath::parse("s3://b/x y.txt").unwrap()), "x y.txt");
        assert_eq!(name_of("a/b/"), "b");
        assert_eq!(name_of("file"), "file");
    }
}
