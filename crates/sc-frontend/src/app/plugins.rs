//! Plugin commands and scripts from the panels (Alt+X).
//!
//! Alt+X lists the commands of the installed plugins, and, when the cursor
//! is on a `.ts` or `.js` file, "run this script". A plugin runs with what
//! its manifest asks for once the user allowed it; a script from a panel
//! may change only the folder it is in, after a confirmation each time.
//! Both run on a thread of their own, so a stuck script never blocks the
//! interface; their printed lines are shown when they end.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sc_vfs::VfsPath;

use super::{App, AppEvent};
use crate::dialogs::{ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, Overlay};

/// One row of the Alt+X list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginChoice {
    /// A command of the plugin in `dir`.
    Command {
        /// The plugin's folder.
        dir: PathBuf,
        /// The command's id.
        command: String,
    },
    /// The script file under the cursor.
    Script(PathBuf),
}

/// Whether `path` is a script the panels can run.
fn is_script(path: &VfsPath) -> bool {
    let local = path.scheme() == "file" && path.container().is_none();
    let name = path.file_name().unwrap_or("");
    local && [".ts", ".mts", ".js", ".mjs"].iter().any(|ext| name.ends_with(ext))
}

impl App {
    /// Alt+X: choose a plugin command (or the script under the cursor).
    pub(super) fn choose_plugin_command(&mut self) {
        if !cfg!(feature = "scripting") {
            self.set_error("Plugins and scripts are not part of this build");
            return;
        }
        let mut items = Vec::new();
        let mut choices = Vec::new();
        if let Some(current) = self.workspace.active().current_path().filter(is_script) {
            items.push(format!("Run script {}", current.file_name().unwrap_or("")));
            choices.push(PluginChoice::Script(PathBuf::from(current.path())));
        }
        let (plugins, problems) = list_plugins(&self.context.paths.plugins_dir());
        for (dir, name, commands) in plugins {
            for (id, title) in commands {
                items.push(format!("{name}: {title}"));
                choices.push(PluginChoice::Command { dir: dir.clone(), command: id });
            }
        }
        if let Some(problem) = problems.first() {
            self.set_error(problem.clone());
        }
        if items.is_empty() {
            self.set_status(
                "No plugins installed: add one with `sc plugin install FOLDER`, \
                 or put the cursor on a .ts script",
            );
            return;
        }
        let dialog =
            ChooseDialog::new("Plugins and scripts", items, ChoosePurpose::Plugin(choices));
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// A row of the Alt+X list was chosen.
    pub(super) fn plugin_chosen(&mut self, choice: PluginChoice) {
        let folder = self.workspace.active().location().clone();
        let (title, lines) = match &choice {
            PluginChoice::Script(script) => {
                if folder.scheme() != "file" || folder.container().is_some() {
                    self.set_error("Scripts run from local folders only");
                    return;
                }
                let name = script.file_name().unwrap_or_default().to_string_lossy().into_owned();
                (
                    "Run script".to_owned(),
                    vec![
                        format!("Run {name}?"),
                        format!("It may read and change {}", super::display_location(&folder)),
                        "and read the other panel's folder.".to_owned(),
                    ],
                )
            }
            PluginChoice::Command { dir, .. } => {
                match permission_question(dir, &self.context.paths.plugin_grants_file()) {
                    Ok(None) => return self.run_plugin_choice(choice),
                    Ok(Some(lines)) => ("Allow plugin".to_owned(), lines),
                    Err(error) => return self.set_error(error),
                }
            }
        };
        self.overlay = Some(Overlay::Confirm(ConfirmDialog {
            title,
            lines,
            purpose: ConfirmPurpose::RunPlugin(choice),
        }));
    }

    /// Runs the choice (permission given).
    pub(super) fn run_plugin_choice(&mut self, choice: PluginChoice) {
        let panel = self.workspace.active().location().clone();
        let other = self.workspace.inactive().location().clone();
        let selection = self.targets();
        let registry = self.context.registry.clone();
        let grants = self.context.paths.plugin_grants_file();
        self.spawn_task(async move {
            let printed = Arc::new(Mutex::new(Vec::<String>::new()));
            let sink = printed.clone();
            let output: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
                sink.lock().unwrap_or_else(|e| e.into_inner()).push(line.to_owned())
            });
            let (title, result) =
                run_choice(choice, panel, other, selection, registry, output, &grants).await;
            let lines = printed.lock().unwrap_or_else(|e| e.into_inner()).clone();
            AppEvent::ScriptDone { title, lines, error: result.err() }
        });
    }
}

