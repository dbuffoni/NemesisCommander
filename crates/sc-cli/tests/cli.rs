//! End-to-end tests of the `sc` command line.
//!
//! Every test runs the real binary with settings and data redirected to a
//! temporary directory, so nothing touches the developer's own setup.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// A temporary sandbox with its own settings directories.
struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let sandbox = Sandbox { dir: TempDir::new().unwrap() };
        fs::create_dir_all(sandbox.path("src/sub")).unwrap();
        fs::write(sandbox.path("src/hello.txt"), "hello world\n").unwrap();
        fs::write(sandbox.path("src/sub/code.rs"), "fn main() {}\n").unwrap();
        fs::write(sandbox.path("src/.hidden"), "secret").unwrap();
        sandbox
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn arg(&self, relative: &str) -> String {
        self.path(relative).to_string_lossy().into_owned()
    }

    fn sc(&self) -> Command {
        let mut command = Command::cargo_bin("sc").unwrap();
        command
            .env("SC_CONFIG_DIR", self.path("config"))
            .env("SC_DATA_DIR", self.path("data"))
            .current_dir(self.dir.path());
        command
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn ls_lists_sorted_entries_with_directories_first() {
    let sandbox = Sandbox::new();
    sandbox.sc().args(["ls", "src"]).assert().success().stdout("sub/\nhello.txt\n");
}

#[test]
fn ls_all_shows_hidden_files() {
    let sandbox = Sandbox::new();
    sandbox
        .sc()
        .args(["ls", "-a", "src"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".hidden"));
}

#[test]
fn ls_json_is_one_object_per_line() {
    let sandbox = Sandbox::new();
    let output = sandbox.sc().args(["ls", "--json", "src"]).output().unwrap();
    assert!(output.status.success());
    let lines: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["name"], "hello.txt");
    assert_eq!(lines[1]["kind"], "file");
    assert_eq!(lines[1]["size"], 12);
}

#[test]
fn ls_of_missing_directory_fails() {
    let sandbox = Sandbox::new();
    sandbox
        .sc()
        .args(["ls", "nothing-here"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn cp_copies_a_tree_into_an_existing_directory() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("out")).unwrap();
    sandbox.sc().args(["cp", "src", "out"]).assert().success();
    assert_eq!(read(&sandbox.path("out/src/hello.txt")), "hello world\n");
    assert_eq!(read(&sandbox.path("out/src/sub/code.rs")), "fn main() {}\n");
}

#[test]
fn cp_conflict_skip_keeps_the_existing_file() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("out")).unwrap();
    fs::write(sandbox.path("out/hello.txt"), "old").unwrap();
    sandbox.sc().args(["cp", "--conflict", "skip", "src/hello.txt", "out"]).assert().success();
    assert_eq!(read(&sandbox.path("out/hello.txt")), "old");
}

#[test]
fn cp_conflict_rename_keeps_both_files() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("out")).unwrap();
    fs::write(sandbox.path("out/hello.txt"), "old").unwrap();
    sandbox.sc().args(["cp", "--conflict", "rename", "src/hello.txt", "out"]).assert().success();
    assert_eq!(read(&sandbox.path("out/hello.txt")), "old");
    let names: Vec<_> = fs::read_dir(sandbox.path("out")).unwrap().map(|e| e.unwrap()).collect();
    assert_eq!(names.len(), 2);
}

#[test]
fn cp_without_a_terminal_fails_on_conflicts_by_default() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("out")).unwrap();
    fs::write(sandbox.path("out/hello.txt"), "old").unwrap();
    sandbox.sc().args(["cp", "src/hello.txt", "out"]).assert().failure();
    assert_eq!(read(&sandbox.path("out/hello.txt")), "old");
}

#[test]
fn mv_moves_files() {
    let sandbox = Sandbox::new();
    sandbox.sc().args(["mv", "src/hello.txt", "moved.txt"]).assert().success();
    assert!(!sandbox.path("src/hello.txt").exists());
    assert_eq!(read(&sandbox.path("moved.txt")), "hello world\n");
}

#[test]
fn mkdir_and_rm() {
    let sandbox = Sandbox::new();
    sandbox.sc().args(["mkdir", "-p", "a/b/c"]).assert().success();
    assert!(sandbox.path("a/b/c").is_dir());
    sandbox.sc().args(["mkdir", "x/y"]).assert().failure();
    sandbox.sc().args(["rm", "-y", "a"]).assert().success();
    assert!(!sandbox.path("a").exists());
}

#[test]
fn rm_without_a_terminal_requires_yes() {
    let sandbox = Sandbox::new();
    sandbox.sc().args(["rm", "src"]).assert().failure();
    assert!(sandbox.path("src").exists());
}

#[test]
fn cat_prints_files() {
    let sandbox = Sandbox::new();
    sandbox.sc().args(["cat", "src/hello.txt"]).assert().success().stdout("hello world\n");
}

#[test]
fn hash_supports_all_algorithms() {
    let sandbox = Sandbox::new();
    let file = sandbox.arg("src/hello.txt");
    // sha256 of "hello world\n" is well known.
    sandbox.sc().args(["hash", "-a", "sha256", &file]).assert().success().stdout(
        predicate::str::starts_with(
            "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447",
        ),
    );
    for algorithm in ["blake3", "xxh3"] {
        sandbox.sc().args(["hash", "-a", algorithm, &file]).assert().success();
    }
    sandbox.sc().args(["hash", "-a", "md4", &file]).assert().failure();
}

