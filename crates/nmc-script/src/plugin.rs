//! Plugins: a folder with a `plugin.json` manifest and a script whose
//! commands appear in the front ends (ADR 0016).
//!
//! ```json
//! {
//!   "name": "tidy",
//!   "version": "1.0.0",
//!   "description": "Moves old downloads away",
//!   "main": "main.ts",
//!   "commands": [{ "id": "tidy", "title": "Tidy this folder", "category": "Files" }],
//!   "permissions": { "read": ["$SELECTION"], "write": ["$PANEL"] },
//!   "limits": { "memory": 256, "timeout": 60 }
//! }
//! ```
//!
//! A plugin may only touch what its manifest asks for, and only once the
//! user granted it ([`Grants`]). Locations in the manifest are absolute
//! paths, URIs or placeholders filled in when a command starts: `$PANEL`
//! and `$OTHER` (the panels' folders), `$SELECTION` (each selected entry)
//! and `$HOME`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use nmc_vfs::VfsPath;
use serde::{Deserialize, Serialize};

use crate::{Permissions, ScriptError};

/// The manifest's file name in a plugin folder.
pub const MANIFEST: &str = "plugin.json";

/// Limits a plugin gets when its manifest names none.
pub const DEFAULT_MEMORY_MIB: usize = 256;
/// See [`DEFAULT_MEMORY_MIB`].
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

/// Why a plugin cannot be loaded or run.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The folder has no readable, valid manifest.
    #[error("{path}: {message}")]
    Manifest {
        /// The manifest file.
        path: PathBuf,
        /// What is wrong with it.
        message: String,
    },
    /// No plugin or command by that name.
    #[error("{0}")]
    NotFound(String),
    /// The user has not granted the permissions the plugin asks for.
    #[error("plugin {0} needs your permission first")]
    NotGranted(String),
    /// A location in the manifest cannot be used.
    #[error("plugin {plugin}: {message}")]
    Location {
        /// The plugin's name.
        plugin: String,
        /// What is wrong.
        message: String,
    },
    /// The script failed.
    #[error("plugin {plugin}: {error}")]
    Script {
        /// The plugin's name.
        plugin: String,
        /// The script's error.
        error: ScriptError,
    },
    /// A plugin could not be fetched from, or updated from, its Git source.
    #[error("{0}")]
    Source(String),
    /// The grants file cannot be read or written.
    #[error("{path}: {message}")]
    Grants {
        /// The grants file.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
}

/// The contents of `plugin.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Lowercase letters, digits and dashes; unique among installed plugins.
    pub name: String,
    /// The plugin's own version.
    pub version: String,
    /// One line shown to the user.
    #[serde(default)]
    pub description: String,
    /// Who wrote it (shown in the catalogue).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Where to read about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Its licence, as an SPDX name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// The oldest `nmc` version it works with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_nmc: Option<String>,
    /// The script, relative to the plugin folder.
    #[serde(default = "default_main")]
    pub main: String,
    /// The commands the plugin adds.
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
    /// Panel columns the plugin fills (EXIF date, page count...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnSpec>,
    /// Viewers the plugin adds for kinds of files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previewers: Vec<PreviewerSpec>,
    /// What the plugin may touch.
    #[serde(default)]
    pub permissions: Requested,
    /// Memory and time limits.
    #[serde(default)]
    pub limits: Limits,
}

fn default_main() -> String {
    "main.ts".into()
}

/// A command of a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    /// Passed to the script as `nmc.context.command`.
    pub id: String,
    /// Shown in the command list as `CATEGORY/TITLE`.
    pub title: String,
    /// The command list category: a built-in one (`Files`, `Select`,
    /// `Navigate`, `View`, `Tools`, `NemesisCommander`); missing or another
    /// name puts the command under `Plugins`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// A panel column a plugin fills.
