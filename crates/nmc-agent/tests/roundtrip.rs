//! The agent provider against a real agent service, connected in memory:
//! every file operation goes through the protocol exactly as over SSH.
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use nmc_agent::{AgentProvider, AgentService, Limits};
use nmc_ops::{ConflictPolicy, ExecuteOptions, Executor, PlanOptions, plan_copy};
use nmc_proto::transport::{connect, serve};
use nmc_vfs::{
    Capabilities, EntryKind, HashAlgorithm, Provider, SetAttributes, VfsError, VfsPath, WriteMode,
    WriteOptions, collect_listing,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn agent(limits: Limits) -> AgentProvider {
    let (controller, agent) = tokio::io::duplex(1024 * 1024);
    let service = AgentService::new(limits).unwrap();
    tokio::spawn(serve(service, agent));
    let client = connect(controller).await.unwrap();
    AgentProvider::handshake(client, None).await.unwrap()
}

/// The `agent://test/...` path of a local path.
fn remote(path: &Path) -> VfsPath {
    VfsPath::parse(&format!("agent://test{}", path.display())).unwrap()
}

fn local(path: &Path) -> VfsPath {
    VfsPath::from_local(path).unwrap()
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
async fn handshake_reports_the_limits() {
    let dir = TempDir::new().unwrap();
    let provider =
        agent(Limits { roots: vec![dir.path().into()], read_only: true, ..Default::default() })
            .await;
    assert!(provider.info().read_only);
    assert_eq!(provider.info().roots.len(), 1);
    assert_eq!(provider.info().agent_version, nmc_agent::VERSION);
    assert!(!provider.capabilities().contains(Capabilities::WRITE));
    assert_eq!(provider.settings_dir(&remote(dir.path())), None, "not told");

    let settings = dir.path().join("settings");
    let limits = Limits { settings_dir: Some(settings.clone()), ..Default::default() };
    let provider = agent(limits).await;
    assert_eq!(provider.settings_dir(&remote(dir.path())), Some(remote(&settings)));
}

#[tokio::test]
async fn stat_list_and_read() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("a.txt"), "hello agent").unwrap();
    let provider = agent(Limits::default()).await;

    let entry = provider.stat(&remote(&dir.path().join("a.txt"))).await.unwrap();
    assert_eq!((entry.kind, entry.size), (EntryKind::File, 11));
    assert!(entry.modified.is_some() && entry.permissions.is_some());

    let mut names: Vec<String> = collect_listing(&provider, &remote(dir.path()))
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    assert_eq!(names, ["a.txt", "sub"]);

    let path = remote(&dir.path().join("a.txt"));
    assert_eq!(read_all(&provider, &path, 0).await, b"hello agent");
    assert_eq!(read_all(&provider, &path, 6).await, b"agent", "reads start at an offset");
}

#[tokio::test]
async fn errors_keep_their_meaning() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a"), "x").unwrap();
    let provider = agent(Limits::default()).await;
    let missing = remote(&dir.path().join("missing"));
    assert!(matches!(provider.stat(&missing).await, Err(VfsError::NotFound(p)) if p == missing));
    assert!(provider.read(&missing, 0).await.is_err());

    let existing = remote(&dir.path().join("a"));
    let error = provider.write(&existing, WriteOptions::create_new()).await;
    // Depending on timing the agent refuses at once or when finishing.
    let error = match error {
        Err(e) => e,
        Ok(mut sink) => {
            let _ = sink.write_all(b"y").await;
            let e = sink.shutdown().await.unwrap_err();
            assert!(e.to_string().contains("already exists"), "{e}");
            return;
        }
    };
    assert!(matches!(error, VfsError::AlreadyExists(_)));
}

