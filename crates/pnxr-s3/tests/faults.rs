//! Network faults between the S3 provider and a real server: toxiproxy
//! (in the nightly CI) sits in front of MinIO and cuts, slows or delays the
//! connection. Variables (see scripts/heavy/up.sh):
//!
//! - `PNXR_TEST_TOXIPROXY`: toxiproxy's API, `http://host:port`
//! - `PNXR_TEST_S3_PROXY_ENDPOINT`: the proxied S3 endpoint
//! - and the `PNXR_TEST_S3_*` of `real_server.rs`
//!
//! Without them the tests are skipped (they fail when `PNXR_REQUIRE_SERVERS`
//! asks for `toxiproxy`). Timeouts are generous; only the proxy setup is
//! retried, never an assertion.

use std::time::Duration;

use pnxr_s3::S3Provider;
use pnxr_s3::settings::Settings;
use pnxr_s3::sigv4::Credentials;
use pnxr_vfs::{Provider, VfsPath, WriteMode, WriteOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The toxics apply to every connection of the one proxy: the tests take
/// turns, or one test's cut would break another's requests.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Setup {
    api: String,
    endpoint: String,
    bucket: String,
    access: String,
    secret: String,
}

fn setup() -> Option<Setup> {
    let var = |name: &str| std::env::var(name).ok();
    let found = (|| {
        Some(Setup {
            api: var("PNXR_TEST_TOXIPROXY")?,
            endpoint: var("PNXR_TEST_S3_PROXY_ENDPOINT")?,
            bucket: var("PNXR_TEST_S3_BUCKET")?,
            access: var("PNXR_TEST_S3_ACCESS_KEY")?,
            secret: var("PNXR_TEST_S3_SECRET_KEY")?,
        })
    })();
    if found.is_none() {
        let required = std::env::var("PNXR_REQUIRE_SERVERS").unwrap_or_default();
        assert!(
            !(required == "1" || required.split(',').any(|name| name == "toxiproxy")),
            "PNXR_REQUIRE_SERVERS asks for toxiproxy (PNXR_TEST_TOXIPROXY)"
        );
        eprintln!("skipping: no toxiproxy (PNXR_TEST_TOXIPROXY)");
    }
    found
}

/// One HTTP request to toxiproxy's API, by hand (no client crate needed).
async fn api(base: &str, method: &str, path: &str, body: &str) -> String {
    let host = base.trim_start_matches("http://");
    let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

impl Setup {
    fn provider(&self) -> S3Provider {
        let settings = Settings {
            credentials: Some(Credentials {
                access_key: self.access.clone(),
                secret_key: self.secret.clone(),
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

    async fn add_toxic(&self, name: &str, kind: &str, attributes: &str) {
        let body = format!(r#"{{"name":"{name}","type":"{kind}","attributes":{attributes}}}"#);
        let response = api(&self.api, "POST", "/proxies/minio/toxics", &body).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    async fn remove_toxic(&self, name: &str) {
        api(&self.api, "DELETE", &format!("/proxies/minio/toxics/{name}"), "").await;
    }
}

async fn upload(provider: &S3Provider, path: &VfsPath, data: &[u8]) -> std::io::Result<()> {
    let options = WriteOptions { mode: WriteMode::Truncate, size_hint: Some(data.len() as u64) };
    let mut sink = provider.write(path, options).await.map_err(std::io::Error::other)?;
    sink.write_all(data).await?;
    sink.shutdown().await
}

#[tokio::test]
async fn a_cut_during_an_upload_is_an_error_and_the_next_upload_works() {
    let _turn = ONE_AT_A_TIME.lock().await;
    let Some(s) = setup() else { return };
    let provider = s.provider();
    let data: Vec<u8> = (0..24 * 1024 * 1024u32).map(|i| (i % 253) as u8).collect();
    let file = s.path(&format!("faults-{}/cut.bin", std::process::id()));

    // Slow the link, then reset it a moment into the transfer.
    s.add_toxic("slow", "bandwidth", r#"{"rate":4096}"#).await;
    s.add_toxic("reset", "reset_peer", r#"{"timeout":1500}"#).await;
    let outcome =
        tokio::time::timeout(Duration::from_secs(120), upload(&provider, &file, &data)).await;
    s.remove_toxic("reset").await;
    s.remove_toxic("slow").await;
    let outcome = outcome.expect("the upload hung instead of failing");
    assert!(outcome.is_err(), "a cut upload must report an error");

    // The link is healthy again: a fresh upload works and is complete.
    upload(&provider, &file, &data).await.unwrap();
    assert_eq!(provider.stat(&file).await.unwrap().size, data.len() as u64);
    provider.remove_file(&file).await.unwrap();
}

#[tokio::test]
async fn latency_slows_requests_but_does_not_break_them() {
    let _turn = ONE_AT_A_TIME.lock().await;
    let Some(s) = setup() else { return };
    let provider = s.provider();
    s.add_toxic("lag", "latency", r#"{"latency":400,"jitter":100}"#).await;
    let file = s.path(&format!("faults-{}/lag.txt", std::process::id()));
    let result = upload(&provider, &file, b"slow but fine").await;
    let size = provider.stat(&file).await.map(|e| e.size);
    s.remove_toxic("lag").await;
    result.unwrap();
    assert_eq!(size.unwrap(), 13);
    provider.remove_file(&file).await.unwrap();
}

#[tokio::test]
async fn a_dead_link_fails_with_an_error_after_a_timeout() {
    let _turn = ONE_AT_A_TIME.lock().await;
    let Some(s) = setup() else { return };
    let provider = s.provider();
    // `timeout` with 0 holds the connection open and sends nothing.
    s.add_toxic("dead", "timeout", r#"{"timeout":0}"#).await;
    let outcome = tokio::time::timeout(Duration::from_secs(180), provider.stat(&s.path(""))).await;
    s.remove_toxic("dead").await;
    let result = outcome.expect("a dead link must end in an error, not hang");
    assert!(result.is_err());
}
