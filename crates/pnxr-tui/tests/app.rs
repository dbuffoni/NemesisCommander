//! Drives the terminal application with key presses, exactly as a user
//! would, against real temporary folders, and checks both the resulting
//! files and what is drawn on screen.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use pnxr_app::AppContext;
use pnxr_config::{Action, AppPaths, Key};
use pnxr_ops::ConflictPolicy;
use pnxr_panel::SideId;
use pnxr_tui::{App, AppEvent, KeyPress, Overlay, draw};
use pnxr_vfs::VfsPath;
#[cfg(unix)]
use pnxr_vfs::{Provider, ProviderFactory, ProviderRegistry};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;

struct Harness {
    app: App,
    events: UnboundedReceiver<AppEvent>,
    dir: TempDir,
}

impl Harness {
    /// Left panel: `left/` with `a.txt`, `notes.txt` and `docs/readme.md`.
    /// Right panel: the empty folder `right/`.
    async fn new() -> Self {
        Self::with_settings(|_| {}).await
    }

    async fn with_settings(change: impl FnOnce(&mut pnxr_config::Config)) -> Self {
        Self::with_context(|context| change(&mut context.config)).await
    }

    async fn with_context(change: impl FnOnce(&mut AppContext)) -> Self {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("left/docs")).unwrap();
        fs::create_dir_all(root.join("right")).unwrap();
        fs::write(root.join("left/a.txt"), "alpha\n").unwrap();
        fs::write(root.join("left/notes.txt"), "the needle is here\n").unwrap();
        fs::write(root.join("left/docs/readme.md"), "# readme\n").unwrap();

        let paths = AppPaths::in_dir(root.join("settings"));
        let mut context = AppContext::load_from(paths).unwrap();
        // Tests must not depend on a desktop recycle bin.
        context.config.general.use_trash = false;
        change(&mut context);
        let left = VfsPath::from_local(root.join("left")).unwrap();
        let right = VfsPath::from_local(root.join("right")).unwrap();
        let (app, events) = App::new(context, left, right).unwrap();
        let mut harness = Harness { app, events, dir };
        harness.settle().await;
        harness
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn press(&mut self, press: KeyPress) {
        self.app.handle_key(press);
    }

    fn key(&mut self, chord: &str) {
        self.press(KeyPress::chord(chord));
    }

    /// Replaces the text of the open input field.
    fn type_text(&mut self, text: &str) {
        self.key("Ctrl+U");
        for c in text.chars() {
            self.press(KeyPress::char(c));
        }
    }

    /// Processes background results until nothing is running.
    async fn settle(&mut self) {
        self.settle_until(|app| !app.is_busy()).await;
    }

    /// Processes background results until `done` holds.
    async fn settle_until(&mut self, done: impl Fn(&App) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            while let Ok(event) = self.events.try_recv() {
                self.app.handle_event(event);
            }
            if done(&self.app) {
                return;
            }
            let wait = tokio::time::timeout_at(deadline, self.events.recv()).await;
            match wait {
                Ok(Some(event)) => self.app.handle_event(event),
                Ok(None) => panic!("event channel closed"),
                Err(_) => panic!("timed out; status: {:?}", self.app.status()),
            }
        }
    }

    fn names(&self, side: SideId) -> Vec<String> {
        self.app.workspace().side(side).panel().entries().iter().map(|e| e.name.clone()).collect()
    }

    fn current(&self) -> String {
        self.app.workspace().active().current_entry().map(|e| e.name.clone()).unwrap_or_default()
    }

    /// Moves the cursor of the focused panel to `name`.
    fn focus(&mut self, name: &str) {
        self.press(KeyPress::key(Key::Home));
        for _ in 0..100 {
            if self.current() == name {
                return;
            }
            self.press(KeyPress::key(Key::Down));
        }
        panic!("{name} not found in {:?}", self.names(self.app.workspace().active_side()));
    }

    fn screen(&mut self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
        terminal.draw(|frame| draw(frame, &mut self.app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    fn status(&self) -> String {
        self.app.status().map(|s| s.text.clone()).unwrap_or_default()
    }
}

#[tokio::test]
async fn both_panels_list_their_folders() {
    let mut h = Harness::new().await;
    assert_eq!(h.names(SideId::Left), ["..", "docs", "a.txt", "notes.txt"]);
    assert_eq!(h.names(SideId::Right), [".."]);
    let screen = h.screen();
    assert!(screen.contains("docs/"));
    assert!(screen.contains("<DIR>"));
    assert!(screen.contains("F1Help"), "function key bar:\n{screen}");
}

#[tokio::test]
async fn enter_and_backspace_navigate() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "readme.md"]);
    h.key("Backspace");
    h.settle().await;
    assert_eq!(h.current(), "docs", "cursor returns to the folder we left");
    h.key("Ctrl+O");
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "readme.md"], "history goes back");
}

#[tokio::test]
async fn copy_asks_for_the_destination_and_queues_a_job() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("F5");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("copy dialog") };
    assert_eq!(dialog.field.text(), h.path("right").display().to_string());
    h.key("Enter");
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/a.txt")).unwrap(), "alpha\n");
    assert!(h.status().starts_with("Done"), "{}", h.status());
    assert!(h.names(SideId::Right).contains(&"a.txt".to_owned()), "right panel reloaded");
}

#[tokio::test]
async fn move_of_a_selection() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Insert");
    h.key("Insert");
    h.key("F6");
    h.key("Enter");
    h.settle().await;
    assert!(h.path("right/a.txt").exists());
    assert!(h.path("right/notes.txt").exists());
    assert!(!h.path("left/a.txt").exists());
}

#[tokio::test]
async fn conflicts_are_answered_in_a_dialog() {
    let mut h = Harness::with_settings(|c| c.transfers.conflict = ConflictPolicy::Ask).await;
    fs::write(h.path("right/a.txt"), "old").unwrap();
    h.focus("a.txt");
    h.key("F5");
    h.key("Enter");
    h.settle_until(|app| app.pending_conflict().is_some()).await;
    assert!(h.screen().contains("File exists"));
    h.press(KeyPress::char('o'));
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/a.txt")).unwrap(), "alpha\n");
}

#[tokio::test]
async fn mkdir_creates_nested_folders_and_focuses_them() {
    let mut h = Harness::new().await;
    h.key("F7");
    h.type_text("new/sub");
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/new/sub").is_dir());
    assert_eq!(h.current(), "new");
}

#[tokio::test]
async fn delete_asks_first() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("F8");
    h.settle().await;
    let Some(Overlay::Confirm(dialog)) = h.app.overlay() else { panic!("confirmation") };
    assert!(dialog.lines[0].contains("permanently"), "{:?}", dialog.lines);
    h.press(KeyPress::char('n'));
    assert!(h.path("left/docs").exists());

    h.key("F8");
    h.settle().await;
    // Permanent: Enter says no, only y deletes.
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/docs").exists());
    h.key("F8");
    h.settle().await;
    h.press(KeyPress::char('y'));
    h.settle().await;
    assert!(!h.path("left/docs").exists());
    assert!(!h.names(SideId::Left).contains(&"docs".to_owned()));
}

#[tokio::test]
async fn rename_in_place() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("F2");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("rename dialog") };
    assert_eq!(dialog.field.cursor(), 1, "cursor before the extension");
    h.type_text("b.txt");
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/b.txt").exists());
    assert_eq!(h.current(), "b.txt");
}

#[tokio::test]
async fn multi_rename_previews_then_renames() {
    let mut h = Harness::new().await;
    h.key("+");
    h.type_text("*.txt");
    h.key("Enter");
    h.key("Shift+F2");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("template dialog") };
    assert_eq!(dialog.preview, ["  (no name changes)"]);
    h.key("Ctrl+U");
    h.type_text("file[C][X]");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("template dialog") };
    assert!(dialog.preview.iter().any(|l| l.contains("a.txt → file1.txt")), "{:?}", dialog.preview);
    assert!(h.screen().contains("a.txt → file1.txt"), "the preview is drawn");
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Confirm(dialog)) = h.app.overlay() else { panic!("preview") };
    assert!(dialog.lines.iter().any(|l| l.contains("a.txt → file1.txt")), "{:?}", dialog.lines);
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/file1.txt").exists());
    assert!(h.path("left/file2.txt").exists());
}

#[tokio::test]
async fn multi_rename_has_options_presets_and_help() {
    let mut h = Harness::new().await;
    h.key("+");
    h.type_text("*.txt");
    h.key("Enter");
    h.key("Shift+F2");
    h.type_text("doc-[C][X] | width=3 | upper");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("dialog") };
    assert!(dialog.hint().contains("Ctrl+O: ready-made    F1: help"), "{}", dialog.hint());
    assert!(dialog.preview[0].contains("2 of 2 names change"), "{:?}", dialog.preview);
    assert!(
        dialog.preview.iter().any(|l| l.contains("a.txt → DOC-001.TXT")),
        "{:?}",
        dialog.preview
    );
    h.type_text("[N] | bogus");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("dialog") };
    assert!(dialog.preview[0].contains("Unknown option"), "{:?}", dialog.preview);
    h.key("F1");
    assert!(h.screen().contains("Rename many files"));
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Input(_))), "help returns to the question");
    h.key("Ctrl+O");
    assert!(h.screen().contains("Ready-made rename rules"));
    h.key("Down");
    h.key("Down");
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("dialog") };
    assert_eq!(dialog.field.text(), "[C][X] | width=3");
}

#[tokio::test]
async fn multi_rename_shows_names_that_exist_and_renames_nothing() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Shift+F2");
    h.key("Ctrl+U");
    // "docs" is a folder next to the file.
    h.type_text("docs");
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Message(dialog)) = h.app.overlay() else { panic!("conflict preview") };
    assert!(
        dialog.lines.iter().any(|l| l.ends_with("→ docs  (already exists)")),
        "{:?}",
        dialog.lines
    );
    h.key("Esc");
    assert!(h.path("left/a.txt").exists(), "nothing renamed");
}

#[tokio::test]
async fn selection_by_mask_and_invert() {
    let mut h = Harness::new().await;
    h.key("+");
    h.type_text("*.txt");
    h.key("Enter");
    assert_eq!(h.app.workspace().active().selection_summary().files, 2);
    h.key("Ctrl+A");
    let summary = h.app.workspace().active().selection_summary();
    assert_eq!((summary.files, summary.dirs), (2, 1), "select all");
    h.focus("a.txt");
    h.key("Space");
    h.key("*");
    let summary = h.app.workspace().active().selection_summary();
    // Inverting flips files only; the folder stays selected.
    assert_eq!((summary.files, summary.dirs), (1, 1));
    assert!(h.app.workspace().active().is_selected("a.txt"));
    assert!(h.screen().contains("2 selected"));
}

#[tokio::test]
async fn the_mask_dialog_shows_matches_cycles_actions_and_remembers() {
    let mut h = Harness::new().await;
    h.key("+");
    h.type_text("*.txt");
    let screen = h.screen();
    assert!(screen.contains("2 of 2 files match"), "{screen}");
    // F2 turns the question into Unselect, then Keep only.
    h.key("F2");
    assert!(h.screen().contains("Unselect"));
    h.key("F2");
    assert!(h.screen().contains("Keep only"));
    h.key("F2");
    assert!(h.screen().contains("Select"));
    // Presets.
    h.key("Ctrl+O");
    assert!(h.screen().contains("Ready-made masks"));
    h.key("Esc");
    h.key("Esc");
    // The last mask is offered again.
    h.key("+");
    assert!(h.screen().contains("*.txt") || h.screen().contains("*"));
    h.key("Enter");
    assert_eq!(h.app.workspace().active().selection_summary().files, 2);
}

// Permission bits exist on Unix only.
#[cfg(unix)]
#[tokio::test]
async fn f1_in_a_list_explains_the_row_under_the_cursor() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Shift+F4");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))));
    h.type_text("e");
    for _ in 0..30 {
        let Some(Overlay::Choose(d)) = h.app.overlay() else { panic!("properties") };
        let pnxr_tui::ChoosePurpose::Properties(state) = &d.purpose else { panic!("properties") };
        if matches!(state.rows.get(d.cursor), Some(pnxr_frontend::properties::PropRow::Bit(_))) {
            break;
        }
        h.key("Down");
    }
    h.key("F1");
    assert!(h.screen().contains("switches this permission"), "{}", h.screen());
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))), "back to the list");
}

#[tokio::test]
async fn git_actions_stage_and_commit() {
    let mut h = Harness::new().await;
    let left = h.path("left");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").arg("-C").arg(&left).args(args).output();
        out.map(|o| o.status.success()).unwrap_or(false)
    };
    if !git(&["init", "-q"]) {
        return; // no git here
    }
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    h.focus("a.txt");
    h.key("Ctrl+F11");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))), "{}", h.status());
    h.key("Enter"); // stage
    h.settle().await;
    h.key("Ctrl+F11");
    h.key("Down");
    h.key("Down");
    h.key("Down");
    h.key("Enter"); // commit
    h.type_text("first");
    h.key("Enter");
    h.settle().await;
    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(&left)
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("first"));
}

#[tokio::test]
async fn quick_filter_narrows_while_typing() {
    let mut h = Harness::new().await;
    h.key("Ctrl+S");
    h.type_text("not");
    assert_eq!(h.names(SideId::Left), ["..", "notes.txt"]);
    h.key("Enter");
    assert!(h.screen().contains("filter: not"));
    h.key("Esc");
    assert_eq!(h.names(SideId::Left).len(), 4, "Esc clears the filter");
}

#[tokio::test]
async fn f1_shows_the_main_shortcuts() {
    let mut h = Harness::new().await;
    h.key("F1");
    let screen = h.screen();
    assert!(screen.contains("Main shortcuts"));
    assert!(screen.contains("F5") && screen.contains("Copy"));
    assert!(screen.contains("Ctrl+S") && screen.contains("Quick filter"));
    h.press(KeyPress::char('x'));
    assert!(h.app.overlay().is_none(), "any key closes help");
}

#[tokio::test]
async fn the_command_list_runs_commands() {
    let mut h = Harness::new().await;
    h.key("F9");
    let screen = h.screen();
    assert!(screen.contains("Commands"), "{screen}");
    assert!(
        screen.contains(" All ") && screen.contains(" Navigate ") && screen.contains(" Tools ")
    );
    assert!(screen.contains("Files/Copy") && screen.contains("F5"), "shortcuts are listed");
    // Left from All: the last category (Pairnox), whose first command is Help.
    h.key("Left");
    assert!(!h.screen().contains("Files/Copy"));
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Help)));

    // Clicks (the desktop window): a category button, then a row.
    h.key("Esc");
    h.key("F9");
    h.app.pick("category", 1);
    let screen = h.screen();
    assert!(screen.contains("Files/Copy") && !screen.contains("View/"), "{screen}");
    h.app.pick("category", 6);
    assert!(h.screen().contains("Pairnox/Show main shortcuts"));
    h.app.pick("command", 0);
    assert!(matches!(h.app.overlay(), Some(Overlay::Help)), "a click runs the row");
}

/// The number of screen rows from the title border to the bottom border of
/// the command list.
fn command_list_height(screen: &str) -> usize {
    let lines: Vec<&str> = screen.lines().collect();
    let top = lines.iter().position(|l| l.contains(" Commands ")).expect("title border");
    lines[top..].iter().position(|l| l.contains('└')).expect("bottom border")
}

#[tokio::test]
async fn the_command_list_keeps_its_height() {
    // the window must not change size as the filter narrows the list.
    let mut h = Harness::new().await;
    h.key("F9");
    let full = command_list_height(&h.screen());
    h.type_text("settings");
    assert_eq!(command_list_height(&h.screen()), full);
    h.key("Esc");
    h.type_text("zzz");
    assert_eq!(command_list_height(&h.screen()), full);
    h.key("Esc");
    h.key("F1");
    assert_eq!(command_list_height(&h.screen()), full, "with the help line");
}

#[tokio::test]
async fn typing_in_the_command_list_finds_commands() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("settings");
    let screen = h.screen();
    assert!(screen.contains("> settings_"), "{screen}");
    assert!(screen.contains("Pairnox/Settings"), "{screen}");
    assert!(!screen.contains("Files/Copy"));
    // Backspace erases; Esc empties the filter, a second Esc closes.
    h.key("Backspace");
    assert!(h.screen().contains("> setting_"));
    h.key("Esc");
    assert!(h.screen().contains("Files/Copy"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Commands(_))));
    h.type_text("zzz");
    assert!(h.screen().contains("Nothing matches"));
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Commands(_))), "nothing to run");
    h.key("Esc");
    h.type_text("main short");
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Help)), "Enter runs what was found");
}

#[tokio::test]
async fn a_function_key_is_refused_as_a_shortcut_and_the_function_key_stays() {
    let mut h = Harness::new().await;
    h.key("F9");
    save_screen("commands-two-columns", &h.screen());
    h.type_text("refresh");
    h.key("Ctrl+K");
    h.key("Shift+F8");
    assert!(h.screen().contains("function key"), "a hint to use the other action: {}", h.screen());
    assert!(h.app.context().config.keys.is_empty());
    assert!(h.app.context().config.fkeys.is_empty());
    // A shortcut given to a command with a function key keeps it.
    h.key("Esc");
    h.key("Esc");
    h.key("F9");
    h.type_text("files/copy");
    h.key("Ctrl+K");
    h.key("Ctrl+C");
    let keymap = &h.app.context().keymap;
    assert_eq!(keymap.fkey_for_id("files.copy"), Some("F5".parse().unwrap()));
    assert_eq!(keymap.shortcuts_for_id("files.copy"), ["Ctrl+C".parse().unwrap()]);
    assert!(h.app.context().config.fkeys.is_empty(), "the function key is still the built-in one");
}

#[tokio::test]
async fn a_command_gets_a_free_function_key_with_its_own_name_and_the_bar_shows_it() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("refresh");
    h.key("Ctrl+F");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))));
    let screen = h.screen();
    assert!(screen.contains("Function key for View/Refresh"), "{screen}");
    assert!(screen.contains("Ctrl+N: short name"), "{screen}");
    save_screen("function-key-list", &screen);
    // The list starts on the first free slot: Ctrl+F10 is taken (Choose), so go and look.
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!() };
    assert_eq!(dialog.items.len(), 30);
    assert!(dialog.items[4].contains("Copy"), "{:?}", dialog.items[4]);
    h.key("Enter");
    assert!(h.app.overlay().is_none());
    let keymap = &h.app.context().keymap;
    let chord = keymap.fkey_for_id("view.refresh").expect("it has a function key now");
    assert_eq!(h.app.context().config.fkeys["view.refresh"], chord.to_string());
    assert_eq!(keymap.shortcuts_for_id("view.refresh"), ["Ctrl+R".parse().unwrap()]);
    assert_eq!(keymap.bar_label(&chord).as_deref(), Some("Refresh"));
}

