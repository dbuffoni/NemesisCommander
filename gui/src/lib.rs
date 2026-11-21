//! # nmc-gui
//!
//! NemesisCommander's desktop interface: the same two panels, dialogs and
//! commands as the terminal interface, in a Qt 6 window. There are no
//! toolbars or icons: every command is in the Commands list (F9, with its
//! shortcut) and on the keyboard, and F1 lists the main shortcuts.
//!
//! The application itself is the shared [`nmc_frontend::App`]:
//!
//! * `ffi` and `bridge`: the `Commander` object that QML talks to.
//! * `keys`: Qt key events to key presses.
//! * `qml/`: the window, which only draws the screen description.
//!
//! Usage: `nmc-gui [LEFT] [RIGHT]`, where each location is a folder or a URL
//! such as `sftp://host/path`.

mod bridge;
// Generated C++ glue, see the module.
#[allow(unsafe_code)]
mod ffi;
pub mod keys;

use std::sync::atomic::Ordering;

pub use bridge::{SelfTest, Startup};
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

/// Opens the window and runs until it is closed. Returns `false` when the
/// window could not be loaded (Qt has printed why).
pub fn run(startup: Startup) -> bool {
    let _ = bridge::STARTUP.set(startup);
    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_application_name(&QString::from("NemesisCommander"));
    }
    for png in nmc_frontend::about::ICON_PNGS {
        ffi::add_window_icon(png);
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/NemesisCommander/qml/Main.qml"));
    }
    // Loading the window starts the application.
    if !bridge::STARTED.load(Ordering::SeqCst) {
        eprintln!("nmc-gui: the window could not be loaded");
        return false;
    }
    if !bridge::ENDED_EARLY.load(Ordering::SeqCst)
        && let Some(app) = app.as_mut()
    {
        app.exec();
    }
    true
}
