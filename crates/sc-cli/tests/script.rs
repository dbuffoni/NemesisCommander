//! `sc run`: scripts through the real program, with permissions granted on
//! the command line.
#![cfg(feature = "scripting")]

use std::fs;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

fn sc(dir: &TempDir) -> Command {
    let mut command = Command::cargo_bin("sc").unwrap();
    command
        .current_dir(dir.path())
        .env("SC_CONFIG_DIR", dir.path().join("config"))
        .env("SC_DATA_DIR", dir.path().join("data"));
    command
}

#[test]
fn a_script_runs_with_the_locations_it_is_given() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("work")).unwrap();
    fs::write(dir.path().join("work/a.log"), "x").unwrap();
    fs::write(dir.path().join("work/b.txt"), "y").unwrap();
    let script = r#"
        const [folder, suffix] = sc.args;
        for (const entry of await sc.list(folder)) {
          if (entry.name.endsWith(suffix)) {
            await sc.rename(entry.path, entry.path + ".old");
            sc.print(`renamed ${entry.name}`);
          }
        }
    "#;
    fs::write(dir.path().join("tidy.ts"), script).unwrap();

    sc(&dir)
        .args(["run", "tidy.ts", "--", "work", ".log"])
        .assert()
        .failure()
        .stderr(contains("--allow-read"));

    sc(&dir)
        .args(["run", "tidy.ts", "--allow-write", "work", "--", "work", ".log"])
        .assert()
        .success()
        .stdout("renamed a.log\n");
    assert!(dir.path().join("work/a.log.old").exists());
    assert!(dir.path().join("work/b.txt").exists());
}

#[test]
fn errors_and_types() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.ts"), "throw new Error('stop here');").unwrap();
    sc(&dir).args(["run", "bad.ts"]).assert().failure().stderr(contains("stop here"));
    sc(&dir).args(["run", "missing.ts"]).assert().failure().stderr(contains("missing.ts"));
    fs::write(dir.path().join("loop.js"), "while (true) {}").unwrap();
    sc(&dir)
        .args(["run", "loop.js", "--timeout", "0.3"])
        .assert()
        .failure()
        .stderr(contains("longer than 0.3 s"));
    sc(&dir).args(["run", "--types"]).assert().success().stdout(contains("readText"));
}