///
/// The plugin's script runs as the command `column:ID` with
/// `nmc.context.selection` holding the files of the folder that match
/// `match` (at most [`COLUMN_FILES`] at a time) and prints one line per
/// file, in that order: the text of the column (empty for none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnSpec {
    /// Passed to the script as `column:ID`; unique among the columns.
    pub id: String,
    /// The column heading.
    pub title: String,
    /// File name masks the column is for (`*.jpg`); `*` for every file.
    #[serde(rename = "match")]
    pub patterns: Vec<String>,
    /// Width in characters (default 12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u16>,
}

/// A viewer a plugin adds for some kinds of files.
///
/// The plugin's script runs as the command `preview:ID` with the file in
/// `nmc.context.selection` and prints Markdown, which the viewer and the
/// quick view show instead of the bytes of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewerSpec {
    /// Passed to the script as `preview:ID`; unique among the previewers.
    pub id: String,
    /// What the viewer is called.
    pub title: String,
    /// File name masks the previewer is for (`*.csv`).
    #[serde(rename = "match")]
    pub patterns: Vec<String>,
}

/// How many files one run of a column gets.
pub const COLUMN_FILES: usize = 300;

/// Whether `name` fits one of `patterns` (`*.jpg`, `README*`, `*`), ignoring
/// case. Only `*` and `?` are wildcards.
pub fn name_matches(patterns: &[String], name: &str) -> bool {
    fn fits(pattern: &[char], name: &[char]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| fits(rest, &name[skip..])),
            Some(('?', rest)) => !name.is_empty() && fits(rest, &name[1..]),
            Some((c, rest)) => name.first() == Some(c) && fits(rest, &name[1..]),
        }
    }
    let name: Vec<char> = name.to_lowercase().chars().collect();
    patterns.iter().any(|p| fits(&p.to_lowercase().chars().collect::<Vec<_>>(), &name))
}

/// Permissions a manifest asks for. Network and process access do not
/// exist yet: a manifest asking for them is refused rather than ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requested {
    /// Locations to read.
    #[serde(default)]
    pub read: Vec<String>,
    /// Locations to read and write.
    #[serde(default)]
    pub write: Vec<String>,
    /// Everything the user can reach.
    #[serde(default)]
    pub all: bool,
    /// Programs the plugin may start from the PATH, by bare name
    /// (`nmc.run`); never through a shell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,
}

impl Requested {
    /// The request in words, one line per grant, for confirmation dialogs.
    pub fn describe(&self) -> Vec<String> {
        let mut lines = self.describe_files();
        if !self.run.is_empty() {
            lines.push(format!("may run: {}", self.run.join(", ")));
        }
        lines
    }

    fn describe_files(&self) -> Vec<String> {
        if self.all {
            return vec!["read and change every file you can".into()];
        }
        let words = |location: &str| match location {
            "$PANEL" => "the folder in the active panel".to_owned(),
            "$OTHER" => "the folder in the other panel".to_owned(),
            "$SELECTION" => "the selected files and folders".to_owned(),
            "$HOME" => "your home folder".to_owned(),
            other => other.to_owned(),
        };
        let mut lines: Vec<String> =
            self.read.iter().map(|l| format!("read {}", words(l))).collect();
        lines.extend(self.write.iter().map(|l| format!("read and change {}", words(l))));
        if lines.is_empty() && self.run.is_empty() {
            lines.push("no access to files".into());
        }
        lines
    }
}

/// A plugin's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Heap limit in MiB.
    #[serde(default = "default_memory")]
    pub memory: usize,
    /// Seconds a command may run.
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_memory() -> usize {
    DEFAULT_MEMORY_MIB
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

impl Default for Limits {
    fn default() -> Self {
        Limits { memory: DEFAULT_MEMORY_MIB, timeout: DEFAULT_TIMEOUT_SECONDS }
    }
}

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    /// Its folder.
    pub dir: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
}

