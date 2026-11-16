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

/// `sc test` runs a script in a throwaway folder and reports what changed,
/// without touching the files next to it.
#[test]
fn test_reports_what_a_script_did() {
    let dir = TempDir::new().unwrap();
    let script = r#"
        const [folder] = sc.args;
        for (const entry of await sc.list(folder)) {
          if (entry.kind === "file" && entry.size === 0) {
            await sc.remove(entry.path);
            sc.print(`removed ${entry.name}`);
          }
        }
        await sc.writeText(sc.join(folder, "notes.txt"), "rewritten");
    "#;
    fs::write(dir.path().join("tidy.ts"), script).unwrap();
    let output = sc(&dir)
        .env("SC_TSC", "no-such-tsc")
        .args(["test", "tidy.ts", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["passed"], true, "{report}");
    assert_eq!(report["checks"][1]["name"], "types");
    assert_eq!(report["checks"][1]["status"], "skipped");
    let run = &report["runs"][0];
    assert_eq!(run["output"], serde_json::json!(["removed empty.log"]));
    assert_eq!(run["removed"], serde_json::json!(["panel/empty.log"]));
    assert_eq!(run["changed"], serde_json::json!(["panel/notes.txt"]));

    // Errors and syntax errors fail the test, with the message.
    fs::write(dir.path().join("bad.ts"), "await sc.remove('/etc/passwd');").unwrap();
    sc(&dir)
        .env("SC_TSC", "no-such-tsc")
        .args(["test", "bad.ts"])
        .assert()
        .code(1)
        .stdout(contains("PermissionDenied").or(contains("no permission")).and(contains("FAILED")));
    fs::write(dir.path().join("broken.ts"), "const = 1;").unwrap();
    sc(&dir).args(["test", "broken.ts"]).assert().code(1).stdout(contains("syntax  "));
}

/// Plugins are tested command by command, with their permissions filled
/// in with the test folders.
#[test]
fn test_runs_plugin_commands() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let dir = TempDir::new().unwrap();
    sc(&dir)
        .env("SC_TSC", "no-such-tsc")
        .arg("test")
        .arg(examples.join("plugins/sort-by-type"))
        .args(["--select", "photo.JPG", "--select", "notes.txt"])
        .assert()
        .success()
        .stdout(
            contains("run sort  passed")
                .and(contains("| moved 2 files"))
                .and(contains("created panel/jpg/photo.JPG"))
                .and(contains("removed panel/notes.txt")),
        );
    // Nothing was installed.
    assert!(!dir.path().join("data/plugins").exists());

    // A plugin asking for too little fails, and says why.
    let source = dir.path().join("reader");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("plugin.json"),
        r#"{ "name": "reader", "version": "1.0.0",
             "commands": [{ "id": "touch", "title": "Touch" }],
             "permissions": { "read": ["$PANEL"], "write": ["/somewhere"] } }"#,
    )
    .unwrap();
    fs::write(source.join("main.ts"), "await sc.writeText(sc.join(sc.context!.panel!, 'x'), '');")
        .unwrap();
    sc(&dir)
        .args(["test", "reader", "--json"])
        .env("SC_TSC", "no-such-tsc")
        .assert()
        .code(1)
        .stdout(contains("/somewhere is not granted").and(contains("--allow-write")));
    sc(&dir)
        .args(["test", "reader", "--command", "nope"])
        .assert()
        .code(1)
        .stdout(contains("no command"));
}

/// With the TypeScript compiler installed, type errors fail the test.
#[test]
fn test_checks_types_with_tsc() {
    if std::process::Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("skipped: tsc is not installed");
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("good.ts"),
        "const n: number = (await sc.list('.')).length; sc.print(n);",
    )
    .unwrap();
    sc(&dir).args(["test", "good.ts"]).assert().success().stdout(contains("types     passed"));
    fs::write(dir.path().join("typo.ts"), "await sc.lst('.');").unwrap();
    sc(&dir).args(["test", "typo.ts"]).assert().code(1).stdout(contains("lst"));
}

#[test]
fn the_guide_is_printed() {
    let dir = TempDir::new().unwrap();
    sc(&dir).args(["run", "--guide"]).assert().success().stdout(contains("sc test"));
}