#[tokio::test]
async fn taking_a_function_key_from_another_command_asks_first() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("refresh");
    h.key("Ctrl+F");
    // Go to F5 (Copy's).
    h.key("Home");
    for _ in 0..4 {
        h.key("Down");
    }
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Confirm(_))));
    assert!(h.screen().contains("F5 belongs to"), "{}", h.screen());
    save_screen("function-key-replace", &h.screen());
    // No: nothing changes.
    h.key("n");
    assert!(h.app.context().config.fkeys.is_empty());
    assert_eq!(h.app.context().keymap.fkey_for_id("files.copy"), Some("F5".parse().unwrap()));
    // Yes: Copy loses it, Refresh has it, and Copy keeps its other keys.
    h.key("F9");
    h.type_text("refresh");
    h.key("Ctrl+F");
    h.key("Home");
    for _ in 0..4 {
        h.key("Down");
    }
    h.key("Enter");
    h.key("y");
    let keymap = &h.app.context().keymap;
    assert_eq!(keymap.fkey_for_id("view.refresh"), Some("F5".parse().unwrap()));
    assert_eq!(keymap.fkey_for_id("files.copy"), None);
    assert_eq!(h.app.context().config.fkeys["files.copy"], "");
}

#[tokio::test]
async fn the_short_name_of_a_button_is_changed_behind_ctrl_n_and_can_be_restored() {
    let mut h = Harness::new().await;
    assert!(h.screen().contains("F5Copy"));
    h.key("F9");
    h.type_text("files/copy");
    h.key("Ctrl+F");
    h.key("Ctrl+N");
    assert!(matches!(h.app.overlay(), Some(Overlay::Input(_))));
    save_screen("function-key-name", &h.screen());
    // The built-in name is in the field: nothing has to be typed to assign a key.
    h.key("Ctrl+U");
    h.type_text("Dup");
    h.key("Enter");
    assert_eq!(h.app.context().config.labels["files.copy"], "Dup");
    let screen = h.screen();
    assert!(screen.contains("F5Dup") && !screen.contains("F5Copy"), "{screen}");
    save_screen("bar-custom-name", &screen);
    // Empty text brings the built-in name back.
    h.key("F9");
    h.type_text("files/copy");
    h.key("Ctrl+F");
    h.key("Ctrl+N");
    h.key("Ctrl+U");
    h.key("Enter");
    assert!(h.app.context().config.labels.is_empty());
    assert!(h.screen().contains("F5Copy"), "{}", h.screen());
    // Too long: refused.
    h.key("F9");
    h.type_text("files/copy");
    h.key("Ctrl+F");
    h.key("Ctrl+N");
    h.key("Ctrl+U");
    h.type_text("A name that is far too long");
    h.key("Enter");
    assert!(h.app.context().config.labels.is_empty());
    assert!(h.screen().contains("at most 12"), "{}", h.screen());
}

#[tokio::test]
async fn delete_in_the_slot_list_removes_the_function_key_only() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("files/delete");
    h.key("Ctrl+F");
    h.key("Delete");
    let keymap = &h.app.context().keymap;
    assert_eq!(keymap.fkey_for_id("files.delete"), None);
    assert_eq!(keymap.shortcuts_for_id("files.delete"), ["Delete".parse().unwrap()]);
    // Ctrl+R in the list brings the built-in key and name back.
    h.key("F9");
    h.type_text("files/delete");
    h.key("Ctrl+F");
    h.key("Ctrl+R");
    assert_eq!(h.app.context().keymap.fkey_for_id("files.delete"), Some("F8".parse().unwrap()));
    assert!(h.app.context().config.fkeys.is_empty());
}

#[tokio::test]
async fn the_bar_row_key_stands_in_for_a_held_shift_or_ctrl() {
    let mut h = Harness::new().await;
    assert!(h.screen().contains("F5Copy"));
    h.key("Ctrl+F");
    let shifted = h.screen();
    assert!(shifted.contains("F5Pack"), "{shifted}");
    save_screen("bar-shift-row", &shifted);
    h.key("Ctrl+F");
    let ctrl = h.screen();
    assert!(!ctrl.contains("F5Pack") && !ctrl.contains("F5Copy"), "{ctrl}");
    save_screen("bar-ctrl-row", &ctrl);
    h.key("Ctrl+F");
    assert!(h.screen().contains("F5Copy"));
}

#[tokio::test]
async fn shortcuts_are_given_from_the_command_list() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("refresh");
    h.key("Ctrl+K");
    assert!(h.screen().contains("Press the new shortcut"));
    h.key("Alt+R");
    let screen = h.screen();
    assert!(screen.contains("Alt+R"), "{screen}");
    assert_eq!(h.app.context().config.keys["view.refresh"], ["Alt+R"]);
    // Delete takes it away again; Esc closes and the new key works.
    h.key("Delete");
    assert_eq!(h.app.context().config.keys["view.refresh"], Vec::<String>::new());
    h.key("Ctrl+K");
    h.key("Alt+R");
    h.key("Esc");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
    assert_eq!(
        h.app.context().keymap.action_for(&"Alt+R".parse().unwrap()),
        Some(pnxr_config::Action::Refresh)
    );
}

#[cfg(feature = "scripting")]
#[tokio::test]
async fn scripts_and_plugin_commands_are_listed_and_take_keys() {
    let mut h = Harness::new().await;
    let scripts = h.app.context().paths.scripts_dir();
    fs::create_dir_all(&scripts).unwrap();
    fs::write(scripts.join("hello.ts"), "pnxr.print('hello from a script');").unwrap();
    let plugin = h.app.context().paths.plugins_dir().join("tidy");
    fs::create_dir_all(&plugin).unwrap();
    let manifest = r#"{
        "name": "tidy",
        "version": "1.0.0",
        "commands": [
            { "id": "run", "title": "Tidy this folder", "category": "files" },
            { "id": "stats", "title": "Folder statistics" }
        ]
    }"#;
    fs::write(plugin.join("plugin.json"), manifest).unwrap();
    fs::write(plugin.join("main.ts"), "pnxr.print(pnxr.context!.command);").unwrap();

    h.key("F9");
    let screen = h.screen();
    assert!(screen.contains(" Plugins ") && screen.contains(" Scripts "), "{screen}");
    h.type_text("tidy");
    assert!(h.screen().contains("Files/Tidy this folder"), "the manifest's category");
    h.type_text("statistics");
    assert!(h.screen().contains("Plugins/Folder statistics"));
    h.type_text("hello");
    assert!(h.screen().contains("Scripts/hello.ts"));
    h.key("Ctrl+K");
    h.key("Alt+H");
    assert_eq!(h.app.context().config.keys["script.hello.ts"], ["Alt+H"]);
    assert!(h.screen().contains("Alt+H"), "the new key is listed");
    h.key("Esc");
    h.key("Esc");
    // The key runs the script, with no question: the user put it there.
    h.key("Alt+H");
    h.settle().await;
    assert!(h.status().contains("hello from a script"), "{}", h.status());

    // A plugin command takes a key the same way.
    h.key("F9");
    h.type_text("statistics");
    h.key("Ctrl+K");
    h.key("Alt+S");
    assert_eq!(h.app.context().config.keys["plugin.tidy.stats"], ["Alt+S"]);
}

#[tokio::test]
async fn viewer_shows_text_and_hex() {
    let mut h = Harness::new().await;
    h.focus("notes.txt");
    h.key("F3");
    h.settle().await;
    assert!(h.screen().contains("the needle is here"));
    h.press(KeyPress::char('h'));
    let screen = h.screen();
    assert!(screen.contains("[hex]") && screen.contains("74 68 65"), "{screen}");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn viewer_colours_source_files_by_language() {
    let mut h = Harness::new().await;
    fs::write(h.path("left/main.rs"), "fn main() {\n    let n = 42; // answer\n}\n").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("main.rs");
    h.key("F3");
    h.settle().await;
    let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
    let buffer = terminal.backend().buffer();
    let colour_of = |word: &str| -> Option<ratatui::style::Color> {
        let area = buffer.area;
        (0..area.height).find_map(|y| {
            let row: String = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
            let at = row.find(word)?;
            let x = row[..at].chars().count() as u16;
            Some(buffer[(x, y)].fg)
        })
    };
    use ratatui::style::Color;
    assert_eq!(colour_of("fn main"), Some(Color::Magenta), "keyword");
    assert_eq!(colour_of("42"), Some(Color::Yellow), "number");
    assert_eq!(colour_of("// answer"), Some(Color::DarkGray), "comment");
    assert!(h.screen().contains("Rust"), "the language is named");
    // Plain text stays plain.
    h.key("Esc");
    h.focus("notes.txt");
    h.key("F3");
    h.settle().await;
    let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
    let plain = terminal.backend().buffer().content().iter().any(|c| c.fg == Color::Magenta);
    assert!(!plain);
}

#[tokio::test]
async fn about_shows_the_logo_and_the_version() {
    let mut h = Harness::new().await;
    h.app.perform(pnxr_config::keymap::Action::About);
    let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = buffer.content().iter().map(|c| c.symbol()).collect();
    let title = format!("Pairnox {}", env!("CARGO_PKG_VERSION"));
    assert!(text.contains(&title), "version");
    assert!(text.contains("MIT"), "license");
    assert!(text.contains("github.com/dbuffoni/Pairnox"), "source");
    save_screen("about-new-logo", &text_of(buffer));
    // The logo is drawn with half blocks, some of them the blue band.
    let blue = buffer
        .content()
        .iter()
        .filter(
            |c| matches!(c.fg, ratatui::style::Color::Rgb(r, g, b) if b > 200 && r < 90 && g > 70 && g < 150),
        )
        .count();
    assert!(blue > 5, "{blue} blue cells");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn viewer_draws_images() {
    let mut h = Harness::new().await;
    // A small solid green PNG.
    let picture = image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 200, 0, 255]));
    picture.save(h.path("left/green.png")).unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("green.png");
    h.key("F3");
    h.settle().await;
    assert!(h.screen().contains("[image]"));
    let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
    let buffer = terminal.backend().buffer();
    let green = buffer
        .content()
        .iter()
        .filter(|cell| cell.symbol() == "▀" && cell.fg == ratatui::style::Color::Rgb(0, 200, 0))
        .count();
    assert_eq!(green, 8 * 4, "8 columns of 4 cells, two pixels each");
    h.press(KeyPress::char('h'));
    assert!(h.screen().contains("[hex]"));
}

/// Writes the preview samples to `left/` and shows them in the panel.
async fn with_samples() -> Harness {
    let mut h = Harness::new().await;
    for (name, data) in pnxr_preview::samples::all() {
        fs::write(h.path("left").join(name), data).unwrap();
    }
    h.key("Ctrl+R");
    h.settle().await;
    h
}

/// The picture the open viewer shows.
fn viewer_picture(h: &Harness) -> Option<std::sync::Arc<image::RgbaImage>> {
    match h.app.overlay() {
        Some(Overlay::Viewer(viewer)) => viewer.image.clone(),
        _ => None,
    }
}

/// Counts the cells drawn as picture pixels of `colour`.
fn cells_of(h: &mut Harness, colour: ratatui::style::Color) -> usize {
    let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .filter(|c| c.symbol() == "▀" && c.fg == colour)
        .count()
}

#[tokio::test]
async fn viewer_formats_documents() {
    let mut h = with_samples().await;
    h.focus("guide.md");
    h.key("F3");
    h.settle().await;
    let screen = h.screen();
    assert!(screen.contains("[document]"), "{screen}");
    assert!(screen.contains("Project notes") && !screen.contains("# Project"), "{screen}");
    assert!(screen.contains("• Install the AppImage"), "{screen}");
    assert!(screen.contains("F5  │ Copy"), "{screen}");
    h.key("Esc");

    h.focus("offer.docx");
    h.key("F3");
    h.settle().await;
    let screen = h.screen();
    assert!(
        screen.contains("Offer for the office move") && screen.contains("4,350 EUR"),
        "{screen}"
    );
    assert!(screen.contains("Transport │ 8"), "{screen}");
    h.key("Esc");

    h.focus("budget.xlsx");
    h.key("F3");
    h.settle().await;
    assert!(h.screen().contains("Servers  │ 1250.5"), "{}", h.screen());
    h.press(KeyPress::char('h'));
    assert!(
        h.screen().contains("[hex]") && h.screen().contains("50 4b 03 04"),
        "back to the bytes"
    );
}

#[tokio::test]
async fn viewer_turns_pdf_pages() {
    let mut h = with_samples().await;
    h.focus("report.pdf");
    h.key("F3");
    h.settle().await;
    let screen = h.screen();
    assert!(screen.contains("[pdf]") && screen.contains("page 1 of 3"), "{screen}");
    // The band of page 1 is blue, the one of page 2 green.
    let blue = ratatui::style::Color::Rgb(41, 97, 181);
    assert!(cells_of(&mut h, blue) > 20, "page 1 drawn");
    h.key("PageDown");
    h.settle().await;
    assert!(h.screen().contains("page 2 of 3"), "{}", h.screen());
    assert_eq!(cells_of(&mut h, blue), 0, "page 2 is another picture");
    h.key("End");
    h.settle().await;
    assert!(h.screen().contains("page 3 of 3"));
    h.key("PageDown");
    h.settle().await;
    assert!(h.screen().contains("page 3 of 3"), "stops at the last page");
}

#[tokio::test]
async fn viewer_turns_models_around() {
    let mut h = with_samples().await;
    h.focus("torus.stl");
    h.key("F3");
    h.settle().await;
    let screen = h.screen();
    assert!(screen.contains("[model]") && screen.contains("2304 triangles"), "{screen}");
    let before = viewer_picture(&h).expect("drawn");
    h.key("Right");
    h.settle().await;
    let after = viewer_picture(&h).expect("drawn");
    assert_ne!(before.as_raw(), after.as_raw(), "a new angle is a new picture");
    h.press(KeyPress::char('0'));
    h.settle().await;
    let reset = viewer_picture(&h).unwrap();
    assert_eq!(before.as_raw(), reset.as_raw(), "0 goes back to the first angle");
}

#[tokio::test]
async fn pictures_zoom_in_and_out() {
    let mut h = Harness::new().await;
    // Left half red, right half blue.
    let picture = image::RgbaImage::from_fn(64, 32, |x, _| {
        if x < 32 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) }
    });
    picture.save(h.path("left/halves.png")).unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("halves.png");
    h.key("F3");
    h.settle().await;
    let red = ratatui::style::Color::Rgb(255, 0, 0);
    let blue = ratatui::style::Color::Rgb(0, 0, 255);
    let (red_fit, blue_fit) = (cells_of(&mut h, red), cells_of(&mut h, blue));
    assert!(red_fit > 0 && red_fit == blue_fit, "{red_fit} {blue_fit}");
    h.press(KeyPress::char('+'));
    h.press(KeyPress::char('+'));
    for _ in 0..10 {
        h.key("Left");
    }
    assert!(h.screen().contains("200%"), "{}", h.screen());
    assert_eq!(cells_of(&mut h, blue), 0, "zoomed into the red half");
    h.press(KeyPress::char('0'));
    assert_eq!(cells_of(&mut h, blue), blue_fit, "fit again");
}

#[tokio::test]
async fn quick_view_follows_the_cursor() {
    let mut h = Harness::new().await;
    h.key("Ctrl+Q");
    h.focus("notes.txt");
    h.settle().await;
    assert!(h.screen().contains("Quick view"));
    assert!(h.screen().contains("the needle is here"));
    h.focus("a.txt");
    h.settle().await;
    assert!(h.screen().contains("alpha"));
}

#[tokio::test]
async fn bookmarks_are_saved_and_opened() {
    let mut h = Harness::new().await;
    h.key("Ctrl+Shift+D");
    assert!(h.status().starts_with("Bookmarked"), "{}", h.status());
    let saved = fs::read_to_string(h.app.context().paths.config_file()).unwrap();
    assert!(saved.contains("left"));
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    h.key("Ctrl+D");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))));
    h.key("Enter");
    h.settle().await;
    assert!(h.names(SideId::Left).contains(&"docs".to_owned()));
}

/// Desktops such as GNOME keep Shift+F1/F5/F7 for themselves, so the same
/// commands answer to Shift+F too.
#[tokio::test]
async fn shift_function_keys_do_what_the_alt_ones_do() {
    for (key, title) in [("Shift+F5", "Pack"), ("Ctrl+F2", "Search"), ("Shift+F1", "Drives")] {
        let mut h = Harness::new().await;
        h.focus("docs");
        h.key(key);
        h.settle().await;
        assert!(h.app.overlay().is_some(), "{key} opens something");
        assert!(h.screen().contains(title), "{key} shows {title}: {}", h.screen());
    }
}

#[tokio::test]
async fn bookmark_with_one_target_opens_both_panels() {
    let mut h = Harness::new().await;
    // The target is inside an archive: any VFS source works, remote ones too.
    h.focus("docs");
    h.key("Shift+F5");
    h.key("Enter");
    h.settle().await;
    h.key("Tab");
    h.focus("docs.zip");
    h.key("Enter");
    h.settle().await;
    h.key("Tab");
    h.key("Ctrl+Shift+B");
    assert!(h.status().starts_with("Bookmarked"), "{}", h.status());
    let targets = &h.app.context().config.bookmarks[0].targets;
    assert!(targets.len() == 1 && targets[0].starts_with("zip://"), "{targets:?}");

    // Move both panels away, then open the bookmark.
    h.focus("docs");
    h.key("Enter");
    h.key("Tab");
    h.focus("..");
    h.key("Enter");
    h.key("Tab");
    h.settle().await;
    h.key("Ctrl+D");
    assert!(h.screen().contains("→ zip://"), "the list shows the target");
    h.key("Enter");
    h.settle().await;
    assert!(h.app.overlay().is_none(), "one target needs no question");
    assert!(h.names(SideId::Left).contains(&"a.txt".to_owned()));
    assert_eq!(h.names(SideId::Right), ["..", "docs"], "other panel inside the archive");
}

#[tokio::test]
async fn bookmark_with_several_targets_asks_which() {
    let mut h = Harness::new().await;
    h.key("Ctrl+Shift+B"); // target: right/
    h.key("Ctrl+Up"); // the other panel shows left/ too
    h.settle().await;
    h.key("Ctrl+Shift+B"); // second target: left/
    assert!(h.status().contains("2 targets"), "{}", h.status());
    h.key("Ctrl+Shift+B");
    assert!(h.status().contains("already has target"), "{}", h.status());
    assert_eq!(h.app.context().config.bookmarks.len(), 1);

    assert_eq!(h.app.context().config.bookmarks[0].targets.len(), 2);
    h.key("Ctrl+D");
    // (A long temporary folder, as on macOS, pushes the "→ 2 targets" off the row.)
    assert!(h.screen().contains("→ 2 targets") || h.screen().contains(".tmp"), "{}", h.screen());
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("target picker") };
    assert_eq!(dialog.items.len(), 2);
    assert!(dialog.items[0].ends_with("right"), "{:?}", dialog.items);
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.names(SideId::Right), [".."], "first target: the empty right/");

    // Esc in the picker leaves the other panel where it is.
    h.key("Ctrl+D");
    h.key("Enter");
    h.settle().await;
    h.key("Esc");
    h.settle().await;
    assert!(h.app.overlay().is_none());
    assert_eq!(h.names(SideId::Right), [".."]);

    h.key("Ctrl+D");
    h.key("Enter");
    h.settle().await;
    h.key("Down");
    h.key("Enter");
    h.settle().await;
    assert!(h.names(SideId::Right).contains(&"a.txt".to_owned()), "second target: left/");
}

