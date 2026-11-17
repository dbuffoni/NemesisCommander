//! The settings file.

use std::collections::BTreeMap;
use std::path::Path;

use sc_index::{SortKey, SortSpec};
use sc_ops::{ConflictPolicy, VerifyMode};
use serde::{Deserialize, Serialize};

/// Errors while loading or saving settings.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// File involved.
        path: String,
        /// Cause.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid TOML for this structure.
    #[error("{path}: {message}")]
    Parse {
        /// File involved.
        path: String,
        /// Parser message (with line and column).
        message: String,
    },
    /// An export file from an incompatible version or another program.
    #[error("not a SuperCommander export file (or from a newer version): {0}")]
    WrongFormat(String),
}

/// Settings that apply everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralSettings {
    /// Ask before deleting.
    pub confirm_delete: bool,
    /// Delete to the recycle bin when the provider has one.
    pub use_trash: bool,
    /// External editor command for "Edit" (`$EDITOR` when empty).
    pub editor: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        GeneralSettings { confirm_delete: true, use_trash: true, editor: String::new() }
    }
}

/// Fonts of the window (the terminal interface uses the terminal's font).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceSettings {
    /// Font family of the text; empty for the system font.
    pub font_family: String,
    /// Font size in points; 0 for the system size.
    pub font_size: u32,
    /// Fixed-width font family (viewers, columns); empty for `monospace`.
    pub fixed_font_family: String,
}

/// How panels show files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PanelSettings {
    /// Show dot files.
    pub show_hidden: bool,
    /// Initial sort column.
    pub sort: SortKey,
    /// Initial sort direction.
    pub descending: bool,
    /// Show sizes as `1.2 MiB` instead of exact bytes.
    pub human_sizes: bool,
    /// Left goes to the parent folder and Right enters the folder under the
    /// cursor (Double Commander's "Lynx-like" movement).
    pub arrow_navigation: bool,
}

impl Default for PanelSettings {
    fn default() -> Self {
        PanelSettings {
            show_hidden: false,
            sort: SortKey::Name,
            descending: false,
            human_sizes: true,
            arrow_navigation: false,
        }
    }
}

impl PanelSettings {
    /// The initial sort as a [`SortSpec`].
    pub fn sort_spec(&self) -> SortSpec {
        SortSpec { key: self.sort, descending: self.descending }
    }
}

/// Defaults for copy, move and delete jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TransferSettings {
    /// Jobs running at the same time.
    pub parallel_jobs: usize,
    /// Files copied at the same time within a job.
    pub parallel_files: usize,
    /// Verification after copying.
    pub verify: VerifyMode,
    /// What to do when a file exists.
    pub conflict: ConflictPolicy,
    /// Global bandwidth limit in KiB/s (0 = unlimited).
    pub bandwidth_limit_kib: u64,
    /// Retries for transient errors.
    pub retries: u32,
}

impl Default for TransferSettings {
    fn default() -> Self {
        TransferSettings {
            parallel_jobs: 2,
            parallel_files: 4,
            verify: VerifyMode::Size,
            conflict: ConflictPolicy::Ask,
            bandwidth_limit_kib: 0,
            retries: 3,
        }
    }
}

/// Built-in viewer settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewerSettings {
    /// Wrap long lines in text mode.
    pub wrap: bool,
    /// Spaces per tab.
    pub tab_width: usize,
    /// Files larger than this (MiB) open in hex mode by default.
    pub hex_above_mib: u64,
}

impl Default for ViewerSettings {
    fn default() -> Self {
        ViewerSettings { wrap: true, tab_width: 4, hex_above_mib: 64 }
    }
}

/// A named shortcut to a location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bookmark {
    /// Display name.
    pub name: String,
    /// The location, as a VFS URI.
    pub path: String,
    /// Folders for the other panel, as VFS URIs. Opening the bookmark
    /// shows the only one there, or asks which one when there are several.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
}