impl Plugin {
    /// Loads and checks the plugin in `dir`.
    pub fn load(dir: &Path) -> Result<Self, PluginError> {
        let path = dir.join(MANIFEST);
        let invalid = |message: String| PluginError::Manifest { path: path.clone(), message };
        let text = fs::read_to_string(&path).map_err(|e| invalid(e.to_string()))?;
        let manifest: Manifest = serde_json::from_str(&text).map_err(|e| invalid(e.to_string()))?;
        validate(&manifest).map_err(invalid)?;
        Ok(Plugin { dir: dir.to_path_buf(), manifest })
    }

    /// The plugin's name.
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// The command `id`, if the plugin has it.
    pub fn command(&self, id: &str) -> Option<&CommandSpec> {
        self.manifest.commands.iter().find(|command| command.id == id)
    }

    /// The column `id`, if the plugin has it.
    pub fn column(&self, id: &str) -> Option<&ColumnSpec> {
        self.manifest.columns.iter().find(|column| column.id == id)
    }

    /// The previewer `id`, if the plugin has it.
    pub fn previewer(&self, id: &str) -> Option<&PreviewerSpec> {
        self.manifest.previewers.iter().find(|previewer| previewer.id == id)
    }

    /// Whether the plugin can run `command`: one of its commands, or
    /// `column:ID` / `preview:ID` of a column or previewer it declares.
    pub fn provides(&self, command: &str) -> bool {
        if let Some(id) = command.strip_prefix("column:") {
            return self.column(id).is_some();
        }
        if let Some(id) = command.strip_prefix("preview:") {
            return self.previewer(id).is_some();
        }
        self.command(command).is_some()
    }

    /// The permissions of one run: placeholders filled in from `context`.
    /// A placeholder with nothing to fill in (no selection, say) grants
    /// nothing.
    pub fn permissions(&self, context: &Context) -> Result<Permissions, PluginError> {
        let requested = &self.manifest.permissions;
        let mut permissions = if requested.all { Permissions::all() } else { Permissions::none() };
        for program in &requested.run {
            permissions.allow_run(program);
        }
        if requested.all {
            return Ok(permissions);
        }
        for location in &requested.read {
            for path in self.expand(location, context)? {
                permissions.allow_read(path);
            }
        }
        for location in &requested.write {
            for path in self.expand(location, context)? {
                permissions.allow_write(path);
            }
        }
        Ok(permissions)
    }

    fn expand(&self, location: &str, context: &Context) -> Result<Vec<VfsPath>, PluginError> {
        let found = match location {
            "$PANEL" => context.panel.iter().cloned().collect(),
            "$OTHER" => context.other.iter().cloned().collect(),
            "$SELECTION" => context.selection.clone(),
            "$HOME" => context.home.iter().filter_map(|h| VfsPath::from_local(h).ok()).collect(),
            text => {
                let parsed = if text.contains("://") {
                    VfsPath::parse(text)
                } else if Path::new(text).is_absolute() {
                    VfsPath::from_local(text)
                } else {
                    return Err(PluginError::Location {
                        plugin: self.name().to_owned(),
                        message: format!("{text:?} is neither absolute nor a placeholder"),
                    });
                };
                vec![parsed.map_err(|e| PluginError::Location {
                    plugin: self.name().to_owned(),
                    message: e.to_string(),
                })?]
            }
        };
        Ok(found)
    }

