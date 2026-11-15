//! The SMB provider against a real Samba server. Start one with
//! `scripts/smb-test-server.sh`, which prints the variables these tests
//! read:
//!
//! - `SC_TEST_SMB_URL`: `smb://user@host:port/share`
//! - `SC_TEST_SMB_PASSWORD`: the user's password
//! - `SC_TEST_SMB_DIR`: the shared folder on this machine, to check results
//!
//! Without them the tests are skipped. Each test works in its own folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_ops::{ConflictPolicy, ExecuteOptions, Executor, PlanOptions, plan_copy};
use sc_smb::SmbProvider;
use sc_vfs::{
    EntryKind, Provider, SetAttributes, VfsError, VfsPath, WriteMode, WriteOptions, collect_listing,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture {
    provider: SmbProvider,
    /// `smb://user@host:port/share/<test folder>`.
    base: VfsPath,
    /// The same folder on this machine.
    local: PathBuf,
}

impl Fixture {
    /// Connects and creates the test folder, or `None` without a server.
    async fn new(name: &str) -> Option<Self> {
        let url = std::env::var("SC_TEST_SMB_URL").ok()?;
        let password = std::env::var("SC_TEST_SMB_PASSWORD").unwrap_or_default();
        let dir = PathBuf::from(std::env::var("SC_TEST_SMB_DIR").ok()?);
        let share = VfsPath::parse(&url).unwrap();
        let (user, addr) = share.authority().rsplit_once('@').unwrap();
        let provider = SmbProvider::connect(addr, user, &password).await.unwrap();
        let local = dir.join(name);
        let _ = fs::remove_dir_all(&local);
        let base = share.join(name);
        provider.create_dir(&base).await.unwrap();
        Some(Fixture { provider, base, local })
    }

    fn remote(&self, relative: &str) -> VfsPath {
        relative.split('/').fold(self.base.clone(), |path, name| path.join(name))
    }
}

macro_rules! fixture {
    ($name:expr) => {
        match Fixture::new($name).await {
            Some(fixture) => fixture,
            None => {
                eprintln!("skipping: no SMB test server (see scripts/smb-test-server.sh)");
                return;
            }
        }
    };
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
    let f = fixture!("stat-list-read");
    fs::create_dir(f.local.join("sub")).unwrap();
    fs::write(f.local.join("café.txt"), "hello smb").unwrap();

    let entry = f.provider.stat(&f.remote("café.txt")).await.unwrap();
    assert_eq!((entry.kind, entry.size), (EntryKind::File, 9));
    assert!(entry.modified.is_some());
    assert!(f.provider.stat(&f.remote("sub")).await.unwrap().is_dir());

    let mut names: Vec<String> =
        collect_listing(&f.provider, &f.base).await.unwrap().into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, ["café.txt", "sub"], "no . and .. entries");

    assert_eq!(read_all(&f.provider, &f.remote("café.txt"), 0).await, b"hello smb");
    assert_eq!(read_all(&f.provider, &f.remote("café.txt"), 6).await, b"smb");

    let server = f.base.parent().unwrap().parent().unwrap();
    let shares = collect_listing(&f.provider, &server).await.unwrap();
    assert!(shares.iter().any(|s| s.name == "share" && s.is_dir()), "{shares:?}");
}

#[tokio::test]
async fn errors_keep_their_meaning() {
    let f = fixture!("errors");
    fs::write(f.local.join("a"), "x").unwrap();
    fs::create_dir_all(f.local.join("full/inner")).unwrap();

    assert!(matches!(f.provider.stat(&f.remote("missing")).await, Err(VfsError::NotFound(_))));
    assert!(matches!(f.provider.read(&f.remote("missing"), 0).await, Err(VfsError::NotFound(_))));
    let error = f.provider.write(&f.remote("a"), WriteOptions::create_new()).await.err().unwrap();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");
    let error = f.provider.create_dir(&f.remote("a")).await.unwrap_err();
    assert!(matches!(error, VfsError::AlreadyExists(_)), "{error}");
    let error = f.provider.remove_dir(&f.remote("full")).await.unwrap_err();
    assert!(matches!(error, VfsError::DirectoryNotEmpty(_)), "{error}");
    assert!(f.local.join("full/inner").exists());
}

