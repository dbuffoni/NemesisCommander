//! End-to-end tests: pack a tree in every format, browse it through the VFS,
//! and extract it again with nmc-ops.

use std::path::Path;
use std::sync::Arc;

use nmc_archive::{ArchiveFormat, Compression, PackOptions, pack, register_archive_providers};
use nmc_ops::{ConflictPolicy, ExecuteOptions, Executor, PlanOptions, plan_copy};
use nmc_vfs::{EntryKind, ProviderRegistry, VfsPath, collect_listing};
use tokio::io::AsyncReadExt;

fn registry() -> Arc<ProviderRegistry> {
    let mut registry = nmc_providers::default_registry();
    register_archive_providers(&mut registry);
    Arc::new(registry)
}

/// src/
///   hello.txt         "hello archive"
///   big.bin           300 KiB of patterned bytes
///   nested/deep/x.md  "# x"
///   empty/
fn make_tree(dir: &Path) -> VfsPath {
    let src = dir.join("src");
    std::fs::create_dir_all(src.join("nested/deep")).unwrap();
    std::fs::create_dir_all(src.join("empty")).unwrap();
    std::fs::write(src.join("hello.txt"), b"hello archive").unwrap();
    std::fs::write(src.join("big.bin"), big()).unwrap();
    std::fs::write(src.join("nested/deep/x.md"), b"# x").unwrap();
    VfsPath::from_local(&src).unwrap()
}

fn big() -> Vec<u8> {
    (0..300 * 1024u32).map(|i| (i * 7 % 256) as u8).collect()
}

async fn read_all(registry: &ProviderRegistry, path: &VfsPath) -> Vec<u8> {
    let provider = registry.provider_for(path).await.unwrap();
    let mut out = Vec::new();
    provider.read(path, 0).await.unwrap().read_to_end(&mut out).await.unwrap();
    out
}

async fn check_tree_format(format: ArchiveFormat) {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry();
    let src = make_tree(dir.path());
    let output =
        VfsPath::from_local(dir.path().join(format!("out.{}", format.extension()))).unwrap();

    let count = pack(&registry, &[src], &output, PackOptions::new(format)).await.unwrap();
    assert_eq!(count, 7, "{format:?}: src, hello, big, nested, deep, x.md, empty");
    assert!(!dir.path().join(format!(".out.{}.nmc-part", format.extension())).exists());

    // Browse it like a folder.
    let root = nmc_archive_root(&output);
    let provider = registry.provider_for(&root).await.unwrap();
    let top = collect_listing(provider.as_ref(), &root).await.unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].name, "src");
    let mut names: Vec<String> = collect_listing(provider.as_ref(), &root.join("src"))
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    assert_eq!(names, ["big.bin", "empty", "hello.txt", "nested"], "{format:?}");
    let hello = provider.stat(&root.join("src/hello.txt")).await.unwrap();
    assert_eq!(hello.kind, EntryKind::File);
    assert_eq!(hello.size, 13);
    assert!(hello.modified.is_some(), "{format:?} keeps modification times");
    assert_eq!(read_all(&registry, &root.join("src/big.bin")).await, big());
    assert_eq!(read_all(&registry, &root.join("src/nested/deep/x.md")).await, b"# x");

    // Reading with an offset skips bytes.
    let mut tail = String::new();
    provider
        .read(&root.join("src/hello.txt"), 6)
        .await
        .unwrap()
        .read_to_string(&mut tail)
        .await
        .unwrap();
    assert_eq!(tail, "archive");

    // Extract with the ordinary copy machinery.
    let target = VfsPath::from_local(dir.path().join("extracted")).unwrap();
    std::fs::create_dir(dir.path().join("extracted")).unwrap();
    let plan =
        plan_copy(&registry, &[root.join("src")], &target, PlanOptions::default()).await.unwrap();
    let options = ExecuteOptions { conflict: ConflictPolicy::Fail, ..Default::default() };
    let report = Executor::new(registry.clone(), options).run(&plan).await.unwrap();
    assert!(report.failed.is_empty(), "{format:?}: {:?}", report.failed);
    assert_eq!(std::fs::read(dir.path().join("extracted/src/big.bin")).unwrap(), big());
    assert!(dir.path().join("extracted/src/empty").is_dir());
}

fn nmc_archive_root(file: &VfsPath) -> VfsPath {
    nmc_archive::archive_root(file).expect("an archive file name")
}

#[tokio::test]
async fn zip_roundtrip() {
    check_tree_format(ArchiveFormat::Zip).await;
}

#[tokio::test]
async fn sevenz_roundtrip() {
    check_tree_format(ArchiveFormat::SevenZ).await;
}

