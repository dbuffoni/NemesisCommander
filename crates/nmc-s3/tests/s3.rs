//! The S3 provider against a real S3 server (`s3s-fs`, which stores
//! buckets as folders) started in-process on a local port. The server
//! checks every request signature, so signing is tested too.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use nmc_ops::{ConflictPolicy, ExecuteOptions, Executor, PlanOptions, plan_copy};
use nmc_s3::S3Provider;
use nmc_s3::settings::Settings;
use nmc_s3::sigv4::Credentials;
use nmc_vfs::{EntryKind, Provider, VfsError, VfsPath, WriteMode, WriteOptions, collect_listing};
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const ACCESS_KEY: &str = "AKTEST";
const SECRET_KEY: &str = "secret/key+test";

/// Serves `root` (one folder per bucket); returns the endpoint URL.
async fn server(root: &Path) -> String {
    let mut builder = S3ServiceBuilder::new(s3s_fs::FileSystem::new(root).unwrap());
    builder.set_auth(SimpleAuth::from_single(ACCESS_KEY, SECRET_KEY));
    let service = builder.build();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let service = service.clone();
            tokio::spawn(async move {
                let connection = Builder::new(TokioExecutor::new());
                let _ = connection.serve_connection(TokioIo::new(socket), service).await;
            });
        }
    });
    endpoint
}

struct Fixture {
    root: TempDir,
    provider: S3Provider,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_secret(SECRET_KEY).await
    }

    async fn with_secret(secret: &str) -> Self {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("bucket")).unwrap();
        let endpoint = server(root.path()).await;
        let settings = Settings {
            credentials: Some(Credentials {
                access_key: ACCESS_KEY.into(),
                secret_key: secret.into(),
                session_token: None,
            }),
            region: "us-east-1".into(),
            endpoint: Some(endpoint),
        };
        Fixture { root, provider: S3Provider::new("bucket", settings).unwrap() }
    }

    /// The file behind an object.
    fn local(&self, key: &str) -> PathBuf {
        self.root.path().join("bucket").join(key)
    }

    fn remote(&self, key: &str) -> VfsPath {
        VfsPath::parse(&format!("s3://bucket/{key}")).unwrap()
    }
}

async fn read_all(provider: &dyn Provider, path: &VfsPath, offset: u64) -> Vec<u8> {
    let mut data = Vec::new();
    provider.read(path, offset).await.unwrap().read_to_end(&mut data).await.unwrap();
    data
}

async fn write_all(provider: &dyn Provider, path: &VfsPath, mode: WriteMode, data: &[u8]) {
    let mut sink = provider.write(path, WriteOptions { mode, size_hint: None }).await.unwrap();
    sink.write_all(data).await.unwrap();
    sink.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_wrong_key_is_refused() {
    let f = Fixture::with_secret("wrong").await;
    let error = f.provider.stat(&f.remote("")).await.unwrap_err();
    assert!(!matches!(error, VfsError::NotFound(_)), "{error}");
}

#[tokio::test]
async fn keys_are_shown_as_folders() {
    let f = Fixture::new().await;
    write_all(&f.provider, &f.remote("photos/2026/a & b.jpg"), WriteMode::CreateNew, b"jpeg").await;
    write_all(&f.provider, &f.remote("photos/top.txt"), WriteMode::CreateNew, b"hello s3").await;
    write_all(&f.provider, &f.remote("readme"), WriteMode::CreateNew, b"r").await;

    let root = collect_listing(&f.provider, &f.remote("")).await.unwrap();
    let mut names: Vec<(String, EntryKind)> = root.into_iter().map(|e| (e.name, e.kind)).collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        names,
        [("photos".into(), EntryKind::Directory), ("readme".into(), EntryKind::File)]
    );

    let photos = collect_listing(&f.provider, &f.remote("photos")).await.unwrap();
    assert_eq!(photos.len(), 2);
    let file = photos.iter().find(|e| e.name == "top.txt").unwrap();
    assert_eq!(file.size, 8);
    assert!(file.modified.is_some());

    let entry = f.provider.stat(&f.remote("photos/2026/a & b.jpg")).await.unwrap();
    assert_eq!((entry.kind, entry.size, entry.name.as_str()), (EntryKind::File, 4, "a & b.jpg"));
    assert!(f.provider.stat(&f.remote("photos/2026")).await.unwrap().is_dir());
    assert!(f.provider.stat(&f.remote("")).await.unwrap().is_dir());

    assert_eq!(read_all(&f.provider, &f.remote("photos/top.txt"), 6).await, b"s3");
    assert!(matches!(f.provider.list(&f.remote("readme")).await, Err(VfsError::NotADirectory(_))));
}

