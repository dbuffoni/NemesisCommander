//! `pnxr run`: scripts through the real program, with permissions granted on
//! the command line.
#![cfg(feature = "scripting")]

use std::fs;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

fn pnxr(dir: &TempDir) -> Command {
    let mut command = Command::cargo_bin("pnxr").unwrap();
    command
        .current_dir(dir.path())
        .env("PNXR_CONFIG_DIR", dir.path().join("config"))
        .env("PNXR_DATA_DIR", dir.path().join("data"));
    command
}

#[test]
fn a_script_runs_with_the_locations_it_is_given() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("work")).unwrap();
    fs::write(dir.path().join("work/a.log"), "x").unwrap();
    fs::write(dir.path().join("work/b.txt"), "y").unwrap();
    let script = r#"
        const [folder, suffix] = pnxr.args;
        for (const entry of await pnxr.list(folder)) {
          if (entry.name.endsWith(suffix)) {
            await pnxr.rename(entry.path, entry.path + ".old");
            pnxr.print(`renamed ${entry.name}`);
          }
        }
    "#;
    fs::write(dir.path().join("tidy.ts"), script).unwrap();

    pnxr(&dir)
        .args(["run", "tidy.ts", "--", "work", ".log"])
        .assert()
        .failure()
        .stderr(contains("--allow-read"));

    pnxr(&dir)
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
    pnxr(&dir).args(["run", "bad.ts"]).assert().failure().stderr(contains("stop here"));
    pnxr(&dir).args(["run", "missing.ts"]).assert().failure().stderr(contains("missing.ts"));
    fs::write(dir.path().join("loop.js"), "while (true) {}").unwrap();
    pnxr(&dir)
        .args(["run", "loop.js", "--timeout", "0.3"])
        .assert()
        .failure()
        .stderr(contains("longer than 0.3 s"));
    pnxr(&dir).args(["run", "--types"]).assert().success().stdout(contains("readText"));
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
        for (const path of pnxr.context!.selection) {
          pnxr.print(`${pnxr.basename(path)}: ${(await pnxr.readText(path)).length}`);
        }
    "#;
    fs::write(source.join("main.ts"), main).unwrap();

    pnxr(&dir)
        .args(["plugin", "install", "count-source"])
        .assert()
        .success()
        .stdout("installed count 0.1.0\n")
        .stderr(contains("read the selected files"));
    pnxr(&dir).args(["plugin", "list"]).assert().success().stdout(contains("not allowed yet"));

    // Not interactive here: running needs the permission first.
    pnxr(&dir)
        .args(["plugin", "run", "count", "chars", "work/a.txt"])
        .assert()
        .failure()
        .stderr(contains("pnxr plugin allow count"));
    pnxr(&dir).args(["plugin", "allow", "count"]).assert().success();
    pnxr(&dir)
        .args(["plugin", "run", "count", "chars", "work/a.txt"])
        .assert()
        .success()
        .stdout("a.txt: 3\n");

    pnxr(&dir).args(["plugin", "remove", "count"]).assert().success();
    pnxr(&dir)
        .args(["plugin", "run", "count", "chars"])
        .assert()
        .failure()
        .stderr(contains("no plugin"));
}

