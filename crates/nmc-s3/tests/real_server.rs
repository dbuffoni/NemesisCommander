//! The S3 provider against a real server (MinIO in the nightly CI), not the
//! in-process one. Read from the environment, so the same suite runs against
//! any S3 service:
//!
//! - `NMC_TEST_S3_ENDPOINT`: `http://host:port`
//! - `NMC_TEST_S3_BUCKET`: an existing bucket
//! - `NMC_TEST_S3_ACCESS_KEY`, `NMC_TEST_S3_SECRET_KEY`
//!
//! Without them the tests are skipped (they fail when `NMC_REQUIRE_SERVERS` asks for them).

use nmc_s3::S3Provider;
use nmc_s3::settings::Settings;
use nmc_s3::sigv4::Credentials;
use nmc_vfs::{Provider, VfsError, VfsPath, WriteMode, WriteOptions, collect_listing};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    endpoint: String,
    bucket: String,
    access: String,
    secret: String,
}

fn server() -> Option<Server> {
    let var = |name: &str| std::env::var(name).ok();
    let found = (|| {
        Some(Server {
            endpoint: var("NMC_TEST_S3_ENDPOINT")?,
            bucket: var("NMC_TEST_S3_BUCKET")?,
            access: var("NMC_TEST_S3_ACCESS_KEY")?,
            secret: var("NMC_TEST_S3_SECRET_KEY")?,
        })
    })();
    if found.is_none() {
        skip("no S3 test server (NMC_TEST_S3_*)");
    }
    found
}

impl Server {
    fn provider(&self, secret: &str) -> S3Provider {
        let settings = Settings {
            credentials: Some(Credentials {
                access_key: self.access.clone(),
                secret_key: secret.into(),
                session_token: None,
            }),
            region: "us-east-1".into(),
            endpoint: Some(self.endpoint.clone()),
        };
        S3Provider::new(&self.bucket, settings).unwrap()
    }

    fn path(&self, key: &str) -> VfsPath {
        VfsPath::parse(&format!("s3://{}/{key}", self.bucket)).unwrap()
    }
}

async fn write_all(provider: &dyn Provider, path: &VfsPath, data: &[u8]) {
    let options = WriteOptions { mode: WriteMode::Truncate, size_hint: Some(data.len() as u64) };
    let mut sink = provider.write(path, options).await.unwrap();
    sink.write_all(data).await.unwrap();
    sink.shutdown().await.unwrap();
}

async fn read_from(provider: &dyn Provider, path: &VfsPath, offset: u64) -> Vec<u8> {
    let mut data = Vec::new();
    provider.read(path, offset).await.unwrap().read_to_end(&mut data).await.unwrap();
    data
}

#[tokio::test]
async fn a_wrong_secret_key_is_refused() {
    let Some(s) = server() else { return };
    let provider = s.provider("not-the-secret");
    let error = provider.stat(&s.path("")).await.unwrap_err();
    assert!(!matches!(error, VfsError::NotFound(_)), "{error}");
}

#[tokio::test]
async fn objects_round_trip_with_folders_and_ranges() {
    let Some(s) = server() else { return };
    let provider = s.provider(&s.secret);
    let base = format!("real-{}", std::process::id());
    let file = s.path(&format!("{base}/dir/a & b.txt"));
    write_all(&provider, &file, b"hello minio").await;

    let entry = provider.stat(&file).await.unwrap();
    assert_eq!((entry.size, entry.name.as_str()), (11, "a & b.txt"));
    assert!(provider.stat(&s.path(&format!("{base}/dir"))).await.unwrap().is_dir());
    let names: Vec<String> = collect_listing(&provider, &s.path(&format!("{base}/dir")))
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["a & b.txt"]);
    // Resume: reading from an offset returns the rest.
    assert_eq!(read_from(&provider, &file, 6).await, b"minio");

    provider.remove_file(&file).await.unwrap();
    assert!(matches!(provider.stat(&file).await, Err(VfsError::NotFound(_))));
}

#[tokio::test]
async fn a_large_object_is_uploaded_in_parts_and_read_back() {
    let Some(s) = server() else { return };
    let provider = s.provider(&s.secret);
    // Larger than any part size, so the upload is multipart.
    let data: Vec<u8> = (0..40 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    let file = s.path(&format!("real-{}/large.bin", std::process::id()));
    write_all(&provider, &file, &data).await;
    assert_eq!(provider.stat(&file).await.unwrap().size, data.len() as u64);
    let tail = read_from(&provider, &file, data.len() as u64 - 1000).await;
    assert_eq!(tail, data[data.len() - 1000..]);
    provider.remove_file(&file).await.unwrap();
}

/// A missing test server skips the test, unless the nightly CI requires it
/// (`NMC_REQUIRE_SERVERS=1` for all servers, or a list like `sftp,smb`):
/// a skipped test proves nothing there.
fn skip(why: &str) {
    let required = std::env::var("NMC_REQUIRE_SERVERS").unwrap_or_default();
    assert!(
        !(required == "1" || required.split(',').any(|name| name == "s3")),
        "NMC_REQUIRE_SERVERS asks for {why}"
    );
    eprintln!("skipping: {why}");
}
