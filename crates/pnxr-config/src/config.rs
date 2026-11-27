//! The settings file.

use std::collections::BTreeMap;
use std::path::Path;

use pnxr_index::{SortKey, SortSpec};
use pnxr_ops::{ConflictPolicy, VerifyMode};
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
    #[error("not a Pairnox export file (or from a newer version): {0}")]
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
    /// Editor of the window; empty: `editor` if it suits a window, else the
    /// desktop's. `{file}` and `{line}` may be used.
    pub gui_editor: String,
    /// Editor of the terminal interface; empty: `editor` if it suits a
    /// terminal, else `$VISUAL`, `$EDITOR`, then `vi`.
    pub terminal_editor: String,
    /// Command that opens a terminal in a folder ("Open terminal here");
    /// `{dir}` is the folder. Empty: the terminal of the system.
    pub terminal: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        GeneralSettings {
            confirm_delete: true,
            use_trash: true,
            editor: String::new(),
            gui_editor: String::new(),
            terminal_editor: String::new(),
            terminal: String::new(),
        }
    }
}

/// Git support: marks in the panels and the actions on a repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GitSettings {
    /// The `git` program (a name on `PATH` or a full path); empty: `git`.
    pub executable: String,
}

impl GitSettings {
    /// The program to run.
    pub fn program(&self) -> &str {
        let program = self.executable.trim();
        if program.is_empty() { "git" } else { program }
    }
}

/// A plugin repository added by the user (ADR 0054).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRepository {
    /// What `git clone` accepts.
    pub url: String,
    /// A branch or tag (the default branch when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// Where plugins can be installed from by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginSettings {
    /// List the official repository (built in; it cannot be removed).
    pub official: bool,
    /// Repositories added by the user, in list order.
    pub repositories: Vec<PluginRepository>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        PluginSettings { official: true, repositories: Vec::new() }
    }
}

impl PluginRepository {
    /// `url` or `url#reference`, as typed and listed.
    pub fn label(&self) -> String {
        match &self.reference {
            Some(reference) => format!("{}#{reference}", self.url),
            None => self.url.clone(),
        }
    }
}

impl PluginSettings {
    /// Adds a repository; `false` when the list already has it.
    pub fn add(&mut self, repository: PluginRepository) -> bool {
        if self.repositories.contains(&repository) {
            return false;
        }
        self.repositories.push(repository);
        true
    }

    /// Removes the repository written as `label` (`url` or `url#reference`);
    /// `false` when there is none.
    pub fn remove(&mut self, label: &str) -> bool {
        let before = self.repositories.len();
        self.repositories.retain(|r| r.label() != label);
        self.repositories.len() != before
    }
}

/// The font the window starts with: JetBrains Mono with the Nerd Font symbols
/// (a window on a computer that does not have it shows the system fonts).
pub const DEFAULT_FONT_FAMILY: &str = "JetBrainsMono Nerd Font Mono";

/// The size, in points, the window starts with.
pub const DEFAULT_FONT_SIZE: u32 = 11;

/// Fonts of the window (the terminal interface uses the terminal's font).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceSettings {
    /// Font family of the text; empty for the system font.
    pub font_family: String,
    /// Font size in points; 0 for the system size.
    pub font_size: u32,
    /// Fixed-width font family (viewers, columns); empty for `monospace`.
    pub fixed_font_family: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        AppearanceSettings {
            font_family: DEFAULT_FONT_FAMILY.to_owned(),
            font_size: DEFAULT_FONT_SIZE,
            fixed_font_family: DEFAULT_FONT_FAMILY.to_owned(),
        }
    }
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
    /// Mark changed files and show the branch inside git repositories.
    pub git_status: bool,
    /// Draw a small outline icon for the kind of each file before its name.
    pub icons: bool,
    /// Never list macOS `.DS_Store` files, even with hidden files shown.
    pub hide_ds_store: bool,
}

impl Default for PanelSettings {
    fn default() -> Self {
        PanelSettings {
            show_hidden: false,
            sort: SortKey::Name,
            descending: false,
            human_sizes: true,
            arrow_navigation: false,
            git_status: true,
            icons: true,
            hide_ds_store: false,
        }
    }
}

impl PanelSettings {
    /// The initial sort as a [`SortSpec`].
    pub fn sort_spec(&self) -> SortSpec {
        SortSpec { key: self.sort, descending: self.descending }
    }
}

/// How boldly the automatic parallelism of transfers works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransferProfile {
    /// A good speed without overloading anything.
    #[default]
    Balanced,
    /// As fast as possible.
    MaxSpeed,
    /// Few files at once, leaves bandwidth and servers free.
    Discreet,
}