/// The examples in the repository keep working.
#[test]
fn the_examples_work() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let examples = repository.join("examples");
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("work/deep")).unwrap();
    fs::write(dir.path().join("work/big.bin"), vec![0u8; 3 * 1024 * 1024]).unwrap();
    fs::write(dir.path().join("work/deep/small.txt"), "s").unwrap();
    fs::write(dir.path().join("work/photo.JPG"), "j").unwrap();
    let largest = examples.join("scripts/largest-files.ts");
    pnxr(&dir)
        .arg("run")
        .arg(&largest)
        .args(["--allow-read", "work", "--", "work", "1"])
        .assert()
        .success()
        .stdout(contains("3.0 MiB").and(contains("big.bin")).and(contains("small").not()));

    pnxr(&dir)
        .args(["plugin", "install"])
        .arg(repository.join("plugins/sort-by-type"))
        .assert()
        .success();
    pnxr(&dir).args(["plugin", "allow", "sort-by-type"]).assert().success();
    pnxr(&dir)
        .args(["plugin", "run", "sort-by-type", "sort", "--panel", "work"])
        .args(["work/photo.JPG", "work/big.bin", "work/deep"])
        .assert()
        .success()
        .stdout("moved 2 files\n");
    assert!(dir.path().join("work/jpg/photo.JPG").exists());
    assert!(dir.path().join("work/bin/big.bin").exists());
    assert!(dir.path().join("work/deep/small.txt").exists());

    // The template plugin creates one file of a known format, several times.
    pnxr(&dir)
        .args(["plugin", "install"])
        .arg(repository.join("plugins/random-file"))
        .assert()
        .success();
    pnxr(&dir).args(["plugin", "allow", "random-file"]).assert().success();
    for _ in 0..12 {
        pnxr(&dir)
            .args(["plugin", "run", "random-file", "create", "--panel", "work"])
            .assert()
            .success()
            .stdout(contains("created random-"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in fs::read_dir(dir.path().join("work")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(extension) = name.strip_prefix("random-").and_then(|n| n.split('.').nth(1)) else {
            continue;
        };
        let bytes = fs::read(&path).unwrap();
        match extension {
            "json" => {
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
            }
            "xml" => assert!(bytes.starts_with(b"<?xml")),
            "txt" => assert!(!bytes.is_empty()),
            "png" => {
                assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
                assert!(bytes.ends_with(b"IEND\xaeB`\x82"));
            }
            other => panic!("unexpected {name} ({other})"),
        }
        seen.insert(extension.to_owned());
    }
    assert!(!seen.is_empty());
}

/// `pnxr test` runs a script in a throwaway folder and reports what changed,
/// without touching the files next to it.
#[test]
fn test_reports_what_a_script_did() {
    let dir = TempDir::new().unwrap();
    let script = r#"
        const [folder] = pnxr.args;
        for (const entry of await pnxr.list(folder)) {
          if (entry.kind === "file" && entry.size === 0) {
            await pnxr.remove(entry.path);
            pnxr.print(`removed ${entry.name}`);
          }
        }
        await pnxr.writeText(pnxr.join(folder, "notes.txt"), "rewritten");
    "#;
    fs::write(dir.path().join("tidy.ts"), script).unwrap();
    let output = pnxr(&dir)
        .env("PNXR_TSC", "no-such-tsc")
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
    fs::write(dir.path().join("bad.ts"), "await pnxr.remove('/etc/passwd');").unwrap();
    pnxr(&dir)
        .env("PNXR_TSC", "no-such-tsc")
        .args(["test", "bad.ts"])
        .assert()
        .code(1)
        .stdout(contains("PermissionDenied").or(contains("no permission")).and(contains("FAILED")));
    fs::write(dir.path().join("broken.ts"), "const = 1;").unwrap();
    pnxr(&dir).args(["test", "broken.ts"]).assert().code(1).stdout(contains("syntax  "));
}

/// Plugins are tested command by command, with their permissions filled
/// in with the test folders.
#[test]
fn test_runs_plugin_commands() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = TempDir::new().unwrap();
    pnxr(&dir)
        .env("PNXR_TSC", "no-such-tsc")
        .arg("test")
        .arg(repository.join("plugins/sort-by-type"))
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
    fs::write(
        source.join("main.ts"),
        "await pnxr.writeText(pnxr.join(pnxr.context!.panel!, 'x'), '');",
    )
    .unwrap();
    pnxr(&dir)
        .args(["test", "reader", "--json"])
        .env("PNXR_TSC", "no-such-tsc")
        .assert()
        .code(1)
        .stdout(contains("/somewhere is not granted").and(contains("--allow-write")));
    pnxr(&dir)
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
        "const n: number = (await pnxr.list('.')).length; pnxr.print(n);",
    )
    .unwrap();
    pnxr(&dir).args(["test", "good.ts"]).assert().success().stdout(contains("types     passed"));
    fs::write(dir.path().join("typo.ts"), "await pnxr.lst('.');").unwrap();
    pnxr(&dir).args(["test", "typo.ts"]).assert().code(1).stdout(contains("lst"));
}

#[test]
fn the_guide_is_printed() {
    let dir = TempDir::new().unwrap();
    pnxr(&dir).args(["run", "--guide"]).assert().success().stdout(contains("pnxr test"));
}

