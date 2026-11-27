//! # pnxr-frontend
//!
//! The interactive application shared by the terminal interface (`pnxr-tui`)
//! and the desktop interface (`pnxr-gui`): what the two panels show, every
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

pub mod about;
mod app;
pub mod commands;
pub mod connections;
mod dialogs;
pub mod diff;
pub mod disks;
pub mod drives;
pub mod editors;
pub mod export;
pub mod frecency;
pub mod fuzzy;
pub mod git;
pub mod help;
pub mod hints;
pub mod icons;
mod input;
pub mod install;
pub mod masks;
pub mod ports;
pub mod preview;
pub mod properties;
pub mod renames;
pub mod searches;
pub mod settings;
pub mod speed;
pub mod syncplan;
pub mod terminal;
pub mod usage;
pub mod view;
pub mod viewer;

pub use app::{
    App, AppEvent, ColumnCells, ColumnHead, External, PickMode, Status, display_location,
    resolve_location, side_index,
};
pub use dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, InputDialog, InputPurpose,
    JobsView, MessageDialog, Overlay, PendingConflict, menu_actions,
};
pub use input::{KeyPress, TextField};
pub use pnxr_panel::SideId;
pub use viewer::Viewer;
