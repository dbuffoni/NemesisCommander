//! Drives the terminal application with key presses, exactly as a user
//! would, against real temporary folders, and checks both the resulting
//! files and what is drawn on screen.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use sc_app::AppContext;
use sc_config::{Action, AppPaths, Key};
use sc_ops::ConflictPolicy;
use sc_panel::SideId;
use sc_tui::{App, AppEvent, KeyPress, Overlay, draw};
use sc_vfs::VfsPath;
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

    async fn with_settings(change: impl FnOnce(&mut sc_config::Config)) -> Self {
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
        change(&mut context.config);
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
    assert!(screen.contains("1Help"), "function key bar:\n{screen}");
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
    h.key("Alt+Left");
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
    h.key("Enter");
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
    h.key("Ctrl+M");
    h.type_text("file[C][X]");
    h.key("Enter");
    let Some(Overlay::Confirm(dialog)) = h.app.overlay() else { panic!("preview") };
    assert!(dialog.lines.iter().any(|l| l.contains("a.txt → file1.txt")), "{:?}", dialog.lines);
    h.key("Enter");
    h.settle().await;
    assert!(h.path("left/file1.txt").exists());
    assert!(h.path("left/file2.txt").exists());
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
    // Left from All: the last category (SuperCommander), whose first command is Help.
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
    assert!(h.screen().contains("SuperCommander/Show main shortcuts"));
    h.app.pick("command", 0);
    assert!(matches!(h.app.overlay(), Some(Overlay::Help)), "a click runs the row");
}

#[tokio::test]
async fn typing_in_the_command_list_finds_commands() {
    let mut h = Harness::new().await;
    h.key("F9");
    h.type_text("settings");
    let screen = h.screen();
    assert!(screen.contains("> settings_"), "{screen}");
    assert!(screen.contains("SuperCommander/Settings"), "{screen}");
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
        Some(sc_config::Action::Refresh)
    );
}

