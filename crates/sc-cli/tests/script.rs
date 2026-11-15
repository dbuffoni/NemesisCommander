//! `sc run`: scripts through the real program, with permissions granted on
//! the command line.
#![cfg(feature = "scripting")]

use std::fs;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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

#[test]
fn plugins_are_installed_allowed_and_run() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("count-source");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(dir.path().join("work")).unwrap();
    fs::write(dir.path().join("work/a.txt"), "abc").unwrap();
    let manifest = r#"{
        "name": "count",
        "version": "0.1.0",
        "description": "Counts characters",
        "commands": [{ "id": "chars", "title": "Count characters" }],
        "permissions": { "read": ["$SELECTION"] }
    }"#;
    fs::write(source.join("plugin.json"), manifest).unwrap();
    let main = r#"
        for (const path of sc.context!.selection) {
          sc.print(`${sc.basename(path)}: ${(await sc.readText(path)).length}`);
        }
    "#;
    fs::write(source.join("main.ts"), main).unwrap();

    sc(&dir)
        .args(["plugin", "install", "count-source"])
        .assert()
        .success()
        .stdout("installed count 0.1.0\n")
        .stderr(contains("read the selected files"));
    sc(&dir).args(["plugin", "list"]).assert().success().stdout(contains("not allowed yet"));

    // Not interactive here: running needs the permission first.
    sc(&dir)
        .args(["plugin", "run", "count", "chars", "work/a.txt"])
        .assert()
        .failure()
        .stderr(contains("sc plugin allow count"));
    sc(&dir).args(["plugin", "allow", "count"]).assert().success();
    sc(&dir)
        .args(["plugin", "run", "count", "chars", "work/a.txt"])
        .assert()
        .success()
        .stdout("a.txt: 3\n");

    sc(&dir).args(["plugin", "remove", "count"]).assert().success();
    sc(&dir)
        .args(["plugin", "run", "count", "chars"])
        .assert()
        .failure()
        .stderr(contains("no plugin"));
}

/// The examples in the repository keep working.
#[test]
fn the_examples_work() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("work/deep")).unwrap();
    fs::write(dir.path().join("work/big.bin"), vec![0u8; 3 * 1024 * 1024]).unwrap();
    fs::write(dir.path().join("work/deep/small.txt"), "s").unwrap();
    fs::write(dir.path().join("work/photo.JPG"), "j").unwrap();
    let largest = examples.join("scripts/largest-files.ts");
    sc(&dir)
        .arg("run")
        .arg(&largest)
        .args(["--allow-read", "work", "--", "work", "1"])
        .assert()
        .success()
        .stdout(contains("3.0 MiB").and(contains("big.bin")).and(contains("small").not()));

    sc(&dir)
        .args(["plugin", "install"])
        .arg(examples.join("plugins/sort-by-type"))
        .assert()
        .success();
    sc(&dir).args(["plugin", "allow", "sort-by-type"]).assert().success();
    sc(&dir)
        .args(["plugin", "run", "sort-by-type", "sort", "--panel", "work"])
        .args(["work/photo.JPG", "work/big.bin", "work/deep"])
        .assert()
        .success()
        .stdout("moved 2 files\n");
    assert!(dir.path().join("work/jpg/photo.JPG").exists());
    assert!(dir.path().join("work/bin/big.bin").exists());
    assert!(dir.path().join("work/deep/small.txt").exists());
}
