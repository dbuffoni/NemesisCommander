//! Exporting all settings to one file and importing them back.
//!
//! The export file is TOML with a small header:
//!
//! ```toml
//! format = "pairnox-settings"
//! version = 1
//!
//! [settings.panels]
//! show_hidden = true
//! # ...
//! ```
//!
//! It never contains secrets (the configuration does not store any).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Config;
use crate::config::{ConfigError, write_atomically};

const FORMAT: &str = "pairnox-settings";
const VERSION: u32 = 1;

/// The content of an export file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportBundle {
    /// Always `"pairnox-settings"`.
    pub format: String,
    /// File format version.
    pub version: u32,
    /// Application version that wrote the file.
    #[serde(default)]
    pub exported_by: String,
    /// The settings.
    pub settings: Config,
}

/// How an import combines with the current settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// The imported settings replace everything.
    Replace,
    /// Imported settings replace the current ones, but bookmarks,
    /// connections and key overrides are merged (imported entries win on
    /// name clashes).
    Merge,
}

impl ExportBundle {
    /// Wraps `settings` for export.
    pub fn new(settings: &Config) -> Self {
        ExportBundle {
            format: FORMAT.into(),
            version: VERSION,
            exported_by: format!("Pairnox {}", env!("CARGO_PKG_VERSION")),
            settings: settings.clone(),
        }
    }

    /// Writes the bundle to `path`.
    pub fn write(&self, path: &Path) -> Result<(), ConfigError> {
        let text = toml::to_string_pretty(self).expect("settings are always representable as TOML");
        write_atomically(path, &text)
    }

    /// Reads and validates an export file.
    pub fn read(path: &Path) -> Result<Self, ConfigError> {
        let origin = path.display().to_string();
        let text = std::fs::read_to_string(path)
            .map_err(|source| ConfigError::Io { path: origin.clone(), source })?;
        Self::parse(&text, &origin)
    }

    /// Parses and validates export text.
    pub fn parse(text: &str, origin: &str) -> Result<Self, ConfigError> {
        // Check the header first, so a wrong file gets a clear message
        // instead of a schema error.
        #[derive(Deserialize)]
        struct Header {
            format: Option<String>,
            version: Option<u32>,
        }
        let header: Header = toml::from_str(text)
            .map_err(|e| ConfigError::Parse { path: origin.into(), message: e.to_string() })?;
        if header.format.as_deref() != Some(FORMAT) || header.version.is_none_or(|v| v > VERSION) {
            return Err(ConfigError::WrongFormat(origin.into()));
        }
        toml::from_str(text)
            .map_err(|e| ConfigError::Parse { path: origin.into(), message: e.to_string() })
    }

    /// Applies the imported settings to `current`.
    pub fn apply(self, current: &Config, mode: ImportMode) -> Config {
        match mode {
            ImportMode::Replace => self.settings,
            ImportMode::Merge => {
                let mut merged = self.settings;
                let mut keys = current.keys.clone();
                keys.extend(std::mem::take(&mut merged.keys));
                merged.keys = keys;
                merged.bookmarks =
                    merge_by_name(&current.bookmarks, merged.bookmarks, |b| b.name.clone());
                merged.connections =
                    merge_by_name(&current.connections, merged.connections, |c| c.name.clone());
                merged.searches =
                    merge_by_name(&current.searches, merged.searches, |s| s.name.clone());
                merged
            }
        }
    }
}

/// Keeps the order of `current`, replacing items whose name is also in
/// `imported`, then appends the new ones.
fn merge_by_name<T: Clone>(current: &[T], imported: Vec<T>, name: impl Fn(&T) -> String) -> Vec<T> {
    let mut out: Vec<T> = current
        .iter()
        .map(|item| {
            imported.iter().find(|i| name(i) == name(item)).cloned().unwrap_or_else(|| item.clone())
        })
        .collect();
    for item in imported {
        if !current.iter().any(|c| name(c) == name(&item)) {
            out.push(item);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bookmark, Connection};

    fn bookmark(name: &str, path: &str) -> Bookmark {
        Bookmark { name: name.into(), path: path.into(), targets: vec![] }
    }

    #[test]
    fn export_import_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.panels.show_hidden = true;
        config.bookmarks.push(bookmark("home", "/home/me"));
        let file = dir.path().join("settings.pnxr.toml");
        ExportBundle::new(&config).write(&file).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("format = \"pairnox-settings\""), "{text}");
        let bundle = ExportBundle::read(&file).unwrap();
        assert_eq!(bundle.apply(&Config::default(), ImportMode::Replace), config);
    }

    #[test]
    fn wrong_files_are_rejected_clearly() {
        let err =
            ExportBundle::parse("[general]\nconfirm_delete = true\n", "plain.toml").unwrap_err();
        assert!(matches!(err, ConfigError::WrongFormat(_)));
        let future = "format = \"pairnox-settings\"\nversion = 99\n[settings]\n";
        assert!(matches!(
            ExportBundle::parse(future, "f").unwrap_err(),
            ConfigError::WrongFormat(_)
        ));
        assert!(matches!(
            ExportBundle::parse("not toml = = =", "f").unwrap_err(),
            ConfigError::Parse { .. }
        ));
    }

    #[test]
    fn merge_keeps_local_entries() {
        let current = Config {
            bookmarks: vec![bookmark("a", "/old-a"), bookmark("b", "/b")],
            keys: [("files.copy".into(), vec!["Ctrl+C".into()])].into(),
            ..Default::default()
        };
        let imported = Config {
            bookmarks: vec![bookmark("c", "/c"), bookmark("a", "/new-a")],
            connections: vec![Connection {
                name: "srv".into(),
                uri: "sftp://srv/".into(),
                identity_file: None,
                options: Default::default(),
            }],
            keys: [("files.move".into(), vec!["Ctrl+X".into()])].into(),
            ..Default::default()
        };

        let merged = ExportBundle::new(&imported).apply(&current, ImportMode::Merge);
        assert_eq!(
            merged.bookmarks,
            [bookmark("a", "/new-a"), bookmark("b", "/b"), bookmark("c", "/c")]
        );
        assert_eq!(merged.connections.len(), 1);
        assert_eq!(merged.keys.len(), 2);
    }
}