#[tokio::test]
async fn tabs_open_and_close() {
    let mut h = Harness::new().await;
    h.key("Ctrl+T");
    h.settle().await;
    assert_eq!(h.app.workspace().side(SideId::Left).tabs().len(), 2);
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    assert!(h.screen().contains("2:docs"));
    h.key("Ctrl+W");
    assert_eq!(h.app.workspace().side(SideId::Left).tabs().len(), 1);
    assert_eq!(h.names(SideId::Left).len(), 4, "first tab is unchanged");
}

#[tokio::test]
async fn search_by_content_and_jump_to_the_hit() {
    let mut h = Harness::new().await;
    h.key("Ctrl+F2");
    h.type_text("*.txt");
    h.key("Enter");
    h.type_text("NEEDLE");
    h.key("Enter");
    h.key("Enter"); // no size or age conditions
    h.settle().await;
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("results") };
    assert_eq!(dialog.items.len(), 1, "{:?}", dialog.items);
    assert!(dialog.items[0].contains("notes.txt:1: the needle is here"));
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.current(), "notes.txt");
}

#[tokio::test]
async fn compare_selects_differences() {
    let mut h = Harness::new().await;
    fs::write(h.path("right/a.txt"), "alpha\n").unwrap();
    fs::write(h.path("right/only-right.txt"), "x").unwrap();
    filetime_copy(&h.path("left/a.txt"), &h.path("right/a.txt"));
    h.key("Ctrl+R");
    h.settle().await;
    h.key("Ctrl+F3");
    let left = h.app.workspace().side(SideId::Left).panel();
    assert!(left.is_selected("notes.txt") && left.is_selected("docs"));
    assert!(!left.is_selected("a.txt"), "identical files are not marked");
    assert!(h.app.workspace().side(SideId::Right).panel().is_selected("only-right.txt"));
    assert!(h.status().contains("Compared folders"));
}

/// Gives `to` the modification time of `from`.
fn filetime_copy(from: &std::path::Path, to: &std::path::Path) {
    let modified = fs::metadata(from).unwrap().modified().unwrap();
    fs::File::options().write(true).open(to).unwrap().set_modified(modified).unwrap();
}

#[tokio::test]
async fn compare_two_files() {
    let mut h = Harness::new().await;
    fs::write(h.path("right/a.txt"), "alpha\n").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("a.txt");
    h.key("Tab");
    h.focus("a.txt");
    h.key("Ctrl+F3");
    h.settle().await;
    assert_eq!(h.status(), "Files are identical");
}

#[tokio::test]
async fn pack_and_browse_the_archive() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Shift+F5");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("pack dialog") };
    assert!(dialog.field.text().ends_with("docs.zip"));
    h.key("Enter");
    h.settle().await;
    assert!(h.path("right/docs.zip").exists());
    h.key("Tab");
    h.focus("docs.zip");
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.names(SideId::Right), ["..", "docs"]);
    let open = h.app.context().registry.open_count();

    // Leaving the archive frees its index.
    h.focus("..");
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.app.context().registry.open_count(), open - 1);
}

#[tokio::test]
async fn unpack_queues_a_copy_of_the_members() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Shift+F5");
    h.type_text(&h.path("left/docs.zip").display().to_string());
    h.key("Enter");
    h.settle().await;
    h.focus("docs.zip");
    h.key("Shift+F6");
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/docs/readme.md")).unwrap(), "# readme\n");
}

#[tokio::test]
async fn unpacking_an_encrypted_archive_asks_the_password_first() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Shift+F5");
    h.key("Ctrl+O");
    h.key("Enter"); // zip
    h.key("Enter"); // normal level
    for _ in 0..2 {
        h.type_text("s3cret");
        h.key("Enter");
    }
    let target = h.path("right/locked").display().to_string();
    h.type_text(&target);
    h.key("Enter");
    h.settle().await;
    assert!(h.path("right/locked.zip").exists());

    // Unpack from the folder that holds the archive: it asks, no queue error.
    // (The destination must not have the folder already: that is a conflict.)
    fs::remove_dir_all(h.path("left/docs")).unwrap();
    h.key("Tab");
    h.focus("locked.zip");
    h.key("Shift+F6");
    h.settle().await;
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no password asked") };
    assert!(dialog.prompt.contains("locked.zip is encrypted"), "{}", dialog.prompt);
    assert!(h.app.jobs().is_empty(), "nothing is queued before the password is right");
    // A wrong password asks again, saying so.
    h.type_text("wrong one");
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("not asked again") };
    assert!(dialog.prompt.starts_with("Wrong password"), "{}", dialog.prompt);
    // The right one unpacks.
    h.type_text("s3cret");
    h.key("Enter");
    h.settle().await;
    assert!(h.app.overlay().is_none());
    assert_eq!(fs::read_to_string(h.path("left/docs/readme.md")).unwrap(), "# readme\n");
}

#[tokio::test]
async fn jobs_window_lists_transfers() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("F5");
    h.key("Enter");
    h.settle().await;
    h.key("Ctrl+J");
    let screen = h.screen();
    assert!(screen.contains("Transfer queue") && screen.contains("done"), "{screen}");
    // Where the items came from and went to, with the system of each.
    // (The system is "Local" on Unix and the drive, "C:", on Windows.)
    assert!(screen.contains("from ") && screen.contains("→  to "), "{screen}");
    h.press(KeyPress::char('x'));
    assert!(h.app.jobs().is_empty(), "finished jobs removed");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn checksums_and_properties() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Ctrl+K");
    h.settle().await;
    let Some(Overlay::Message(message)) = h.app.overlay() else { panic!("checksums") };
    assert!(message.lines.iter().any(|l| l.starts_with("BLAKE3")));
    assert!(message.fixed, "digests in the fixed-width font");
    h.key("Esc");
    h.key("Ctrl+E");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("properties") };
    assert!(dialog.items.iter().any(|l| l.contains("6 bytes")));
    assert!(dialog.footer.contains("e or F2: edit"), "opens read-only: {}", dialog.footer);
    assert!(!dialog.footer.contains("Enter:"), "nothing to choose: {}", dialog.footer);
    h.key("F2");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("properties") };
    assert_eq!(dialog.footer.matches("Enter:").count(), 1, "{}", dialog.footer);
}

#[tokio::test]
async fn checksum_files_are_written_and_checked() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Ctrl+K");
    h.settle().await;
    assert!(matches!(h.app.overlay(), Some(Overlay::Message(_))));
    h.key("Enter"); // the list of checksum files to write
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("a list") };
    assert!(dialog.items[0].contains("SHA256SUMS"), "{:?}", dialog.items);
    h.key("Enter");
    h.settle().await;
    let text = fs::read_to_string(h.path("left/SHA256SUMS")).unwrap();
    assert!(text.ends_with("  a.txt\n"), "{text}");
    assert!(h.status().contains("Wrote SHA256SUMS"), "{}", h.status());
    // On the checksum file the first row checks it.
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("SHA256SUMS");
    h.key("Ctrl+K");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("a list") };
    assert!(dialog.items[0].starts_with("Check the files listed in SHA256SUMS"));
    assert_eq!(dialog.title, "Check or write checksums");
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Message(message)) = h.app.overlay() else { panic!("the result") };
    assert!(message.lines[0].contains("1 OK, 0 FAILED"), "{:?}", message.lines);
    h.key("Esc");
    // Writing again asks before it replaces the file.
    h.focus("a.txt");
    h.key("Ctrl+K");
    h.settle().await;
    h.key("Enter");
    h.key("Enter");
    h.settle().await;
    assert!(matches!(h.app.overlay(), Some(Overlay::Confirm(_))), "asks to replace");
}

#[cfg(unix)]
#[tokio::test]
async fn permissions_can_be_changed() {
    use std::os::unix::fs::PermissionsExt;
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Ctrl+E");
    h.type_text("e"); // edit mode: the cursor is on the date
    h.key("Down"); // the permissions as an octal number
    h.key("Enter");
    for _ in 0..4 {
        h.key("Backspace");
    }
    h.type_text("600");
    h.key("Enter");
    h.key("Ctrl+S");
    h.settle().await;
    let mode = fs::metadata(h.path("left/a.txt")).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);

    // A check box changes one permission and the octal number follows.
    h.key("Ctrl+E");
    h.type_text("e");
    h.key("Down");
    h.key("Down"); // owner read
    h.key("Space");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("properties") };
    assert!(dialog.items.iter().any(|l| l.contains("[ ] Owner: read")), "{:?}", dialog.items);
    assert!(dialog.items.iter().any(|l| l.contains("(0200)")), "{:?}", dialog.items);
}

#[tokio::test]
async fn settings_export_and_import() {
    let mut h = Harness::new().await;
    h.key("Ctrl+Shift+D");
    let file = h.path("exported.toml").display().to_string();
    h.app.perform(Action::ExportConfig);
    h.type_text(&file);
    h.key("Enter");
    assert!(fs::read_to_string(&file).unwrap().contains("pairnox-settings"));

    h.app.perform(Action::ImportConfig);
    h.type_text(&file);
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))));
    h.key("Down");
    h.key("Enter");
    assert_eq!(h.status(), "Settings imported");
    assert_eq!(h.app.context().config.bookmarks.len(), 1, "merge does not duplicate");
}

#[tokio::test]
async fn go_to_accepts_relative_paths_and_uris() {
    let mut h = Harness::new().await;
    h.key("Ctrl+L");
    h.type_text("docs");
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "readme.md"]);
    h.key("Ctrl+L");
    let uri = VfsPath::from_local(h.path("right")).unwrap().to_string();
    h.type_text(&uri);
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.names(SideId::Left), [".."]);
}

#[tokio::test]
async fn errors_are_shown_and_the_panel_stays() {
    let mut h = Harness::new().await;
    h.key("Ctrl+L");
    h.type_text("missing-folder");
    h.key("Enter");
    h.settle().await;
    assert!(h.status().contains("not found"), "{}", h.status());
    assert_eq!(h.names(SideId::Left).len(), 4);
}

#[tokio::test]
async fn f10_quits() {
    let mut h = Harness::new().await;
    assert!(h.app.is_running());
    h.key("F10");
    assert!(!h.app.is_running());
}

#[tokio::test]
async fn edit_hands_the_file_to_the_editor() {
    let mut h = Harness::with_settings(|c| c.general.editor = "my-editor --wait".into()).await;
    h.focus("a.txt");
    h.key("F4");
    let external = h.app.take_external().expect("editor requested");
    assert_eq!(external.command[..2], ["my-editor", "--wait"]);
    assert!(external.command[2].ends_with("a.txt"));
}

#[tokio::test]
async fn a_desktop_window_does_not_use_terminal_editors() {
    let mut h = Harness::new().await;
    h.app.graphical = true;
    h.focus("a.txt");
    h.key("F4");
    let external = h.app.take_external().expect("editor requested");
    let expected = if cfg!(windows) {
        "notepad"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    assert_eq!(external.command[0], expected, "not $EDITOR, which needs a terminal");
}

#[cfg(feature = "scripting")]
#[tokio::test]
async fn the_script_under_the_cursor_runs_after_a_confirmation() {
    let mut h = Harness::new().await;
    let script = r#"
        const names = (await pnxr.list(pnxr.context!.panel!)).map((e) => e.name).sort();
        await pnxr.writeText("listing.txt", names.join(","));
        pnxr.print(`listed ${names.length}`);
        pnxr.print("second line");
    "#;
    fs::write(h.path("left/list.ts"), script).unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("list.ts");
    h.key("Ctrl+X");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("a list") };
    assert_eq!(dialog.items[0], "Run script list.ts");
    h.key("Enter");
    let Some(Overlay::Confirm(confirm)) = h.app.overlay() else { panic!("a confirmation") };
    assert!(confirm.lines[1].contains("left"), "{:?}", confirm.lines);
    h.key("Enter");
    h.settle().await;
    assert_eq!(
        fs::read_to_string(h.path("left/listing.txt")).unwrap(),
        "a.txt,docs,list.ts,notes.txt"
    );
    let Some(Overlay::Message(message)) = h.app.overlay() else { panic!("the output") };
    assert_eq!(message.lines, ["listed 4", "second line"]);
    h.key("Enter");
    h.settle_until(|app| {
        app.workspace().active().entries().iter().any(|e| e.name == "listing.txt")
    })
    .await;
}

#[cfg(feature = "scripting")]
#[tokio::test]
async fn plugin_commands_ask_once_then_run_on_the_selection() {
    let mut h = Harness::new().await;
    let plugin = h.path("settings/data/plugins/upper");
    fs::create_dir_all(&plugin).unwrap();
    let manifest = r#"{
        "name": "upper",
        "version": "1.0.0",
        "commands": [{ "id": "copy", "title": "Uppercase copy to the other panel" }],
        "permissions": { "read": ["$SELECTION"], "write": ["$OTHER"] }
    }"#;
    fs::write(plugin.join("plugin.json"), manifest).unwrap();
    let main = r#"
        const ctx = pnxr.context!;
        for (const path of ctx.selection) {
          const text = await pnxr.readText(path);
          await pnxr.writeText(pnxr.join(ctx.other!, pnxr.basename(path)), text.toUpperCase());
        }
        pnxr.print(`${ctx.selection.length} copied`);
    "#;
    fs::write(plugin.join("main.ts"), main).unwrap();

    h.focus("a.txt");
    h.key("Ctrl+X");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("a list") };
    assert_eq!(
        dialog.items,
        ["upper: Uppercase copy to the other panel", "Browse and install plugins…"]
    );
    h.key("Enter");
    let Some(Overlay::Confirm(confirm)) = h.app.overlay() else { panic!("a permission question") };
    assert!(confirm.lines.iter().any(|l| l.contains("the selected files")), "{:?}", confirm.lines);
    h.key("Enter");
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/a.txt")).unwrap(), "ALPHA\n");
    assert_eq!(h.status(), "Uppercase copy to the other panel: 1 copied");

    // Allowed now: the second run asks nothing.
    h.focus("notes.txt");
    h.key("Ctrl+X");
    h.key("Enter");
    assert!(h.app.overlay().is_none(), "no second question");
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/notes.txt")).unwrap(), "THE NEEDLE IS HERE\n");
}

#[tokio::test]
async fn without_plugins_ctrl_x_offers_the_plugin_list() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Ctrl+X");
    let status = h.status();
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else {
        panic!("a list; status: {status}; {:?}", h.app.overlay().is_some())
    };
    assert_eq!(dialog.items, ["Browse and install plugins…"]);
}

#[cfg(feature = "scripting")]
#[tokio::test]
async fn the_plugin_list_says_it_loads_and_why_it_failed() {
    let mut h = Harness::with_settings(|c| {
        c.plugins.official = false;
        c.plugins.repositories = vec![pnxr_config::PluginRepository {
            url: "file:///nonexistent/plugins.git".into(),
            reference: None,
        }];
    })
    .await;
    h.key("Ctrl+X");
    h.key("Enter");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("the list") };
    assert_eq!(dialog.items, ["Loading the plugin list…"]);
    h.settle_until(
        |app| matches!(app.overlay(), Some(Overlay::Choose(d)) if d.items[0].starts_with('!')),
    )
    .await;
    let screen = h.screen();
    assert!(screen.contains("nonexistent") && screen.contains("F5 tries again"), "{screen}");
}

/// A question too long for the screen scrolls with the keys and still
/// takes its answer.
#[tokio::test]
async fn a_long_question_scrolls_and_is_still_answered() {
    let mut h = Harness::new().await;
    h.app.perform(Action::InstallServe);
    h.type_text("me@nas.lan");
    h.key("Enter");
    let small = |h: &mut Harness| {
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
        terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..14)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol().to_owned()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let screen = small(&mut h);
    assert!(screen.contains("install on me@nas.lan"), "{screen}");
    assert!(screen.contains("PgUp/PgDn: scroll") && screen.contains("more lines"), "{screen}");
    h.key("End");
    let screen = small(&mut h);
    assert!(screen.contains("Nothing needs root") && screen.contains("earlier lines"), "{screen}");
    h.key("n");
    assert!(h.app.overlay().is_none());
}

/// Moves the settings screen's cursor to the row whose label starts with
/// `label`.
fn settings_row(h: &mut Harness, label: &str) {
    h.key("Home");
    for _ in 0..200 {
        if let Some(Overlay::Settings(screen)) = h.app.overlay() {
            let view = pnxr_frontend::view::settings_view(&h.app, screen, 1000);
            if view.rows[view.cursor].label.starts_with(label) {
                return;
            }
        }
        h.key("Down");
    }
    let labels: Vec<String> = match h.app.overlay() {
        Some(Overlay::Settings(screen)) => pnxr_frontend::view::settings_view(&h.app, screen, 1000)
            .rows
            .into_iter()
            .map(|r| r.label)
            .collect(),
        other => vec![format!("{other:?}")],
    };
    panic!("no settings row {label} in {labels:?}");
}

#[tokio::test]
async fn the_settings_screen_changes_options_and_saves_them() {
    let mut h = Harness::new().await;
    h.key("Ctrl+,");
    let screen = h.screen();
    assert!(screen.contains("Settings") && screen.contains("Ask before deleting"), "{screen}");
    assert!(
        screen.contains("General") && screen.contains("Enter: change    ←/→: change"),
        "{screen}"
    );

    settings_row(&mut h, "Show hidden files");
    h.key("Enter");
    assert!(h.app.context().config.panels.show_hidden);
    settings_row(&mut h, "Check after copying");
    h.key("Right");
    assert_eq!(h.app.context().config.transfers.verify, pnxr_ops::VerifyMode::Fast);

    // Numbers are typed; Esc or Enter come back to the screen.
    settings_row(&mut h, "Retries after");
    h.key("Enter");
    h.type_text("1");
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))), "back on the screen");
    assert_eq!(h.app.context().config.transfers.retries, 1);
    let saved = fs::read_to_string(h.app.context().paths.config_file()).unwrap();
    assert!(saved.contains("retries = 1") && saved.contains("show_hidden = true"), "{saved}");

    // Delete puts the default back; typing finds settings instead.
    h.key("Delete");
    assert_eq!(h.app.context().config.transfers.retries, 3);
    h.press(KeyPress::char('r'));
    assert_eq!(h.app.context().config.transfers.retries, 3, "r only types now");
    h.key("Esc");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn typing_in_the_settings_screen_finds_a_setting() {
    let mut h = Harness::new().await;
    h.key("Ctrl+,");
    h.type_text("");
    for c in "fixed".chars() {
        h.press(KeyPress::char(c));
    }
    let screen = h.screen();
    assert!(screen.contains("Find: fixed_"), "{screen}");
    assert!(screen.contains("Fixed-width") && !screen.contains("Ask before deleting"), "{screen}");
    // Esc clears the filter first, and closes only the second time.
    h.key("Esc");
    let screen = h.screen();
    assert!(screen.contains("Ask before deleting"), "{screen}");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn the_command_list_explains_the_highlighted_command() {
    let mut h = Harness::new().await;
    h.key("F9");
    for c in "swap".chars() {
        h.press(KeyPress::char(c));
    }
    h.key("F1");
    let screen = h.screen();
    assert!(screen.contains("Swaps the two panels"), "{screen}");
    h.key("Esc");
    assert!(!h.screen().contains("Swaps the two panels"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Commands(_))), "still the list");
}