    /// Runs `command` with `context`. The caller checks the grant first
    /// ([`Grants::is_granted`]); `output` receives what the plugin prints.
    #[cfg(feature = "engine")]
    pub async fn run(
        &self,
        command: &str,
        context: &Context,
        registry: std::sync::Arc<nmc_vfs::ProviderRegistry>,
        output: crate::Output,
    ) -> Result<(), PluginError> {
        let failed = |error| PluginError::Script { plugin: self.name().to_owned(), error };
        if !self.provides(command) {
            return Err(PluginError::NotFound(format!(
                "plugin {} has no command {command:?}",
                self.name()
            )));
        }
        let main = self.dir.join(&self.manifest.main);
        let source = fs::read_to_string(&main)
            .map_err(|e| PluginError::Manifest { path: main.clone(), message: e.to_string() })?;
        let base = context
            .panel
            .as_ref()
            .filter(|panel| panel.scheme() == "file" && panel.container().is_none())
            .and_then(VfsPath::to_local_path)
            .or_else(|| context.home.clone())
            .unwrap_or_else(|| self.dir.clone());
        let limits = self.manifest.limits;
        let options = crate::ScriptOptions {
            registry,
            base,
            permissions: self.permissions(context)?,
            args: Vec::new(),
            output,
            heap_limit: limits.memory.max(16) * 1024 * 1024,
            time_limit: Some(std::time::Duration::from_secs(limits.timeout.max(1))),
            invocation: Some(crate::Invocation::new(
                command,
                context.panel.as_ref(),
                context.other.as_ref(),
                &context.selection,
            )),
        };
        crate::run(&main.display().to_string(), &source, options).await.map_err(failed)
    }
}

/// Where a command starts: the front end fills this in.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The active panel's folder.
    pub panel: Option<VfsPath>,
    /// The other panel's folder.
    pub other: Option<VfsPath>,
    /// The selected entries, or the one under the cursor.
    pub selection: Vec<VfsPath>,
    /// The user's home folder.
    pub home: Option<PathBuf>,
}

fn validate(manifest: &Manifest) -> Result<(), String> {
    let name_ok = !manifest.name.is_empty()
        && manifest.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !name_ok {
        return Err(format!("name {:?}: use lowercase letters, digits and dashes", manifest.name));
    }
    let main = Path::new(&manifest.main);
    if main.is_absolute() || main.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(format!("main {:?} must be inside the plugin folder", manifest.main));
    }
    if manifest.commands.is_empty() && manifest.columns.is_empty() && manifest.previewers.is_empty()
    {
        return Err("a plugin needs at least one command, column or previewer".into());
    }
    for (kind, ids) in [
        ("column", manifest.columns.iter().map(|c| (&c.id, &c.patterns)).collect::<Vec<_>>()),
        ("previewer", manifest.previewers.iter().map(|p| (&p.id, &p.patterns)).collect()),
    ] {
        let mut seen: Vec<&String> = Vec::new();
        for (id, patterns) in ids {
            if id.is_empty() {
                return Err(format!("a {kind} needs an id"));
            }
            if seen.contains(&id) {
                return Err(format!("{kind} {id:?} appears twice"));
            }
            seen.push(id);
            if patterns.is_empty() {
                return Err(format!("{kind} {id:?} needs \"match\" masks such as [\"*.jpg\"]"));
            }
        }
    }
    let mut ids: Vec<&str> = manifest.commands.iter().map(|c| c.id.as_str()).collect();
    ids.sort_unstable();
    if let Some(pair) = ids.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(format!("command {:?} appears twice", pair[0]));
    }
    if ids.iter().any(|id| id.is_empty()) {
        return Err("a command needs an id".into());
    }
    Ok(())
}

/// The plugins installed in `dir` (one folder each), sorted by name, and a
/// message for each folder that is not a valid plugin.
pub fn discover(dir: &Path) -> (Vec<Plugin>, Vec<String>) {
    let mut plugins: Vec<Plugin> = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return (plugins, problems);
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        match Plugin::load(&entry.path()) {
            Ok(plugin) if plugins.iter().any(|p| p.name() == plugin.name()) => {
                problems.push(format!(
                    "{}: plugin {} is installed twice",
                    entry.path().display(),
                    plugin.name()
                ));
            }
            Ok(plugin) => plugins.push(plugin),
            Err(error) => problems.push(error.to_string()),
        }
    }
    plugins.sort_by(|a, b| a.name().cmp(b.name()));
    (plugins, problems)
}