/// A saved remote location. Secrets are never stored: authentication uses
/// SSH keys/agents or is asked interactively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    /// Display name.
    pub name: String,
    /// The root URI, e.g. `sftp://me@host:22/srv` or `agent://build01/`.
    pub uri: String,
    /// Private key file to use instead of the SSH agent's keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// Free-form provider options (`compression = "yes"`, ...).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
}

/// All persistent settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// General behavior.
    pub general: GeneralSettings,
    /// Panels.
    pub panels: PanelSettings,
    /// Transfers.
    pub transfers: TransferSettings,
    /// Viewer.
    pub viewer: ViewerSettings,
    /// Fonts of the window.
    pub appearance: AppearanceSettings,
    /// Shortcut overrides: action id → key chords (empty list unbinds).
    pub keys: BTreeMap<String, Vec<String>>,
    /// Bookmarks, in menu order.
    pub bookmarks: Vec<Bookmark>,
    /// Saved connections, in menu order.
    pub connections: Vec<Connection>,
}

impl Config {
    /// Parses settings from TOML text. Missing sections and keys get their
    /// defaults; unknown keys are an error (to catch typos).
    pub fn from_toml(text: &str, origin: &str) -> Result<Self, ConfigError> {
        toml::from_str(text)
            .map_err(|e| ConfigError::Parse { path: origin.to_owned(), message: e.to_string() })
    }

    /// Serializes to TOML.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("settings are always representable as TOML")
    }

    /// Loads `path`, or returns defaults if the file does not exist.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text, &path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(source) => Err(ConfigError::Io { path: path.display().to_string(), source }),
        }
    }

    /// Writes `path` atomically (temp file + rename), creating parent
    /// directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        write_atomically(path, &self.to_toml())
    }
}

/// Writes `text` to a sibling temp file and renames it over `path`, so a
/// crash never leaves a half-written settings file.
pub(crate) fn write_atomically(path: &Path, text: &str) -> Result<(), ConfigError> {
    let io = |source| ConfigError::Io { path: path.display().to_string(), source };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, text).map_err(io)?;
    std::fs::rename(&temp, path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::from_toml("", "test").unwrap(), Config::default());
    }

    #[test]
    fn partial_files_keep_other_defaults() {
        let config = Config::from_toml("[panels]\nshow_hidden = true\n", "test").unwrap();
        assert!(config.panels.show_hidden);
        assert_eq!(config.transfers, TransferSettings::default());
    }

    #[test]
    fn unknown_keys_are_reported_with_location() {
        let err = Config::from_toml("[panels]\nshow_hiden = true\n", "my.toml").unwrap_err();
        let message = err.to_string();
        assert!(message.starts_with("my.toml:"), "{message}");
        assert!(message.contains("show_hiden"), "{message}");
    }

    #[test]
    fn roundtrip_through_toml_and_disk() {
        let mut config = Config::default();
        config.transfers.verify = VerifyMode::Full;
        config.keys.insert("panel.copy".into(), vec!["Ctrl+C".into()]);
        config.bookmarks.push(Bookmark {
            name: "logs".into(),
            path: "sftp://srv/var/log".into(),
            targets: vec!["/home/me/logs".into(), "s3://backup/logs".into()],
        });
        config.connections.push(Connection {
            name: "build".into(),
            uri: "agent://build01/".into(),
            identity_file: Some("~/.ssh/id_ed25519".into()),
            options: BTreeMap::from([("compression".into(), "yes".into())]),
        });
        assert_eq!(Config::from_toml(&config.to_toml(), "t").unwrap(), config);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.toml");
        assert_eq!(Config::load(&path).unwrap(), Config::default(), "missing file = defaults");
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), config);
        assert!(!path.with_extension("toml.tmp").exists());
    }

    #[test]
    fn bookmark_targets_are_optional() {
        let old = "[[bookmarks]]\nname = \"logs\"\npath = \"/var/log\"\n";
        let config = Config::from_toml(old, "t").unwrap();
        assert!(config.bookmarks[0].targets.is_empty(), "files written before targets still load");
        assert!(!config.to_toml().contains("targets"), "no targets, no key");
    }
}