#[tokio::test]
async fn esc_clears_what_compare_selected() {
    let mut h = Harness::new().await;
    fs::write(h.path("right/only-there.txt"), "x").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.key("Ctrl+F3");
    assert!(h.status().contains("Esc clears"), "{}", h.status());
    let marked = |h: &Harness| h.app.workspace.active().selection_summary().files;
    assert!(marked(&h) > 0);
    h.key("Tab");
    assert!(marked(&h) > 0);
    h.key("Esc");
    assert_eq!(marked(&h), 0);
    h.key("Tab");
    assert_eq!(marked(&h), 0, "both panels are cleared");
}

#[tokio::test]
async fn the_filter_says_how_to_remove_it() {
    let mut h = Harness::new().await;
    h.key("Ctrl+S");
    h.type_text("not");
    h.key("Enter");
    assert!(h.screen().contains("(Esc clears)"), "{}", h.screen());
    h.key("Esc");
    assert!(!h.screen().contains("filter:"));
}

#[tokio::test]
async fn the_key_bar_follows_the_modifiers() {
    let mut h = Harness::new().await;
    assert!(h.screen().contains("F5Copy"));
    h.app.set_modifiers(false, false, true);
    let screen = h.screen();
    assert!(screen.contains("F5Pack") && screen.contains("F6Unpac"), "{screen}");
    assert!(!screen.contains("F5Copy"), "{screen}");
    // Alt does not change the bar.
    h.app.set_modifiers(false, true, false);
    assert!(h.screen().contains("F5Copy"), "{}", h.screen());
    h.app.set_modifiers(false, false, false);
    assert!(h.screen().contains("F5Copy"));
}

#[tokio::test]
async fn swapping_with_the_quick_view_keeps_the_file_panel_with_its_focus() {
    let mut h = Harness::new().await;
    h.focus("notes.txt");
    h.key("Ctrl+Q");
    let before = h.app.workspace.active_side();
    h.key("Ctrl+U");
    assert_eq!(h.app.workspace.active_side(), before.other(), "the file panel moved with the swap");
    assert_eq!(h.current(), "notes.txt");
    assert!(h.app.quick_view.is_some());
}

#[tokio::test]
async fn shift_keys_scroll_the_quick_view() {
    let mut h = Harness::new().await;
    let lines: String = (1..=200).map(|n| format!("line {n}\n")).collect();
    fs::write(h.path("left/long.txt"), lines).unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("long.txt");
    h.key("Ctrl+Q");
    h.settle().await;
    let scroll = |h: &Harness| match &h.app.quick_view {
        Some(Some(viewer)) => viewer.scroll,
        _ => panic!("no quick view"),
    };
    assert_eq!(scroll(&h), 0);
    h.key("Shift+PageDown");
    let page = scroll(&h);
    assert!(page > 0);
    h.key("Shift+Down");
    assert_eq!(scroll(&h), page + 1);
    h.key("Shift+End");
    assert!(scroll(&h) > page);
    h.key("Shift+Home");
    assert_eq!(scroll(&h), 0);
    assert_eq!(h.current(), "long.txt", "the cursor of the panel stayed");
}

#[tokio::test]
async fn settings_are_sent_to_another_folder_from_the_screen() {
    let mut h = Harness::new().await;
    h.key("Ctrl+,");
    settings_row(&mut h, "Send these settings");
    h.key("Enter");
    let target = h.path("other/config.toml").display().to_string();
    h.type_text(&target);
    h.key("Enter");
    h.settle().await;
    assert!(h.status().starts_with("Settings sent to"), "{}", h.status());
    assert!(h.path("other/config.toml").exists());
    settings_row(&mut h, "Get the settings");
    h.key("Enter");
    h.type_text(&target);
    h.key("Enter");
    h.settle().await;
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))), "replace or merge?");
}

#[tokio::test]
async fn drives_and_connections_share_one_list() {
    // A saved connection opens like a drive: here one to a local folder.
    let backup = TempDir::new().unwrap();
    fs::write(backup.path().join("saved.txt"), "kept\n").unwrap();
    let backup_uri = VfsPath::from_local(backup.path()).unwrap().to_string();
    let mut h = Harness::with_settings(|config| {
        for (name, uri) in [
            ("backup", backup_uri.as_str()),
            ("nas", "sftp://me@nas.invalid/srv"),
            ("site", "zip:///srv/site.zip!/"),
        ] {
            config.connections.push(pnxr_config::Connection {
                name: name.into(),
                uri: uri.into(),
                identity_file: None,
                options: Default::default(),
            });
        }
    })
    .await;
    for key in ["Shift+F1", "Ctrl+N"] {
        h.key(key);
        let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("{key}: no drive list") };
        let items = dialog.items.clone();
        assert!(items[0].starts_with("Disk     Home"), "{items:?}");
        let category = |name: &str| {
            let row = items.iter().find(|r| r.split_whitespace().nth(1) == Some(name)).unwrap();
            row.split_whitespace().next().unwrap().to_owned()
        };
        assert_eq!(category("backup"), "Disk");
        assert_eq!(category("nas"), "SFTP");
        assert_eq!(category("site"), "Archive");
        assert_eq!(items.last().unwrap(), "New connection…");
        let screen = h.screen();
        assert!(screen.contains("Drives") && screen.contains("SFTP     nas"), "{screen}");
        h.key("Esc");
    }
    h.key("Shift+F1");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no drive list") };
    let row = dialog.items.iter().position(|r| r.contains(" backup ")).unwrap();
    for _ in 0..row {
        h.key("Down");
    }
    h.key("Enter");
    h.settle().await;
    assert!(h.app.overlay().is_none());
    assert_eq!(h.names(SideId::Left), ["..", "saved.txt"]);
}

#[tokio::test]
async fn a_new_connection_is_saved_in_the_drive_list() {
    let backup = TempDir::new().unwrap();
    fs::write(backup.path().join("saved.txt"), "kept\n").unwrap();
    let uri = VfsPath::from_local(backup.path()).unwrap().to_string();
    let mut h = Harness::new().await;
    h.key("Ctrl+N");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Input(_))));
    h.type_text(&uri);
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no name asked") };
    assert_eq!(dialog.title, "New connection");
    for _ in 0..40 {
        h.key("Backspace");
    }
    h.type_text("backup");
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "saved.txt"]);
    assert!(h.status().contains("Saved backup"), "{}", h.status());
    let saved = fs::read_to_string(h.app.context().paths.config_file()).unwrap();
    assert!(saved.contains("name = \"backup\""), "{saved}");

    // F2 gives a saved connection another name; local drives have none to change.
    h.key("Shift+F1");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no drive list") };
    let row = dialog.items.iter().position(|r| r.contains(" backup ")).unwrap();
    for _ in 0..row {
        h.key("Down");
    }
    h.key("F2");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no name asked") };
    assert_eq!(dialog.field.text(), "backup");
    h.type_text("Backup disk");
    h.key("Enter");
    assert!(h.status().contains("Renamed to Backup disk"), "{}", h.status());
    assert_eq!(h.app.context().config.connections[0].name, "Backup disk");
    h.key("Shift+F1");
    h.key("Home");
    h.key("F2");
    assert!(h.status().contains("Only saved connections"), "{}", h.status());
    h.key("Shift+F1");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no drive list") };
    assert!(dialog.items.iter().any(|r| r.contains("Backup disk")));
    let rename_back = dialog.items.iter().position(|r| r.contains("Backup disk")).unwrap();
    for _ in 0..rename_back {
        h.key("Down");
    }
    h.key("F2");
    h.type_text("backup");
    h.key("Enter");

    // Delete takes it out of the list and the settings again.
    h.key("Shift+F1");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no drive list") };
    let row = dialog.items.iter().position(|r| r.contains(" backup ")).unwrap();
    for _ in 0..row {
        h.key("Down");
    }
    h.key("Delete");
    // A saved connection may hold a login: Enter says no, y says yes.
    let Some(Overlay::Confirm(question)) = h.app.overlay() else { panic!("no question") };
    assert!(question.default_no(), "{question:?}");
    h.key("Enter");
    assert!(h.app.overlay().is_none());
    assert!(h.app.context().config.connections.iter().any(|c| c.name == "backup"));
    h.key("Shift+F1");
    for _ in 0..row {
        h.key("Down");
    }
    h.key("Delete");
    h.key("y");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("list closed") };
    assert!(!dialog.items.iter().any(|r| r.contains(" backup ")));
    // Local drives cannot be removed.
    h.key("Home");
    h.key("Up");
    h.key("Delete");
    let Some(Overlay::Choose(after)) = h.app.overlay() else { panic!("list closed") };
    assert!(after.items[0].starts_with("Disk     Home"));
    assert!(h.app.context().config.connections.is_empty());
}

#[tokio::test]
async fn connections_have_no_key_setting_ssh_finds_its_keys() {
    // Settings written when a key file could be set still load.
    let mut h = Harness::with_settings(|config| {
        config.connections.push(pnxr_config::Connection {
            name: "nas".into(),
            uri: "sftp://me@nas.invalid/srv".into(),
            identity_file: Some("~/.ssh/old.pem".into()),
            options: Default::default(),
        });
    })
    .await;
    h.key("Shift+F1");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no drive list") };
    assert!(!dialog.footer.contains("key"), "{}", dialog.footer);
    let row = dialog.items.iter().position(|r| r.contains(" nas ")).unwrap();
    for _ in 0..row {
        h.key("Down");
    }
    h.key("F4");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("F4 edits the connection") };
    assert_eq!(dialog.title, "Edit connection");
    assert!(
        !dialog.prompt.to_lowercase().contains("key file"),
        "no key question: {}",
        dialog.prompt
    );
    h.key("Esc");

    // A new SFTP connection is saved after its name and the login question
    // (the cursor is on "keep it as it is": SSH finds its keys).
    h.key("Ctrl+N");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text("sftp://me@other.invalid/");
    h.key("Enter");
    h.type_text("other");
    h.key("Enter");
    let Some(Overlay::Choose(login)) = h.app.overlay() else { panic!("no login question") };
    assert_eq!(login.cursor, 2);
    h.key("Enter");
    assert!(h.status().contains("Saved other"), "{}", h.status());
    assert_eq!(h.app.context().config.connections.len(), 2);
    let saved = fs::read_to_string(h.app.context().paths.config_file()).unwrap();
    assert!(!saved.contains("identity_file"), "{saved}");
}

#[tokio::test]
async fn ctrl_arrows_open_the_folder_under_the_cursor_in_a_panel() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Ctrl+Right");
    h.settle().await;
    assert_eq!(h.names(SideId::Right), ["..", "readme.md"], "the folder opens on the right");
    assert_eq!(h.app.workspace().active_side(), SideId::Left, "the focus stays");
    // On a file the panel's own folder is shown.
    h.focus("a.txt");
    h.key("Ctrl+Right");
    h.settle().await;
    assert_eq!(h.names(SideId::Right), h.names(SideId::Left));
    h.focus("docs");
    h.key("Ctrl+Left");
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "readme.md"], "into the panel itself");
}

#[tokio::test]
async fn left_and_right_move_between_folders_when_asked() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Right");
    assert_eq!(h.names(SideId::Left)[1], "docs", "off by default: the arrows do nothing");
    let mut h = Harness::with_settings(|c| c.panels.arrow_navigation = true).await;
    h.focus("docs");
    h.key("Right");
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "readme.md"]);
    h.key("Left");
    h.settle().await;
    assert_eq!(h.current(), "docs", "back up with the cursor where it was");
    h.focus("a.txt");
    h.key("Right");
    h.settle().await;
    assert_eq!(h.current(), "a.txt", "a file is not entered");
}

#[tokio::test]
async fn d_and_a_click_on_the_drive_open_the_list_for_that_panel() {
    let mut h = Harness::new().await;
    h.key("d");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))), "d lists the drives");
    h.key("Esc");
    assert_eq!(h.app.workspace().active_side(), SideId::Left);
    h.app.pick("drive", 1);
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))));
    assert_eq!(h.app.workspace().active_side(), SideId::Right, "the clicked panel takes it");
    let expected = if cfg!(windows) { ":]" } else { "[Local]" };
    assert!(h.screen().contains(expected), "the panel title names the drive");
}

#[cfg(unix)]
#[tokio::test]
async fn dates_and_permissions_change_on_the_selection() {
    use std::os::unix::fs::PermissionsExt;
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Shift+F4");
    h.type_text("e");
    h.key("Enter"); // the date
    for _ in 0..3 {
        h.key("Backspace");
    }
    h.type_text("2001-02-03 04:05");
    h.key("Enter");
    h.key("Down");
    h.key("Enter"); // the octal number
    for _ in 0..4 {
        h.key("Backspace");
    }
    h.type_text("640");
    h.key("Enter");
    h.key("Ctrl+S");
    h.settle().await;
    let meta = fs::metadata(h.path("left/a.txt")).unwrap();
    assert_eq!(meta.permissions().mode() & 0o7777, 0o640);
    let seconds = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!((981_000_000..982_000_000).contains(&seconds), "February 2001: {seconds}");
    // A wrong word is refused where it is typed, and says why.
    h.key("Shift+F4");
    h.type_text("e");
    h.key("Enter");
    for _ in 0..3 {
        h.key("Backspace");
    }
    h.type_text("yesterday");
    h.key("Enter");
    assert!(h.status().contains("Not understood"), "{}", h.status());
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))), "back in the dialog");
}

#[tokio::test]
async fn ctrl_z_takes_back_renames_new_folders_and_moves() {
    let mut h = Harness::new().await;
    h.key("Ctrl+Z");
    assert!(h.status().contains("Nothing to undo"), "{}", h.status());

    h.key("F7");
    h.type_text("fresh");
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/fresh").is_dir());
    h.key("Ctrl+Z");
    h.settle().await;
    assert!(!h.path("left/fresh").exists(), "the new folder is gone");

    h.focus("a.txt");
    h.key("F2");
    for _ in 0..5 {
        h.key("Backspace");
    }
    h.type_text("b.txt");
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/b.txt").exists() && !h.path("left/a.txt").exists());
    h.key("Ctrl+Z");
    h.settle().await;
    assert!(h.path("left/a.txt").exists() && !h.path("left/b.txt").exists(), "name back");

    h.focus("a.txt");
    h.key("F6");
    h.key("Enter");
    h.settle_until(|app| !app.is_busy() && app.jobs.iter().all(|j| j.state.is_final())).await;
    assert!(h.path("right/a.txt").exists() && !h.path("left/a.txt").exists(), "moved");
    h.key("Ctrl+Z");
    h.settle_until(|app| !app.is_busy() && app.jobs.iter().all(|j| j.state.is_final())).await;
    assert!(h.path("left/a.txt").exists() && !h.path("right/a.txt").exists(), "moved back");
    h.key("Ctrl+Z");
    assert!(h.status().contains("Nothing to undo"), "the move back is not undone again");
}

#[tokio::test]
async fn panels_in_a_git_repository_show_marks_and_the_branch() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git").arg("-C").arg(root.join("left")).args(args).output()
    };
    fs::create_dir_all(root.join("left")).unwrap();
    fs::write(root.join("left/tracked.txt"), "t").unwrap();
    if !matches!(git(&["init", "-q", "-b", "trunk"]), Ok(o) if o.status.success()) {
        return; // no git on this machine
    }
    let _ = git(&["add", "tracked.txt"]);
    fs::write(root.join("left/loose.txt"), "l").unwrap();
    fs::create_dir_all(root.join("right")).unwrap();

    let paths = AppPaths::in_dir(root.join("settings"));
    let context = AppContext::load_from(paths).unwrap();
    let left = VfsPath::from_local(root.join("left")).unwrap();
    let right = VfsPath::from_local(root.join("right")).unwrap();
    let (app, events) = App::new(context, left, right).unwrap();
    let mut h = Harness { app, events, dir };
    h.settle().await;
    h.settle_until(|app| app.git[0].is_some()).await;
    let screen = h.screen();
    assert!(screen.contains("⎇ trunk"), "branch in the title:\n{screen}");
    let line = |name: &str| screen.lines().find(|l| l.contains(name)).unwrap_or("").to_owned();
    assert!(line("tracked.txt").contains("M ≡ tracked.txt"), "{screen}");
    assert!(line("loose.txt").contains("? ≡ loose.txt"), "{screen}");
    assert!(h.app.git[1].is_none(), "the other folder is not a repository");
}

#[tokio::test]
async fn alt_c_on_two_different_text_files_opens_a_side_by_side_diff() {
    let mut h = Harness::new().await;
    fs::write(h.path("right/a.txt"), "alpha\nbeta\n").unwrap();
    fs::write(h.path("left/a.txt"), "alpha\ngamma\ndelta\n").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("a.txt");
    h.key("Tab");
    h.focus("a.txt");
    h.key("Ctrl+F3");
    h.settle_until(|app| matches!(app.overlay(), Some(Overlay::Viewer(_)))).await;
    let screen = h.screen();
    assert!(screen.contains("a.txt ↔ a.txt"), "{screen}");
    // The focused (right) panel's file is on the left of the comparison.
    let changed = screen.lines().find(|l| l.contains("gamma")).unwrap();
    assert!(changed.contains('~') && changed.contains("beta"), "paired on one row: {changed}");
    let added = screen.lines().find(|l| l.contains("delta")).unwrap();
    assert!(added.contains('+'), "only on the right: {added}");
    assert!(screen.contains("n / p: next change"), "{screen}");
    h.key("n");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn the_mouse_clicks_scrolls_and_drags_between_panels() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use pnxr_tui::Pointer;
    use ratatui::layout::Rect;
    let mut h = Harness::new().await;
    let screen = Rect::new(0, 0, 80, 24);
    let event = |kind, column, row| MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
    let mut pointer = Pointer::default();
    // The panels have been drawn once, so the scroll offsets are known.
    h.screen();

    // Rows of the left panel: .., docs, a.txt, notes.txt. A click on the
    // fourth row (cells start two lines below the top) moves the cursor.
    pointer.handle(&mut h.app, event(MouseEventKind::Down(MouseButton::Left), 3, 5), screen);
    assert_eq!(h.current(), "notes.txt");
    pointer.handle(&mut h.app, event(MouseEventKind::Up(MouseButton::Left), 3, 5), screen);
    pointer.handle(&mut h.app, event(MouseEventKind::ScrollUp, 3, 5), screen);
    assert_eq!(h.current(), "..", "the wheel moves three rows, up to the first");

    // A double click on a folder opens it.
    for _ in 0..2 {
        pointer.handle(&mut h.app, event(MouseEventKind::Down(MouseButton::Left), 3, 3), screen);
        pointer.handle(&mut h.app, event(MouseEventKind::Up(MouseButton::Left), 3, 3), screen);
    }
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "readme.md"]);
    // Back up with the keyboard, then drag a.txt over to the right panel.
    h.key("Backspace");
    h.settle().await;
    h.screen();
    pointer.handle(&mut h.app, event(MouseEventKind::Down(MouseButton::Left), 3, 4), screen);
    assert_eq!(h.current(), "a.txt");
    pointer.handle(&mut h.app, event(MouseEventKind::Up(MouseButton::Left), 60, 4), screen);
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("copy question") };
    assert!(dialog.field.text().ends_with("right"), "{}", dialog.field.text());
    h.key("Enter");
    h.settle_until(|app| !app.is_busy() && app.jobs.iter().all(|j| j.state.is_final())).await;
    assert!(h.path("right/a.txt").exists());
}