/// Copies the plugin in `source` into `plugins_dir` (replacing an older
/// copy) and returns it as installed.
pub fn install(source: &Path, plugins_dir: &Path) -> Result<Plugin, PluginError> {
    let plugin = Plugin::load(source)?;
    let io = |path: &Path, e: std::io::Error| PluginError::Manifest {
        path: path.to_path_buf(),
        message: e.to_string(),
    };
    let target = plugins_dir.join(plugin.name());
    if target.exists() {
        fs::remove_dir_all(&target).map_err(|e| io(&target, e))?;
    }
    copy_tree(source, &target).map_err(|e| io(&target, e))?;
    Plugin::load(&target)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        // The repository a plugin came from is not part of the plugin.
        if entry.file_name() == ".git" {
            continue;
        }
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The permissions the user granted, by plugin. A grant holds the exact
/// request it answered: when an update asks for more, it must be granted
/// again.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    path: PathBuf,
    granted: BTreeMap<String, Requested>,
}

impl Grants {
    /// The grants stored in `path` (none when the file does not exist).
    pub fn load(path: &Path) -> Result<Self, PluginError> {
        let failed = |message: String| PluginError::Grants { path: path.to_path_buf(), message };
        let granted = match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| failed(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(failed(e.to_string())),
        };
        Ok(Grants { path: path.to_path_buf(), granted })
    }

    /// Whether `plugin` may run with what its manifest asks for now.
    pub fn is_granted(&self, plugin: &Plugin) -> bool {
        self.granted.get(plugin.name()) == Some(&plugin.manifest.permissions)
    }

    /// Grants `plugin` what its manifest asks for, and saves.
    pub fn grant(&mut self, plugin: &Plugin) -> Result<(), PluginError> {
        self.granted.insert(plugin.name().to_owned(), plugin.manifest.permissions.clone());
        self.save()
    }

    /// Withdraws the grant of the plugin `name`, and saves.
    pub fn revoke(&mut self, name: &str) -> Result<(), PluginError> {
        self.granted.remove(name);
        self.save()
    }

