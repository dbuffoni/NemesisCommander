//! The whole `pnxr serve` workflow with the real program: create the
//! authority, issue two server bundles, start two `pnxr serve` processes on
//! local ports, browse one and copy between them.
#![cfg(target_os = "linux")]

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use assert_cmd::cargo::CommandCargoExt;
use predicates::str::contains;
use tempfile::TempDir;

struct Setup {
    dir: TempDir,
    base: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let base = fs::canonicalize(dir.path()).unwrap();
        Setup { dir, base }
    }

    /// `pnxr` with this setup's settings (and so its certificate authority).
    fn pnxr(&self) -> Command {
        let mut command = Command::cargo_bin("pnxr").unwrap();
        command
            .env("PNXR_CONFIG_DIR", self.base.join("config"))
            .env("PNXR_DATA_DIR", self.base.join("data"));
        command
    }
}

/// A running `pnxr serve`, stopped when dropped.
struct Served {
    child: Child,
    port: u16,
    root: PathBuf,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn serve(setup: &Setup, name: &str, host: &str) -> Served {
    let bundle = setup.base.join(format!("{name}-tls"));
    setup
        .pnxr()
        .args(["tls", "server", host, "--out"])
        .arg(&bundle)
        .assert()
        .success()
        .stdout(contains("pnxr serve --tls"));
    let root = setup.base.join(name);
    fs::create_dir_all(&root).unwrap();
    let port = free_port();
    let child = std::process::Command::cargo_bin("pnxr")
        .unwrap()
        .args(["serve", "--listen", &format!("127.0.0.1:{port}"), "--tls"])
        .arg(&bundle)
        .arg("--root")
        .arg(&root)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "pnxr serve did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    Served { child, port, root }
}

fn url(host: &str, served: &Served, path: &Path) -> String {
    format!("scs://{host}:{}{}", served.port, path.display())
}

#[test]
fn browse_and_copy_between_servers() {
    let setup = Setup::new();
    setup.pnxr().args(["tls", "init"]).assert().success();
    setup.pnxr().args(["tls", "init"]).assert().failure().stderr(contains("exists already"));

    let a = serve(&setup, "a", "127.0.0.1");
    let b = serve(&setup, "b", "localhost");
    fs::create_dir_all(a.root.join("photos/2026")).unwrap();
    fs::write(a.root.join("photos/2026/beach.jpg"), vec![1u8; 300_000]).unwrap();

    setup
        .pnxr()
        .args(["ls", &url("127.0.0.1", &a, &a.root)])
        .assert()
        .success()
        .stdout("photos/\n");

    setup
        .pnxr()
        .args(["cp", &url("127.0.0.1", &a, &a.root.join("photos")), &url("localhost", &b, &b.root)])
        .assert()
        .success()
        .stderr(contains("route: direct"));
    let copied = fs::read(b.root.join("photos/2026/beach.jpg")).unwrap();
    assert_eq!(copied.len(), 300_000);

    setup
        .pnxr()
        .args(["cp", "--route", "reverse", "--conflict", "overwrite"])
        .args([&url("127.0.0.1", &a, &a.root.join("photos")), &url("localhost", &b, &b.root)])
        .assert()
        .success()
        .stderr(contains("route: reverse"));

    // Explicitly through this machine: the normal copy engine.
    setup
        .pnxr()
        .args(["cp", "--route", "controller", "--conflict", "skip"])
        .args([&url("127.0.0.1", &a, &a.root.join("photos")), &url("localhost", &b, &b.root)])
        .assert()
        .success()
        .stderr(contains("route: via controller"));
    drop(setup.dir);
}

#[test]
fn without_an_authority_the_hint_says_what_to_run() {
    let setup = Setup::new();
    setup
        .pnxr()
        .args(["ls", "scs://127.0.0.1:1/"])
        .assert()
        .failure()
        .stderr(contains("pnxr tls init"));
}