/// Runs the search dialogs: mask, text, conditions.
async fn search(h: &mut Harness, mask: &str, content: &str, filters: &str) {
    h.key("Ctrl+F2");
    h.type_text(mask);
    h.key("Enter");
    h.type_text(content);
    h.key("Enter");
    h.type_text(filters);
    h.key("Enter");
    h.settle().await;
}

fn results(h: &Harness) -> Vec<String> {
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("a list was expected") };
    dialog.items.clone()
}

#[tokio::test]
async fn copy_path_puts_the_chosen_form_in_the_clipboard() {
    let mut h = Harness::new().await;
    h.focus("notes.txt");
    h.key("Ctrl+P");
    assert_eq!(results(&h).len(), 4);
    h.key("Enter"); // the first row: the full path
    let text = h.app.take_clipboard().expect("something was copied");
    // Windows writes the folders of the path with backslashes.
    let same = |a: &str, b: &str| a.replace('\\', "/") == b.replace('\\', "/");
    assert!(same(&text, &h.path("left/notes.txt").display().to_string()), "{text}");
    assert!(h.app.take_clipboard().is_none(), "handed over once");

    h.key("Ctrl+P");
    h.key("Down"); // name
    h.key("Enter");
    assert_eq!(h.app.take_clipboard().unwrap(), "notes.txt");

    h.focus("a.txt");
    h.key("Insert");
    h.focus("notes.txt");
    h.key("Insert");
    h.key("Ctrl+P");
    h.key("Down");
    h.key("Down"); // URI
    h.key("Enter");
    let uris = h.app.take_clipboard().unwrap();
    assert_eq!(uris.lines().count(), 2, "{uris}");
    assert!(uris.lines().all(|l| l.starts_with("file://")));
}

#[tokio::test]
async fn the_listing_is_exported_to_a_file_or_the_clipboard() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("export listing");
    h.key("Enter");
    assert_eq!(results(&h), ["CSV", "JSON", "Markdown", "HTML"]);
    h.key("Down");
    h.key("Enter"); // JSON
    h.key("Enter"); // empty: the clipboard
    let json = h.app.take_clipboard().expect("copied");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let names: Vec<&str> =
        value.as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["docs", "a.txt", "notes.txt"]);

    // Only the selection, written to a file.
    h.focus("a.txt");
    h.key("Insert");
    h.key("F9");
    h.type_text("export listing");
    h.key("Enter");
    h.key("Enter"); // CSV
    h.type_text("list.csv");
    h.key("Enter");
    h.settle().await;
    let csv = fs::read_to_string(h.path("left/list.csv")).unwrap();
    assert_eq!(csv.lines().count(), 2, "{csv}");
    assert!(csv.lines().nth(1).unwrap().starts_with("a.txt,"));

    // An existing file is never overwritten.
    h.key("F9");
    h.type_text("export listing");
    h.key("Enter");
    h.key("Enter");
    h.type_text("list.csv");
    h.key("Enter");
    h.settle().await;
    assert!(h.status().contains("already exists"), "{}", h.status());
}

#[cfg(unix)]
#[tokio::test]
async fn links_are_made_in_the_other_panel() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Shift+F7");
    assert_eq!(results(&h).len(), 3);
    h.key("Enter"); // relative symbolic link
    h.settle().await;
    let link = h.path("right/a.txt");
    assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("../left/a.txt"));
    assert_eq!(fs::read_to_string(&link).unwrap(), "alpha\n");

    h.key("Shift+F7");
    h.key("Down");
    h.key("Down"); // hard link
    h.key("Enter");
    h.settle().await;
    assert!(h.status().contains("already exists"), "{}", h.status());
}

#[tokio::test]
async fn renaming_in_the_editor_previews_then_renames() {
    let mut h = Harness::with_settings(|c| c.general.editor = "my-editor --wait".into()).await;
    h.focus("a.txt");
    h.key("Insert");
    h.focus("notes.txt");
    h.key("Insert");
    h.app.perform(Action::BulkRename);
    let external = h.app.take_external().expect("the editor is started");
    let (file, paths) = external.rename.clone().expect("a rename");
    assert_eq!(paths.len(), 2);
    assert_eq!(fs::read_to_string(&file).unwrap(), "a.txt\nnotes.txt\n");

    // What the user does in the editor.
    fs::write(&file, "first.txt\nsecond.txt\n").unwrap();
    h.app.after_external(&external, Ok(()));
    h.settle().await;
    let Some(Overlay::Confirm(dialog)) = h.app.overlay() else { panic!("a preview") };
    assert!(dialog.lines.iter().any(|l| l.contains("a.txt") && l.contains("first.txt")));
    assert!(!file.exists(), "the temporary file is removed");
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/first.txt").exists() && h.path("left/second.txt").exists());
    assert!(!h.path("left/a.txt").exists());
}

#[tokio::test]
async fn names_from_the_editor_are_checked() {
    let mut h = Harness::with_settings(|c| c.general.editor = "my-editor --wait".into()).await;
    h.focus("a.txt");
    h.key("Insert");
    h.focus("notes.txt");
    h.key("Insert");
    for (text, complaint) in [
        ("only-one\n", "1 lines for 2 items"),
        ("same\nsame\n", "Two items would be called"),
        ("ok.txt\n../bad\n", "Not a file name"),
        ("a.txt\nnotes.txt\n", "no name would change"),
    ] {
        h.app.perform(Action::BulkRename);
        let external = h.app.take_external().unwrap();
        fs::write(&external.rename.as_ref().unwrap().0, text).unwrap();
        h.app.after_external(&external, Ok(()));
        h.settle().await;
        assert!(h.status().contains(complaint), "{text:?}: {}", h.status());
    }
    assert!(h.path("left/a.txt").exists() && h.path("left/notes.txt").exists());

    // A desktop window needs an editor that waits.
    let mut window = Harness::new().await;
    window.app.graphical = true;
    window.focus("a.txt");
    window.app.perform(Action::BulkRename);
    assert!(window.app.take_external().is_none());
    assert!(window.status().contains("editor that waits"), "{}", window.status());
}

#[tokio::test]
async fn bookmarks_filter_as_you_type_and_list_recent_folders() {
    let mut h = Harness::new().await;
    // Visit docs so it is remembered, then go back.
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    h.key("Backspace");
    h.settle().await;
    h.key("Ctrl+Shift+D");
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    h.key("Ctrl+D");
    let rows = results(&h);
    assert!(rows.iter().any(|r| r.contains("Recent folders")), "{rows:?}");
    // Typing narrows the bookmarks (case does not matter) and the recent folders.
    h.type_text("ZZZ");
    assert_eq!(results(&h), ["(nothing matches)"], "nothing matches ZZZ");
    h.key("Backspace");
    h.key("Backspace");
    h.key("Backspace");
    h.type_text("LEF");
    let rows = results(&h);
    assert!(rows.iter().any(|r| r.contains("left")), "{rows:?}");
    // Enter on the first bookmark opens it.
    h.key("Enter");
    h.settle().await;
    assert!(h.app.workspace().active().location().to_string().ends_with("/left"));
    // The recent folders can be chosen too (the header cannot).
    h.key("Ctrl+D");
    h.key("Down"); // the title of the recent folders
    h.key("Down");
    h.key("Enter");
    h.settle().await;
    assert!(h.app.workspace().active().location().to_string().ends_with("/left/docs"));
}

#[tokio::test]
async fn the_bookmark_list_has_a_command_to_add_the_current_folder() {
    let mut h = Harness::new().await;
    assert!(h.app.context().config.bookmarks.is_empty());
    h.key("Ctrl+D");
    // The command is in the key line at the bottom, not a row of the list.
    let rows = results(&h);
    assert!(rows.iter().all(|r| !r.contains("Bookmark this folder")), "{rows:?}");
    assert!(h.screen().contains("Ctrl+D: bookmark this folder"), "{}", h.screen());
    h.key("Ctrl+D");
    assert_eq!(h.app.context().config.bookmarks.len(), 1);
    // Once bookmarked, the command is no longer offered.
    assert!(!h.screen().contains("Ctrl+D: bookmark this folder"), "{}", h.screen());
}

#[tokio::test]
async fn searches_are_saved_run_from_the_command_list_and_forgotten() {
    let mut h = Harness::new().await;
    h.app.perform(Action::SaveSearch);
    assert!(h.status().contains("Search first"), "{}", h.status());

    fs::write(h.path("left/big.log"), "x".repeat(5000)).unwrap();
    fs::write(h.path("left/small.log"), "x").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    search(&mut h, "*.log", "", ">1k").await;
    assert_eq!(results(&h).len(), 1, "{:?}", results(&h));
    assert!(results(&h)[0].ends_with("big.log"));
    h.key("Esc");

    h.app.perform(Action::SaveSearch);
    h.type_text("big logs");
    h.key("Enter");
    assert_eq!(h.app.context().config.searches.len(), 1);
    let saved = fs::read_to_string(h.path("settings/config/config.toml")).unwrap();
    assert!(saved.contains("big logs") && saved.contains(">1k"), "{saved}");

    // Run from the command list, in the folder we are in now.
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    h.key("Backspace");
    h.settle().await;
    h.key("F9");
    h.type_text("big logs");
    h.key("Enter");
    h.settle().await;
    assert_eq!(results(&h).len(), 1);
    h.key("Esc");

    // The same from the list of saved searches, which also forgets them.
    h.app.perform(Action::SavedSearches);
    assert!(results(&h)[0].starts_with("big logs  (*.log"));
    h.key("Enter");
    h.settle().await;
    assert_eq!(results(&h).len(), 1);
    h.key("Esc");
    h.app.perform(Action::SavedSearches);
    h.key("Delete");
    assert!(matches!(h.app.overlay(), Some(Overlay::Confirm(_))), "asks first");
    assert_eq!(h.app.context().config.searches.len(), 1);
    h.key("Enter");
    assert!(h.app.context().config.searches.is_empty());
    assert!(h.app.overlay().is_none(), "the list closes when empty");
}

#[tokio::test]
async fn search_results_go_to_the_basket_and_are_acted_on_together() {
    let mut h = Harness::new().await;
    fs::write(h.path("left/docs/more.txt"), "needle\n").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.key("Ctrl+B");
    assert!(h.status().contains("basket is empty"), "{}", h.status());

    search(&mut h, "*.txt", "", "").await;
    assert_eq!(results(&h).len(), 3);
    h.key("Ctrl+G");
    assert!(h.app.overlay().is_none());
    assert_eq!(h.app.basket.len(), 3);

    // The selection of another folder joins them; duplicates do not.
    h.focus("a.txt");
    h.key("Ctrl+G");
    assert_eq!(h.app.basket.len(), 3);

    h.key("Ctrl+B");
    assert_eq!(results(&h).len(), 3);
    h.key("Delete"); // forget the first
    assert_eq!(results(&h).len(), 2);
    h.key("F5"); // copy what is left to the other panel
    h.key("Enter");
    h.settle_until(|app| !app.is_busy() && app.jobs.iter().all(|j| j.state.is_final())).await;
    let copied: Vec<String> = h.names(SideId::Right);
    assert_eq!(copied.len(), 3, "the parent row and two files: {copied:?}");
    assert!(h.app.basket.len() == 2, "copying keeps the basket");

    // Moving takes them out of the basket.
    fs::create_dir(h.path("moved")).unwrap();
    h.key("Ctrl+B");
    h.key("F6");
    h.type_text(&h.path("moved").display().to_string());
    h.key("Enter");
    h.settle_until(|app| !app.is_busy() && app.jobs.iter().all(|j| j.state.is_final())).await;
    assert!(h.app.basket.is_empty(), "{:?}", h.app.basket);
    assert_eq!(fs::read_dir(h.path("moved")).unwrap().count(), 2);
}

#[tokio::test]
async fn the_basket_can_be_emptied_and_shows_an_item() {
    let mut h = Harness::new().await;
    h.focus("notes.txt");
    h.key("Ctrl+G");
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    h.key("Ctrl+B");
    h.key("Enter"); // show it in the panel
    h.settle().await;
    assert_eq!(h.current(), "notes.txt");
    h.key("Ctrl+B");
    h.key("Shift+Delete");
    assert!(matches!(h.app.overlay(), Some(Overlay::Confirm(_))), "asks first");
    h.key("Esc");
    assert!(!h.app.basket.is_empty(), "No keeps the basket");
    h.key("Ctrl+B");
    h.key("Shift+Delete");
    h.key("Enter");
    assert!(h.app.basket.is_empty());
}

#[tokio::test]
async fn finding_similar_pictures_is_no_built_in_command_any_more() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("similar");
    assert!(h.screen().contains("Nothing matches"), "{}", h.screen());
    assert!(pnxr_config::keymap::Action::from_id("tools.similar-images").is_none());
}

#[tokio::test]
async fn comparing_by_content_marks_what_differs_even_inside_folders() {
    let mut h = Harness::new().await;
    fs::create_dir_all(h.path("right/docs")).unwrap();
    fs::write(h.path("right/a.txt"), "alpha\n").unwrap(); // same content
    fs::write(h.path("right/notes.txt"), "the NEEDLE is here\n").unwrap(); // same size, different
    fs::write(h.path("right/docs/readme.md"), "# README\n").unwrap(); // different, inside docs
    fs::write(h.path("right/only-right.txt"), "x").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.app.perform(Action::CompareHash);
    h.settle().await;
    let left = h.app.workspace().side(SideId::Left).panel();
    assert!(left.is_selected("notes.txt") && left.is_selected("docs"));
    assert!(!left.is_selected("a.txt"), "same content, different time: still the same");
    let right = h.app.workspace().side(SideId::Right).panel();
    assert!(right.is_selected("notes.txt") && right.is_selected("only-right.txt"));
    assert!(h.status().contains("1 identical, 2 different"), "{}", h.status());
}

#[tokio::test]
async fn synchronizing_shows_the_plan_and_runs_it_on_enter() {
    let mut h = Harness::new().await;
    fs::write(h.path("right/extra.txt"), "extra").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.key("F9");
    h.type_text("synchronize");
    h.key("Enter");
    assert_eq!(results(&h).len(), 3);
    h.key("Enter"); // mirror
    h.settle().await;
    let Some(Overlay::Viewer(viewer)) = h.app.overlay() else { panic!("the plan") };
    let text: Vec<String> = viewer
        .report
        .as_ref()
        .unwrap()
        .iter()
        .map(|line| line.iter().map(|s| s.text.as_str()).collect())
        .collect();
    assert!(text.iter().any(|l| l.starts_with("→ new") && l.contains("docs/")), "{text:?}");
    assert!(text.iter().any(|l| l.starts_with("✗ delete") && l.contains("extra.txt")), "{text:?}");
    assert!(!h.path("right/a.txt").exists(), "nothing is done before Enter");

    h.key("Enter");
    h.settle_until(|app| !app.is_busy() && app.jobs.iter().all(|j| j.state.is_final())).await;
    assert_eq!(fs::read_to_string(h.path("right/a.txt")).unwrap(), "alpha\n");
    assert!(h.path("right/docs/readme.md").exists());
    assert!(!h.path("right/extra.txt").exists(), "a mirror deletes what only the target has");

    // Now they agree: the plan says so.
    h.app.perform(Action::Sync);
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Viewer(viewer)) = h.app.overlay() else { panic!("the plan") };
    assert!(viewer.sync.is_none(), "nothing to run");
}

#[tokio::test]
async fn fonts_are_picked_from_the_installed_ones() {
    use pnxr_frontend::settings::{Field, Row};
    let mut h = Harness::new().await;
    h.app.set_fonts(vec!["Fira Sans".into(), "DejaVu Sans".into(), "Fira Code".into()]);
    h.key("Ctrl+,");
    for _ in 0..60 {
        let Some(Overlay::Settings(screen)) = h.app.overlay() else { panic!("settings expected") };
        if screen.current() == Row::Option(Field::FontFamily) {
            break;
        }
        h.key("Down");
    }
    h.key("Enter");
    assert_eq!(results(&h)[0], "(system default)");
    assert_eq!(results(&h).len(), 4);
    h.type_text("FIRA");
    assert_eq!(results(&h), ["Fira Code", "Fira Sans"], "sorted, case-insensitive match");
    h.key("Down");
    h.key("Enter");
    assert_eq!(h.app.context().config.appearance.font_family, "Fira Sans");
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))), "back to the settings");
}

#[tokio::test]
async fn the_terminal_has_no_font_list() {
    use pnxr_frontend::settings::{Field, Row};
    let mut h = Harness::new().await;
    h.key("Ctrl+,");
    for _ in 0..60 {
        let Some(Overlay::Settings(screen)) = h.app.overlay() else { panic!("settings expected") };
        if screen.current() == Row::Option(Field::FontFamily) {
            break;
        }
        h.key("Down");
    }
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
    assert!(h.status().contains("terminal uses its own font"), "{}", h.status());
}

#[tokio::test]
async fn disk_usage_lists_by_size_and_drills_down() {
    let mut h = Harness::new().await;
    h.key("Ctrl+F7");
    h.settle().await;
    let rows = results(&h);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows[0].ends_with("notes.txt"), "biggest first: {rows:?}");
    assert!(rows[1].ends_with("docs/"), "{rows:?}");
    h.key("Down");
    h.key("Enter");
    h.settle().await;
    assert!(results(&h)[0].ends_with("readme.md"));
    h.key("Backspace");
    h.settle().await;
    assert_eq!(results(&h).len(), 3, "back in the first folder");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn duplicates_are_found_by_content_across_both_panels() {
    let mut h = Harness::new().await;
    fs::write(h.path("left/same.txt"), "the needle is here\n").unwrap();
    fs::write(h.path("right/copy.txt"), "the needle is here\n").unwrap();
    fs::write(h.path("right/other.txt"), "the needle is HERE\n").unwrap(); // same size, other content
    h.app.perform(Action::Duplicates);
    h.settle().await;
    let rows = results(&h);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[0].starts_with("Put 2 extra copies in the basket"), "{rows:?}");
    assert!(rows[1].starts_with("3 copies of 19 B"), "{rows:?}");
    h.key("Enter");
    assert!(h.status().contains("2 extra copies are in the basket"), "{}", h.status());
}

#[tokio::test]
async fn no_duplicates_is_said() {
    let mut h = Harness::new().await;
    h.app.perform(Action::Duplicates);
    h.settle().await;
    assert!(h.status().contains("No duplicates among"), "{}", h.status());
}

