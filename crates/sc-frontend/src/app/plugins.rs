//! Plugin commands and scripts from the panels (Alt+X).
//!
//! Alt+X lists the commands of the installed plugins, and, when the cursor
//! is on a `.ts` or `.js` file, "run this script". A plugin runs with what
//! its manifest asks for once the user allowed it; a script from a panel
//! may change only the folder it is in, after a confirmation each time.
//! Both run on a thread of their own, so a stuck script never blocks the
//! interface; their printed lines are shown when they end.
//!
//! Built without `script-engine` (the desktop window), there is no V8 here:
//! the chosen command runs in the `sc` program installed next to this one,
//! which has the same settings folders.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sc_config::AppPaths;
use sc_vfs::VfsPath;

use sc_config::{Action, Category};

use super::{App, AppEvent};
use crate::commands::{CommandEntry, SCRIPTS, Target, plugin_category};
use crate::dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, InputPurpose, Overlay,
};

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
    /// A script of the user's scripts folder: it runs like a script from a
    /// panel, without the question, since the user put it there.
    UserScript(PathBuf),
}

/// Script file name endings.
const SCRIPT_EXTENSIONS: [&str; 4] = [".ts", ".mts", ".js", ".mjs"];

/// Whether `path` is a script the panels can run.
fn is_script(path: &VfsPath) -> bool {
    let local = path.scheme() == "file" && path.container().is_none();
    let name = path.file_name().unwrap_or("");
    local && SCRIPT_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
}

/// The scripts in `dir`, sorted by name.
fn user_scripts(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut scripts: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            SCRIPT_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
        })
        .collect();
    scripts.sort();
    scripts
}

impl App {
    /// Every command for the command list (F9): the built-in ones, the
    /// commands of the installed plugins, the user's scripts and the script
    /// under the cursor.
    pub fn commands(&self) -> Vec<CommandEntry> {
        let mut entries: Vec<CommandEntry> =
            Action::ALL.iter().map(|&a| CommandEntry::action(a)).collect();
        if !cfg!(feature = "scripting") {
            return entries;
        }
        let (plugins, _) = list_plugins(&self.context.paths.plugins_dir());
        for (dir, name, commands) in plugins {
            for (id, title, category) in commands {
                entries.push(CommandEntry {
                    id: format!("plugin.{name}.{id}"),
                    category: plugin_category(category.as_deref()),
                    label: title,
                    target: Target::Plugin(PluginChoice::Command { dir: dir.clone(), command: id }),
                });
            }
        }
        for script in user_scripts(&self.context.paths.scripts_dir()) {
            let name = script.file_name().unwrap_or_default().to_string_lossy().into_owned();
            entries.push(CommandEntry {
                id: format!("script.{name}"),
                category: SCRIPTS.to_owned(),
                label: name,
                target: Target::Plugin(PluginChoice::UserScript(script)),
            });
        }
        if let Some(current) = self.workspace.active().current_path().filter(is_script) {
            entries.push(CommandEntry {
                id: String::new(),
                category: SCRIPTS.to_owned(),
                label: format!("Run {}", current.file_name().unwrap_or("")),
                target: Target::Plugin(PluginChoice::Script(
                    current.to_local_path().unwrap_or_default(),
                )),
            });
        }
        // Plugin commands join the built-in ones of their category.
        let order = |category: &str| {
            Category::ALL.iter().position(|c| c.title() == category).unwrap_or(Category::ALL.len())
        };
        entries.sort_by_key(|e| (order(&e.category), e.category == SCRIPTS));
        entries
    }

    /// The name of command `id` for messages (`Files/Copy`).
    pub fn command_label(&self, id: &str) -> String {
        if let Some(action) = Action::from_id(id) {
            return CommandEntry::action(action).name();
        }
        self.commands().into_iter().find(|e| e.id == id).map(|e| e.name()).unwrap_or(id.to_owned())
    }

    /// Runs the command `id` (built-in, plugin or script); `false` when
    /// there is no such command.
    pub fn run_command(&mut self, id: &str) -> bool {
        if let Some(action) = Action::from_id(id) {
            self.perform(action);
            return true;
        }
        match self.commands().into_iter().find(|e| e.id == id) {
            Some(entry) => {
                self.run_entry(entry.target);
                true
            }
            None => false,
        }
    }