#[test]
fn find_by_name_and_content() {
    let sandbox = Sandbox::new();
    sandbox
        .sc()
        .args(["find", "src", "-n", "*.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("code.rs").and(predicate::str::contains("hello").not()));
    sandbox
        .sc()
        .args(["find", "src", "-c", "WORLD", "-i"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello.txt:1: hello world"));
}

#[test]
fn pack_browse_and_unpack_round_trip() {
    let sandbox = Sandbox::new();
    for extension in ["zip", "tar.zst", "7z"] {
        let archive = format!("bundle.{extension}");
        sandbox.sc().args(["pack", "src", "-o", &archive]).assert().success();

        // Archives can be browsed as folders.
        let scheme = if extension == "7z" { "7z" } else { extension.split('.').next().unwrap() };
        let scheme = if extension == "tar.zst" { "tar" } else { scheme };
        let inside = format!("{scheme}://{}!/src", sandbox.arg(&archive));
        sandbox
            .sc()
            .args(["ls", &inside])
            .assert()
            .success()
            .stdout(predicate::str::contains("hello.txt"));

        let target = format!("unpacked-{extension}");
        sandbox.sc().args(["unpack", &archive, &target]).assert().success();
        assert_eq!(read(&sandbox.path(&format!("{target}/src/sub/code.rs"))), "fn main() {}\n");
    }
}

#[test]
fn rename_dry_run_changes_nothing() {
    let sandbox = Sandbox::new();
    sandbox
        .sc()
        .args(["rename", "-n", "-t", "file[C][X]", "src/hello.txt"])
        .assert()
        .success()
        .stdout("hello.txt -> file1.txt\n");
    assert!(sandbox.path("src/hello.txt").exists());
}

#[test]
fn rename_preview_marks_names_that_exist_and_renames_nothing() {
    let sandbox = Sandbox::new();
    fs::write(sandbox.path("src/taken.txt"), "keep\n").unwrap();
    for preview in [true, false] {
        let mut args = vec!["rename", "-t", "taken[X]", "src/hello.txt"];
        if preview {
            args.insert(1, "--preview");
        }
        sandbox
            .sc()
            .args(args)
            .assert()
            .failure()
            .stdout("hello.txt -> taken.txt  (already exists)\n");
    }
    assert!(sandbox.path("src/hello.txt").exists());
    assert_eq!(read(&sandbox.path("src/taken.txt")), "keep\n");
}

#[test]
fn rename_applies_templates_and_case() {
    let sandbox = Sandbox::new();
    sandbox
        .sc()
        .args(["rename", "-t", "[N][X]", "--case", "upper", "src/hello.txt"])
        .assert()
        .success();
    assert!(sandbox.path("src/HELLO.TXT").exists());
}

#[test]
fn queued_copy_runs_from_the_journal() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.path("out")).unwrap();
    sandbox
        .sc()
        .args(["cp", "--queue", "src", "out"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("queued"));
    assert!(!sandbox.path("out/src").exists());
    sandbox.sc().args(["jobs"]).assert().success().stdout(predicate::str::contains("queued"));
    sandbox.sc().args(["jobs", "run"]).assert().success();
    assert_eq!(read(&sandbox.path("out/src/hello.txt")), "hello world\n");
    sandbox.sc().args(["jobs", "purge"]).assert().success();
}

#[test]
fn config_export_and_import_round_trip() {
    let sandbox = Sandbox::new();
    sandbox.sc().args(["config", "export", "settings.toml"]).assert().success();
    let exported = read(&sandbox.path("settings.toml"));
    assert!(exported.contains("supercommander-settings"));
    sandbox.sc().args(["config", "import", "settings.toml"]).assert().success();
    sandbox.sc().args(["config", "import", "--merge", "settings.toml"]).assert().success();
    sandbox.sc().args(["config", "import", "src/hello.txt"]).assert().failure();
}

#[test]
fn config_path_uses_the_overrides() {
    let sandbox = Sandbox::new();
    sandbox
        .sc()
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains(sandbox.arg("config")));
}

#[test]
fn keys_lists_essential_shortcuts() {
    let sandbox = Sandbox::new();
    sandbox
        .sc()
        .arg("keys")
        .assert()
        .success()
        .stdout(predicate::str::contains("F5").and(predicate::str::contains("Copy")));
}

#[test]
fn config_push_and_pull_through_any_location() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.path("config")).unwrap();
    fs::write(sandbox.path("config/config.toml"), "[viewer]\ntab_width = 2\n").unwrap();
    let other = sandbox.arg("other/supercommander/config.toml");
    sandbox.sc().args(["config", "push", &other]).assert().success();
    assert!(read(&sandbox.path("other/supercommander/config.toml")).contains("tab_width = 2"));
    // A folder is enough once it exists.
    let folder = sandbox.arg("other/supercommander");
    sandbox.sc().args(["config", "push", "--merge", &folder]).assert().success();
    assert!(sandbox.path("other/supercommander/config.toml.bak").exists());
    sandbox.sc().args(["config", "pull", &folder]).assert().success();
    sandbox.sc().args(["config", "push", &sandbox.arg("missing")]).assert().failure();
}
