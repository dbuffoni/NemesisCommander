//! `sc` talking to a real `sc agent --stdio` child process. SSH is replaced
//! by running the agent directly (`SC_AGENT_COMMAND`), so everything except
//! the SSH login is exercised.
#![cfg(target_os = "linux")]

use std::fs;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use predicates::prelude::*;
use tempfile::TempDir;

fn sc(dir: &TempDir, agent_args: &str) -> Command {
    let agent = format!("{} agent --stdio {agent_args}", cargo_bin("sc").display());
    let mut command = Command::cargo_bin("sc").unwrap();
    command
        .env("SC_CONFIG_DIR", dir.path().join("config"))
        .env("SC_DATA_DIR", dir.path().join("data"))
        .env("SC_AGENT_COMMAND", agent);
    command
}

fn uri(dir: &TempDir, relative: &str) -> String {
    format!("agent://server{}/{relative}", dir.path().display())
}

fn sandbox() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("srv/docs")).unwrap();
    fs::write(dir.path().join("srv/docs/readme.txt"), "remote text\n").unwrap();
    dir
}

#[test]
fn list_and_read_remote_files() {
    let dir = sandbox();
    sc(&dir, "").args(["ls", &uri(&dir, "srv")]).assert().success().stdout("docs/\n");
    sc(&dir, "")
        .args(["cat", &uri(&dir, "srv/docs/readme.txt")])
        .assert()
        .success()
        .stdout("remote text\n");
}

#[test]
fn copy_both_ways() {
    let dir = sandbox();
    let local = dir.path().join("local");
    fs::create_dir(&local).unwrap();
    let local_arg = local.display().to_string();
    sc(&dir, "").args(["cp", &uri(&dir, "srv/docs"), &local_arg]).assert().success();
    assert_eq!(fs::read_to_string(local.join("docs/readme.txt")).unwrap(), "remote text\n");

    fs::write(local.join("upload.bin"), vec![1u8; 1_000_000]).unwrap();
    let upload = local.join("upload.bin").display().to_string();
    sc(&dir, "").args(["cp", "--verify", "full", &upload, &uri(&dir, "srv")]).assert().success();
    assert_eq!(fs::read(dir.path().join("srv/upload.bin")).unwrap().len(), 1_000_000);
}

#[test]
fn the_agent_enforces_its_limits() {
    let dir = sandbox();
    let root = format!("--root {}", dir.path().join("srv").display());
    sc(&dir, &root)
        .args(["ls", &uri(&dir, "")])
        .assert()
        .failure()
        .stderr(predicate::str::contains("permission denied"));
    sc(&dir, "--read-only")
        .args(["mkdir", &uri(&dir, "srv/new")])
        .assert()
        .failure()
        .stderr(predicate::str::contains("permission denied"));
    assert!(!dir.path().join("srv/new").exists());
}

#[test]
fn hash_runs_on_the_server() {
    let dir = sandbox();
    let local = dir.path().join("srv/docs/readme.txt").display().to_string();
    let here = sc(&dir, "").args(["hash", &local]).output().unwrap().stdout;
    let there =
        sc(&dir, "").args(["hash", &uri(&dir, "srv/docs/readme.txt")]).output().unwrap().stdout;
    let digest =
        |out: &[u8]| String::from_utf8_lossy(out).split_whitespace().next().unwrap().to_owned();
    assert_eq!(digest(&here), digest(&there));
}