#[tokio::test]
async fn writes_truncates_and_resumes() {
    let f = fixture!("writes");
    let path = f.remote("big.bin");
    let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    write_all(&f.provider, &path, WriteMode::CreateNew, &data).await;
    assert_eq!(fs::read(f.local.join("big.bin")).unwrap(), data);
    assert_eq!(read_all(&f.provider, &path, 0).await, data);

    fs::write(f.local.join("part"), b"0123").unwrap();
    write_all(&f.provider, &f.remote("part"), WriteMode::ResumeAt(4), b"4567").await;
    assert_eq!(fs::read(f.local.join("part")).unwrap(), b"01234567");

    write_all(&f.provider, &path, WriteMode::Truncate, b"short").await;
    assert_eq!(fs::read(f.local.join("big.bin")).unwrap(), b"short");
}

#[tokio::test]
async fn folder_operations() {
    let f = fixture!("folders");
    let folder = f.remote("new");
    f.provider.create_dir(&folder).await.unwrap();
    write_all(&f.provider, &folder.join("f"), WriteMode::CreateNew, b"1").await;
    f.provider.rename(&folder.join("f"), &folder.join("g")).await.unwrap();
    write_all(&f.provider, &folder.join("h"), WriteMode::CreateNew, b"2").await;
    f.provider.rename(&folder.join("h"), &folder.join("g")).await.unwrap();
    assert_eq!(fs::read(f.local.join("new/g")).unwrap(), b"2", "rename replaces a file");

    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_234_567_890);
    let attributes = SetAttributes { permissions: None, modified: Some(when) };
    f.provider.set_attributes(&folder.join("g"), &attributes).await.unwrap();
    assert_eq!(f.provider.stat(&folder.join("g")).await.unwrap().modified, Some(when));

    match f.provider.copy_within(&folder.join("g"), &folder.join("c")).await {
        Ok(()) => assert_eq!(fs::read(f.local.join("new/c")).unwrap(), b"2"),
        // Not every server copies on its side.
        Err(VfsError::Unsupported { .. }) => {}
        Err(e) => panic!("{e}"),
    }

    for name in ["g", "c"] {
        let _ = f.provider.remove_file(&folder.join(name)).await;
    }
    f.provider.remove_dir(&folder).await.unwrap();
    assert!(!f.local.join("new").exists());
}

#[tokio::test]
async fn copies_between_local_and_smb_with_the_normal_engine() {
    let f = fixture!("engine");
    let work = TempDir::new().unwrap();
    let source = work.path().join("source");
    fs::create_dir_all(source.join("deep/er")).unwrap();
    fs::write(source.join("one.txt"), "1").unwrap();
    fs::write(source.join("deep/er/two.bin"), vec![7u8; 700_000]).unwrap();
    fs::create_dir(work.path().join("back")).unwrap();

    let registry = sc_providers::default_registry();
    let Fixture { provider, base, local: shared } = f;
    registry.insert(&base, Arc::new(provider));
    let registry = Arc::new(registry);
    let local = |p: &Path| VfsPath::from_local(p).unwrap();

    let options = ExecuteOptions { conflict: ConflictPolicy::Fail, ..Default::default() };
    let upload =
        plan_copy(&registry, &[local(&source)], &base, PlanOptions::default()).await.unwrap();
    let report = Executor::new(registry.clone(), options.clone()).run(&upload).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read(shared.join("source/deep/er/two.bin")).unwrap().len(), 700_000);

    let back = local(&work.path().join("back"));
    let download =
        plan_copy(&registry, &[base.join("source")], &back, PlanOptions::default()).await.unwrap();
    let report = Executor::new(registry, options).run(&download).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read(work.path().join("back/source/one.txt")).unwrap(), b"1");
    let two = fs::read(work.path().join("back/source/deep/er/two.bin")).unwrap();
    assert_eq!(two, vec![7u8; 700_000]);
}