    /// Runs a row of the command list.
    pub(super) fn run_entry(&mut self, target: Target) {
        match target {
            Target::Action(action) => self.perform(action),
            Target::Plugin(choice) => self.plugin_chosen(choice),
        }
    }

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
            choices.push(PluginChoice::Script(current.to_local_path().unwrap_or_default()));
        }
        let (plugins, problems) = list_plugins(&self.context.paths.plugins_dir());
        for (dir, name, commands) in plugins {
            for (id, title, _) in commands {
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
            PluginChoice::UserScript(_)
                if folder.scheme() != "file" || folder.container().is_some() =>
            {
                return self.set_error("Scripts run from local folders only");
            }
            PluginChoice::UserScript(_) => return self.run_plugin_choice(choice),
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
        let paths = self.context.paths.clone();
        self.spawn_task(async move {
            let printed = Arc::new(Mutex::new(Vec::<String>::new()));
            let sink = printed.clone();
            let output: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
                sink.lock().unwrap_or_else(|e| e.into_inner()).push(line.to_owned())
            });
            let (title, result) =
                run_choice(choice, panel, other, selection, registry, output, &paths).await;
            let lines = printed.lock().unwrap_or_else(|e| e.into_inner()).clone();
            AppEvent::ScriptDone { title, lines, error: result.err() }
        });
    }
}

impl App {
    /// Tools, "Write a script with AI…": asks what the script should do.
    pub(super) fn ask_ai_script(&mut self) {
        if !cfg!(feature = "scripting") {
            self.set_error("Plugins and scripts are not part of this build");
            return;
        }
        self.ask(
            "Write a script with AI",
            "What should the script do? (Claude writes and tests it)",
            "",
            InputPurpose::AiScript,
        );
    }

    /// Runs `sc ai new` with the typed description; the new script is in
    /// the command list (Scripts) when it passes its test.
    pub(super) fn write_script_with_ai(&mut self, description: String) {
        if description.trim().is_empty() {
            return;
        }
        let paths = self.context.paths.clone();
        self.set_status("Claude is writing and testing the script…");
        self.spawn_task(async move {
            let printed = Arc::new(Mutex::new(Vec::<String>::new()));
            let sink = printed.clone();
            let output: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
                sink.lock().unwrap_or_else(|e| e.into_inner()).push(line.to_owned())
            });
            let args = vec!["ai".to_owned(), "new".to_owned(), description];
            let result = run_sc(args, &paths, None, output).await;
            let lines = printed.lock().unwrap_or_else(|e| e.into_inner()).clone();
            AppEvent::ScriptDone {
                title: "Write a script with AI".into(),
                lines,
                error: result.err(),
            }
        });
    }
}

impl App {
    /// Files, "Multi-rename with AI…": asks how to rename `targets`.
    pub(super) fn ask_ai_rename(&mut self, targets: Vec<VfsPath>) {
        if !cfg!(feature = "scripting") {
            self.set_error("AI is not part of this build");
            return;
        }
        self.ask(
            "Multi-rename with AI",
            "How should the names change? (Claude writes the rule)",
            "",
            InputPurpose::AiRename(targets),
        );
    }

    /// Runs `sc ai rename` for `paths`; its rule is shown with the new
    /// names before anything is renamed.
    pub(super) fn write_rename_with_ai(&mut self, paths: Vec<VfsPath>, description: String) {
        if description.trim().is_empty() {
            return;
        }
        let config = self.context.paths.clone();
        self.set_status("Claude is writing the rename rule…");
        self.spawn_task(async move {
            let printed = Arc::new(Mutex::new(String::new()));
            let sink = printed.clone();
            let output: Arc<dyn Fn(&str) + Send + Sync> =
                Arc::new(move |line| sink.lock().unwrap_or_else(|e| e.into_inner()).push_str(line));
            let mut args = vec!["ai".into(), "rename".into(), "--json".into(), description];
            args.extend(paths.iter().map(VfsPath::to_string));
            let result = run_sc(args, &config, None, output).await.and_then(|()| {
                let text = printed.lock().unwrap_or_else(|e| e.into_inner()).clone();
                rename_answer(&text)
            });
            AppEvent::AiRenameDone { paths, result }
        });
    }
}

/// The rule and explanation in the JSON `sc ai rename --json` prints.
fn rename_answer(text: &str) -> Result<(sc_ops::RenameRule, String), String> {
    #[derive(serde::Deserialize)]
    struct Answer {
        explanation: String,
        rule: sc_ops::RenameRule,
    }
    let answer: Answer =
        serde_json::from_str(text).map_err(|e| format!("unexpected answer from sc: {e}"))?;
    Ok((answer.rule, answer.explanation))
}

