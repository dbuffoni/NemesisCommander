//! The WebDAV provider against a real WebDAV server (the `dav-server`
//! crate serving a temporary folder), started in-process on a local port.
//! The server asks for a Basic login, like most real deployments.

use std::convert::Infallible;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use dav_server::DavHandler;
use dav_server::fakels::FakeLs;
use dav_server::localfs::LocalFs;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use pnxr_ops::{ConflictPolicy, ExecuteOptions, Executor, PlanOptions, plan_copy};
use pnxr_vfs::{EntryKind, Provider, VfsError, VfsPath, WriteMode, WriteOptions, collect_listing};
use pnxr_webdav::WebDavProvider;
use pnxr_webdav::netrc::Login;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// "alice:s3cret", as sent in a Basic `Authorization` header.
const AUTHORIZATION: &str = "Basic YWxpY2U6czNjcmV0";

/// Serves `root` over WebDAV; returns the `http://127.0.0.1:port` origin.
async fn server(root: &Path) -> String {
    let handler = DavHandler::builder()
        .filesystem(LocalFs::new(root, false, false, false))
        .locksystem(FakeLs::new())
        .build_handler();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let handler = handler.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<_>| {
                    let handler = handler.clone();
                    async move {
                        let authorized = request
                            .headers()
                            .get("authorization")
                            .is_some_and(|value| value == AUTHORIZATION);
                        if !authorized {
                            let response = hyper::Response::builder()
                                .status(401)
                                .header("WWW-Authenticate", "Basic realm=\"test\"")
                                .body(dav_server::body::Body::from("login required"))
                                .unwrap();
                            return Ok::<_, Infallible>(response);
                        }
                        Ok(handler.handle(request).await)
                    }
                });
                let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
            });
        }
    });
    origin
}

struct Fixture {
    dir: TempDir,
    provider: WebDavProvider,
}

impl Fixture {
    async fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let origin = server(dir.path()).await;
        let login = Login { user: "alice".into(), password: "s3cret".into() };
        let provider = WebDavProvider::with_login(&origin, "dav", Some(login)).unwrap();
        Fixture { dir, provider }
    }

    fn local(&self, relative: &str) -> std::path::PathBuf {
        self.dir.path().join(relative)
    }

    fn remote(&self, relative: &str) -> VfsPath {
        VfsPath::parse(&format!("dav://test/{relative}")).unwrap()
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
async fn a_wrong_login_is_refused() {
    let dir = TempDir::new().unwrap();
    let origin = server(dir.path()).await;
    let login = Login { user: "alice".into(), password: "wrong".into() };
    let provider = WebDavProvider::with_login(&origin, "dav", Some(login)).unwrap();
    let root = VfsPath::parse("dav://test/").unwrap();
    assert!(matches!(provider.stat(&root).await, Err(VfsError::PermissionDenied(_))));
}

#[tokio::test]
async fn stat_list_and_read() {
    let f = Fixture::new().await;
    fs::create_dir(f.local("My Docs")).unwrap();
    fs::write(f.local("My Docs/café & co.txt"), "hello webdav").unwrap();

    let entry = f.provider.stat(&f.remote("My Docs/café & co.txt")).await.unwrap();
    assert_eq!((entry.kind, entry.size), (EntryKind::File, 12));
    assert_eq!(entry.name, "café & co.txt");
    assert!(entry.modified.is_some());
    assert!(f.provider.stat(&f.remote("My Docs")).await.unwrap().is_dir());

    let listing = collect_listing(&f.provider, &f.remote("My Docs")).await.unwrap();
    let names: Vec<&str> = listing.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["café & co.txt"], "the folder itself is not listed");
    let root = collect_listing(&f.provider, &f.remote("")).await.unwrap();
    assert_eq!(root.len(), 1);
    assert!(root[0].is_dir());

    let file = f.remote("My Docs/café & co.txt");
    assert_eq!(read_all(&f.provider, &file, 0).await, b"hello webdav");
    assert_eq!(read_all(&f.provider, &file, 6).await, b"webdav", "reads start at an offset");
    assert!(matches!(f.provider.list(&file).await, Err(VfsError::NotADirectory(_))));
}

