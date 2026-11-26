//! The `Commander` QML object: the shared [`App`] behind the window.
//!
//! QML forwards every key press and command choice here; after each change the
//! object publishes the whole screen as JSON (see [`nmc_frontend::view`]) in
//! its `screen` property, which the QML files only draw. Background results
//! (listings, transfer progress) arrive on a tokio runtime and are moved to
//! the Qt thread in small batches.

use core::pin::Pin;
use std::collections::VecDeque;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use nmc_app::AppContext;
use nmc_frontend::view::Screen;
use nmc_frontend::{App, AppEvent, External, KeyPress, SideId, resolve_location};
use nmc_vfs::VfsPath;
use tokio::runtime::Runtime;

use crate::ffi::qobject;

/// What the window starts with: the two locations given on the command line.
#[derive(Debug, Default, Clone)]
pub struct Startup {
    /// Left panel location, as typed.
    pub left: Option<String>,
    /// Right panel location, as typed.
    pub right: Option<String>,
    /// Run [`SelfTest`] instead of waiting for the user.
    pub self_test: Option<SelfTest>,
    /// The window's size in pixels when it opens (default 1200×760).
    pub size: Option<(i32, i32)>,
}

/// A scripted run, for tests and screenshots: once the listings are loaded,
/// press `keys` one at a time (each time waiting for the work it starts),
/// then print the screen as JSON, save a picture of the window when asked,
/// and quit.
#[derive(Debug, Default, Clone)]
pub struct SelfTest {
    /// Key chords in key-map syntax (`F1`, `Ctrl+Q`), pressed in order.
    pub keys: VecDeque<String>,
    /// Where to save a PNG picture of the window at the end.
    pub screenshot: Option<String>,
}

/// Set by `main` before the QML engine creates the [`qobject::Commander`].
pub static STARTUP: OnceLock<Startup> = OnceLock::new();

/// The window called [`qobject::Commander::start`]: it loaded.
pub static STARTED: AtomicBool = AtomicBool::new(false);

/// The application ended while the window was still loading (an error, or
/// the self test), before Qt's event loop could be asked to quit.
pub static ENDED_EARLY: AtomicBool = AtomicBool::new(false);

/// The Rust side of [`qobject::Commander`].
pub struct CommanderRust {
    pub(crate) screen: QString,
    pub(crate) running: bool,
    /// Text for the system clipboard, and a number that grows with each, so
    /// QML notices a repeat.
    pub(crate) clipboard: QString,
    pub(crate) clipboard_seq: i32,
    /// Grows when the window should paste the clipboard (the self test has
    /// no real key press to do it).
    pub(crate) paste_seq: i32,
    pub(crate) screenshot: QString,
    /// The window's size when it opens.
    pub(crate) window_width: i32,
    pub(crate) window_height: i32,
    test: Option<SelfTest>,
    runtime: Option<Runtime>,
    app: Option<App>,
    rows: usize,
    viewer_rows: usize,
    /// Characters that fit across the viewer, for laying out documents.
    columns: usize,
    /// The window has reported its real size.
    sized: bool,
    /// The last published screen, to skip unchanged updates.
    last: String,
}

/// The window's size at start: `--size`, else the one remembered from the
/// last run, else 1200×760.
fn initial_window_size() -> (i32, i32) {
    if let Some(size) = STARTUP.get().and_then(|s| s.size) {
        return size;
    }
    nmc_config::AppPaths::detect().load_window_size().unwrap_or((1200, 760))
}

impl Default for CommanderRust {
    fn default() -> Self {
        CommanderRust {
            screen: QString::default(),
            running: true,
            clipboard: QString::default(),
            clipboard_seq: 0,
            paste_seq: 0,
            screenshot: QString::default(),
            window_width: initial_window_size().0,
            window_height: initial_window_size().1,
            test: None,
            runtime: None,
            app: None,
            rows: 20,
            viewer_rows: 20,
            columns: 100,
            sized: false,
            last: String::new(),
        }
    }
}

impl CommanderRust {
    /// Runs `change` on the application inside the runtime, which the
    /// application needs to start background work.
    fn with_app(&mut self, change: impl FnOnce(&mut App)) {
        if let (Some(runtime), Some(app)) = (&self.runtime, &mut self.app) {
            let _inside = runtime.enter();
            change(app);
        }
    }
}

impl qobject::Commander {
    pub(crate) fn start(mut self: Pin<&mut Self>, fonts: &QString) {
        if self.rust().app.is_some() {
            return;
        }
        STARTED.store(true, Ordering::SeqCst);
        let startup = STARTUP.get().cloned().unwrap_or_default();
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(e) => return self.fail(&format!("cannot start: {e}")),
        };
        let created = {
            let _inside = runtime.enter();
            create_app(&startup)
        };
        let (mut app, mut events) = match created {
            Ok(created) => created,
            Err(e) => return self.fail(&e),
        };
        app.set_fonts(fonts.to_string().lines().map(str::to_owned).collect());

