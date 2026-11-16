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
        // Windows and macOS ignore XDG: keep each test's settings and
        // transfer journal apart, or parallel tests lock each other out.
        .env("SC_CONFIG_DIR", home.join("config"))
        .env("SC_DATA_DIR", home.join("data"))
        .env("XDG_RUNTIME_DIR", home);
    if let Some(path) = screenshot {
        command.arg(format!("--screenshot={}", path.display()));
    }
    let output = command.output().expect("sc-gui runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "sc-gui failed: {stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let screen = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("keys {keys:?}: {e}\nstdout: {stdout}\nstderr: {stderr}"));
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
        // Typing in the menu finds commands.
        ("F9 s e t", "menu"),
        ("F9 z z z", "menu"),
        ("F7", "input"),
        ("Down F8", "confirm"),
        ("Ctrl+Shift+D Ctrl+D", "choose"),
        ("Down F3", "viewer"),
        ("Ctrl+J", "jobs"),
        ("Ctrl+,", "settings"),
        // Recording a shortcut.
        ("Ctrl+, End Up Up Up Up Up Up Up Up Up Enter", "settings"),
    ] {
        let run = run(home.path(), keys, None);
        assert_no_qml_warnings(&run);
        assert_eq!(run.screen["overlay"]["kind"], kind, "{keys}: {}", run.screen["overlay"]);
    }
}

#[test]
fn typing_in_the_menu_finds_commands() {
    let home = home();
    let found = run(home.path(), "F9 s e t t", None);
    assert_no_qml_warnings(&found);
    let menu = &found.screen["overlay"];
    assert_eq!(menu["filter"], "sett");
    assert_eq!(menu["shown"][0]["label"], "Settings…", "{menu}");
    let ran = run(home.path(), "F9 s e t t Enter", None);
    assert_eq!(ran.screen["overlay"]["kind"], "settings");
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

#[test]
fn every_viewer_draws_cleanly() {
    let home = home();
    for (name, data) in sc_preview::samples::all() {
        std::fs::write(home.path().join("left").join(name), data).unwrap();
    }
    let shot = home.path().join("viewer.png");
    // left/: .., budget.xlsx, guide.md, kickoff.pptx, notes.txt, offer.docx, report.pdf, torus.stl
    for (downs, mode) in
        [(1, "document"), (2, "document"), (5, "document"), (6, "pdf"), (7, "model")]
    {
        let keys = format!("{}F3", "Down ".repeat(downs));
        let run = run(home.path(), &keys, Some(&shot));
        assert_no_qml_warnings(&run);
        let viewer = &run.screen["overlay"];
        assert_eq!(viewer["mode"], mode, "{keys}: {viewer}");
        if mode == "document" {
            assert!(viewer["rich"].as_array().is_some_and(|l| !l.is_empty()), "{viewer}");
        } else {
            let url = viewer["image_url"].as_str().expect("a rendered picture");
            assert!(url.starts_with("file://") && url.ends_with(".png"), "{url}");
        }
    }
}

#[test]
fn the_settings_screen_saves_changes() {
    let home = home();
    // Down to "Show hidden files" (General has three options), Enter.
    let run = run(home.path(), "Ctrl+, Down Down Down Enter", None);
    assert_no_qml_warnings(&run);
    let rows = run.screen["overlay"]["rows"].as_array().unwrap();
    let hidden = rows.iter().find(|r| r["label"] == "Show hidden files").unwrap();
    assert_eq!(hidden["value"], "yes");
    assert_eq!(hidden["changed"], true);
    let saved = std::fs::read_to_string(home.path().join("config/config.toml")).unwrap();
    assert!(saved.contains("show_hidden = true"), "{saved}");
}
