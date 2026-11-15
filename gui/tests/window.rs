//! Starts the real window (with Qt's offscreen platform, so no display is
//! needed), opens each dialog through the keyboard and checks that QML drew
//! it without complaints. `--self-test` prints the final screen as JSON.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

struct Run {
    screen: serde_json::Value,
    stderr: String,
}

/// Runs the window on two folders of `home`, pressing `keys`.
fn run(home: &Path, keys: &str, screenshot: Option<&Path>) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sc-gui"));
    command
        .arg("--self-test")
        .arg(format!("--keys={keys}"))
        .arg(home.join("left"))
        .arg(home.join("right"))
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_RUNTIME_DIR", home);
    if let Some(path) = screenshot {
        command.arg(format!("--screenshot={}", path.display()));
    }
    let output = command.output().expect("sc-gui runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "sc-gui failed: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let screen = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    Run { screen, stderr }
}

fn home() -> TempDir {
    let home = TempDir::new().unwrap();
    std::fs::create_dir(home.path().join("left")).unwrap();
    std::fs::create_dir(home.path().join("right")).unwrap();
    std::fs::write(home.path().join("left/notes.txt"), "first line\nsecond line\n").unwrap();
    home
}

/// QML reports binding errors and missing properties as warnings that name
/// the file; a clean run has none.
fn assert_no_qml_warnings(run: &Run) {
    assert!(!run.stderr.contains("qrc:/"), "QML warnings:\n{}", run.stderr);
}

#[test]
fn the_window_shows_both_panels() {
    let home = home();
    let shot = home.path().join("window.png");
    let run = run(home.path(), "", Some(&shot));
    assert_no_qml_warnings(&run);
    let panels = &run.screen["panels"];
    assert_eq!(panels[0]["active"], true);
    assert_eq!(panels[0]["rows"][1]["name"], "notes.txt");
    assert_eq!(run.screen["keys"][0]["label"], "Help");
    let picture = std::fs::metadata(&shot).expect("screenshot saved");
    assert!(picture.len() > 1000);
}

#[test]
fn every_dialog_draws_cleanly() {
    let home = home();
    for (keys, kind) in [
        ("F1", "help"),
        ("F9", "menu"),
        ("F7", "input"),
        ("Down F8", "confirm"),
        ("Ctrl+Shift+D Ctrl+D", "choose"),
        ("Down F3", "viewer"),
        ("Ctrl+J", "jobs"),
    ] {
        let run = run(home.path(), keys, None);
        assert_no_qml_warnings(&run);
        assert_eq!(run.screen["overlay"]["kind"], kind, "{keys}: {}", run.screen["overlay"]);
    }
}

#[test]
fn the_quick_view_replaces_the_other_panel() {
    let home = home();
    let run = run(home.path(), "Down Ctrl+Q", None);
    assert_no_qml_warnings(&run);
    let lines = &run.screen["panels"][1]["quick_view"]["lines"];
    assert_eq!(lines[0], "first line");
}

#[test]
fn keys_reach_the_application() {
    let home = home();
    let run = run(home.path(), "F7 Esc Tab", None);
    assert_no_qml_warnings(&run);
    assert!(run.screen["overlay"].is_null(), "Esc closed the dialog");
    assert_eq!(run.screen["panels"][1]["active"], true, "Tab switched panels");
}
