//! `nmc` reaching files over SFTP. SSH is replaced by running OpenSSH's
//! `sftp-server` directly (`NMC_SFTP_COMMAND`), so everything except the SSH
//! login is exercised. Skipped when `sftp-server` is not installed.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

const SERVERS: &[&str] = &[
    "/usr/lib/openssh/sftp-server",
    "/usr/libexec/openssh/sftp-server",
    "/usr/libexec/sftp-server",
    "/usr/lib/ssh/sftp-server",
];

fn nmc(dir: &TempDir) -> Option<Command> {
    let server = SERVERS.iter().find(|p| Path::new(p).exists())?;
    let mut command = Command::cargo_bin("nmc").unwrap();
    command
        .env("NMC_CONFIG_DIR", dir.path().join("config"))
        .env("NMC_DATA_DIR", dir.path().join("data"))
        .env("NMC_SFTP_COMMAND", server);
    Some(command)
}

fn uri(dir: &TempDir, relative: &str) -> String {
    format!("sftp://server{}/{relative}", dir.path().display())
}

#[test]
fn list_read_and_copy_over_sftp() {
    let dir = TempDir::new().unwrap();
    let Some(mut ls) = nmc(&dir) else {
        skip("no sftp-server installed");
        return;
    };
    fs::create_dir_all(dir.path().join("srv/docs")).unwrap();
    fs::write(dir.path().join("srv/docs/readme.txt"), "remote text\n").unwrap();

    ls.args(["ls", &uri(&dir, "srv")]).assert().success().stdout("docs/\n");
    nmc(&dir)
        .unwrap()
        .args(["cat", &uri(&dir, "srv/docs/readme.txt")])
        .assert()
        .success()
        .stdout("remote text\n");

    let local = dir.path().join("local");
    fs::create_dir(&local).unwrap();
    nmc(&dir)
        .unwrap()
        .args(["cp", "--verify", "full", &uri(&dir, "srv/docs"), local.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(local.join("docs/readme.txt")).unwrap(), "remote text\n");

    fs::write(local.join("upload.bin"), vec![1u8; 1_000_000]).unwrap();
    let upload = local.join("upload.bin");
    nmc(&dir)
        .unwrap()
        .args(["cp", "--verify", "full", upload.to_str().unwrap(), &uri(&dir, "srv")])
        .assert()
        .success();
    assert_eq!(fs::read(dir.path().join("srv/upload.bin")).unwrap().len(), 1_000_000);
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
