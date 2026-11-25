//! RAR archives are read with libarchive (feature `rar`). The sample files
//! in `tests/rar-samples` come from libarchive's own test suite
//! (BSD-2-Clause, see tests/rar-samples/README.md); this project ships no
//! tool that creates RAR files.
#![cfg(feature = "rar")]

use std::path::PathBuf;

use nmc_archive::{ArchiveFormat, ArchiveProvider, archive_root};
use nmc_vfs::{Provider, VfsPath, collect_listing};
use tokio::io::AsyncReadExt;

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/rar-samples").join(name)
}

async fn open(name: &str) -> Result<(ArchiveProvider, VfsPath), nmc_vfs::VfsError> {
    let provider = ArchiveProvider::open_local(ArchiveFormat::Rar, sample(name)).await?;
    let root = archive_root(&VfsPath::parse(&sample(name).display().to_string()).unwrap()).unwrap();
    Ok((provider, root))
}

async fn read(provider: &ArchiveProvider, path: &VfsPath) -> Vec<u8> {
    let mut data = Vec::new();
    provider.read(path, 0).await.unwrap().read_to_end(&mut data).await.unwrap();
    data
}

/// Every file below `dir`, with its content.
async fn files(provider: &ArchiveProvider, dir: &VfsPath) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(dir) = stack.pop() {
        for entry in collect_listing(provider, &dir).await.unwrap() {
            let path = dir.join(&entry.name);
            if entry.is_dir() {
                stack.push(path);
            } else {
                found.push((
                    path.path().trim_start_matches('/').to_owned(),
                    read(provider, &path).await,
                ));
            }
        }
    }
    found.sort();
    found
}

/// libarchive's `verify_data`: word `i` of file number `magic` is
/// `max(0, k*k - 3*k + 1 + magic)` with `k = i + 1`.
fn is_pattern(data: &[u8], magic: i32) -> bool {
    data.chunks_exact(4).enumerate().all(|(i, word)| {
        let k = i as i32 + 1;
        let expected = (k * k - 3 * k + 1 + magic).max(0) as u32;
        word == expected.to_le_bytes()
    })
}

#[tokio::test]
async fn rar4_with_folders_and_a_link() {
    let (p, root) = open("test_read_format_rar.rar").await.unwrap();
    let found = files(&p, &root).await;
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["test.txt", "testdir/test.txt", "testlink"]);
    assert_eq!(found[0].1.len(), 20);
    assert_eq!(&found[0].1[..9], b"test text");
}

#[tokio::test]
async fn rar5_compressed() {
    let (p, root) = open("test_read_format_rar5_compressed.rar").await.unwrap();
    let found = files(&p, &root).await;
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].0.as_str(), found[0].1.len()), ("test.bin", 1200));
    assert!(is_pattern(&found[0].1, 0));
}

#[tokio::test]
async fn solid_archives_give_each_file_its_own_data() {
    for name in
        ["test_read_format_rar5_multiple_files_solid.rar", "test_read_format_rar5_solid.rar"]
    {
        let (p, root) = open(name).await.unwrap();
        let found = files(&p, &root).await;
        let numbered: Vec<_> =
            found.iter().filter(|(n, _)| n.starts_with("test") && n != "test.bin").collect();
        assert!(numbered.len() >= 4, "{name}");
        for (n, data) in numbered {
            let number: i32 =
                n.trim_start_matches("test").trim_end_matches(".bin").parse().unwrap();
            assert_eq!(data.len(), 4096, "{name}: {n}");
            assert!(is_pattern(data, number), "{name}: {n}");
        }
    }
}

#[tokio::test]
async fn one_file_of_a_solid_archive_can_be_read_on_its_own() {
    let (p, root) = open("test_read_format_rar5_solid.rar").await.unwrap();
    let data = read(&p, &root.join("test4.bin")).await;
    assert!(is_pattern(&data, 4));
}

#[tokio::test]
async fn an_encrypted_archive_says_so_instead_of_failing_obscurely() {
    for name in ["test_read_format_rar5_encrypted.rar", "test_read_format_rar4_solid_encrypted.rar"]
    {
        let error = open(name).await.err().expect("encrypted archives cannot be read");
        assert!(error.to_string().contains("encrypted"), "{error}");
    }
}

#[tokio::test]
async fn a_file_that_is_not_a_rar_archive_is_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fake.rar");
    std::fs::write(&path, b"this is not a rar archive").unwrap();
    let error = ArchiveProvider::open_local(ArchiveFormat::Rar, path).await.err().unwrap();
    assert!(error.to_string().contains("cannot read"), "{error}");
}