#[tokio::test]
async fn tar_roundtrips() {
    for compression in [
        Compression::None,
        Compression::Gzip,
        Compression::Xz,
        Compression::Bzip2,
        Compression::Zstd,
    ] {
        check_tree_format(ArchiveFormat::Tar(compression)).await;
    }
}

#[tokio::test]
async fn single_compressed_files() {
    for compression in [Compression::Gzip, Compression::Xz, Compression::Bzip2, Compression::Zstd] {
        let dir = tempfile::tempdir().unwrap();
        let registry = registry();
        std::fs::write(dir.path().join("notes.txt"), b"some notes ".repeat(1000)).unwrap();
        let source = VfsPath::from_local(dir.path().join("notes.txt")).unwrap();
        let format = ArchiveFormat::Compressed(compression);
        let output =
            VfsPath::from_local(dir.path().join(format!("notes.txt.{}", compression.extension())))
                .unwrap();
        pack(&registry, &[source], &output, PackOptions::new(format)).await.unwrap();

        let root = nmc_archive_root(&output);
        let provider = registry.provider_for(&root).await.unwrap();
        let listing = collect_listing(provider.as_ref(), &root).await.unwrap();
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].name, "notes.txt");
        if matches!(compression, Compression::Gzip | Compression::Zstd) {
            assert_eq!(listing[0].size, 11_000, "{compression:?} records the size");
        }
        assert_eq!(read_all(&registry, &root.join("notes.txt")).await, b"some notes ".repeat(1000));
    }
}

#[tokio::test]
async fn compressed_file_needs_exactly_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry();
    let src = make_tree(dir.path());
    let output = VfsPath::from_local(dir.path().join("x.gz")).unwrap();
    let err = pack(
        &registry,
        &[src],
        &output,
        PackOptions::new(ArchiveFormat::Compressed(Compression::Gzip)),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("exactly one file"), "{err}");
    assert!(!dir.path().join("x.gz").exists());
    assert!(!dir.path().join(".x.gz.nmc-part").exists());
}

#[tokio::test]
async fn archives_on_other_providers_and_nested_archives() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry();
    let src = make_tree(dir.path());
    // Pack straight into the in-memory provider (upload path).
    let remote = VfsPath::parse("mem://box/inner.zip").unwrap();
    pack(&registry, &[src], &remote, PackOptions::new(ArchiveFormat::Zip)).await.unwrap();
    let root = nmc_archive_root(&remote);
    assert_eq!(read_all(&registry, &root.join("src/hello.txt")).await, b"hello archive");

    // Put that zip inside a tar on disk and read through both layers.
    let staging = dir.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    std::fs::write(staging.join("inner.zip"), read_all(&registry, &remote).await).unwrap();
    let outer = VfsPath::from_local(dir.path().join("outer.tar")).unwrap();
    let inner_file = VfsPath::from_local(staging.join("inner.zip")).unwrap();
    pack(&registry, &[inner_file], &outer, PackOptions::new(ArchiveFormat::Tar(Compression::None)))
        .await
        .unwrap();
    let nested =
        VfsPath::parse(&format!("zip://tar://{outer}!/inner.zip!/src/nested/deep/x.md")).unwrap();
    assert_eq!(read_all(&registry, &nested).await, b"# x");
}

#[tokio::test]
async fn corrupt_archives_report_errors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.zip"), b"this is not a zip").unwrap();
    let registry = registry();
    let root = nmc_archive_root(&VfsPath::from_local(dir.path().join("bad.zip")).unwrap());
    let err = registry.provider_for(&root).await.err().unwrap();
    assert!(err.to_string().contains("cannot read archive"), "{err}");
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_are_kept_in_tar_and_zip() {
    for format in [ArchiveFormat::Tar(Compression::None), ArchiveFormat::Zip] {
        let dir = tempfile::tempdir().unwrap();
        let registry = registry();
        let src = make_tree(dir.path());
        std::os::unix::fs::symlink("hello.txt", dir.path().join("src/link")).unwrap();
        let output =
            VfsPath::from_local(dir.path().join(format!("l.{}", format.extension()))).unwrap();
        pack(&registry, &[src], &output, PackOptions::new(format)).await.unwrap();
        let root = nmc_archive_root(&output);
        let provider = registry.provider_for(&root).await.unwrap();
        let link = provider.stat(&root.join("src/link")).await.unwrap();
        assert_eq!(link.kind, EntryKind::Symlink, "{format:?}");
        assert_eq!(link.symlink_target.as_deref(), Some("hello.txt"), "{format:?}");
    }
}
