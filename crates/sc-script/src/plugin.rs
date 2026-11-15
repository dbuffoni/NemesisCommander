//! Plugins: a folder with a `plugin.json` manifest and a script whose
//! commands appear in the front ends (ADR 0016).
//!
//! ```json
//! {
//!   "name": "tidy",
//!   "version": "1.0.0",
//!   "description": "Moves old downloads away",
//!   "main": "main.ts",
//!   "commands": [{ "id": "tidy", "title": "Tidy this folder" }],
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
use std::time::Duration;

use sc_vfs::VfsPath;
use serde::{Deserialize, Serialize};

use crate::{Invocation, Permissions, ScriptError, ScriptOptions};

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
    /// The script, relative to the plugin folder.
    #[serde(default = "default_main")]
    pub main: String,
    /// The commands the plugin adds.
    pub commands: Vec<CommandSpec>,
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
    /// Passed to the script as `sc.context.command`.
    pub id: String,
    /// Shown in menus.
    pub title: String,
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
}

impl Requested {
    /// The request in words, one line per grant, for confirmation dialogs.
    pub fn describe(&self) -> Vec<String> {
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
        if lines.is_empty() {
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

    /// The permissions of one run: placeholders filled in from `context`.
    /// A placeholder with nothing to fill in (no selection, say) grants
    /// nothing.
    pub fn permissions(&self, context: &Context) -> Result<Permissions, PluginError> {
        let requested = &self.manifest.permissions;
        if requested.all {
            return Ok(Permissions::all());
        }
        let mut permissions = Permissions::none();
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
    pub async fn run(
        &self,
        command: &str,
        context: &Context,
        registry: std::sync::Arc<sc_vfs::ProviderRegistry>,
        output: crate::Output,
    ) -> Result<(), PluginError> {
        let failed = |error| PluginError::Script { plugin: self.name().to_owned(), error };
        if self.command(command).is_none() {
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
            .map(|panel| PathBuf::from(panel.path()))
            .or_else(|| context.home.clone())
            .unwrap_or_else(|| self.dir.clone());
        let limits = self.manifest.limits;
        let options = ScriptOptions {
            registry,
            base,
            permissions: self.permissions(context)?,
            args: Vec::new(),
            output,
            heap_limit: limits.memory.max(16) * 1024 * 1024,
            time_limit: Some(Duration::from_secs(limits.timeout.max(1))),
            invocation: Some(Invocation::new(
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
    if manifest.commands.is_empty() {
        return Err("a plugin needs at least one command".into());
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
        let plugin = Plugin {
            dir: PathBuf::from("/plugins/x"),
            manifest: manifest(
                r#"{"name":"x","version":"1","commands":[{"id":"a","title":"A"}],
                    "permissions":{"read":["$SELECTION","/etc/hosts"],"write":["$OTHER"]}}"#,
            )
            .unwrap(),
        };
        let path = |p: &str| VfsPath::from_local(p).unwrap();
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
                "read /etc/hosts",
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
}
