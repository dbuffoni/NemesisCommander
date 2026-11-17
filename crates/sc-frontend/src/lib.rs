//! # sc-frontend
//!
//! The interactive application shared by the terminal interface (`sc-tui`)
//! and the desktop interface (`sc-gui`): what the two panels show, every
//! dialog, the viewer, the job list and what each command does. A front end
//! only turns its own key events into [`KeyPress`]es, draws the state, and
//! runs the background results ([`AppEvent`]) through [`App::handle_event`].
//! Both interfaces therefore behave the same way, and this crate is tested
//! without any screen.
//!
//! * `app`: the [`App`] state machine (keys and background results in,
//!   state out).
//! * `commands`: the command list (F9), with plugins and scripts.
//! * `dialogs`: the data of every overlay window.
//! * `drives`: the drive list, local drives and connections together.
//! * `input`: [`KeyPress`] and the one-line [`TextField`].
//! * `viewer`: the viewer of text, hex, pictures and previews.
//! * `preview`: documents, PDF pages and 3D models for the viewer.
//! * `view`: what the screen shows, as plain data (and shared text helpers).

mod app;
pub mod commands;
mod dialogs;
pub mod drives;
pub mod git;
mod input;
pub mod preview;
pub mod settings;
pub mod view;
pub mod viewer;

pub use app::{App, AppEvent, External, Status, display_location, resolve_location, side_index};
pub use dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, InputDialog, InputPurpose,
    JobsView, MessageDialog, Overlay, PendingConflict, menu_actions,
};
pub use input::{KeyPress, TextField};
pub use viewer::Viewer;
