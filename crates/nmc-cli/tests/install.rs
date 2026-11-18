//! `nmc serve install` and `uninstall` against a real OpenSSH server. Start
//! one with `scripts/ssh-test-server.sh`, which prints the variable this
//! test reads (`NMC_TEST_SSH_HOST`, an `ssh` host name that reaches this
//! machine); without it the test is skipped. The server has no systemd, so
//! its `systemctl` is a stand-in that runs the unit's command.
//!
//! The server side of the install is this user's own home folder.
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use tempfile::TempDir;

const PORT: &str = "17443";

fn nmc(dir: &TempDir) -> Command {
    let mut command = Command::cargo_bin("nmc").unwrap();
    command
        .env("NMC_CONFIG_DIR", dir.path().join("config"))
        .env("NMC_DATA_DIR", dir.path().join("data"))
        .env("NMC_AGENT_BINARY", cargo_bin("nmc"));
    command
}

/// Removes what the test installed, even when it fails halfway.
struct Cleanup<'a>(&'a TempDir, &'a str);

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = nmc(self.0).args(["serve", "uninstall", self.1]).output();
    }
}

#[test]
fn install_serve_on_a_server_and_remove_it() {
    let Ok(host) = std::env::var("NMC_TEST_SSH_HOST") else {
        eprintln!("skipping: no SSH test server (see scripts/ssh-test-server.sh)");
        return;
    };
    let dir = TempDir::new().unwrap();
    let _cleanup = Cleanup(&dir, &host);
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let unit = home.join(".config/systemd/user/nmc-serve.service");
    let key = home.join(".config/nmc/serve-tls/server.key");

    nmc(&dir)
        .args(["serve", "install", &host, "--name", "localhost", "--port", PORT])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("scs://localhost:{PORT}/")))
        .stderr(predicate::str::contains("Issuing the certificate"));
    let unit_text = fs::read_to_string(&unit).unwrap();
    assert!(unit_text.contains(&format!("--listen 0.0.0.0:{PORT}")), "{unit_text}");
    assert_eq!(fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600);

    // The new server answers over mutual TLS, from this controller only.
    nmc(&dir)
        .args(["ls", &format!("scs://localhost:{PORT}{}", dir.path().display())])
        .assert()
        .success();

    // Running it again keeps the certificates and the server keeps working.
    nmc(&dir)
        .args(["serve", "install", &host, "--name", "localhost", "--port", PORT])
        .assert()
        .success()
        .stderr(predicate::str::contains("Keeping the certificates"));
    nmc(&dir)
        .args(["ls", &format!("scs://localhost:{PORT}{}", dir.path().display())])
        .assert()
        .success();

    nmc(&dir).args(["serve", "uninstall", &host]).assert().success();
    assert!(!unit.exists());
    assert!(!key.exists());
    nmc(&dir)
        .args(["ls", &format!("scs://localhost:{PORT}{}", dir.path().display())])
        .assert()
        .failure();
}