/// A plugin's folder, name and commands (id, title, category).
type PluginRow = (PathBuf, String, Vec<(String, String, Option<String>)>);

/// Installed plugins as (folder, name, [(command id, title, category)]),
/// and problems.
#[cfg(feature = "scripting")]
fn list_plugins(dir: &Path) -> (Vec<PluginRow>, Vec<String>) {
    let (plugins, problems) = sc_script::plugin::discover(dir);
    let rows = plugins
        .into_iter()
        .map(|plugin| {
            let commands = plugin
                .manifest
                .commands
                .iter()
                .map(|c| (c.id.clone(), c.title.clone(), c.category.clone()))
                .collect();
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
#[cfg(feature = "script-engine")]
async fn run_choice(
    choice: PluginChoice,
    panel: VfsPath,
    other: VfsPath,
    selection: Vec<VfsPath>,
    registry: Arc<sc_vfs::ProviderRegistry>,
    output: sc_script::Output,
    paths: &AppPaths,
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
            let granted = Grants::load(&paths.plugin_grants_file()).and_then(|mut grants| {
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
        PluginChoice::Script(script) | PluginChoice::UserScript(script) => {
            let name = script.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let source = match std::fs::read_to_string(&script) {
                Ok(source) => source,
                Err(e) => return (name, Err(format!("{}: {e}", script.display()))),
            };
            let mut permissions = Permissions::none();
            permissions.allow_write(panel.clone()).allow_read(other.clone());
            let options = ScriptOptions {
                registry,
                base: panel.to_local_path().unwrap_or_default(),
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

/// Runs a choice with the `sc` program: what it prints is the output,
/// what it reports on failure the error.
#[cfg(all(feature = "scripting", not(feature = "script-engine")))]
async fn run_choice(
    choice: PluginChoice,
    panel: VfsPath,
    other: VfsPath,
    selection: Vec<VfsPath>,
    _registry: Arc<sc_vfs::ProviderRegistry>,
    output: Arc<dyn Fn(&str) + Send + Sync>,
    paths: &AppPaths,
) -> (String, Result<(), String>) {
    use sc_script::plugin::{Grants, Plugin};

    let title = match &choice {
        PluginChoice::Command { dir, command } => {
            let plugin = match Plugin::load(dir) {
                Ok(plugin) => plugin,
                Err(e) => return (dir.display().to_string(), Err(e.to_string())),
            };
            // The user allowed it (or had already): `sc` checks the grant.
            let granted = Grants::load(&paths.plugin_grants_file()).and_then(|mut grants| {
                if !grants.is_granted(&plugin) {
                    grants.grant(&plugin)?;
                }
                Ok(())
            });
            let title = plugin.command(command).map(|c| c.title.clone()).unwrap_or(command.clone());
            if let Err(e) = granted {
                return (title, Err(e.to_string()));
            }
            title
        }
        PluginChoice::Script(script) | PluginChoice::UserScript(script) => {
            script.file_name().unwrap_or_default().to_string_lossy().into_owned()
        }
    };
    let script = matches!(choice, PluginChoice::Script(_) | PluginChoice::UserScript(_));
    let folder = if script { panel.to_local_path() } else { None };
    let args = sc_arguments(&choice, &panel, &other, &selection);
    let result = run_sc(args, paths, folder, output).await;
    (title, result)
}

/// Runs the `sc` program with `args` (in `folder`, if given), with this
/// program's settings folders: what it prints goes to `output`, what it
/// reports on failure is the error.
#[cfg(feature = "scripting")]
async fn run_sc(
    args: Vec<String>,
    paths: &AppPaths,
    folder: Option<PathBuf>,
    output: Arc<dyn Fn(&str) + Send + Sync>,
) -> Result<(), String> {
    let program = sc_program();
    let mut command = std::process::Command::new(&program);
    command
        .args(args)
        .env("SC_CONFIG_DIR", &paths.config_dir)
        .env("SC_DATA_DIR", &paths.data_dir)
        .stdin(std::process::Stdio::null());
    if let Some(folder) = folder {
        command.current_dir(folder);
    }
    match tokio::task::spawn_blocking(move || command.output()).await {
        Ok(Ok(finished)) => {
            for line in String::from_utf8_lossy(&finished.stdout).lines() {
                output(line);
            }
            if finished.status.success() {
                Ok(())
            } else {
                let error = String::from_utf8_lossy(&finished.stderr);
                let error = error.trim().trim_start_matches("sc: ");
                Err(if error.is_empty() {
                    format!("sc ended with {}", finished.status)
                } else {
                    error.to_owned()
                })
            }
        }
        Ok(Err(e)) => Err(format!("cannot start {}: {e}", program.display())),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(feature = "scripting"))]
async fn run_sc(
    _args: Vec<String>,
    _paths: &AppPaths,
    _folder: Option<PathBuf>,
    _output: Arc<dyn Fn(&str) + Send + Sync>,
) -> Result<(), String> {
    Err("Plugins and scripts are not part of this build".into())
}

/// The `sc` program: `SC_PROGRAM` if set, else this program when it is
/// `sc` (the terminal UI), else the one next to it, else the one on the
/// `PATH`.
#[cfg(feature = "scripting")]
fn sc_program() -> PathBuf {
    if let Some(program) = std::env::var_os("SC_PROGRAM") {
        return program.into();
    }
    let name = format!("sc{}", std::env::consts::EXE_SUFFIX);
    let exe = std::env::current_exe().ok();
    if let Some(exe) = exe.as_ref().filter(|exe| exe.file_name() == Some(name.as_ref())) {
        return exe.clone();
    }
    exe.and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|sibling| sibling.is_file())
        .unwrap_or_else(|| name.into())
}

/// The command line of `sc` that runs `choice` from these panels.
#[cfg(all(feature = "scripting", not(feature = "script-engine")))]
fn sc_arguments(
    choice: &PluginChoice,
    panel: &VfsPath,
    other: &VfsPath,
    selection: &[VfsPath],
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    match choice {
        PluginChoice::Command { dir, command } => {
            let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            args.extend(["plugin".into(), "run".into(), name, command.clone()]);
            args.extend(selection.iter().map(VfsPath::to_string));
        }
        PluginChoice::Script(script) | PluginChoice::UserScript(script) => {
            args.extend(["run".into(), script.display().to_string()]);
            args.extend(["--allow-write".into(), panel.to_string()]);
            args.extend(["--allow-read".into(), other.to_string()]);
            for selected in selection {
                args.extend(["--selected".into(), selected.to_string()]);
            }
        }
    }
    args.extend(["--panel".into(), panel.to_string(), "--other".into(), other.to_string()]);
    args
}

#[cfg(not(feature = "scripting"))]
async fn run_choice(
    _choice: PluginChoice,
    _panel: VfsPath,
    _other: VfsPath,
    _selection: Vec<VfsPath>,
    _registry: Arc<sc_vfs::ProviderRegistry>,
    _output: Arc<dyn Fn(&str) + Send + Sync>,
    _paths: &AppPaths,
) -> (String, Result<(), String>) {
    ("Plugins".into(), Err("Plugins are not part of this build".into()))
}

#[cfg(all(test, feature = "scripting", not(feature = "script-engine")))]
mod tests {
    use super::*;

    #[test]
    fn sc_gets_the_panels_on_its_command_line() {
        let panel = VfsPath::parse("/work").unwrap();
        let other = VfsPath::parse("sftp://host/srv").unwrap();
        let selection = vec![panel.join("a.txt")];
        let script = PluginChoice::Script(PathBuf::from("/work/tidy.ts"));
        assert_eq!(
            sc_arguments(&script, &panel, &other, &selection),
            [
                "run",
                "/work/tidy.ts",
                "--allow-write",
                "file:///work",
                "--allow-read",
                "sftp://host/srv",
                "--selected",
                "file:///work/a.txt",
                "--panel",
                "file:///work",
                "--other",
                "sftp://host/srv"
            ]
        );
        let command =
            PluginChoice::Command { dir: PathBuf::from("/p/rename-all"), command: "go".into() };
        assert_eq!(
            sc_arguments(&command, &panel, &other, &selection)[..5],
            ["plugin", "run", "rename-all", "go", "file:///work/a.txt"]
        );
    }
}

#[cfg(test)]
mod rename_tests {
    #[test]
    fn the_rule_is_read_from_sc() {
        let text = r#"{"explanation":"Date first.","names":["b"],"rule":{"template":"[N][X]",
            "find":"(\\d+)","replace":"x$1","regex":true,"case":"Lower","counter_start":1,
            "counter_step":1,"counter_width":1}}"#;
        let (rule, explanation) = super::rename_answer(text).unwrap();
        assert_eq!(explanation, "Date first.");
        assert!(rule.regex);
        assert_eq!(rule.find, "(\\d+)");
        assert_eq!(rule.case, sc_ops::CaseChange::Lower);
        assert!(super::rename_answer("oops").is_err());
    }
}