#[cfg(feature = "scripting")]
#[tokio::test]
async fn scripts_and_plugin_commands_are_listed_and_take_keys() {
    let mut h = Harness::new().await;
    let scripts = h.app.context().paths.scripts_dir();
    fs::create_dir_all(&scripts).unwrap();
    fs::write(scripts.join("hello.ts"), "sc.print('hello from a script');").unwrap();
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
    fs::write(plugin.join("main.ts"), "sc.print(sc.context!.command);").unwrap();

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
    for (name, data) in sc_preview::samples::all() {
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
    h.key("Alt+F7");
    h.type_text("*.txt");
    h.key("Enter");
    h.type_text("NEEDLE");
    h.key("Enter");
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
    h.key("Alt+C");
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
    h.key("Alt+C");
    h.settle().await;
    assert_eq!(h.status(), "Files are identical");
}

#[tokio::test]
async fn pack_and_browse_the_archive() {
    let mut h = Harness::new().await;
    h.focus("docs");
    h.key("Alt+F5");
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
    h.key("Alt+F5");
    h.type_text(&h.path("left/docs.zip").display().to_string());
    h.key("Enter");
    h.settle().await;
    h.focus("docs.zip");
    h.key("Alt+F9");
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/docs/readme.md")).unwrap(), "# readme\n");
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
    h.key("Esc");
    h.key("Alt+Enter");
    let Some(Overlay::Message(message)) = h.app.overlay() else { panic!("properties") };
    assert!(message.lines.iter().any(|l| l.contains("6 bytes")));
}

#[cfg(unix)]
#[tokio::test]
async fn permissions_can_be_changed() {
    use std::os::unix::fs::PermissionsExt;
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Alt+Enter");
    h.key("Enter");
    h.type_text("600");
    h.key("Enter");
    h.settle().await;
    let mode = fs::metadata(h.path("left/a.txt")).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn settings_export_and_import() {
    let mut h = Harness::new().await;
    h.key("Ctrl+Shift+D");
    let file = h.path("exported.toml").display().to_string();
    h.app.perform(Action::ExportConfig);
    h.type_text(&file);
    h.key("Enter");
    assert!(fs::read_to_string(&file).unwrap().contains("supercommander-settings"));

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
        const names = (await sc.list(sc.context!.panel!)).map((e) => e.name).sort();
        await sc.writeText("listing.txt", names.join(","));
        sc.print(`listed ${names.length}`);
        sc.print("second line");
    "#;
    fs::write(h.path("left/list.ts"), script).unwrap();
    h.key("Ctrl+R");
    h.settle().await;
    h.focus("list.ts");
    h.key("Alt+X");
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
        const ctx = sc.context!;
        for (const path of ctx.selection) {
          const text = await sc.readText(path);
          await sc.writeText(sc.join(ctx.other!, sc.basename(path)), text.toUpperCase());
        }
        sc.print(`${ctx.selection.length} copied`);
    "#;
    fs::write(plugin.join("main.ts"), main).unwrap();

    h.focus("a.txt");
    h.key("Alt+X");
    let Some(Overlay::Choose(dialog)) = h.app.overlay() else { panic!("a list") };
    assert_eq!(dialog.items, ["upper: Uppercase copy to the other panel"]);
    h.key("Enter");
    let Some(Overlay::Confirm(confirm)) = h.app.overlay() else { panic!("a permission question") };
    assert!(confirm.lines.iter().any(|l| l.contains("the selected files")), "{:?}", confirm.lines);
    h.key("Enter");
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/a.txt")).unwrap(), "ALPHA\n");
    assert_eq!(h.status(), "Uppercase copy to the other panel: 1 copied");

    // Allowed now: the second run asks nothing.
    h.focus("notes.txt");
    h.key("Alt+X");
    h.key("Enter");
    assert!(h.app.overlay().is_none(), "no second question");
    h.settle().await;
    assert_eq!(fs::read_to_string(h.path("right/notes.txt")).unwrap(), "THE NEEDLE IS HERE\n");
}

#[tokio::test]
async fn without_plugins_alt_x_says_how_to_add_one() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Alt+X");
    assert!(h.app.overlay().is_none());
    assert!(h.status().contains("plugin") || h.status().contains("Plugins"), "{}", h.status());
}

/// Moves the settings screen's cursor to the row whose label starts with
/// `label`.
fn settings_row(h: &mut Harness, label: &str) {
    h.key("Home");
    for _ in 0..200 {
        if let Some(Overlay::Settings(screen)) = h.app.overlay() {
            let view = sc_frontend::view::settings_view(&h.app, screen, 1000);
            if view.rows[view.cursor].label.starts_with(label) {
                return;
            }
        }
        h.key("Down");
    }
    let labels: Vec<String> = match h.app.overlay() {
        Some(Overlay::Settings(screen)) => sc_frontend::view::settings_view(&h.app, screen, 1000)
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
    assert!(screen.contains("General") && screen.contains("Enter / ← →: change"), "{screen}");

    settings_row(&mut h, "Show hidden files");
    h.key("Enter");
    assert!(h.app.context().config.panels.show_hidden);
    settings_row(&mut h, "Check after copying");
    h.key("Right");
    assert_eq!(h.app.context().config.transfers.verify, sc_ops::VerifyMode::Fast);

    // Numbers are typed; Esc or Enter come back to the screen.
    settings_row(&mut h, "Jobs at the same time");
    h.key("Enter");
    h.type_text("1");
    h.key("Enter");
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))), "back on the screen");
    assert_eq!(h.app.context().config.transfers.parallel_jobs, 1);
    let saved = fs::read_to_string(h.app.context().paths.config_file()).unwrap();
    assert!(saved.contains("parallel_jobs = 1") && saved.contains("show_hidden = true"), "{saved}");

    // r puts the default back.
    h.press(KeyPress::char('r'));
    assert_eq!(h.app.context().config.transfers.parallel_jobs, 2);
    h.key("Esc");
    assert!(h.app.overlay().is_none());
}

#[tokio::test]
async fn the_settings_screen_records_shortcuts() {
    let mut h = Harness::new().await;
    h.focus("a.txt");
    h.key("Ctrl+,");
    settings_row(&mut h, "Refresh");
    h.key("Enter");
    assert!(h.screen().contains("Press the new shortcut"), "{}", h.screen());
    h.key("F5");
    assert!(h.status().contains("taken from Files/Copy"), "{}", h.status());
    assert_eq!(h.app.context().config.keys["view.refresh"], ["F5"]);
    h.key("Esc");
    // F5 now refreshes; Copy has no key left.
    h.key("F5");
    assert!(h.app.overlay().is_none(), "F5 no longer opens the copy question");
    h.key("Ctrl+,");
    settings_row(&mut h, "Copy");
    assert!(h.screen().contains("none"), "{}", h.screen());
    h.press(KeyPress::char('r'));
    h.key("Esc");
    h.key("F5");
    assert!(matches!(h.app.overlay(), Some(Overlay::Input(_))), "F5 copies again");
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