/// A plugin's folder, name and commands (id, title).
type PluginRow = (PathBuf, String, Vec<(String, String)>);

/// Installed plugins as (folder, name, [(command id, title)]), and problems.
#[cfg(feature = "scripting")]
fn list_plugins(dir: &Path) -> (Vec<PluginRow>, Vec<String>) {
    let (plugins, problems) = sc_script::plugin::discover(dir);
    let rows = plugins
        .into_iter()
        .map(|plugin| {
            let commands =
                plugin.manifest.commands.iter().map(|c| (c.id.clone(), c.title.clone())).collect();
            (plugin.dir.clone(), plugin.manifest.name.clone(), commands)
        })
        .collect();
    (rows, problems)
}

#[cfg(not(feature = "scripting"))]
fn list_plugins(_dir: &Path) -> (Vec<PluginRow>, Vec<String>) {
    (Vec::new(), Vec::new())
}

/// `None` when the plugin in `dir` is allowed already, otherwise the
/// question to ask.
#[cfg(feature = "scripting")]
fn permission_question(dir: &Path, grants: &Path) -> Result<Option<Vec<String>>, String> {
    use sc_script::plugin::{Grants, Plugin};
    let plugin = Plugin::load(dir).map_err(|e| e.to_string())?;
    let grants = Grants::load(grants).map_err(|e| e.to_string())?;
    if grants.is_granted(&plugin) {
        return Ok(None);
    }
    let mut lines = vec![format!("Allow plugin {} {} to:", plugin.name(), plugin.manifest.version)];
    lines
        .extend(plugin.manifest.permissions.describe().into_iter().map(|line| format!("  {line}")));
    lines.push("You are asked again if an update wants more.".into());
    Ok(Some(lines))
}

#[cfg(not(feature = "scripting"))]
fn permission_question(_dir: &Path, _grants: &Path) -> Result<Option<Vec<String>>, String> {
    Err("Plugins are not part of this build".into())
}

/// Runs a choice to its end: its title for messages, and its result.
#[cfg(feature = "scripting")]
async fn run_choice(
    choice: PluginChoice,
    panel: VfsPath,
    other: VfsPath,
    selection: Vec<VfsPath>,
    registry: Arc<sc_vfs::ProviderRegistry>,
    output: sc_script::Output,
    grants: &Path,
) -> (String, Result<(), String>) {
    use sc_script::plugin::{Context, Grants, Plugin};
    use sc_script::{Invocation, Permissions, ScriptOptions};

    match choice {
        PluginChoice::Command { dir, command } => {
            let plugin = match Plugin::load(&dir) {
                Ok(plugin) => plugin,
                Err(e) => return (dir.display().to_string(), Err(e.to_string())),
            };
            let title =
                plugin.command(&command).map(|c| c.title.clone()).unwrap_or(command.clone());
            let granted = Grants::load(grants).and_then(|mut grants| {
                if !grants.is_granted(&plugin) {
                    grants.grant(&plugin)?;
                }
                Ok(())
            });
            if let Err(e) = granted {
                return (title, Err(e.to_string()));
            }
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
            let context = Context {
                panel: Some(panel),
                other: Some(other),
                selection,
                home: home.map(PathBuf::from),
            };
            let result = plugin.run(&command, &context, registry, output).await;
            (title, result.map_err(|e| e.to_string()))
        }
        PluginChoice::Script(script) => {
            let name = script.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let source = match std::fs::read_to_string(&script) {
                Ok(source) => source,
                Err(e) => return (name, Err(format!("{}: {e}", script.display()))),
            };
            let mut permissions = Permissions::none();
            permissions.allow_write(panel.clone()).allow_read(other.clone());
            let options = ScriptOptions {
                registry,
                base: PathBuf::from(panel.path()),
                permissions,
                args: Vec::new(),
                output,
                heap_limit: sc_script::DEFAULT_HEAP_LIMIT,
                time_limit: None,
                invocation: Some(Invocation::new("run", Some(&panel), Some(&other), &selection)),
            };
            let result = sc_script::run(&script.display().to_string(), &source, options).await;
            (name, result.map_err(|e| e.to_string()))
        }
    }
}

#[cfg(not(feature = "scripting"))]
async fn run_choice(
    _choice: PluginChoice,
    _panel: VfsPath,
    _other: VfsPath,
    _selection: Vec<VfsPath>,
    _registry: Arc<sc_vfs::ProviderRegistry>,
    _output: Arc<dyn Fn(&str) + Send + Sync>,
    _grants: &Path,
) -> (String, Result<(), String>) {
    ("Plugins".into(), Err("Plugins are not part of this build".into()))
}