#[tokio::test]
async fn writes_in_many_chunks_and_resumes() {
    let dir = TempDir::new().unwrap();
    let provider = agent(Limits::default()).await;
    let path = remote(&dir.path().join("big.bin"));
    let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    write_all(&provider, &path, WriteMode::CreateNew, &data).await;
    assert_eq!(fs::read(dir.path().join("big.bin")).unwrap(), data);

    write_all(&provider, &path, WriteMode::ResumeAt(1000), b"tail").await;
    let on_disk = fs::read(dir.path().join("big.bin")).unwrap();
    assert_eq!(on_disk.len(), 1004);
    assert_eq!(&on_disk[1000..], b"tail");
}

#[tokio::test]
async fn folder_operations() {
    let dir = TempDir::new().unwrap();
    let provider = agent(Limits::default()).await;
    let folder = remote(&dir.path().join("new"));
    provider.create_dir(&folder).await.unwrap();
    write_all(&provider, &folder.join("f"), WriteMode::CreateNew, b"1").await;
    provider.rename(&folder.join("f"), &folder.join("g")).await.unwrap();
    provider.copy_within(&folder.join("g"), &folder.join("h")).await.unwrap();
    provider.create_symlink(&folder.join("link"), "g").await.unwrap();
    let link = provider.stat(&folder.join("link")).await.unwrap();
    assert_eq!(link.kind, EntryKind::Symlink);
    assert_eq!(link.symlink_target.as_deref(), Some("g"));

    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_234_567_890);
    let attributes = SetAttributes { permissions: Some(0o640), modified: Some(when) };
    provider.set_attributes(&folder.join("h"), &attributes).await.unwrap();
    let entry = provider.stat(&folder.join("h")).await.unwrap();
    assert_eq!((entry.permissions, entry.modified), (Some(0o640), Some(when)));

    let remote_hash = provider.checksum(&folder.join("h"), HashAlgorithm::Blake3).await.unwrap();
    assert_eq!(remote_hash, blake3::hash(b"1").to_hex().to_string());

    assert!(matches!(provider.remove_dir(&folder).await, Err(VfsError::DirectoryNotEmpty(_))));
    for name in ["g", "h", "link"] {
        provider.remove_file(&folder.join(name)).await.unwrap();
    }
    provider.remove_dir(&folder).await.unwrap();
    assert!(!dir.path().join("new").exists());
}

#[tokio::test]
async fn read_only_and_roots_are_enforced() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(dir.path().join("outside"), "secret").unwrap();
    let root = fs::canonicalize(&root).unwrap();

    let provider =
        agent(Limits { roots: vec![root.clone()], read_only: false, ..Default::default() }).await;
    let outside = remote(&root.join("../outside"));
    assert!(matches!(provider.stat(&outside).await, Err(VfsError::PermissionDenied(_))));
    provider.create_dir(&remote(&root.join("ok"))).await.unwrap();

    let read_only = agent(Limits { roots: vec![], read_only: true, ..Default::default() }).await;
    let result = read_only.create_dir(&remote(&root.join("nope"))).await;
    assert!(matches!(result, Err(VfsError::PermissionDenied(_))));
    assert!(read_only.stat(&remote(&root.join("ok"))).await.is_ok());
}

#[tokio::test]
async fn copies_between_local_and_agent_with_the_normal_engine() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source");
    fs::create_dir_all(source.join("deep/er")).unwrap();
    fs::write(source.join("one.txt"), "1").unwrap();
    fs::write(source.join("deep/er/two.bin"), vec![7u8; 700_000]).unwrap();
    fs::create_dir(dir.path().join("remote")).unwrap();
    fs::create_dir(dir.path().join("back")).unwrap();

    let registry = nmc_providers::default_registry();
    let destination = remote(&dir.path().join("remote"));
    registry.insert(&destination, Arc::new(agent(Limits::default()).await));
    let registry = Arc::new(registry);

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
    let report = Executor::new(registry.clone(), options).run(&download).await.unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read_to_string(dir.path().join("back/source/one.txt")).unwrap(), "1");
}
