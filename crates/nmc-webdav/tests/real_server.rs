//! The WebDAV provider against a real server (Apache `mod_dav`, Nextcloud
//! in the nightly CI). Read from the environment:
//!
//! - `NMC_TEST_DAV_ORIGIN`: `http://host:port` (or https)
//! - `NMC_TEST_DAV_BASE`: the folder to work in, e.g. `dav` (no slashes)
//! - `NMC_TEST_DAV_USER`, `NMC_TEST_DAV_PASSWORD`
//!
//! Without them the tests are skipped (they fail when `NMC_REQUIRE_SERVERS` asks for them).

use nmc_vfs::{Provider, VfsError, VfsPath, WriteMode, WriteOptions, collect_listing};
use nmc_webdav::WebDavProvider;
use nmc_webdav::netrc::Login;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    origin: String,
    base: String,
    user: String,
    password: String,
}

fn server() -> Option<Server> {
    let var = |name: &str| std::env::var(name).ok();
    let found = (|| {
        Some(Server {
            origin: var("NMC_TEST_DAV_ORIGIN")?,
            base: var("NMC_TEST_DAV_BASE").unwrap_or_default(),
            user: var("NMC_TEST_DAV_USER")?,
            password: var("NMC_TEST_DAV_PASSWORD")?,
        })
    })();
    if found.is_none() {
        skip("no WebDAV test server (NMC_TEST_DAV_*)");
    }
    found
}

impl Server {
    fn provider(&self, password: &str) -> WebDavProvider {
        let login = Login { user: self.user.clone(), password: password.into() };
        WebDavProvider::with_login(&self.origin, "dav", Some(login)).unwrap()
    }

    fn path(&self, relative: &str) -> VfsPath {
        let base = self.base.trim_matches('/');
        let joined =
            if base.is_empty() { relative.to_owned() } else { format!("{base}/{relative}") };
        VfsPath::parse(&format!("dav://test/{joined}")).unwrap()
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
async fn a_wrong_password_is_refused() {
    let Some(s) = server() else { return };
    let provider = s.provider("not-the-password");
    let error = provider.stat(&s.path("")).await.unwrap_err();
    assert!(matches!(error, VfsError::PermissionDenied(_)), "{error}");
}

#[tokio::test]
async fn files_and_folders_work_on_the_server() {
    let Some(s) = server() else { return };
    let provider = s.provider(&s.password);
    let dir = s.path(&format!("real-{}", std::process::id()));
    provider.create_dir(&dir).await.unwrap();
    let file = dir.join("a b.txt");
    write_all(&provider, &file, b"hello dav").await;

    let entry = provider.stat(&file).await.unwrap();
    assert_eq!((entry.size, entry.name.as_str()), (9, "a b.txt"));
    let names: Vec<String> =
        collect_listing(&provider, &dir).await.unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["a b.txt"]);
    assert_eq!(read_from(&provider, &file, 6).await, b"dav");

    let renamed = dir.join("c.txt");
    provider.rename(&file, &renamed).await.unwrap();
    assert!(matches!(provider.stat(&file).await, Err(VfsError::NotFound(_))));
    provider.remove_file(&renamed).await.unwrap();
    provider.remove_dir(&dir).await.unwrap();
}

/// A missing test server skips the test, unless the nightly CI requires it
/// (`NMC_REQUIRE_SERVERS=1` for all servers, or a list like `sftp,smb`):
/// a skipped test proves nothing there.
fn skip(why: &str) {
    let required = std::env::var("NMC_REQUIRE_SERVERS").unwrap_or_default();
    assert!(
        !(required == "1" || required.split(',').any(|name| name == "webdav")),
        "NMC_REQUIRE_SERVERS asks for {why}"
    );
    eprintln!("skipping: {why}");
}
