//! Where SuperCommander keeps its files.

use std::path::{Path, PathBuf};

/// Locations of configuration and data files.
///
/// By default these follow each platform's conventions (XDG on Linux,
/// `~/Library/Application Support` on macOS, `%APPDATA%` on Windows). The
/// `SC_CONFIG_DIR` and `SC_DATA_DIR` environment variables override them,
/// which is handy for portable installs and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Directory of `config.toml`.
    pub config_dir: PathBuf,
    /// Directory of the transfer journal and caches.
    pub data_dir: PathBuf,
}

impl AppPaths {
    /// The platform default locations (or the environment overrides).
    pub fn detect() -> Self {
        let dirs = directories::ProjectDirs::from("org", "SuperCommander", "supercommander");
        let fallback = || std::env::temp_dir().join("supercommander");
        let config_dir = std::env::var_os("SC_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs.as_ref().map(|d| d.config_dir().to_path_buf()))
            .unwrap_or_else(fallback);
        let data_dir = std::env::var_os("SC_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs.as_ref().map(|d| d.data_dir().to_path_buf()))
            .unwrap_or_else(fallback);
        AppPaths { config_dir, data_dir }
    }

    /// Everything inside one directory (portable mode, tests).
    pub fn in_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        AppPaths { config_dir: dir.join("config"), data_dir: dir.join("data") }
    }

    /// `config.toml`.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// The controller's certificates for `sc serve` (`sc tls init`).
    pub fn tls_dir(&self) -> PathBuf {
        self.config_dir.join("tls")
    }

    /// Installed TypeScript plugins, one folder each.
    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    /// The user's own scripts (`.ts`, `.js`), listed in the command list.
    pub fn scripts_dir(&self) -> PathBuf {
        self.data_dir.join("scripts")
    }

    /// The permissions the user granted to plugins.
    pub fn plugin_grants_file(&self) -> PathBuf {
        self.config_dir.join("plugin-grants.json")
    }

    /// The SQLite transfer journal.
    pub fn journal_file(&self) -> PathBuf {
        self.data_dir.join("transfers.db")
    }
}