// Runs the system's own tool; PowerShell on a Windows runner takes too long
// to wait for, and the parsers of every system are tested in pnxr-frontend.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_disk_table_opens_or_says_why_not() {
    let mut h = Harness::new().await;
    h.key("Ctrl+F8");
    h.settle().await;
    let shown = matches!(h.app.overlay(), Some(Overlay::Choose(_)));
    assert!(shown || h.status().contains("Disks:"), "{}", h.status());
}

#[tokio::test]
async fn running_transfers_show_a_bar_and_their_numbers() {
    let mut h = Harness::with_settings(|c| c.transfers.conflict = ConflictPolicy::Ask).await;
    let idle = h.screen();
    assert!(!idle.contains("Copying"), "idle: no transfer line");
    fs::write(h.path("right/a.txt"), "old").unwrap();
    h.focus("a.txt");
    h.key("F5");
    h.key("Enter");
    // The copy waits for the answer, so it is running.
    h.settle_until(|app| app.pending_conflict().is_some()).await;
    let screen = h.screen();
    assert!(screen.contains("Copying ·"), "{screen}");
    h.press(KeyPress::char('o'));
    h.settle_until(|app| pnxr_frontend::view::jobs_progress(&app.jobs, app).is_none()).await;
    let screen = h.screen();
    assert!(!screen.contains("Copying ·"), "the line goes with the job: {screen}");
}

#[tokio::test]
async fn files_get_the_icon_of_their_kind_unless_icons_are_off() {
    let mut h = Harness::new().await;
    let screen = h.screen();
    let row = |name: &str| screen.lines().find(|l| l.contains(name)).unwrap_or_default().to_owned();
    assert!(row("docs/").contains("▸ docs/"), "{}", row("docs/"));
    assert!(row("a.txt").contains("≡ a.txt"), "{}", row("a.txt"));
    assert!(!row("..").contains('▸'), "no icon for the parent entry");

    let mut h = Harness::with_settings(|c| c.panels.icons = false).await;
    let screen = h.screen();
    assert!(!screen.contains("▸ docs/") && !screen.contains("≡ a.txt"), "{screen}");
}

/// Opens `agent://` places on an agent running in this process (agents run on
/// Unix servers, and their paths are Unix paths), and counts
/// the connections it makes.
#[cfg(unix)]
struct InProcessAgents(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[cfg(unix)]
#[async_trait::async_trait]
impl ProviderFactory for InProcessAgents {
    fn scheme(&self) -> &str {
        "agent"
    }

    async fn create(
        &self,
        _path: &VfsPath,
        _registry: &ProviderRegistry,
    ) -> pnxr_vfs::Result<std::sync::Arc<dyn Provider>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (controller, agent) = tokio::io::duplex(1024 * 1024);
        let service = pnxr_agent::AgentService::new(pnxr_agent::Limits::default())
            .map_err(|e| pnxr_vfs::VfsError::Other(e.to_string()))?;
        tokio::spawn(pnxr_proto::transport::serve(service, agent));
        let client = pnxr_proto::transport::connect(controller)
            .await
            .map_err(|e| pnxr_vfs::VfsError::Other(e.to_string()))?;
        let provider = pnxr_agent::AgentProvider::handshake(client, None)
            .await
            .map_err(|e| pnxr_vfs::VfsError::Other(e.to_string()))?;
        Ok(std::sync::Arc::new(provider))
    }
}

#[cfg(unix)]
#[tokio::test]
async fn ctrl_right_opens_a_remote_subfolder_in_the_other_panel() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("srv/docs")).unwrap();
    fs::write(root.join("srv/a.txt"), "alpha\n").unwrap();
    fs::write(root.join("srv/docs/readme.md"), "# readme\n").unwrap();
    fs::create_dir_all(root.join("right")).unwrap();

    let connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = pnxr_providers::default_registry();
    registry.register(InProcessAgents(connections.clone()));
    let mut context = AppContext::load_from(AppPaths::in_dir(root.join("settings"))).unwrap();
    context.registry = std::sync::Arc::new(registry);
    // The right panel starts on this machine; the left one on the server.
    let left = VfsPath::parse(&format!("agent://nas{}", root.join("srv").display())).unwrap();
    let right = VfsPath::from_local(root.join("right")).unwrap();
    let (app, events) = App::new(context, left, right).unwrap();
    let mut h = Harness { app, events, dir };
    h.settle().await;
    assert_eq!(h.names(SideId::Left), ["..", "docs", "a.txt"]);
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 1);

    // The right panel reaches the same server and shows the subfolder.
    h.focus("docs");
    h.key("Ctrl+Right");
    h.settle().await;
    assert_eq!(h.names(SideId::Right), ["..", "readme.md"]);
    assert!(
        h.app
            .workspace()
            .side(SideId::Right)
            .panel()
            .location()
            .to_string()
            .starts_with("agent://nas/")
    );
    assert_eq!(
        connections.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one connection is shared"
    );
    assert_eq!(h.app.workspace().active_side(), SideId::Left, "the focus stays");
}

#[tokio::test]
async fn the_basket_packs_what_it_holds() {
    let mut h = Harness::with_settings(|_| {}).await;
    h.focus("notes.txt");
    h.key("Ctrl+G");
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    h.focus("readme.md");
    h.key("Ctrl+G");
    h.key("Ctrl+B");
    h.key("Shift+F5");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no pack dialog") };
    assert!(dialog.field.text().ends_with("basket.zip"), "{}", dialog.field.text());
    h.key("Enter");
    h.settle().await;
    assert_eq!(h.status(), "Packed 2 entries into basket.zip");
    assert!(h.path("right/basket.zip").exists());
}

// ------------------------------------------------------------- pick mode

fn paths(h: &Harness) -> Vec<PathBuf> {
    h.app.picked().expect("something was chosen").iter().filter_map(|p| p.to_local_path()).collect()
}

#[tokio::test]
async fn pick_mode_chooses_the_file_under_the_cursor() {
    let mut h = Harness::new().await;
    h.app.set_picker(pnxr_tui::PickMode::default());
    // The cursor starts on `..`; folders are entered, files are chosen.
    h.key("Down");
    h.key("Down"); // docs/ is first, then a.txt
    h.key("Enter");
    h.settle().await;
    assert!(!h.app.is_running());
    assert_eq!(paths(&h), vec![h.path("left/a.txt")]);
}

#[tokio::test]
async fn pick_mode_enters_folders_and_esc_cancels() {
    let mut h = Harness::new().await;
    h.app.set_picker(pnxr_tui::PickMode::default());
    h.key("Down"); // docs/
    h.key("Enter");
    h.settle().await;
    assert!(h.app.is_running(), "a folder is entered, not chosen");
    assert!(h.screen().contains("readme.md"));
    h.key("Esc");
    assert!(!h.app.is_running());
    assert!(h.app.picked().is_none(), "cancelled: nothing chosen");
}

#[tokio::test]
async fn pick_mode_with_several_files_gives_the_marked_ones() {
    let mut h = Harness::new().await;
    h.app.set_picker(pnxr_tui::PickMode { folder: false, multiple: true });
    h.key("Down");
    h.key("Down"); // a.txt
    h.key("Space"); // marks a.txt and moves to notes.txt
    h.key("Space");
    h.key("Enter");
    h.settle().await;
    assert_eq!(paths(&h), vec![h.path("left/a.txt"), h.path("left/notes.txt")]);
}

#[tokio::test]
async fn pick_mode_for_a_folder_gives_the_one_shown() {
    let mut h = Harness::new().await;
    h.app.set_picker(pnxr_tui::PickMode { folder: true, multiple: false });
    h.key("Down"); // docs/
    h.key("Enter");
    h.settle().await;
    h.key("Ctrl+F10");
    assert!(!h.app.is_running());
    assert_eq!(paths(&h), vec![h.path("left/docs")]);
}

#[tokio::test]
async fn choosing_does_nothing_outside_pick_mode() {
    let mut h = Harness::new().await;
    h.key("Ctrl+F10");
    assert!(h.app.is_running());
    assert!(h.app.picked().is_none());
}

#[tokio::test]
async fn the_flat_view_lists_files_below_with_their_folder() {
    let mut h = Harness::new().await;
    h.key("Ctrl+Shift+F");
    h.settle().await;
    let screen = h.screen();
    // (The title may be cut on long temporary paths; the column says it too.)
    assert!(screen.contains("Folder"), "a folder column: {screen}");
    let line = screen.lines().find(|l| l.contains("readme.md")).expect("the nested file is listed");
    assert!(line.contains("docs"), "with its folder: {line}");
    assert!(!screen.contains("docs/ "), "folders themselves are not rows");
    // Operations act on the real file: view it with F3.
    h.key("Backspace");
    h.settle().await;
    let screen = h.screen();
    assert!(!screen.contains("Folder "), "Backspace leaves the flat view: {screen}");
    assert!(h.app.is_running());
    h.key("Ctrl+Shift+F");
    h.settle().await;
    assert!(h.screen().contains("Folder "), "Ctrl+Shift+F enters it again");
    h.key("Ctrl+Shift+F");
    h.settle().await;
    assert!(!h.screen().contains("Folder "), "and leaves it");
    assert!(screen.contains("docs"), "{screen}");
}

#[tokio::test]
async fn paste_goes_into_the_open_question_and_nowhere_else() {
    let mut h = Harness::new().await;
    // No window open: pasted text must not run commands.
    h.app.paste("dddd\n");
    assert!(h.app.overlay().is_none());

    h.focus("notes.txt");
    h.key("F6"); // rename/move dialog
    h.key("Ctrl+U");
    h.app.paste("/tmp/some folder/é\r\nsecond line");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("a question") };
    assert_eq!(dialog.field.text(), "/tmp/some folder/é", "first line only");
    h.app.paste("x\ty");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("a question") };
    assert_eq!(dialog.field.text(), "/tmp/some folder/éxy", "control characters dropped");
}

/// Opens the drive list with the cursor on the row that holds `name`.
fn drive_row(h: &mut Harness, name: &str) {
    h.key("Shift+F1");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no drive list") };
    let row = dialog.items.iter().position(|r| r.contains(&format!(" {name} "))).unwrap();
    for _ in 0..row {
        h.key("Down");
    }
}

#[tokio::test]
async fn a_connection_is_edited_with_guidance_and_tested_with_a_log() {
    let backup = TempDir::new().unwrap();
    fs::write(backup.path().join("saved.txt"), "kept\n").unwrap();
    let uri = VfsPath::from_local(backup.path()).unwrap().to_string();
    let mut h = Harness::with_settings(|config| {
        config.connections.push(pnxr_config::Connection {
            name: "backup".into(),
            uri: "sftp://me@nas.invalid/srv".into(),
            identity_file: None,
            options: Default::default(),
        });
    })
    .await;

    // F4 asks for the address (with samples, user and port) and then the name.
    drive_row(&mut h, "backup");
    h.key("F4");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no address asked") };
    assert_eq!(dialog.field.text(), "sftp://me@nas.invalid/srv");
    assert_eq!(dialog.preview[0], "✓ SFTP over SSH · host nas.invalid · user me");
    assert!(dialog.hint().contains("Ctrl+T: test"), "the key is in the key row");

    // A password typed in the address is a warning, not an error.
    h.type_text("sftp://me:secret@nas/srv");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no address asked") };
    assert!(dialog.preview[0].starts_with("✓"), "{:?}", dialog.preview);
    assert!(dialog.preview.iter().any(|l| l.starts_with("⚠")), "{:?}", dialog.preview);
    // Enter with a wrong address stays in the question, with the reason.
    h.type_text("sftp://nas:99999/srv");
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("closed on a bad address") };
    assert_eq!(dialog.title, "Edit connection");
    assert!(dialog.preview[0].contains("1 and 65535"), "{:?}", dialog.preview);
    assert_eq!(dialog.field.text(), "sftp://nas:99999/srv", "the text is kept");

    h.type_text("sftp://me@nas.invalid:2222/data");
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no name asked") };
    assert_eq!(dialog.field.text(), "backup", "the name is offered");
    h.type_text("nas backup");
    h.key("Enter");
    let Some(Overlay::Choose(login)) = h.app.overlay() else { panic!("no login question") };
    assert!(login.items[2].contains("SSH keys"), "{:?}", login.items);
    h.key("Enter");
    let saved = &h.app.context().config.connections;
    assert_eq!(
        (saved[0].name.as_str(), saved[0].uri.as_str()),
        ("nas backup", "sftp://me@nas.invalid:2222/data")
    );
    assert_eq!(saved.len(), 1, "edited in place, not added");

    // Local drives cannot be edited.
    h.key("Shift+F1");
    h.key("Home");
    h.key("F4");
    assert!(h.status().contains("Only saved connections"), "{}", h.status());

    // Ctrl+T in the address question tests it and shows a log; Enter goes
    // back to the question.
    h.key("Ctrl+N");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text(&uri);
    h.key("Ctrl+T");
    h.settle().await;
    let Some(Overlay::Message(log)) = h.app.overlay() else { panic!("no log") };
    let text = log.lines.join("\n");
    assert!(text.contains("Connected"), "{text}");
    assert!(text.contains("1 files"), "{text}");
    assert!(text.contains("Result: it works"), "{text}");
    assert!(h.screen().contains("Test connection"));
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("not back in the question") };
    assert_eq!(dialog.field.text(), uri);

    // A test of a connection that fails says where, with a hint.
    h.key("Esc");
    drive_row(&mut h, "nas backup");
    h.key("Ctrl+T");
    h.settle().await;
    let Some(Overlay::Message(log)) = h.app.overlay() else { panic!("no log") };
    let text = log.lines.join("\n");
    assert!(text.contains("Opening the sftp:// connection"), "{text}");
    assert_eq!(log.lines.last().unwrap(), "Result: failed", "{text}");
    h.key("Esc");

    // Local rows cannot be tested.
    h.key("Shift+F1");
    h.key("Home");
    h.key("Ctrl+T");
    assert!(h.status().contains("Only a remote connection"), "{}", h.status());
}

#[tokio::test]
async fn advanced_pack_protects_an_archive_and_opening_it_asks_the_password() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Shift+F5");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("pack question") };
    assert!(!dialog.prompt.contains("Ctrl+O"), "the key is in the key row: {}", dialog.prompt);
    assert!(dialog.hint().contains("Ctrl+O: more options"), "{}", dialog.hint());
    assert!(h.screen().contains("Ctrl+O: more options"));
    h.key("Ctrl+O");
    // 1. The format.
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no format list") };
    assert!(dialog.title.starts_with("Advanced pack"), "{}", dialog.title);
    assert!(dialog.items[0].starts_with("zip"), "{:?}", dialog.items);
    assert!(dialog.items.iter().any(|r| r.starts_with("7z")));
    assert!(dialog.items.iter().any(|r| r.starts_with("tar.zst")));
    assert!(
        dialog.items.iter().any(|r| r.starts_with("gz")),
        "one folder: a single file format is offered"
    );
    h.key("Enter"); // zip
    // 2. The level, with "Normal" on the cursor.
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no level list") };
    assert_eq!(dialog.cursor, 2);
    h.key("Enter");
    // 3. The password, typed twice; it is drawn as dots.
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no password asked") };
    assert!(dialog.secret);
    h.type_text("s3cret");
    let screen = h.screen();
    assert!(screen.contains("••••••") && !screen.contains("s3cret"), "{screen}");
    h.key("Enter");
    h.type_text("different");
    h.key("Enter");
    assert!(h.status().contains("differ"), "{}", h.status());
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("asked again") };
    assert!(dialog.prompt.contains("Password"));
    h.type_text("s3cret");
    h.key("Enter");
    h.type_text("s3cret");
    h.key("Enter");
    // 4. Where to write it; the extension is the chosen type's.
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no destination") };
    assert!(dialog.field.text().ends_with("docs.zip"), "{}", dialog.field.text());
    assert!(dialog.prompt.contains("password"), "{}", dialog.prompt);
    let target = h.path("right/locked").display().to_string();
    h.type_text(&target);
    h.key("Enter");
    h.settle().await;
    assert!(h.path("right/locked.zip").exists(), "the extension was added");

    // The archive is encrypted: nothing readable without the password.
    let bytes = fs::read(h.path("right/locked.zip")).unwrap();
    assert!(!bytes.windows(8).any(|w| w == b"# readme"), "content is not in clear");

    // Entering it works (names are visible); reading a member asks.
    h.key("Tab");
    h.focus("locked.zip");
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no password asked") };
    assert_eq!(dialog.title, "Password");
    assert!(dialog.prompt.contains("locked.zip is encrypted"), "{}", dialog.prompt);
    // Esc declines: no second question while the archive stays open.
    h.key("Esc");
    assert!(h.app.overlay().is_none(), "declined: no second question");
    h.settle().await;
    assert!(h.app.overlay().is_none());

    // Leave and enter again: it asks again. A wrong password is refused
    // when something is read, and the question comes back.
    h.focus("..");
    h.key("Enter");
    h.settle().await;
    h.focus("locked.zip");
    h.key("Enter");
    h.settle().await;
    let Some(Overlay::Input(_)) = h.app.overlay() else { panic!("asked again after leaving") };
    h.type_text("wrong one");
    h.key("Enter");
    h.settle().await;
    assert!(h.app.overlay().is_none());
    h.focus("docs");
    h.key("Enter");
    h.settle().await;
    h.focus("readme.md");
    h.key("F3");
    h.settle().await;
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no second question") };
    assert!(dialog.prompt.starts_with("Wrong password"), "{}", dialog.prompt);
    h.type_text("s3cret");
    h.key("Enter");
    h.settle().await;
    h.focus("readme.md");
    h.key("F3");
    h.settle().await;
    let screen = h.screen();
    assert!(screen.contains("══════"), "the member is read with the password:\n{screen}");
}

#[tokio::test]
async fn advanced_pack_without_a_password_skips_the_questions_that_do_not_apply() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Shift+F5");
    // A typed .tar.zst name puts the cursor on that format.
    h.type_text("out.tar.zst");
    h.key("Ctrl+O");
    // tar.zst: a level, but no password question.
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("no format list") };
    assert!(dialog.items[dialog.cursor].starts_with("tar.zst"), "{}", dialog.cursor);
    h.key("Enter");
    h.key("Down");
    h.key("Down");
    h.key("Enter"); // "Smallest"
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no destination") };
    assert!(!dialog.secret);
    assert!(dialog.field.text().ends_with("docs.tar.zst"), "{}", dialog.field.text());
    h.key("Enter");
    h.settle().await;
    assert!(h.path("right/docs.tar.zst").exists());

    // 7z with an empty password: no second question, no level.
    h.key("Shift+F5");
    h.key("Ctrl+O");
    h.key("Down");
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("password asked") };
    assert!(dialog.secret);
    h.key("Enter"); // empty: none
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no destination") };
    assert!(dialog.field.text().ends_with("docs.7z"), "{}", dialog.field.text());
    h.key("Esc");
}

