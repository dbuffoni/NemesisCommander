//! # pnxr-config
//!
//! Persistent user settings shared by every Pairnox front end:
//!
//! * [`Config`]: general, panel, transfer and viewer settings, bookmarks
//!   and saved connections, stored as one TOML file.
//! * [`keymap`]: the list of [`Action`]s, their default shortcuts, user
//!   overrides, and the short list shown by the help key (F1).
//! * [`export`]: writing all settings to a single portable file and
//!   importing them back. There is deliberately no synchronization between
//!   machines; moving settings is an explicit export/import.

mod config;
pub mod export;
pub mod keymap;
mod paths;

pub use config::{
    Bookmark, Config, ConfigError, Connection, GeneralSettings, PanelSettings, PluginRepository,
    PluginSettings, SavedSearch, TransferProfile, TransferSettings, ViewerSettings,
};
pub use export::{ExportBundle, ImportMode};
pub use keymap::{Action, Category, Key, KeyChord, Keymap};
pub use paths::AppPaths;
