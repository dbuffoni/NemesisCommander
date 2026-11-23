//! The screen description ([`Screen::of`]) of a real application on
//! temporary folders: what a desktop interface receives.

use std::fs;
use std::time::Duration;

use nmc_app::AppContext;
use nmc_config::{AppPaths, Key};
use nmc_frontend::view::{OverlayView, Screen};
use nmc_frontend::{App, AppEvent, KeyPress};
use nmc_vfs::VfsPath;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;

async fn app() -> (App, UnboundedReceiver<AppEvent>, TempDir) {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("left/sub")).unwrap();
    fs::create_dir_all(dir.path().join("right")).unwrap();
    for i in 0..30 {
        fs::write(dir.path().join(format!("left/file{i:02}.txt")), "x").unwrap();
    }
    let context = AppContext::load_from(AppPaths::in_dir(dir.path().join("settings"))).unwrap();
    let left = VfsPath::from_local(dir.path().join("left")).unwrap();
    let right = VfsPath::from_local(dir.path().join("right")).unwrap();
    let (mut app, mut events) = App::new(context, left, right).unwrap();
    settle(&mut app, &mut events).await;
    (app, events, dir)
}

async fn settle(app: &mut App, events: &mut UnboundedReceiver<AppEvent>) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while app.is_busy() {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Some(event)) => app.handle_event(event),
            _ => panic!("timed out"),
        }
    }
}

#[tokio::test]
async fn panels_show_only_the_rows_that_fit() {
    let (mut app, _events, _dir) = app().await;
    let screen = Screen::of(&mut app, 10, 20);
    let left = &screen.panels[0];
    assert!(left.active && !screen.panels[1].active);
    assert_eq!(left.rows.len(), 10);
    assert_eq!(left.total, 32, "parent entry, a folder and 30 files");
    assert_eq!(left.rows[0].size, "UP");
    assert_eq!(left.rows[1].name, "sub/");
    assert!(left.rows[0].cursor);
    assert_eq!(left.footer, "31 items");
    assert_eq!(screen.keys[4].label, "Copy");

    // Moving to the end scrolls the panel.
    app.handle_key(KeyPress::key(Key::End));
    let screen = Screen::of(&mut app, 10, 20);
    let left = &screen.panels[0];
    assert_eq!(left.offset, 22);
    assert!(left.rows.last().unwrap().cursor);
    assert_eq!(left.rows.last().unwrap().name, "file29.txt");
    assert!(screen.status.text.starts_with("file29.txt"), "{:?}", screen.status);
}

#[tokio::test]
async fn dialogs_are_described() {
    let (mut app, _events, _dir) = app().await;
    app.handle_key(KeyPress::key(Key::F(1)));
    match Screen::of(&mut app, 10, 20).overlay {
        Some(OverlayView::Help { groups, .. }) => assert!(!groups.is_empty()),
        other => panic!("{other:?}"),
    }
    app.handle_key(KeyPress::key(Key::Esc));

    app.handle_key(KeyPress::key(Key::F(7)));
    for c in "new".chars() {
        app.handle_key(KeyPress::char(c));
    }
    match Screen::of(&mut app, 10, 20).overlay {
        Some(OverlayView::Input { text, cursor, .. }) => {
            assert_eq!((text.as_str(), cursor), ("new", 3))
        }
        other => panic!("{other:?}"),
    }

    // As JSON, overlays carry their kind.
    app.handle_key(KeyPress::key(Key::Esc));
    app.handle_key(KeyPress::key(Key::F(9)));
    let json = serde_json::to_value(Screen::of(&mut app, 10, 20)).unwrap();
    assert_eq!(json["overlay"]["kind"], "commands");
    assert_eq!(json["overlay"]["categories"][0], "All");
    let copy = json["overlay"]["rows"].as_array().unwrap().iter().find(|r| r["id"] == "files.copy");
    assert_eq!(copy.unwrap()["name"], "Files/Copy");
    assert_eq!(copy.unwrap()["keys"], "F5");
    assert_eq!(json["panels"][0]["rows"][0]["size"], "UP");
}

/// Tools, "Write a script with AI…" asks what the script should do (the
/// answer goes to `nmc ai new`).
#[tokio::test]
async fn ai_scripts_start_with_a_question() {
    let (mut app, _events, _dir) = app().await;
    assert!(app.run_command("tools.ai-script"));
    match Screen::of(&mut app, 10, 20).overlay {
        Some(OverlayView::Input { title, .. }) if cfg!(feature = "scripting") => {
            assert_eq!(title, "Write a script with AI")
        }
        None if !cfg!(feature = "scripting") => {}
        other => panic!("{other:?}"),
    }
}