/// The login questions of a connection with a password; `dir` holds a fake
/// `secret-tool` that keeps what the "save" answer stores in files. (Not on
/// macOS: there the backend is `security`, whose arguments this script would
/// loop on, and the override would leak into every other test.)
#[cfg(all(unix, not(target_os = "macos")))]
async fn webdav_harness(dir: &std::path::Path) -> Harness {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("secret-tool");
    let body = format!(
        r#"#!/bin/sh
cmd=$1; shift
[ "$cmd" = store ] && shift 2
while [ $# -gt 0 ]; do case $1 in host) h=$2;; user) u=$2;; esac; shift 2; done
f={dir}/$h.$u
case $cmd in
  store) cat > "$f" ;;
  lookup) [ -f "$f" ] && cat "$f" || exit 1 ;;
esac
"#,
        dir = dir.display()
    );
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    pnxr_secrets::keyring::set_command(Some(script));
    Harness::new().await
}

#[tokio::test]
async fn the_new_connection_prompt_lists_one_example_per_line() {
    let mut h = Harness::new().await;
    h.key("Ctrl+N");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no address asked") };
    assert!(dialog.preview.len() > 4, "{:?}", dialog.preview);
    assert!(dialog.preview.iter().any(|l| l.contains("sftp://user@host:22/path")));
    assert!(
        dialog.preview.iter().all(|l| !l.to_lowercase().contains("not stored")),
        "no password note among the examples: {:?}",
        dialog.preview
    );
    assert!(dialog.hint().contains("Ctrl+T: test"));
}

/// The login question saves through the `SecretStore` of the context, so a fake
/// store is enough: no keyring, no `secret-tool` (ADR 0061).
#[tokio::test]
async fn a_login_is_saved_in_the_secret_store_of_the_context() {
    use std::sync::Arc;
    let store = Arc::new(pnxr_secrets::MemorySecrets::new());
    let shared = Arc::clone(&store);
    let mut h = Harness::with_context(move |context| context.secrets = shared).await;
    h.key("Ctrl+N");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text("dav://anna:s3cret@files3.invalid/docs");
    h.key("Enter");
    h.type_text("files3");
    h.key("Enter");
    let Some(Overlay::Choose(choice)) = h.app.overlay() else { panic!("no login question") };
    assert!(
        choice.items.iter().any(|row| row.contains("the test store")),
        "the question names the store of the context: {:?}",
        choice.items
    );
    h.key("Enter");
    h.settle().await;
    use pnxr_secrets::SecretStore as _;
    let stored = store.lookup("files3.invalid", Some("anna")).expect("saved in the fake store");
    assert_eq!(stored.password, "s3cret");
    assert_eq!(h.app.context().config.connections[0].uri, "dav://anna@files3.invalid/docs");
}

#[cfg(all(unix, not(target_os = "macos")))]
#[tokio::test]
async fn a_login_is_saved_in_the_keyring_or_asked_every_time() {
    let dir = TempDir::new().unwrap();
    let mut h = webdav_harness(dir.path()).await;

    // Typing a password in the address warns, then offers where to keep it.
    h.key("Ctrl+N");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text("dav://anna:s3cret@files.invalid/docs");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("no address asked") };
    assert!(dialog.preview.iter().any(|l| l.starts_with("⚠")), "{:?}", dialog.preview);
    h.key("Enter");
    let Some(Overlay::Input(name)) = h.app.overlay() else { panic!("no name asked") };
    assert!(name.hint().contains("Ctrl+T"), "the test is offered here too");
    h.type_text("files");
    h.key("Enter");
    let Some(Overlay::Choose(choice)) = h.app.overlay() else { panic!("no login question") };
    assert_eq!(choice.items.len(), 3);
    assert_eq!(choice.cursor, 1, "the typed password is offered for saving");
    h.key("Enter");
    h.settle().await;
    let saved = &h.app.context().config.connections[0];
    assert_eq!(saved.uri, "dav://anna@files.invalid/docs", "no password in the settings");
    assert!(!saved.options.contains_key("password"));
    let stored = pnxr_secrets::keyring::lookup("files.invalid", Some("anna")).unwrap();
    assert_eq!(stored.password, "s3cret", "the keyring holds the password");
    let settings = format!("{:?}", h.app.context().config.connections);
    assert!(!settings.contains("s3cret"), "the password never reaches the settings");

    // Another connection, whose password is typed each time.
    h.key("Ctrl+N");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text("dav://files2.invalid/docs");
    h.key("Enter");
    h.type_text("second");
    h.key("Enter");
    let Some(Overlay::Choose(_)) = h.app.overlay() else { panic!("no login question") };
    h.key("Home");
    h.key("Enter");
    // No user in the address: it is asked.
    let Some(Overlay::Input(user)) = h.app.overlay() else { panic!("no user asked") };
    assert!(user.prompt.contains("User name"), "{}", user.prompt);
    h.type_text("bob");
    h.key("Enter");
    h.settle().await;
    let saved = &h.app.context().config.connections[1];
    assert_eq!(saved.uri, "dav://bob@files2.invalid/docs");
    assert_eq!(saved.options.get("password").map(String::as_str), Some("ask"));
    assert!(
        pnxr_secrets::keyring::lookup("files2.invalid", Some("bob")).is_none(),
        "nothing saved"
    );

    // Opening it asks for the password (hidden), once per session.
    drive_row(&mut h, "second");
    h.key("Enter");
    let Some(Overlay::Input(ask)) = h.app.overlay() else { panic!("no password asked") };
    assert!(ask.secret && ask.prompt.contains("bob@files2.invalid"), "{}", ask.prompt);
}

#[tokio::test]
async fn a_new_connection_can_be_tested_before_it_is_saved() {
    let folder = TempDir::new().unwrap();
    fs::write(folder.path().join("a.txt"), "a\n").unwrap();
    let uri = VfsPath::from_local(folder.path()).unwrap().to_string();
    let mut h = Harness::new().await;
    h.key("Shift+F1");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text(&uri);
    h.key("Enter");
    // In the name question the test is still there, and Enter in its log
    // comes back to the name.
    h.key("Ctrl+T");
    h.settle().await;
    let Some(Overlay::Message(log)) = h.app.overlay() else { panic!("no log") };
    assert!(log.lines.join("\n").contains("Result: it works"), "{:?}", log.lines);
    h.key("Enter");
    let Some(Overlay::Input(name)) = h.app.overlay() else { panic!("back to the name") };
    assert!(matches!(name.purpose, pnxr_frontend::InputPurpose::NameConnection { .. }));
    assert!(h.app.context().config.connections.is_empty(), "nothing saved by testing");
}

#[tokio::test]
async fn ssh_and_s3_connections_offer_the_login_choice_too() {
    let dir = TempDir::new().unwrap();
    let aws = dir.path().join("credentials");
    pnxr_s3::settings::set_credentials_file(Some(aws.clone()));
    pnxr_secrets::netrc::set_file(Some(dir.path().join("netrc")));
    let mut h = Harness::new().await;

    // S3: save the keys (access key, then hidden secret).
    h.key("Shift+F1");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text("s3://store-bucket/");
    h.key("Enter");
    h.type_text("store");
    h.key("Enter");
    let Some(Overlay::Choose(choice)) = h.app.overlay() else { panic!("no keys question") };
    assert_eq!(choice.title, "Keys for this connection");
    assert!(choice.items[1].contains("~/.aws/credentials"), "{:?}", choice.items);
    h.key("Up");
    h.key("Enter");
    let Some(Overlay::Input(access)) = h.app.overlay() else { panic!("no access key asked") };
    assert!(access.prompt.contains("Access key ID"));
    h.type_text("AKTEST");
    h.key("Enter");
    let Some(Overlay::Input(secret)) = h.app.overlay() else { panic!("no secret asked") };
    assert!(secret.secret);
    h.type_text("topsecret");
    h.key("Enter");
    h.settle().await;
    let keys = fs::read_to_string(&aws).unwrap();
    assert!(keys.contains("aws_access_key_id = AKTEST"), "{keys}");
    let settings = format!("{:?}", h.app.context().config.connections);
    assert!(!settings.contains("topsecret"), "{settings}");
    assert!(pnxr_s3::settings::has_session(None));

    // SFTP: ask every time; the user is asked, nothing is written.
    h.key("Shift+F1");
    for _ in 0..40 {
        h.key("Down");
    }
    h.key("Enter");
    h.type_text("sftp://box.invalid/");
    h.key("Enter");
    h.type_text("box");
    h.key("Enter");
    h.key("Home");
    h.key("Enter");
    let Some(Overlay::Input(user)) = h.app.overlay() else { panic!("no user asked") };
    assert!(user.prompt.contains("User name"));
    h.type_text("carol");
    h.key("Enter");
    let saved = h.app.context().config.connections.last().unwrap().clone();
    assert_eq!(saved.uri, "sftp://carol@box.invalid/");
    assert_eq!(saved.options.get("password").map(String::as_str), Some("ask"));
}

#[tokio::test]
async fn the_command_list_forgives_spelling_mistakes() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("settngs");
    let screen = h.screen();
    assert!(screen.contains("Settings"), "{screen}");
    // Typed right, only the exact matches stay.
    h.key("Esc");
    h.key("F9");
    h.type_text("copy");
    assert!(!h.screen().contains("Settings"), "{}", h.screen());
}

#[tokio::test]
async fn the_install_question_helps_while_typing_and_explains_itself() {
    let mut h = Harness::new().await;
    h.app.perform(Action::InstallServe);
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("the install question") };
    assert!(dialog.preview[0].starts_with("Examples"), "{:?}", dialog.preview);
    assert!(dialog.hint().contains("Ctrl+T") && dialog.hint().contains("F1"));
    h.type_text("me@nas.lan --re");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("still the question") };
    assert!(dialog.preview[0].starts_with("✓ install on me@nas.lan"), "{:?}", dialog.preview);
    assert!(dialog.preview.iter().any(|l| l.contains("--renew")), "{:?}", dialog.preview);
    // F1 explains, Enter goes back to the question with the text kept.
    h.key("F1");
    let Some(Overlay::Message(message)) = h.app.overlay() else { panic!("the explanation") };
    assert!(message.lines.join("\n").contains("systemd"));
    h.key("Enter");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("back to the question") };
    assert_eq!(dialog.field.text(), "me@nas.lan --re");
    // A wrong answer is refused by the test with the reason.
    h.key("Ctrl+U");
    h.type_text("--bogus");
    h.key("Ctrl+T");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("asked again") };
    assert!(dialog.preview[0].starts_with('✗'), "{:?}", dialog.preview);
}

#[tokio::test]
async fn the_entry_line_says_what_a_git_mark_means() {
    let mut h = Harness::new().await;
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(h.path("left"))
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("notes.txt");
    h.settle().await;
    let screen = h.screen();
    assert!(screen.contains("git ?: not tracked by git yet"), "{screen}");
}

#[tokio::test]
async fn install_asks_before_it_starts_and_shift_tab_goes_back() {
    let mut h = Harness::new().await;
    h.app.perform(Action::InstallServe);
    h.type_text("me@nas.lan --read-only");
    h.key("Enter");
    // Nothing is touched before a yes: a question shows what will be done.
    let Some(Overlay::Confirm(dialog)) = h.app.overlay() else { panic!("confirm dialog") };
    assert_eq!(dialog.title, "Install pnxr serve?");
    assert!(dialog.lines.join("\n").contains("read only"), "{:?}", dialog.lines);
    assert!(h.screen().contains("Shift+Tab: back"));
    h.key("Shift+Tab");
    let Some(Overlay::Input(dialog)) = h.app.overlay() else { panic!("back to the question") };
    assert_eq!(dialog.field.text(), "me@nas.lan --read-only", "what was typed is kept");
    h.key("Esc");
    assert!(h.app.overlay().is_none());
    h.key("Shift+Tab");
    assert!(h.app.overlay().is_none(), "nothing to go back to");
}

/// The install runs through the `Remotes` of the context: with a fake, no SSH
/// is started and the whole flow (question, yes, log, save) can be driven
/// (ADR 0061).
#[tokio::test]
async fn the_install_runs_through_the_remotes_of_the_context() {
    use std::sync::Arc;
    let fake = Arc::new(pnxr_app::FakeRemotes::new());
    let shared = Arc::clone(&fake);
    let mut h = Harness::with_context(move |context| context.remotes = shared).await;
    h.app.perform(Action::InstallServe);
    h.type_text("me@nas.lan --port 9000");
    h.key("Enter");
    let Some(Overlay::Confirm(_)) = h.app.overlay() else { panic!("confirm dialog") };
    h.key("Enter");
    h.settle_until(|app| matches!(app.overlay(), Some(Overlay::Message(m)) if !m.busy)).await;
    let Some(Overlay::Message(done)) = h.app.overlay() else { panic!("the install log") };
    let text = done.lines.join("\n");
    assert!(text.contains("scs://me@nas.lan:9000"), "{text}");
    assert_eq!(*fake.installs.lock().unwrap(), ["me@nas.lan"]);
    // A bad server is refused by the port, before anything runs.
    h.key("Esc");
    h.key("Esc");
    h.app.perform(Action::InstallServe);
    h.type_text("me@nas.lan:x");
    h.key("Enter");
    assert!(!matches!(h.app.overlay(), Some(Overlay::Confirm(_))));
}

/// Terminals, Git and disks go through `HostTools`: with the fake, no process
/// is started (ADR 0061).
#[tokio::test]
async fn terminal_git_and_disks_go_through_the_host_port() {
    use pnxr_frontend::disks::DiskRow;
    use pnxr_frontend::git::GitStatus;
    use pnxr_frontend::ports::FakeHost;
    use std::sync::Arc;
    let fake = Arc::new(FakeHost::new());
    let mut h = Harness::new().await;
    h.app.set_host(fake.clone());
    h.app.graphical = true;

    h.app.perform(Action::OpenTerminal);
    let launches = fake.launches.lock().unwrap().clone();
    assert_eq!(launches.len(), 1, "one terminal asked for");
    assert_eq!(launches[0].1.as_deref(), Some(h.path("left").as_path()));

    // Git: the panel marks come from the host, and so does the repository check.
    h.app.perform(Action::Git);
    assert!(!matches!(h.app.overlay(), Some(Overlay::Choose(_))), "no repository by default");
    *fake.git.lock().unwrap() = Some(GitStatus { branch: "main".into(), ..GitStatus::default() });
    h.app.perform(Action::Git);
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))), "the Git actions are offered");
    h.key("Esc");

    // Disks: whatever the host lists is shown.
    fake.disks.lock().unwrap().push(DiskRow {
        name: "fakedisk0".into(),
        kind: "disk".into(),
        ..DiskRow::default()
    });
    h.app.perform(Action::Disks);
    h.settle_until(|app| matches!(app.overlay(), Some(Overlay::Choose(_)))).await;
    assert!(h.screen().contains("fakedisk0"), "{}", h.screen());
}

#[tokio::test]
async fn binary_files_show_their_properties_and_hex_on_request() {
    let mut h = Harness::new().await;
    let mut elf = vec![0u8; 256];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[16] = 2;
    elf[18] = 0x3e;
    fs::write(h.path("left/prog.bin"), &elf).unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("prog.bin");
    h.key("F3");
    h.settle_until(|app| matches!(app.overlay(), Some(Overlay::Viewer(v)) if v.data.is_some()))
        .await;
    let screen = h.screen();
    assert!(screen.contains("ELF") && screen.contains("x86-64"), "{screen}");
    assert!(!screen.contains("00000000"), "no hex dump unless asked:\n{screen}");
    h.key("F4");
    let screen = h.screen();
    assert!(screen.contains("00000000"), "hex on request:\n{screen}");
}

#[tokio::test]
async fn the_transfer_queue_lists_the_newest_first() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("F5");
    h.key("Enter");
    h.settle().await;
    h.focus("notes.txt");
    h.key("F5");
    h.key("Enter");
    h.settle().await;
    let ids: Vec<i64> = h.app.jobs().iter().map(|j| j.id.0).collect();
    assert!(ids.len() >= 2 && ids[0] > ids[1], "{ids:?}");
}

// ---- parallelism, help and generated files (ADR 0044)

/// The characters of a test buffer as lines of text.
fn text_of(buffer: &ratatui::buffer::Buffer) -> String {
    let width = buffer.area.width as usize;
    buffer
        .content()
        .chunks(width)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where the screens of these tests are written when `PNXR_SCREENS` is set
/// (the text "screenshots" of the transfer indicators).
fn save_screen(name: &str, text: &str) {
    if let Ok(dir) = std::env::var("PNXR_SCREENS") {
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(std::path::Path::new(&dir).join(format!("{name}.txt")), text);
    }
}

#[tokio::test]
async fn running_copy_shows_progress_speed_and_files_at_once() {
    let mut h = Harness::with_settings(|c| c.transfers.bandwidth_limit_kib = 512).await;
    for i in 0..6 {
        fs::write(h.path(&format!("left/big{i}.bin")), vec![7u8; 400 * 1024]).unwrap();
    }
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("big0.bin");
    for _ in 0..6 {
        h.key("Insert");
    }
    h.key("F5");
    h.key("Enter");
    h.settle_until(|app| {
        let summary = pnxr_frontend::view::jobs_summary(app.jobs(), app);
        summary.is_some_and(|s| s.contains("at once") && s.contains(" of "))
    })
    .await;
    let screen = h.screen();
    save_screen("status-running", &screen);
    let line = screen.lines().find(|l| l.contains("at once")).unwrap_or_default().to_owned();
    assert!(line.contains("Copying"), "{line}");
    assert!(line.contains("of ") && line.contains('%'), "{line}");
    assert!(line.contains("(auto)"), "automatic mode is said: {line}");
    h.key("Ctrl+J");
    save_screen("job-window", &h.screen());
    h.key("Esc");
    h.settle().await;
}

#[tokio::test]
async fn f1_on_the_settings_screen_explains_parallelism_and_goes_back() {
    let mut h = Harness::new().await;
    h.app.perform(Action::Settings);
    h.key("F1");
    let screen = h.screen();
    save_screen("settings-help", &screen);
    assert!(screen.contains("automatic") && screen.contains("fixed"), "{screen}");
    assert!(screen.contains("discreet"), "{screen}");
    h.key("Esc");
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))), "back on the settings");
}

#[tokio::test]
async fn settings_offer_automatic_or_fixed_and_a_profile() {
    let mut h = Harness::new().await;
    h.app.perform(Action::Settings);
    for c in "parallelism".chars() {
        h.press(KeyPress::char(c));
    }
    let screen = h.screen();
    save_screen("settings-transfers", &screen);
    assert!(
        screen.contains("Parallelism of transfers") && screen.contains("automatic"),
        "{screen}"
    );
}

