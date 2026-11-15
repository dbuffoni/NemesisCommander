//! # sc-tui
//!
//! SuperCommander's terminal interface: two panels with tabs, driven by the
//! keyboard. F1 shows the main shortcuts and F9 opens a menu with every
//! command, so nothing has to be memorized.
//!
//! The crate is split so that everything except the main loop can be tested
//! without a terminal:
//!
//! * `app`: the [`App`] state machine (keys and background results in,
//!   state out).
//! * `dialogs`: the data of every overlay window.
//! * `ui`: drawing with ratatui.
//! * `input`: terminal key events to [`KeyPress`]es, and the text field.
//! * `viewer`: the text and hex viewer.

mod app;
mod dialogs;
mod input;
mod ui;
mod viewer;

use std::io;

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use sc_app::AppContext;
use sc_vfs::VfsPath;

pub use app::{App, AppEvent, External, Status, display_location, resolve_location};
pub use dialogs::{ChoosePurpose, ConfirmPurpose, InputPurpose, Overlay};
pub use input::{KeyPress, TextField};
pub use ui::draw;

/// Runs the terminal interface until the user quits.
///
/// `left` and `right` are the initial locations (the current directory when
/// `None`).
pub async fn run(
    context: AppContext,
    left: Option<VfsPath>,
    right: Option<VfsPath>,
) -> io::Result<()> {
    use std::io::IsTerminal;
    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        return Err(io::Error::other(
            "the terminal UI needs a terminal; see `sc --help` for commands that work in scripts",
        ));
    }
    let here = VfsPath::from_local(std::env::current_dir()?).map_err(io::Error::other)?;
    let left = left.unwrap_or_else(|| here.clone());
    let right = right.unwrap_or(here);
    let (mut app, mut events) = App::new(context, left, right).map_err(io::Error::other)?;

    // `init` switches to the alternate screen in raw mode and restores the
    // terminal if the program panics.
    let mut terminal = ratatui::init();
    let mut keys = EventStream::new();
    let result = loop {
        if let Err(e) = terminal.draw(|frame| draw(frame, &mut app)) {
            break Err(e);
        }
        if !app.is_running() {
            break Ok(());
        }
        tokio::select! {
            key = keys.next() => match key {
                Some(Ok(Event::Key(event))) => {
                    if let Some(press) = KeyPress::from_event(&event) {
                        app.handle_key(press);
                    }
                }
                Some(Ok(_)) => {} // resize and others: just redraw
                Some(Err(e)) => break Err(e),
                None => break Ok(()),
            },
            Some(event) = events.recv() => app.handle_event(event),
        }
        // Apply everything else that is already waiting before redrawing.
        while let Ok(event) = events.try_recv() {
            app.handle_event(event);
        }
        if let Some(external) = app.take_external() {
            ratatui::restore();
            drop(keys);
            let status = run_external(&external);
            terminal = ratatui::init();
            keys = EventStream::new();
            terminal.clear()?;
            app.after_external(&external, status);
        }
    };
    ratatui::restore();
    result
}

/// Runs an external program (the editor) in the foreground.
fn run_external(external: &External) -> Result<(), String> {
    let (program, args) = external.command.split_first().ok_or("no command")?;
    let status =
        std::process::Command::new(program).args(args).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("exited with {status}")) }
}