#[tokio::test]
async fn errors_keep_their_meaning() {
    let f = Fixture::new().await;
    fs::write(f.local("a"), "x").unwrap();
    fs::create_dir_all(f.local("full/inner")).unwrap();

    assert!(matches!(f.provider.stat(&f.remote("missing")).await, Err(VfsError::NotFound(_))));
    assert!(matches!(f.provider.read(&f.remote("missing"), 0).await, Err(VfsError::NotFound(_))));

    let error = f.provider.write(&f.remote("a"), WriteOptions::create_new()).await.err().unwrap();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");
    let error = f.provider.create_dir(&f.remote("a")).await.unwrap_err();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");
    let error = f.provider.create_dir(&f.remote("no/such/parent")).await.unwrap_err();
    assert!(matches!(error, VfsError::NotFound(_)), "{error}");

    let error = f.provider.remove_dir(&f.remote("full")).await.unwrap_err();
    assert!(matches!(error, VfsError::DirectoryNotEmpty(_)), "{error}");
    let error = f.provider.remove_file(&f.remote("full")).await.unwrap_err();
    assert!(matches!(error, VfsError::IsADirectory(_)), "{error}");
    assert!(f.local("full/inner").exists(), "nothing was deleted");
}

#[tokio::test]
async fn writes_and_replaces_files() {
    let f = Fixture::new().await;
    let path = f.remote("big.bin");
    let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    write_all(&f.provider, &path, WriteMode::CreateNew, &data).await;
    assert_eq!(fs::read(f.local("big.bin")).unwrap(), data);

    write_all(&f.provider, &path, WriteMode::Truncate, b"short").await;
    assert_eq!(fs::read(f.local("big.bin")).unwrap(), b"short");

    let resume =
        f.provider.write(&path, WriteOptions { mode: WriteMode::ResumeAt(2), size_hint: None });
    assert!(matches!(resume.await, Err(VfsError::Unsupported { .. })));
}

#[tokio::test]
async fn folder_operations_happen_on_the_server() {
    let f = Fixture::new().await;
    let folder = f.remote("new");
    f.provider.create_dir(&folder).await.unwrap();
    write_all(&f.provider, &folder.join("f"), WriteMode::CreateNew, b"1").await;
    f.provider.rename(&folder.join("f"), &folder.join("g")).await.unwrap();
    f.provider.copy_within(&folder.join("g"), &folder.join("h")).await.unwrap();
    assert_eq!(fs::read(f.local("new/h")).unwrap(), b"1");

    // Renaming over a file replaces it; over a folder it is refused.
    write_all(&f.provider, &folder.join("i"), WriteMode::CreateNew, b"2").await;
    f.provider.rename(&folder.join("i"), &folder.join("h")).await.unwrap();
    assert_eq!(fs::read(f.local("new/h")).unwrap(), b"2");
    f.provider.create_dir(&folder.join("sub")).await.unwrap();
    let error = f.provider.rename(&folder.join("h"), &folder.join("sub")).await.unwrap_err();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");

    f.provider.rename(&folder.join("sub"), &folder.join("moved")).await.unwrap();
    assert!(f.local("new/moved").is_dir());

    for name in ["g", "h"] {
        f.provider.remove_file(&folder.join(name)).await.unwrap();
    }
    f.provider.remove_dir(&folder.join("moved")).await.unwrap();
    f.provider.remove_dir(&folder).await.unwrap();
    assert!(!f.local("new").exists());
}

#[tokio::test]
async fn copies_between_local_and_webdav_with_the_normal_engine() {
    let f = Fixture::new().await;
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    fs::create_dir_all(source.join("deep/er")).unwrap();
    fs::write(source.join("one.txt"), "1").unwrap();
    fs::write(source.join("deep/er/two.bin"), vec![7u8; 700_000]).unwrap();
    fs::create_dir(work.path().join("back")).unwrap();

    let registry = pnxr_providers::default_registry();
    let destination = f.remote("");
    let Fixture { dir, provider } = f;
    registry.insert(&destination, Arc::new(provider));
    let registry = Arc::new(registry);
    let local = |p: &Path| VfsPath::from_local(p).unwrap();

    let options = ExecuteOptions { conflict: ConflictPolicy::Fail, ..Default::default() };
    let upload = plan_copy(&registry, &[local(&source)], &destination, PlanOptions::default())
        .await
        .unwrap();
    let report = Executor::new(registry.clone(), options.clone()).run(&upload).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read(dir.path().join("source/deep/er/two.bin")).unwrap().len(), 700_000);
    let leftovers: Vec<_> = fs::read_dir(dir.path().join("source"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().contains("pnxr-part"))
        .collect();
    assert!(leftovers.is_empty(), "part files were renamed: {leftovers:?}");

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
