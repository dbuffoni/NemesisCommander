//! The `Commander` QML object: the shared [`App`] behind the window.
//!
//! QML forwards every key press and menu choice here; after each change the
//! object publishes the whole screen as JSON (see [`sc_frontend::view`]) in
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
use sc_app::AppContext;
use sc_config::Action;
use sc_frontend::view::{Screen, menus};
use sc_frontend::{App, AppEvent, External, KeyPress, resolve_location};
use sc_vfs::VfsPath;
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
    pub(crate) menus: QString,
    pub(crate) running: bool,
    pub(crate) screenshot: QString,
    test: Option<SelfTest>,
    runtime: Option<Runtime>,
    app: Option<App>,
    rows: usize,
    viewer_rows: usize,
    /// The window has reported its real size.
    sized: bool,
    /// The last published screen, to skip unchanged updates.
    last: String,
}

impl Default for CommanderRust {
    fn default() -> Self {
        CommanderRust {
            screen: QString::default(),
            menus: QString::default(),
            running: true,
            screenshot: QString::default(),
            test: None,
            runtime: None,
            app: None,
            rows: 20,
            viewer_rows: 20,
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
    pub(crate) fn start(mut self: Pin<&mut Self>) {
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
        let (app, mut events) = match created {
            Ok(created) => created,
            Err(e) => return self.fail(&e),
        };

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

        let menus = serde_json::to_string(&menus(&app)).unwrap_or_default();
        let mut rust = self.as_mut().rust_mut();
        rust.runtime = Some(runtime);
        rust.app = Some(app);
        rust.test = startup.self_test;
        self.as_mut().set_menus(QString::from(&menus));
        self.as_mut().refresh();
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

    pub(crate) fn perform(mut self: Pin<&mut Self>, id: &QString) {
        if let Some(action) = Action::from_id(&id.to_string()) {
            self.as_mut().rust_mut().with_app(|app| app.perform(action));
            self.refresh();
        }
    }

    pub(crate) fn resize(mut self: Pin<&mut Self>, rows: i32, viewer_rows: i32) {
        let mut rust = self.as_mut().rust_mut();
        rust.rows = rows.max(1) as usize;
        rust.viewer_rows = viewer_rows.max(1) as usize;
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
        let (rows, viewer_rows) = (rust.rows, rust.viewer_rows);
        let Some(app) = rust.app.as_mut() else { return };
        let running = app.is_running();
        let external = app.take_external();
        let json = serde_json::to_string(&Screen::of(app, rows, viewer_rows)).unwrap_or_default();
        let changed = json != rust.last;
        if changed {
            rust.last = json;
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
        let idle = rust.sized && rust.app.as_ref().is_some_and(|app| !app.is_busy());
        if !idle {
            return;
        }
        let Some(test) = rust.test.as_mut() else { return };
        if let Some(chord) = test.keys.pop_front() {
            match chord.parse() {
                Ok(chord) => {
                    let press = KeyPress { chord, text: None };
                    rust.with_app(|app| app.handle_key(press));
                }
                Err(e) => eprintln!("sc-gui: self test: {e}"),
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
        eprintln!("sc-gui: {message}");
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
