//! # nmc-tui
//!
//! NemesisCommander's terminal interface: two panels with tabs, driven by the
//! keyboard. F1 shows the main shortcuts and F9 opens a menu with every
//! command, so nothing has to be memorized.
//!
//! The application itself (panels, dialogs, commands) is the shared
//! [`nmc_frontend::App`]; this crate adds what is specific to a terminal:
//!
//! * `ui`: drawing the state with ratatui.
//! * `terminal`: terminal key events to [`KeyPress`]es.
//! * [`run`]: the main loop, and handing the terminal to an editor.

mod clipboard;
mod mouse;
mod terminal;
mod ui;

use std::io;

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers,
};
use futures::StreamExt;
use nmc_app::AppContext;
use nmc_vfs::VfsPath;

pub use mouse::{Pointer, hit};
pub use nmc_frontend::{
    App, AppEvent, ChoosePurpose, ConfirmPurpose, External, InputPurpose, KeyPress, Overlay,
    PickMode, Status, TextField, display_location, resolve_location,
};
pub use terminal::key_press;
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
    run_app(context, left, right, None).await.map(|_| ())
}

/// Runs the terminal interface as a chooser (`nmc pick`) and returns what
/// the user chose, `None` when they cancelled.
///
/// The interface is drawn on standard error, so that standard output can be
/// a pipe or `$(...)` that receives the answer.
pub async fn pick(
    context: AppContext,
    start: Option<VfsPath>,
    mode: PickMode,
) -> io::Result<Option<Vec<VfsPath>>> {
    run_app(context, start, None, Some(mode)).await
}

/// Where the interface is drawn: standard output, or standard error in pick
/// mode. Switches the terminal to the alternate screen in raw mode.
struct Screen {
    terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<Box<dyn io::Write>>>,
    to_stderr: bool,
}

impl Screen {
    fn writer(to_stderr: bool) -> Box<dyn io::Write> {
        if to_stderr { Box::new(io::stderr()) } else { Box::new(io::stdout()) }
    }

    fn open(to_stderr: bool) -> io::Result<Self> {
        use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
        // Give the terminal back even if the program panics.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            Screen::restore(to_stderr);
            previous(info);
        }));
        enable_raw_mode()?;
        let mut writer = Self::writer(to_stderr);
        crossterm::execute!(writer, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = ratatui::backend::CrosstermBackend::new(writer);
        let terminal = ratatui::Terminal::new(backend)?;
        Ok(Screen { terminal, to_stderr })
    }

    fn restore(to_stderr: bool) {
        use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};
        let mut writer = Self::writer(to_stderr);
        let _ = crossterm::execute!(
            writer,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        let _ = disable_raw_mode();
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        Screen::restore(self.to_stderr);
    }
}

async fn run_app(
    context: AppContext,
    left: Option<VfsPath>,
    right: Option<VfsPath>,
    pick: Option<PickMode>,
) -> io::Result<Option<Vec<VfsPath>>> {
    use std::io::IsTerminal;
    // A chooser draws on standard error, so only that has to be a terminal.
    let drawn_on_stderr = pick.is_some();
    let drawing_ok =
        if drawn_on_stderr { io::stderr().is_terminal() } else { io::stdout().is_terminal() };
    if !drawing_ok || !io::stdin().is_terminal() {
        return Err(io::Error::other(
            "the terminal UI needs a terminal; see `nmc --help` for commands that work in scripts",
        ));
    }
    let here = VfsPath::from_local(std::env::current_dir()?).map_err(io::Error::other)?;
    let left = left.unwrap_or_else(|| here.clone());
    let right = right.unwrap_or(here);
    let (mut app, mut events) = App::new(context, left, right).map_err(io::Error::other)?;
    if let Some(mode) = pick {
        app.set_picker(mode);
    }

    let mut screen = Screen::open(drawn_on_stderr)?;
    let mut pointer = Pointer::default();
    let mut keys = EventStream::new();
    let result = loop {
        if let Err(e) = screen.terminal.draw(|frame| draw(frame, &mut app)) {
            break Err(e);
        }
        if !app.is_running() {
            break Ok(());
        }
        tokio::select! {
            key = keys.next() => match key {
                Some(Ok(Event::Key(event))) => {
                    if let KeyCode::Modifier(_) = event.code {
                        // Terminals with the keyboard protocol report
                        // Ctrl, Alt and Shift on their own: the function key
                        // bar follows them.
                        let held = event.kind != KeyEventKind::Release;
                        let mods = event.modifiers;
                        app.set_modifiers(
                            held && mods.contains(KeyModifiers::CONTROL),
                            held && mods.contains(KeyModifiers::ALT),
                            held && mods.contains(KeyModifiers::SHIFT),
                        );
                    } else if let Some(press) = key_press(&event) {
                        app.handle_key(press);
                        app.set_modifiers(false, false, false);
                    }
                }
                Some(Ok(Event::Mouse(event))) => {
                    let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
                    pointer.handle(&mut app, event, ratatui::layout::Rect::new(0, 0, width, height));
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
        if let Some(text) = app.take_clipboard() {
            clipboard::write(&text);
        }
        if let Some(external) = app.take_external() {
            drop(keys);
            drop(screen);
            let status = run_external(&external);
            screen = Screen::open(drawn_on_stderr)?;
            keys = EventStream::new();
            screen.terminal.clear()?;
            app.after_external(&external, status);
        }
    };
    drop(screen);
    result.map(|()| app.picked().map(<[VfsPath]>::to_vec))
}

/// Runs an external program (the editor) in the foreground.
fn run_external(external: &External) -> Result<(), String> {
    let (program, args) = external.command.split_first().ok_or("no command")?;
    let status =
        std::process::Command::new(program).args(args).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("exited with {status}")) }
}