    fn save(&self) -> Result<(), PluginError> {
        let failed = |message: String| PluginError::Grants { path: self.path.clone(), message };
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| failed(e.to_string()))?;
        }
        let text =
            serde_json::to_string_pretty(&self.granted).map_err(|e| failed(e.to_string()))?;
        fs::write(&self.path, text).map_err(|e| failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Result<Manifest, String> {
        let manifest: Manifest = serde_json::from_str(json).map_err(|e| e.to_string())?;
        validate(&manifest)?;
        Ok(manifest)
    }

    #[test]
    fn a_minimal_manifest_gets_defaults() {
        let m =
            manifest(r#"{"name":"x","version":"1","commands":[{"id":"a","title":"A"}]}"#).unwrap();
        assert_eq!(m.main, "main.ts");
        assert_eq!(m.limits, Limits::default());
        assert_eq!(m.permissions.describe(), ["no access to files"]);
    }

    #[test]
    fn bad_manifests_are_refused_with_a_reason() {
        let cases = [
            (r#"{"name":"Bad Name","version":"1","commands":[{"id":"a","title":"A"}]}"#, "name"),
            (r#"{"name":"x","version":"1","commands":[]}"#, "at least one"),
            (
                r#"{"name":"x","version":"1","main":"../evil.ts","commands":[{"id":"a","title":"A"}]}"#,
                "inside",
            ),
            (
                r#"{"name":"x","version":"1","commands":[{"id":"a","title":"A"},{"id":"a","title":"B"}]}"#,
                "twice",
            ),
            (
                r#"{"name":"x","version":"1","commands":[{"id":"a","title":"A"}],"permissions":{"net":["*"]}}"#,
                "net",
            ),
        ];
        for (json, expected) in cases {
            let error = manifest(json).unwrap_err();
            assert!(error.contains(expected), "{json}: {error}");
        }
    }

    #[test]
    fn placeholders_follow_the_panels() {
        // An absolute path on this system (Windows paths need a drive).
        let abs = |p: &str| if cfg!(windows) { format!("C:{p}") } else { p.to_owned() };
        let manifest_text = r#"{"name":"x","version":"1","commands":[{"id":"a","title":"A"}],
                    "permissions":{"read":["$SELECTION","HOSTS"],"write":["$OTHER"]}}"#
            .replace("HOSTS", &abs("/etc/hosts"));
        let plugin = Plugin {
            dir: PathBuf::from("/plugins/x"),
            manifest: manifest(&manifest_text).unwrap(),
        };
        let path = |p: &str| VfsPath::from_local(abs(p)).unwrap();
        let context = Context {
            panel: Some(path("/home/me")),
            other: Some(path("/backup")),
            selection: vec![path("/home/me/a.txt")],
            home: None,
        };
        let permissions = plugin.permissions(&context).unwrap();
        use crate::Access::{Read, Write};
        assert!(permissions.check(&path("/home/me/a.txt"), Read).is_ok());
        assert!(permissions.check(&path("/home/me/b.txt"), Read).is_err());
        assert!(permissions.check(&path("/etc/hosts"), Read).is_ok());
        assert!(permissions.check(&path("/backup/new"), Write).is_ok());
        assert!(permissions.check(&path("/home/me/a.txt"), Write).is_err());
        assert_eq!(
            plugin.manifest.permissions.describe(),
            [
                "read the selected files and folders",
                format!("read {}", abs("/etc/hosts")).as_str(),
                "read and change the folder in the other panel"
            ]
        );

        let relative = Plugin {
            manifest: Manifest {
                permissions: Requested { read: vec!["docs".into()], ..Requested::default() },
                ..plugin.manifest.clone()
            },
            ..plugin
        };
        assert!(relative.permissions(&context).is_err());
    }

    #[test]
    fn columns_and_previewers_are_declared_in_the_manifest() {
        let m = manifest(
            r#"{"name":"x","version":"1",
                "columns":[{"id":"pages","title":"Pages","match":["*.pdf"],"width":6}],
                "previewers":[{"id":"csv","title":"CSV table","match":["*.csv","*.tsv"]}]}"#,
        )
        .unwrap();
        assert!(m.commands.is_empty(), "a plugin may only add columns and previewers");
        let plugin = Plugin { dir: PathBuf::from("/plugins/x"), manifest: m };
        assert!(plugin.provides("column:pages") && plugin.provides("preview:csv"));
        assert!(!plugin.provides("column:csv") && !plugin.provides("pages"));
        assert_eq!(plugin.column("pages").unwrap().width, Some(6));
        for (json, expected) in [
            (
                r#"{"name":"x","version":"1","columns":[{"id":"a","title":"A","match":[]}]}"#,
                "match",
            ),
            (
                r#"{"name":"x","version":"1","columns":[{"id":"a","title":"A","match":["*"]},{"id":"a","title":"B","match":["*"]}]}"#,
                "twice",
            ),
            (
                r#"{"name":"x","version":"1","previewers":[{"id":"","title":"A","match":["*"]}]}"#,
                "id",
            ),
        ] {
            let error = manifest(json).unwrap_err();
            assert!(error.contains(expected), "{json}: {error}");
        }
    }

    #[test]
    fn names_are_matched_by_mask_ignoring_case() {
        let masks = |m: &[&str]| m.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(name_matches(&masks(&["*.jpg", "*.png"]), "Holiday.JPG"));
        assert!(name_matches(&masks(&["README*"]), "readme.md"));
        assert!(name_matches(&masks(&["*"]), "anything"));
        assert!(name_matches(&masks(&["a?c"]), "abc"));
        assert!(!name_matches(&masks(&["*.jpg"]), "jpg"));
        assert!(!name_matches(&masks(&["a?c"]), "ac"));
    }
}