#[tokio::test]
async fn generates_random_text_files_with_a_counter() {
    let mut h = Harness::new().await;
    h.app.perform(Action::GenerateFiles);
    assert!(matches!(h.app.overlay(), Some(Overlay::Input(_))));
    h.type_text("sample.txt");
    h.key("Enter");
    h.type_text("3");
    h.key("Enter");
    save_screen("generate-size", &h.screen());
    h.type_text("10k");
    h.key("Enter");
    h.settle().await;
    for name in ["sample-1.txt", "sample-2.txt", "sample-3.txt"] {
        let text = fs::read_to_string(h.path(&format!("left/{name}"))).unwrap();
        assert_eq!(text.len(), 10 * 1024, "{name}");
    }
    assert_ne!(
        fs::read_to_string(h.path("left/sample-1.txt")).unwrap(),
        fs::read_to_string(h.path("left/sample-2.txt")).unwrap(),
        "different random text"
    );
    // one file keeps the name as typed
    h.app.perform(Action::GenerateFiles);
    h.type_text("one.txt");
    h.key("Enter");
    h.type_text("1");
    h.key("Enter");
    h.type_text("2k");
    h.key("Enter");
    h.settle().await;
    assert_eq!(fs::metadata(h.path("left/one.txt")).unwrap().len(), 2048);
}

#[tokio::test]
async fn a_refused_serve_connection_offers_the_install() {
    let mut h = Harness::new().await;
    h.app.perform(Action::GoTo);
    h.type_text("scs://127.0.0.1:1/");
    h.key("Enter");
    h.settle_until(|app| matches!(app.overlay(), Some(Overlay::Confirm(_)))).await;
    let Some(Overlay::Confirm(dialog)) = h.app.overlay() else { panic!("offer") };
    assert_eq!(dialog.title, "Install pnxr serve?");
    assert!(dialog.lines[0].starts_with("127.0.0.1"), "{:?}", dialog.lines);
}

#[tokio::test]
async fn a_log_window_copies_its_lines() {
    use pnxr_frontend::MessageDialog;
    let mut h = Harness::new().await;
    let lines: Vec<String> =
        ["first step", "second step", "Result: done"].map(str::to_owned).to_vec();
    h.app.overlay = Some(Overlay::Message(MessageDialog::new("Install pnxr serve: done", lines)));
    assert!(h.screen().contains("c: copy the log"), "{}", h.screen());
    h.press(KeyPress::char('c'));
    assert_eq!(h.app.take_clipboard().as_deref(), Some("first step\nsecond step\nResult: done"));
    assert!(h.status().contains("Copied"), "{}", h.status());
    assert!(h.app.overlay().is_some(), "the window stays open");

    // A short message is not a log.
    h.app.overlay = Some(Overlay::Message(MessageDialog::new("Hi", vec!["one line".into()])));
    assert!(!h.screen().contains("copy the log"));
    h.press(KeyPress::char('c'));
    assert_eq!(h.app.take_clipboard(), None);
}

#[tokio::test]
async fn the_key_bar_follows_the_modifiers_held() {
    let mut h = Harness::new().await;
    let plain = h.screen();
    assert!(plain.contains("F7Mkdir"), "{plain}");
    h.app.set_modifiers(false, false, true);
    let shifted = h.screen();
    assert!(shifted.contains("F7Link"), "Shift+F7 is the link: {shifted}");
    assert!(!shifted.contains("F7Mkdir"));
    h.app.set_modifiers(true, false, false);
    assert_ne!(h.screen(), plain, "Ctrl changes the bar too");
}

#[tokio::test]
async fn the_folder_text_is_drawn_above_the_listing() {
    let mut h = Harness::new().await;
    fs::write(h.path("left/pnxr_folder.md"), "# Holiday photos\n\nSorted by **month**.\n").unwrap();
    h.key("Ctrl+R");
    h.settle_until(|app| app.folder_text[0].is_some()).await;
    let screen = h.screen();
    let title = screen.lines().position(|l| l.contains("Holiday photos")).expect(&screen);
    let header = screen.lines().position(|l| l.starts_with("│Name")).expect(&screen);
    assert!(title < header, "the text is above the column headings:\n{screen}");
    assert!(screen.contains("Sorted by month"), "{screen}");
    // Rows are still found under the mouse: the first entry is the same.
    assert_eq!(h.app.note_rows[0], 5, "four rows of text and a rule");
}

#[tokio::test]
async fn an_archive_shows_its_files_not_its_bytes() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Shift+F5");
    h.key("Enter");
    h.settle().await;
    h.key("Tab");
    h.focus("docs.zip");
    h.key("F3");
    h.settle_until(|app| matches!(app.overlay(), Some(Overlay::Viewer(v)) if v.rich.is_some()))
        .await;
    let screen = h.screen();
    assert!(screen.contains("Contents") && screen.contains("readme.md"), "{screen}");
    // Below the title, whose temporary folder has a random name.
    assert!(!screen.lines().skip(1).any(|l| l.contains("PK")), "not the bytes: {screen}");
    h.key("Esc");

    // The quick view shows the same.
    h.key("Ctrl+Q");
    h.settle_until(|app| matches!(&app.quick_view, Some(Some(v)) if v.rich.is_some())).await;
    assert!(h.screen().contains("readme.md"), "{}", h.screen());
}

#[cfg(feature = "scripting")]
#[tokio::test]
async fn plugins_add_columns_and_previewers() {
    let mut h = Harness::new().await;
    let plugin = h.app.context().paths.plugins_dir().join("lens");
    fs::create_dir_all(&plugin).unwrap();
    let manifest = r#"{
        "name": "lens",
        "version": "1.0.0",
        "columns": [{ "id": "chars", "title": "Chars", "match": ["*.txt"], "width": 6 }],
        "previewers": [{ "id": "shout", "title": "Shout", "match": ["*.txt"] }],
        "permissions": { "read": ["$SELECTION"] }
    }"#;
    fs::write(plugin.join("plugin.json"), manifest).unwrap();
    fs::write(
        plugin.join("main.ts"),
        r##"const command = pnxr.context!.command;
const files = pnxr.context!.selection;
if (command === "column:chars") {
  for (const file of files) pnxr.print(String((await pnxr.readText(file)).length));
} else if (command === "preview:shout") {
  pnxr.print("# Shouted\n\n" + (await pnxr.readText(files[0])).toUpperCase());
}"##,
    )
    .unwrap();

    // Nothing runs before the user allowed the plugin.
    h.key("Ctrl+R");
    h.settle().await;
    assert!(h.app.plugin_columns(SideId::Left).is_empty());
    assert!(!h.screen().contains("Chars"));

    let grants = h.app.context().paths.plugin_grants_file();
    fs::create_dir_all(grants.parent().unwrap()).unwrap();
    fs::write(&grants, r#"{ "lens": { "read": ["$SELECTION"] } }"#).unwrap();
    h.key("Ctrl+R");
    h.settle_until(|app| !app.plugin_columns(SideId::Left).is_empty()).await;
    let screen = h.screen();
    assert!(screen.contains("Chars"), "{screen}");
    save_screen("plugin-column", &screen);
    // "alpha\n" is 6 characters, "the needle is here\n" 19.
    let row = |name: &str| screen.lines().find(|l| l.contains(name)).unwrap().to_owned();
    assert!(row("a.txt").contains(" 6 "), "{}", row("a.txt"));
    assert!(row("notes.txt").contains("19"), "{}", row("notes.txt"));
    // (a bare "19" could come from the time of the folder)
    assert!(!row("docs").contains(" 19 "), "folders have no value");

    // The previewer makes the text of the viewer.
    h.focus("a.txt");
    h.key("F3");
    h.settle_until(|app| matches!(app.overlay(), Some(Overlay::Viewer(v)) if v.rich.is_some()))
        .await;
    let screen = h.screen();
    assert!(screen.contains("Shouted") && screen.contains("ALPHA"), "{screen}");
    save_screen("plugin-previewer", &screen);
}

/// the F1 help keeps inside its border and leaves a gap between its
/// two columns on every screen size, down to a small terminal.
#[tokio::test]
async fn the_help_fits_every_screen_size() {
    let mut h = Harness::new().await;
    h.key("F1");
    for (w, rows) in [(80u16, 24u16), (60, 20), (100, 30), (140, 50), (40, 12)] {
        let mut terminal = Terminal::new(TestBackend::new(w, rows)).unwrap();
        terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
        let buffer = terminal.backend().buffer();
        let lines: Vec<String> = (0..rows)
            .map(|y| (0..w).map(|x| buffer[(x, y)].symbol().to_owned()).collect())
            .collect();
        let screen = lines.join("\n");
        let top = lines.iter().position(|l| l.contains("Main shortcuts")).expect(&screen);
        let bottom = lines.iter().position(|l| l.contains("Esc: close")).expect(&screen);
        assert!(bottom > top, "{w}x{rows}:\n{screen}");
        let first = lines[top].chars().position(|c| c == '┌').unwrap();
        for line in &lines[top + 1..bottom] {
            let chars: Vec<char> = line.chars().collect();
            assert_eq!(chars[first], '│', "{w}x{rows} left border:\n{screen}");
            assert!(chars[first + 1..].contains(&'│'), "{w}x{rows} right border:\n{screen}");
        }
        // No label touches the next column: keys and labels are cut with …
        assert!(!screen.contains("Delete (to…Pairnox"), "{w}x{rows}:\n{screen}");
    }
}

/// The key row of the open dialog, as the window gets it from the screen
/// description (the terminal draws the same text).
fn dialog_hint(h: &mut Harness) -> Option<String> {
    use pnxr_frontend::view::{OverlayView, Screen};
    let screen = Screen::of(&mut h.app, 20, 30);
    if let Some(conflict) = screen.conflict {
        return Some(conflict.hint);
    }
    Some(match screen.overlay? {
        OverlayView::Help { hint, .. } | OverlayView::Jobs { hint, .. } => hint,
        OverlayView::Commands(view) => view.hint,
        OverlayView::Input { hint, .. }
        | OverlayView::Confirm { hint, .. }
        | OverlayView::Message { hint, .. } => hint,
        OverlayView::Choose { footer, .. } => footer,
        OverlayView::Viewer(view) => view.keys,
        OverlayView::Settings(view) => view.hint,
    })
}

/// Every dialog ends its key row with Esc, keeps one convention, and the
/// terminal shows the same row as the window (first and last parts, the
/// ones that stay when the row is shortened).
#[tokio::test]
async fn every_dialog_has_a_key_row_in_one_convention() {
    let mut h = Harness::new().await;
    fs::write(h.path("left/pack.zip"), b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    let cases: &[(&str, &[&str])] = &[
        ("help", &["F1"]),
        ("commands", &["F9"]),
        ("settings", &["Ctrl+,"]),
        ("jobs", &["Ctrl+J"]),
        ("copy", &["F5"]),
        ("move", &["F6"]),
        ("mkdir", &["F7"]),
        ("rename", &["F2"]),
        ("delete", &["F8"]),
        ("delete forever", &["Shift+F8"]),
        ("search", &["Ctrl+F2"]),
        ("drives", &["Shift+F1"]),
        ("bookmarks", &["Ctrl+D"]),
        ("properties", &["Shift+F4"]),
        ("pack", &["Shift+F5"]),
        ("copy path", &["Ctrl+P"]),
        ("link", &["Shift+F7"]),
        ("disks", &["Ctrl+F8"]),
        ("disk usage", &["Ctrl+F7"]),
        ("viewer", &["F3"]),
        ("select mask", &["+"]),
        ("quick filter", &["Ctrl+S"]),
    ];
    for (name, keys) in cases {
        h.focus("a.txt");
        for key in *keys {
            h.key(key);
            h.settle().await;
        }
        let Some(hint) = dialog_hint(&mut h) else { panic!("{name}: no dialog opened") };
        pnxr_frontend::hints::check(&hint).unwrap_or_else(|e| panic!("{name}: {e}"));
        let screen = h.screen();
        save_screen(&format!("dialog-{}", name.replace(' ', "-")), &screen);
        let parts: Vec<&str> = hint.split(pnxr_frontend::hints::SEP).collect();
        let (first, last) = (parts[0], parts[parts.len() - 1]);
        assert!(screen.contains(first) && screen.contains(last), "{name}: {hint}\n{screen}");
        for _ in 0..3 {
            h.key("Esc");
            h.settle().await;
        }
    }
}

#[tokio::test]
async fn the_conflict_question_has_the_shared_key_row() {
    let mut h = Harness::new().await;
    fs::write(h.path("right/a.txt"), "other").unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("a.txt");
    h.key("F5");
    h.key("Enter");
    h.settle_until(|app| app.pending_conflict().is_some()).await;
    let hint = dialog_hint(&mut h).unwrap();
    pnxr_frontend::hints::check(&hint).unwrap();
    let screen = h.screen();
    let parts: Vec<&str> = hint.split(pnxr_frontend::hints::SEP).collect();
    assert!(screen.contains(parts[0]) && screen.contains(parts[parts.len() - 1]), "{screen}");
}

/// The install, connection-test and advanced-pack dialogs, and the plugin
/// list and its permission question, follow the key-row convention.
#[tokio::test]
async fn install_test_pack_and_plugin_dialogs_follow_the_convention() {
    let mut h = Harness::new().await;
    let plugin = h.app.context().paths.plugins_dir().join("lens");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("plugin.json"),
        r#"{ "name": "lens", "version": "1.0.0",
             "commands": [{ "id": "hi", "title": "Say hi" }],
             "permissions": { "read": ["$SELECTION"] } }"#,
    )
    .unwrap();
    fs::write(plugin.join("main.ts"), "pnxr.print('hi');").unwrap();
    let steps: &[(&str, &[&str])] = &[
        ("install-question", &["F9", "i", "n", "s", "t", "a", "l", "l", "Enter"]),
        ("install-help", &["F1"]),
        ("install-test", &["F9", "i", "n", "s", "t", "a", "l", "l", "Enter"]),
        ("install-confirm", &["F9", "i", "n", "s", "t", "a", "l", "l", "Enter"]),
        ("plugins", &["Ctrl+X"]),
        ("plugin-allow", &["Ctrl+X", "Enter"]),
        ("pack-format", &["Shift+F5", "Ctrl+O"]),
        ("pack-level", &["Shift+F5", "Ctrl+O", "Enter"]),
        ("pack-password", &["Shift+F5", "Ctrl+O", "Enter", "Enter"]),
        ("connection-test", &["Shift+F1", "End", "Enter"]),
    ];
    for (name, keys) in steps {
        // The install help is a step on from the install question.
        if *name != "install-help" {
            h.focus("a.txt");
        }
        for key in *keys {
            h.key(key);
            h.settle().await;
        }
        if *name == "install-test" {
            h.type_text("me@127.0.0.1");
            h.key("Ctrl+T");
            h.settle().await;
        }
        if *name == "install-confirm" {
            h.type_text("me@127.0.0.1");
            h.key("Enter");
            h.settle().await;
        }
        if *name == "connection-test" {
            h.type_text("sftp://me@127.0.0.1:2/x");
            h.key("Ctrl+T");
            h.settle().await;
        }
        let Some(hint) = dialog_hint(&mut h) else { continue };
        let screen = h.screen();
        save_screen(&format!("dialog-{name}"), &screen);
        if cfg!(feature = "scripting") || !name.starts_with("plugin") {
            pnxr_frontend::hints::check(&hint).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        if *name != "install-question" {
            for _ in 0..4 {
                h.key("Esc");
                h.settle().await;
            }
        }
    }
}

/// A message longer than the screen is read from its start: the keys scroll
/// it, what is out of sight is counted and the key row stays.
#[tokio::test]
async fn a_long_message_scrolls_with_the_keys() {
    let mut h = Harness::new().await;
    h.app.perform(Action::InstallServe);
    h.key("F1");
    let small = |h: &mut Harness| {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|frame| draw(frame, &mut h.app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..20)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol().to_owned()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let screen = small(&mut h);
    assert!(screen.contains("Install pnxr serve on a server"), "{screen}");
    assert!(screen.contains("more lines") && !screen.contains("earlier lines"), "{screen}");
    assert!(screen.contains("PgUp/PgDn: scroll"), "{screen}");
    assert!(screen.contains("Esc: close"), "{screen}");
    // Paragraphs break between words.
    assert!(
        screen.contains("lets this computer (and") && screen.contains("│other servers)"),
        "{screen}"
    );
    h.key("PageDown");
    let screen = small(&mut h);
    assert!(screen.contains("earlier lines"), "{screen}");
    // End stops at the last line, so one step up moves at once.
    h.key("End");
    let end = small(&mut h);
    assert!(end.contains("with F4).") && !end.contains("more lines"), "{end}");
    h.key("Up");
    assert!(small(&mut h).contains("more lines"));
    h.key("Home");
    assert!(small(&mut h).contains("Install pnxr serve on a server"));
    // The lines under the answer scroll too, and start from the top again
    // once something is typed.
    h.key("Enter");
    small(&mut h);
    h.key("PageDown");
    let screen = small(&mut h);
    assert!(screen.contains("earlier lines") && screen.contains("Esc: cancel"), "{screen}");
    h.type_text("x");
    assert_eq!(h.app.dialog_scroll, 0);
}

#[tokio::test]
async fn plugin_results_are_sorted_marked_and_deleted_after_a_question() {
    let mut h = Harness::new().await;
    let (a, notes) = (h.path("left/a.txt"), h.path("left/notes.txt"));
    let json = format!(
        r##"{{"title":"Similar pictures","columns":["Group","Match","File"],
        "actions":[{{"key":"m","title":"move aside","command":"move-marked"}}],
        "rows":[
          {{"path":{a:?},"cells":["#1","best","a.txt"],"values":[1,101],"keep":true}},
          {{"path":{notes:?},"cells":["#1","95%","notes.txt"],"values":[1,95]}}]}}"##
    );
    let lines = vec!["a report".to_owned(), format!("{}{json}", pnxr_frontend::results::MARKER)];
    h.app.handle_event(AppEvent::ScriptDone {
        title: "Plugin".into(),
        lines,
        error: None,
        plugin: Some(h.path("plugin")),
    });
    let screen = h.screen();
    assert!(screen.contains("Similar pictures (2 files, sorted by Group ↑)"), "{screen}");
    assert!(screen.contains("[ ] #1     best   a.txt"), "{screen}");
    assert!(screen.contains("m: move aside") && screen.contains("a: extras"), "{screen}");

    // Nothing marked yet: Delete says so and keeps the list.
    h.key("Delete");
    assert!(matches!(h.app.overlay(), Some(Overlay::Choose(_))));
    // a marks the extra copy; s sorts by Match (the lower first).
    h.press(KeyPress::char('a'));
    assert!(h.screen().contains("(2 files, 1 marked, sorted by Group"), "{}", h.screen());
    h.press(KeyPress::char('s'));
    let screen = h.screen();
    let first = screen.find("best   a.txt").unwrap();
    let second = screen.find("95%    notes.txt").unwrap();
    assert!(second < first && screen.contains("sorted by Match ↑"), "sorted: {screen}");
    assert!(
        screen.contains("[x] #1     95%    notes.txt"),
        "the mark stays with its file: {screen}"
    );

    // Delete asks about the marked file only.
    h.key("Delete");
    h.settle().await;
    let Some(Overlay::Confirm(dialog)) = h.app.overlay() else { panic!("confirmation") };
    assert!(dialog.lines[0].contains("notes.txt"), "{:?}", dialog.lines);
    h.press(KeyPress::char('y'));
    h.settle().await;
    assert!(!notes.exists() && a.exists());
}