#[tokio::test]
async fn errors_keep_their_meaning() {
    let f = Fixture::new().await;
    write_all(&f.provider, &f.remote("a"), WriteMode::CreateNew, b"x").await;
    write_all(&f.provider, &f.remote("full/inner"), WriteMode::CreateNew, b"y").await;

    assert!(matches!(f.provider.stat(&f.remote("missing")).await, Err(VfsError::NotFound(_))));
    assert!(matches!(f.provider.list(&f.remote("missing")).await, Err(VfsError::NotFound(_))));
    assert!(matches!(f.provider.read(&f.remote("missing"), 0).await, Err(VfsError::NotFound(_))));
    let error = f.provider.write(&f.remote("a"), WriteOptions::create_new()).await.err().unwrap();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");
    let error = f.provider.remove_file(&f.remote("missing")).await.unwrap_err();
    assert!(matches!(error, VfsError::NotFound(_)), "{error}");
    let error = f.provider.remove_file(&f.remote("full")).await.unwrap_err();
    assert!(matches!(error, VfsError::IsADirectory(_)), "{error}");
    let error = f.provider.remove_dir(&f.remote("full")).await.unwrap_err();
    assert!(matches!(error, VfsError::DirectoryNotEmpty(_)), "{error}");
    let error = f.provider.create_dir(&f.remote("a")).await.unwrap_err();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");

    let missing = VfsPath::parse("s3://nobucket/").unwrap();
    let other = S3Provider::new("nobucket", f.provider_settings()).unwrap();
    assert!(matches!(other.stat(&missing).await, Err(VfsError::NotFound(_))));
}

impl Fixture {
    fn provider_settings(&self) -> Settings {
        // The same server and keys, for another bucket.
        let endpoint = self.provider_endpoint();
        Settings {
            credentials: Some(Credentials {
                access_key: ACCESS_KEY.into(),
                secret_key: SECRET_KEY.into(),
                session_token: None,
            }),
            region: "us-east-1".into(),
            endpoint: Some(endpoint),
        }
    }

    fn provider_endpoint(&self) -> String {
        self.provider.endpoint().to_owned()
    }
}

#[tokio::test]
async fn large_files_use_multipart_uploads() {
    let f = Fixture::new().await;
    let path = f.remote("big.bin");
    // Three parts: two full ones and a short last one.
    let data: Vec<u8> = (0..19_000_000u32).map(|i| (i % 251) as u8).collect();
    write_all(&f.provider, &path, WriteMode::CreateNew, &data).await;
    assert_eq!(fs::read(f.local("big.bin")).unwrap(), data);
    assert_eq!(f.provider.stat(&path).await.unwrap().size, 19_000_000);

    write_all(&f.provider, &path, WriteMode::Truncate, b"short").await;
    assert_eq!(read_all(&f.provider, &path, 0).await, b"short");
    let resume = WriteOptions { mode: WriteMode::ResumeAt(1), size_hint: None };
    assert!(matches!(f.provider.write(&path, resume).await, Err(VfsError::Unsupported { .. })));
}

#[tokio::test]
async fn folders_can_be_created_renamed_and_removed() {
    let f = Fixture::new().await;
    let folder = f.remote("new");
    f.provider.create_dir(&folder).await.unwrap();
    // s3s-fs keeps the `new/` marker as a real directory, which its
    // listings do not show (S3 itself lists the marker object), so an
    // empty folder cannot be checked here.
    assert!(f.local("new").is_dir());

    write_all(&f.provider, &folder.join("f"), WriteMode::CreateNew, b"1").await;
    f.provider.copy_within(&folder.join("f"), &folder.join("g")).await.unwrap();
    f.provider.rename(&folder.join("f"), &folder.join("h")).await.unwrap();
    let mut names: Vec<String> =
        collect_listing(&f.provider, &folder).await.unwrap().into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, ["g", "h"]);

    f.provider.rename(&folder, &f.remote("moved")).await.unwrap();
    assert!(matches!(f.provider.stat(&folder).await, Err(VfsError::NotFound(_))));
    assert_eq!(read_all(&f.provider, &f.remote("moved/h"), 0).await, b"1");

    for name in ["g", "h"] {
        f.provider.remove_file(&f.remote("moved").join(name)).await.unwrap();
    }
    f.provider.remove_dir(&f.remote("moved")).await.unwrap();
    let root = collect_listing(&f.provider, &f.remote("")).await.unwrap();
    assert!(root.is_empty(), "{root:?}");
}

#[tokio::test]
async fn copies_between_local_and_s3_with_the_normal_engine() {
    let f = Fixture::new().await;
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    fs::create_dir_all(source.join("deep/er")).unwrap();
    fs::write(source.join("one.txt"), "1").unwrap();
    fs::write(source.join("deep/er/two.bin"), vec![7u8; 700_000]).unwrap();
    fs::create_dir(work.path().join("back")).unwrap();

    let registry = nmc_providers::default_registry();
    let destination = f.remote("");
    let Fixture { root, provider } = f;
    registry.insert(&destination, Arc::new(provider));
    let registry = Arc::new(registry);
    let local = |p: &Path| VfsPath::from_local(p).unwrap();

    let options = ExecuteOptions { conflict: ConflictPolicy::Fail, ..Default::default() };
    let upload = plan_copy(&registry, &[local(&source)], &destination, PlanOptions::default())
        .await
        .unwrap();
    let report = Executor::new(registry.clone(), options.clone()).run(&upload).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let uploaded = root.path().join("bucket/source/deep/er/two.bin");
    assert_eq!(fs::read(uploaded).unwrap().len(), 700_000);

    let download = plan_copy(
        &registry,
        &[destination.join("source")],
        &local(&work.path().join("back")),
        PlanOptions::default(),
    )
    .await
    .unwrap();
    let report = Executor::new(registry, options).run(&download).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read(work.path().join("back/source/one.txt")).unwrap(), b"1");
    let two = fs::read(work.path().join("back/source/deep/er/two.bin")).unwrap();
    assert_eq!(two, vec![7u8; 700_000]);
}