/// A Git repository (made with the system's git) holding a plugin.
fn plugin_repository(dir: &std::path::Path, version: &str, description: &str) {
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    std::fs::create_dir_all(dir).unwrap();
    if !dir.join(".git").exists() {
        git(&["init", "--quiet", "--initial-branch=main"]);
    }
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"name":"fromgit","version":"{version}","description":"{description}",
                "commands":[{{"id":"go","title":"Go"}}]}}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("main.ts"), "pnxr.print('hi');").unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", version]);
}

#[test]
fn a_plugin_installs_from_git_and_updates() {
    if std::process::Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    plugin_repository(&repo, "1.0.0", "first");
    let url = format!("file://{}", repo.display());

    let pnxr = || {
        let mut command = Command::cargo_bin("pnxr").unwrap();
        command
            .env("PNXR_CONFIG_DIR", dir.path().join("config"))
            .env("PNXR_DATA_DIR", dir.path().join("data"));
        command
    };
    pnxr().args(["plugin", "install", &url]).assert().success().stdout(contains("fromgit 1.0.0"));
    // The repository itself is not copied, only the plugin.
    assert!(!dir.path().join("data/plugins/fromgit/.git").exists());
    assert!(dir.path().join("data/plugins/fromgit/main.ts").exists());
    pnxr()
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(contains("fromgit 1.0.0").and(contains(format!("from {url}"))));
    let lock = std::fs::read_to_string(dir.path().join("data/plugin-lock.json")).unwrap();
    assert!(lock.contains("\"fromgit\"") && lock.contains("\"commit\""), "{lock}");

    pnxr().args(["plugin", "update"]).assert().success().stdout(contains("up to date"));

    plugin_repository(&repo, "1.1.0", "second");
    pnxr()
        .args(["plugin", "update", "fromgit"])
        .assert()
        .success()
        .stdout(contains("1.0.0 -> 1.1.0"));
    pnxr().args(["plugin", "list"]).assert().stdout(contains("fromgit 1.1.0"));
    pnxr().args(["plugin", "update"]).assert().success().stdout(contains("up to date"));

    // A plugin copied from a folder has no source to update from.
    pnxr().args(["plugin", "install", repo.to_str().unwrap()]).assert().success();
    pnxr()
        .args(["plugin", "update", "fromgit"])
        .assert()
        .failure()
        .stderr(contains("not installed from Git"));
    // A missing repository is reported, not hung on.
    pnxr()
        .args(["plugin", "install", &format!("file://{}/nope", dir.path().display())])
        .assert()
        .failure()
        .stderr(contains("git clone"));
}

#[test]
fn plugin_repositories_are_added_listed_and_installed_by_name() {
    if std::process::Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("repo");
    let plugin_dir = repo.join("plugins/hello");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{"name":"hello","version":"1.0.0","description":"Says hello",
            "commands":[{"id":"hi","title":"Hi"}]}"#,
    )
    .unwrap();
    fs::write(plugin_dir.join("main.ts"), "pnxr.print('hello');").unwrap();
    for args in [vec!["init", "-q"], vec!["add", "."], vec!["commit", "-q", "-m", "x"]] {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let url = format!("file://{}", repo.display()).replace('\\', "/");

    // Nothing is added without a confirmation.
    pnxr(&dir).args(["plugin", "repo", "add", &url]).assert().failure().stderr(contains("--yes"));
    pnxr(&dir).args(["plugin", "repo", "add", &url, "--yes"]).assert().success();
    pnxr(&dir).args(["plugin", "repo", "list"]).assert().success().stdout(contains(&url));
    // The official repository is switched off so the test needs no network.
    pnxr(&dir).args(["plugin", "repo", "official", "off"]).assert().success();
    pnxr(&dir)
        .args(["plugin", "available"])
        .assert()
        .success()
        .stdout(contains("hello 1.0.0  [Unofficial").and(contains("Says hello")));
    pnxr(&dir).args(["plugin", "install", "hello"]).assert().success();
    pnxr(&dir)
        .args(["plugin", "available", "--search", "hello"])
        .assert()
        .success()
        .stdout(contains("installed 1.0.0"));
    pnxr(&dir).args(["plugin", "repo", "remove", &url]).assert().success();
    pnxr(&dir).args(["plugin", "available"]).assert().success().stdout(contains("no plugin found"));
    // Installed plugins stay.
    pnxr(&dir).args(["plugin", "list"]).assert().success().stdout(contains("hello 1.0.0"));
}