        // Background results go to the Qt thread in batches, at most about
        // sixty times a second, so a fast transfer cannot flood the window.
        let thread = self.qt_thread();
        runtime.spawn(async move {
            while let Some(first) = events.recv().await {
                let mut batch = vec![first];
                while let Ok(event) = events.try_recv() {
                    batch.push(event);
                }
                if thread.queue(move |this| this.apply(batch)).is_err() {
                    break; // the window is gone
                }
                tokio::time::sleep(Duration::from_millis(16)).await;
            }
        });

        let mut rust = self.as_mut().rust_mut();
        rust.runtime = Some(runtime);
        rust.app = Some(app);
        rust.test = startup.self_test;
        self.as_mut().refresh();
    }

    /// Remembers the window's size for the next start (not for self tests
    /// and not when `--size` forced one).
    pub(crate) fn remember_window(self: Pin<&mut Self>, width: i32, height: i32) {
        if self.rust().test.is_some() || STARTUP.get().is_some_and(|s| s.size.is_some()) {
            return;
        }
        let _ = nmc_config::AppPaths::detect().save_window_size(width, height);
    }

    pub(crate) fn quit(mut self: Pin<&mut Self>) {
        self.as_mut().set_running(false);
    }

    pub(crate) fn key(mut self: Pin<&mut Self>, key: i32, text: &QString, modifiers: i32) {
        if let Some(press) = crate::keys::key_press(key, &text.to_string(), modifiers) {
            self.as_mut().rust_mut().with_app(|app| app.handle_key(press));
            self.refresh();
        }
    }

    pub(crate) fn paste(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        self.as_mut().rust_mut().with_app(|app| app.paste(&text));
        self.refresh();
    }

    pub(crate) fn modifiers(mut self: Pin<&mut Self>, modifiers: i32) {
        use crate::keys::{ALT, CONTROL, SHIFT};
        self.as_mut().rust_mut().with_app(|app| {
            app.set_modifiers(
                modifiers & CONTROL != 0,
                modifiers & ALT != 0,
                modifiers & SHIFT != 0,
            )
        });
        self.refresh();
    }

    pub(crate) fn perform(mut self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.as_mut().rust_mut().with_app(|app| {
            app.run_command(&id);
        });
        self.refresh();
    }

    pub(crate) fn pick(mut self: Pin<&mut Self>, what: &QString, index: i32) {
        let what = what.to_string();
        self.as_mut().rust_mut().with_app(|app| app.pick(&what, index.max(0) as usize));
        self.refresh();
    }

    pub(crate) fn click_row(mut self: Pin<&mut Self>, side: i32, row: i32) {
        self.as_mut().rust_mut().with_app(|app| app.click_row(side_of(side), row.max(0) as usize));
        self.refresh();
    }

    pub(crate) fn scroll_rows(mut self: Pin<&mut Self>, side: i32, lines: i32) {
        self.as_mut().rust_mut().with_app(|app| app.scroll_rows(side_of(side), lines as isize));
        self.refresh();
    }

    pub(crate) fn dialog_scrolled(mut self: Pin<&mut Self>, rows: i32) {
        self.as_mut().rust_mut().with_app(|app| app.dialog_scroll = rows.max(0) as usize);
        self.refresh();
    }

    pub(crate) fn drag_rows(mut self: Pin<&mut Self>, from: i32, row: i32, to: i32) {
        self.as_mut()
            .rust_mut()
            .with_app(|app| app.drag_rows(side_of(from), row.max(0) as usize, side_of(to)));
        self.refresh();
    }

    pub(crate) fn drop_files(mut self: Pin<&mut Self>, side: i32, urls: &QString) {
        let files: Vec<std::path::PathBuf> = urls
            .to_string()
            .lines()
            .filter(|url| url.starts_with("file://"))
            .map(|url| std::path::PathBuf::from(local_path_of(url)))
            .collect();
        self.as_mut().rust_mut().with_app(|app| app.drop_files(side_of(side), &files));
        self.refresh();
    }

    pub(crate) fn resize(mut self: Pin<&mut Self>, rows: i32, viewer_rows: i32, columns: i32) {
        let mut rust = self.as_mut().rust_mut();
        rust.rows = rows.max(1) as usize;
        rust.viewer_rows = viewer_rows.max(1) as usize;
        rust.columns = columns.max(20) as usize;
        rust.sized |= rows > 1;
        self.refresh();
    }

    /// Applies background results.
    fn apply(mut self: Pin<&mut Self>, events: Vec<AppEvent>) {
        self.as_mut().rust_mut().with_app(|app| {
            for event in events {
                app.handle_event(event);
            }
        });
        self.refresh();
    }

    /// The external program (the editor) finished.
    fn external_done(mut self: Pin<&mut Self>, external: External, result: Result<(), String>) {
        self.as_mut().rust_mut().with_app(|app| app.after_external(&external, result));
        self.refresh();
    }

    /// Publishes the screen, starts a requested editor and quits when asked.
    fn refresh(mut self: Pin<&mut Self>) {
        let mut rust = self.as_mut().rust_mut();
        let (rows, viewer_rows, columns) = (rust.rows, rust.viewer_rows, rust.columns);
        let Some(app) = rust.app.as_mut() else { return };
        app.columns = columns;
        let running = app.is_running();
        let external = app.take_external();
        let clipboard = app.take_clipboard();
        let json = serde_json::to_string(&Screen::of(app, rows, viewer_rows)).unwrap_or_default();
        let changed = json != rust.last;
        if changed {
            rust.last = json;
        }
        if let Some(text) = clipboard {
            let seq = self.rust().clipboard_seq + 1;
            self.as_mut().set_clipboard(QString::from(&text));
            self.as_mut().set_clipboard_seq(seq);
        }
        if let Some(external) = external {
            let thread = self.qt_thread();
            std::thread::spawn(move || {
                let result = run_external(&external);
                let _ = thread.queue(move |this| this.external_done(external, result));
            });
        }
        if changed {
            let screen = QString::from(&self.rust().last);
            self.as_mut().set_screen(screen);
        }
        if !running {
            self.as_mut().set_running(false);
        }
        self.advance_test();
    }

    /// The next step of the self test, once the application is idle.
    fn advance_test(mut self: Pin<&mut Self>) {
        let mut rust = self.as_mut().rust_mut();
        // A copy waiting for the answer to a conflict question is busy, but
        // it is as settled as it will get: the picture shows the question.
        let idle = rust.sized
            && rust
                .app
                .as_ref()
                .is_some_and(|app| !app.is_busy() || app.pending_conflict().is_some());
        if !idle {
            return;
        }
        let Some(test) = rust.test.as_mut() else { return };
        if let Some(chord) = test.keys.pop_front() {
            if chord == "Ctrl+V" {
                // The window reads the clipboard, as for the real key.
                let seq = rust.paste_seq + 1;
                self.as_mut().set_paste_seq(seq);
                return self.refresh();
            }
            match chord.parse::<nmc_config::KeyChord>() {
                Ok(_) => {
                    let press = KeyPress::chord(&chord);
                    rust.with_app(|app| app.handle_key(press));
                }
                Err(e) => eprintln!("nmc-gui: self test: {e}"),
            }
            return self.refresh();
        }
        let screenshot = rust.test.take().and_then(|test| test.screenshot);
        println!("{}", rust.last);
        match screenshot {
            // The window saves the picture, then calls `quit`.
            Some(path) => self.as_mut().set_screenshot(QString::from(&path)),
            None => self.as_mut().set_running(false),
        }
    }

    fn fail(mut self: Pin<&mut Self>, message: &str) {
        eprintln!("nmc-gui: {message}");
        ENDED_EARLY.store(true, Ordering::SeqCst);
        self.as_mut().set_running(false);
    }
}