/// Defaults for copy, move and delete jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TransferSettings {
    /// Jobs running at the same time.
    pub parallel_jobs: usize,
    /// Files copied at the same time within a job (the starting level when
    /// `auto_parallelism` is on).
    pub parallel_files: usize,
    /// Adjust the files copied at the same time while a job runs, from the
    /// measured speed. Can be turned off per job in the job monitor.
    pub auto_parallelism: bool,
    /// How boldly the automatic parallelism raises the number of files.
    pub profile: TransferProfile,
    /// Remember, in a small file, the level and limit that worked on each
    /// server, so known servers start well.
    pub keep_stats: bool,
    /// Verification after copying.
    pub verify: VerifyMode,
    /// When `verify` is "same size", check by XXH3 content hash when the
    /// destination is an agent or `pnxr serve` server, which computes it
    /// there at no cost in traffic.
    pub verify_remote_fast: bool,
    /// What to do when a file exists.
    pub conflict: ConflictPolicy,
    /// Global bandwidth limit in KiB/s (0 = unlimited).
    pub bandwidth_limit_kib: u64,
    /// Retries for transient errors.
    pub retries: u32,
    /// When copying, do not carry over the macOS "downloaded from the
    /// internet" flag (`com.apple.quarantine`). Other extended attributes
    /// are always copied.
    pub strip_quarantine: bool,
}

impl Default for TransferSettings {
    fn default() -> Self {
        TransferSettings {
            parallel_jobs: 2,
            parallel_files: 4,
            auto_parallelism: true,
            profile: TransferProfile::Balanced,
            keep_stats: true,
            verify: VerifyMode::Size,
            verify_remote_fast: true,
            conflict: ConflictPolicy::Ask,
            bandwidth_limit_kib: 0,
            retries: 3,
            strip_quarantine: false,
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

/// A search kept under a name (F9 and shortcuts run it again).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSearch {
    /// Display name; also the id of its shortcut (`search.NAME`).
    pub name: String,
    /// File name mask (empty: any name).
    #[serde(default)]
    pub mask: String,
    /// Text to find inside files.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    /// Size and age conditions: `>10M`, `<1G`, `newer:7d`, `older:2w`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub filters: String,
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
    /// Not used any more: `ssh` finds its keys as it does in a terminal.
    /// Read so that settings written before still load; never
    /// written back.
    #[serde(default, skip_serializing)]
    pub identity_file: Option<String>,
    /// Free-form provider options (`compression = "yes"`, ...).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
}

impl Connection {
    /// The most files at once the transfers may use on this server (option
    /// `max_connections`), `None` when not set.
    pub fn max_connections(&self) -> Option<usize> {
        self.options.get("max_connections").and_then(|v| v.trim().parse().ok()).filter(|n| *n >= 1)
    }
}

/// All persistent settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Which shortcut model the file was written for: absent is the first one
    /// (every chord of a command in `keys`, function keys too), 2 splits
    /// them into `keys` (shortcuts), `fkeys` and `labels` (ADR 0059).
    /// Written whenever one of the three is not empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut_model: Option<u32>,
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
    /// Plugin repositories.
    pub plugins: PluginSettings,
    /// Git.
    pub git: GitSettings,
    /// Standard shortcut overrides: command id → chords that are not
    /// function keys (empty list: no shortcut).
    pub keys: BTreeMap<String, Vec<String>>,
    /// Function-key overrides: command id → `F5`, `Shift+F5` or `Ctrl+F5`
    /// (empty text: no function key).
    pub fkeys: BTreeMap<String, String>,
    /// Short names of the bottom bar buttons: command id → name (at most
    /// [`crate::keymap::LABEL_MAX`] characters).
    pub labels: BTreeMap<String, String>,
    /// Bookmarks, in menu order.
    pub bookmarks: Vec<Bookmark>,
    /// Saved connections, in menu order.
    pub connections: Vec<Connection>,
    /// Saved searches, in list order.
    pub searches: Vec<SavedSearch>,
}

impl Config {
    /// Parses settings from TOML text. Missing sections and keys get their
    /// defaults; unknown keys are an error (to catch typos).
    pub fn from_toml(text: &str, origin: &str) -> Result<Self, ConfigError> {
        let mut config: Config = toml::from_str(text)
            .map_err(|e| ConfigError::Parse { path: origin.to_owned(), message: e.to_string() })?;
        config.migrate_shortcuts();
        Ok(config)
    }

