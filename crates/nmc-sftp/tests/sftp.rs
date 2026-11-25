//! The SFTP provider against OpenSSH's real `sftp-server`, started as a
//! local process: the same server program that `ssh -s host sftp` runs on
//! a remote machine. Tests are skipped when `sftp-server` is not installed
//! (on Debian and Ubuntu it is in the `openssh-sftp-server` package).
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use nmc_ops::{ConflictPolicy, ExecuteOptions, Executor, PlanOptions, plan_copy};
use nmc_sftp::SftpProvider;
use nmc_vfs::{
    Capabilities, EntryKind, Provider, SetAttributes, VfsError, VfsPath, WriteMode, WriteOptions,
    collect_listing,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Where distributions install OpenSSH's SFTP server.
const SERVERS: &[&str] = &[
    "/usr/lib/openssh/sftp-server",
    "/usr/libexec/openssh/sftp-server",
    "/usr/libexec/sftp-server",
    "/usr/lib/ssh/sftp-server",
];

fn sftp_server() -> Option<PathBuf> {
    let found = SERVERS.iter().map(PathBuf::from).find(|p| p.exists());
    if found.is_none() {
        skip("no sftp-server installed");
    }
    found
}

/// Starts a local `sftp-server` and an SFTP session on its standard streams.
async fn connect(server: &Path) -> SftpProvider {
    let mut child = Command::new(server)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stream = tokio::io::join(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    SftpProvider::over(stream, Some(child)).await.unwrap()
}

/// Opens a session, or returns from the test when there is no server.
macro_rules! provider {
    () => {
        match sftp_server() {
            Some(server) => connect(&server).await,
            None => return,
        }
    };
}

/// The `sftp://test/...` path of a local path.
fn remote(path: &Path) -> VfsPath {
    VfsPath::parse(&format!("sftp://test{}", path.display())).unwrap()
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
async fn stat_list_and_read() {
    let provider = provider!();
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("a.txt"), "hello sftp").unwrap();

    assert!(provider.capabilities().contains(Capabilities::WRITE));
    let entry = provider.stat(&remote(&dir.path().join("a.txt"))).await.unwrap();
    assert_eq!((entry.kind, entry.size), (EntryKind::File, 10));
    assert!(entry.modified.is_some() && entry.permissions.is_some());
    assert_eq!(provider.stat(&remote(&dir.path().join("sub"))).await.unwrap().kind, {
        EntryKind::Directory
    });

    let mut names: Vec<String> = collect_listing(&provider, &remote(dir.path()))
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    assert_eq!(names, ["a.txt", "sub"], "no . and .. entries");

    let path = remote(&dir.path().join("a.txt"));
    assert_eq!(read_all(&provider, &path, 0).await, b"hello sftp");
    assert_eq!(read_all(&provider, &path, 6).await, b"sftp", "reads start at an offset");
}

#[tokio::test]
async fn home_paths_start_in_the_login_folder() {
    // A local sftp-server starts in its working folder, which plays the
    // part of the login folder here.
    let provider = provider!();
    let home = std::env::current_dir().unwrap();
    let entry = provider.stat(&VfsPath::parse("sftp://test/~").unwrap()).await.unwrap();
    assert_eq!(entry.kind, EntryKind::Directory);
    let real = provider.stat(&remote(&home)).await.unwrap();
    assert_eq!(entry.modified, real.modified);
}

#[tokio::test]
async fn errors_keep_their_meaning() {
    let provider = provider!();
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a"), "x").unwrap();
    fs::create_dir_all(dir.path().join("full/inner")).unwrap();

    let missing = remote(&dir.path().join("missing"));
    assert!(matches!(provider.stat(&missing).await, Err(VfsError::NotFound(_))));
    assert!(matches!(provider.read(&missing, 0).await, Err(VfsError::NotFound(_))));

    let existing = remote(&dir.path().join("a"));
    let error = provider.write(&existing, WriteOptions::create_new()).await.err().unwrap();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");
    let error = provider.create_dir(&existing).await.unwrap_err();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");

    let full = remote(&dir.path().join("full"));
    let error = provider.remove_dir(&full).await.unwrap_err();
    assert!(matches!(error, VfsError::DirectoryNotEmpty(_)), "{error}");
}

#[tokio::test]
async fn writes_truncates_and_resumes() {
    let provider = provider!();
    let dir = TempDir::new().unwrap();
    let path = remote(&dir.path().join("big.bin"));
    let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    write_all(&provider, &path, WriteMode::CreateNew, &data).await;
    assert_eq!(fs::read(dir.path().join("big.bin")).unwrap(), data);

    write_all(&provider, &path, WriteMode::ResumeAt(1000), b"tail").await;
    let on_disk = fs::read(dir.path().join("big.bin")).unwrap();
    assert_eq!(on_disk.len(), 1004);
    assert_eq!(&on_disk[1000..], b"tail");

    write_all(&provider, &path, WriteMode::Truncate, b"short").await;
    assert_eq!(fs::read(dir.path().join("big.bin")).unwrap(), b"short");
}

#[tokio::test]
async fn folder_operations() {
    let provider = provider!();
    let dir = TempDir::new().unwrap();
    let folder = remote(&dir.path().join("new"));
    provider.create_dir(&folder).await.unwrap();
    write_all(&provider, &folder.join("f"), WriteMode::CreateNew, b"1").await;
    provider.rename(&folder.join("f"), &folder.join("g")).await.unwrap();
    assert!(dir.path().join("new/g").exists());

    provider.create_symlink(&folder.join("link"), "g").await.unwrap();
    assert_eq!(fs::read_link(dir.path().join("new/link")).unwrap(), Path::new("g"));
    let link = provider.stat(&folder.join("link")).await.unwrap();
    assert_eq!(link.kind, EntryKind::Symlink);
    assert_eq!(link.symlink_target.as_deref(), Some("g"));

    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_234_567_890);
    let attributes =
        SetAttributes { permissions: Some(0o640), modified: Some(when), ..Default::default() };
    provider.set_attributes(&folder.join("g"), &attributes).await.unwrap();
    let entry = provider.stat(&folder.join("g")).await.unwrap();
    assert_eq!((entry.permissions, entry.modified), (Some(0o640), Some(when)));

    for name in ["g", "link"] {
        provider.remove_file(&folder.join(name)).await.unwrap();
    }
    provider.remove_dir(&folder).await.unwrap();
    assert!(!dir.path().join("new").exists());
}

#[tokio::test]
async fn copies_between_local_and_sftp_with_the_normal_engine() {
    let Some(server) = sftp_server() else { return };
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(source.join("deep/er")).unwrap();
    fs::write(source.join("one.txt"), "1").unwrap();
    fs::write(source.join("deep/er/two.bin"), vec![7u8; 700_000]).unwrap();
    fs::create_dir(dir.path().join("remote")).unwrap();
    fs::create_dir(dir.path().join("back")).unwrap();

    let registry = nmc_providers::default_registry();
    let destination = remote(&dir.path().join("remote"));
    registry.insert(&destination, Arc::new(connect(&server).await));
    let registry = Arc::new(registry);
    let local = |p: &Path| VfsPath::from_local(p).unwrap();

    let options = ExecuteOptions { conflict: ConflictPolicy::Fail, ..Default::default() };
    let upload = plan_copy(&registry, &[local(&source)], &destination, PlanOptions::default())
        .await
        .unwrap();
    let report = Executor::new(registry.clone(), options.clone()).run(&upload).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read(dir.path().join("remote/source/deep/er/two.bin")).unwrap().len(), 700_000);

    let download = plan_copy(
        &registry,
        &[destination.join("source")],
        &local(&dir.path().join("back")),
        PlanOptions::default(),
    )
    .await
    .unwrap();
    let report = Executor::new(registry, options).run(&download).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read(dir.path().join("back/source/one.txt")).unwrap(), b"1");
    assert_eq!(
        fs::read(dir.path().join("back/source/deep/er/two.bin")).unwrap(),
        vec![7u8; 700_000]
    );
}

/// A missing test server skips the test, unless the nightly CI requires it
/// (`NMC_REQUIRE_SERVERS=1` for all servers, or a list like `sftp,smb`):
/// a skipped test proves nothing there.
fn skip(why: &str) {
    let required = std::env::var("NMC_REQUIRE_SERVERS").unwrap_or_default();
    assert!(
        !(required == "1" || required.split(',').any(|name| name == "sftp")),
        "NMC_REQUIRE_SERVERS asks for {why}"
    );
    eprintln!("skipping: {why}");
}