/// Loads the configuration and creates the application on the startup
/// locations (the current folder when none are given).
fn create_app(
    startup: &Startup,
) -> Result<(App, tokio::sync::mpsc::UnboundedReceiver<AppEvent>), String> {
    let context = AppContext::load().map_err(|e| e.to_string())?;
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let here = VfsPath::from_local(here).map_err(|e| e.to_string())?;
    let place = |text: &Option<String>| match text {
        Some(text) => resolve_location(text, &here),
        None => Ok(here.clone()),
    };
    let (left, right) = (place(&startup.left)?, place(&startup.right)?);
    let (mut app, events) = App::new(context, left, right)?;
    app.graphical = true;
    Ok((app, events))
}

/// Runs an external program (the editor) and waits for it.
fn run_external(external: &External) -> Result<(), String> {
    let (program, args) = external.command.split_first().ok_or("no command")?;
    let status =
        std::process::Command::new(program).args(args).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("exited with {status}")) }
}

/// Side 0 is the left panel, anything else the right one.
fn side_of(side: i32) -> SideId {
    if side == 0 { SideId::Left } else { SideId::Right }
}

/// The local path of a `file:` URL from a file picker (anything else is
/// returned unchanged).
fn local_path_of(url: &str) -> String {
    let Some(rest) = url.strip_prefix("file://") else { return url.to_owned() };
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                i += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    let path = String::from_utf8_lossy(&decoded).into_owned();
    // file:///C:/Users/me → C:/Users/me
    match path.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => path[1..].to_owned(),
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::local_path_of;

    #[test]
    fn picked_files_become_local_paths() {
        assert_eq!(local_path_of("file:///home/me/.ssh/my%20key.pem"), "/home/me/.ssh/my key.pem");
        assert_eq!(local_path_of("file:///C:/Users/me/k%C3%A8y.pem"), "C:/Users/me/kèy.pem");
        assert_eq!(local_path_of("/plain/path"), "/plain/path");
        assert_eq!(local_path_of("file:///odd%zz"), "/odd%zz");
    }
}