    /// Moves a settings file written for the first shortcut model to the
    /// current one: function-key chords go to `fkeys`, the others stay in
    /// `keys`. Nothing is lost. Returns whether anything was converted.
    pub fn migrate_shortcuts(&mut self) -> bool {
        if self.shortcut_model.is_some() || self.keys.is_empty() {
            return false;
        }
        self.shortcut_model = Some(crate::keymap::SHORTCUT_MODEL);
        let mut changed = false;
        for (id, list) in self.keys.iter_mut() {
            let (functions, others): (Vec<_>, Vec<_>) =
                list.iter().cloned().partition(|k| crate::keymap::is_function_key_text(k));
            if functions.is_empty() && !crate::keymap::Action::from_id(id).is_some() {
                continue;
            }
            // The first model replaced every chord of a command: no function
            // key in the list meant no function key.
            if !self.fkeys.contains_key(id) {
                self.fkeys.insert(id.clone(), functions.first().cloned().unwrap_or_default());
            }
            if !functions.is_empty() || crate::keymap::Action::from_id(id).is_some() {
                changed = true;
            }
            *list = others;
        }
        changed
    }

    /// Serializes to TOML.
    pub fn to_toml(&self) -> String {
        let mut config = self.clone();
        if !config.keys.is_empty() || !config.fkeys.is_empty() || !config.labels.is_empty() {
            config.shortcut_model = Some(crate::keymap::SHORTCUT_MODEL);
        }
        toml::to_string_pretty(&config).expect("settings are always representable as TOML")
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
    fn plugin_repositories_are_added_once_and_removed_by_label() {
        let mut settings = PluginSettings::default();
        assert!(settings.official);
        let repo = PluginRepository { url: "https://x/y.git".into(), reference: Some("v1".into()) };
        assert!(settings.add(repo.clone()) && !settings.add(repo));
        assert!(!settings.remove("https://x/y.git"), "the label includes the reference");
        assert!(settings.remove("https://x/y.git#v1") && settings.repositories.is_empty());
        let config = Config::from_toml(
            "[plugins]\nofficial = false\n[[plugins.repositories]]\nurl = \"https://a/b.git\"\n",
            "t",
        )
        .unwrap();
        assert!(!config.plugins.official && config.plugins.repositories.len() == 1);
        assert_eq!(Config::from_toml(&config.to_toml(), "t").unwrap(), config);
    }

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
    fn settings_of_the_first_shortcut_model_are_migrated_without_losing_a_key() {
        let old = r#"
[keys]
"files.copy" = ["Ctrl+C", "F5"]
"files.move" = ["Ctrl+X"]
"files.pack" = ["Shift+F5"]
"plugin.tidy.run" = ["Ctrl+K"]
"plugin.tidy.go" = ["Ctrl+F10"]
"#;
        let config = Config::from_toml(old, "test").unwrap();
        assert_eq!(config.shortcut_model, Some(2));
        assert_eq!(config.keys["files.copy"], ["Ctrl+C"]);
        assert_eq!(config.fkeys["files.copy"], "F5");
        // The first model replaced every chord: no function key meant none.
        assert_eq!(config.keys["files.move"], ["Ctrl+X"]);
        assert_eq!(config.fkeys["files.move"], "");
        assert_eq!(config.keys["files.pack"], Vec::<String>::new());
        assert_eq!(config.fkeys["files.pack"], "Shift+F5");
        assert_eq!(config.keys["plugin.tidy.run"], ["Ctrl+K"]);
        assert!(!config.fkeys.contains_key("plugin.tidy.run"));
        assert_eq!(config.fkeys["plugin.tidy.go"], "Ctrl+F10");
        // The behaviour is the same as before.
        let (keymap, warnings) = crate::Keymap::from_config(&config);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(keymap.action_for(&"F5".parse().unwrap()), Some(crate::Action::Copy));
        assert_eq!(keymap.action_for(&"Ctrl+X".parse().unwrap()), Some(crate::Action::Move));
        assert_eq!(keymap.fkey_for_id("files.move"), None);
        // It is written in the new shape and read back unchanged.
        let again = Config::from_toml(&config.to_toml(), "test").unwrap();
        assert_eq!(again, config);
        assert_eq!(again.fkeys["files.move"], "", "still no function key after the second load");
        let mut twice = config.clone();
        assert!(!twice.migrate_shortcuts(), "a second migration changes nothing");
    }

    #[test]
    fn the_new_shortcut_sections_round_trip() {
        let mut config = Config::default();
        config.fkeys.insert("files.copy".into(), "Ctrl+F5".into());
        config.labels.insert("files.copy".into(), "Duplicate".into());
        let back = Config::from_toml(&config.to_toml(), "test").unwrap();
        assert_eq!(back.fkeys, config.fkeys);
        assert_eq!(back.labels, config.labels);
        assert_eq!(back.shortcut_model, Some(2));
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
        config.shortcut_model = Some(2);
        config.keys.insert("panel.copy".into(), vec!["Ctrl+C".into()]);
        config.bookmarks.push(Bookmark {
            name: "logs".into(),
            path: "sftp://srv/var/log".into(),
            targets: vec!["/home/me/logs".into(), "s3://backup/logs".into()],
        });
        config.connections.push(Connection {
            name: "build".into(),
            uri: "agent://build01/".into(),
            identity_file: None,
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
