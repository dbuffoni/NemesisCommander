//! Where NemesisCommander keeps its files.

use std::path::{Path, PathBuf};

/// Smallest window size that is remembered, and the largest side.
const MIN_WINDOW: (i32, i32) = (320, 200);
const MAX_WINDOW: i32 = 20_000;

/// Locations of configuration and data files.
///
/// By default these follow each platform's conventions (XDG on Linux,
/// `~/Library/Application Support` on macOS, `%APPDATA%` on Windows). The
/// `NMC_CONFIG_DIR` and `NMC_DATA_DIR` environment variables override them,
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
        let dirs = directories::ProjectDirs::from("org", "NemesisCommander", "nemesiscommander");
        let fallback = || std::env::temp_dir().join("nemesiscommander");
        let config_dir = std::env::var_os("NMC_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs.as_ref().map(|d| d.config_dir().to_path_buf()))
            .unwrap_or_else(fallback);
        let data_dir = std::env::var_os("NMC_DATA_DIR")
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

    /// The controller's certificates for `nmc serve` (`nmc tls init`).
    pub fn tls_dir(&self) -> PathBuf {
        self.config_dir.join("tls")
    }

    /// Installed TypeScript plugins, one folder each.
    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    /// Where each plugin installed from Git came from (kept next to the
    /// plugins: settings sharing does not carry it to a machine without them).
    pub fn plugin_lock_file(&self) -> PathBuf {
        self.data_dir.join("plugin-lock.json")
    }

    /// The user's own scripts (`.ts`, `.js`), listed in the command list.
    pub fn scripts_dir(&self) -> PathBuf {
        self.data_dir.join("scripts")
    }

    /// The permissions the user granted to plugins.
    pub fn plugin_grants_file(&self) -> PathBuf {
        self.config_dir.join("plugin-grants.json")
    }

    /// The size the window had when it was last closed (`WIDTHxHEIGHT`). It
    /// is per machine, so it lives with the data and is not shared.
    pub fn window_file(&self) -> PathBuf {
        self.data_dir.join("window-size")
    }

    /// The remembered window size, if the file holds a sensible one.
    pub fn load_window_size(&self) -> Option<(i32, i32)> {
        let text = std::fs::read_to_string(self.window_file()).ok()?;
        let (width, height) = text.trim().split_once('x')?;
        let (width, height): (i32, i32) = (width.parse().ok()?, height.parse().ok()?);
        let sensible = (MIN_WINDOW.0..=MAX_WINDOW).contains(&width)
            && (MIN_WINDOW.1..=MAX_WINDOW).contains(&height);
        sensible.then_some((width, height))
    }

    /// Remembers the window size for the next start; sizes that cannot be
    /// right (a minimized window) are ignored.
    pub fn save_window_size(&self, width: i32, height: i32) -> std::io::Result<()> {
        if !(MIN_WINDOW.0..=MAX_WINDOW).contains(&width)
            || !(MIN_WINDOW.1..=MAX_WINDOW).contains(&height)
        {
            return Ok(());
        }
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::write(self.window_file(), format!("{width}x{height}\n"))
    }

    /// What worked on each server (level and limit of files at once).
    pub fn stats_file(&self) -> PathBuf {
        self.data_dir.join("connection-stats.json")
    }

    /// The SQLite transfer journal.
    pub fn journal_file(&self) -> PathBuf {
        self.data_dir.join("transfers.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_size_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::in_dir(dir.path());
        assert_eq!(paths.load_window_size(), None);
        paths.save_window_size(1500, 900).unwrap();
        assert_eq!(paths.load_window_size(), Some((1500, 900)));
        // Nonsense sizes are neither saved nor read.
        paths.save_window_size(10, 10).unwrap();
        assert_eq!(paths.load_window_size(), Some((1500, 900)));
        std::fs::write(paths.window_file(), "huge").unwrap();
        assert_eq!(paths.load_window_size(), None);
    }
}
