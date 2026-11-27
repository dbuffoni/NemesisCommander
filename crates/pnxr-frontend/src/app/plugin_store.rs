//! Browsing plugin repositories from the panels (Ctrl+X, "Browse and
//! install plugins…"; ADR 0054). Shared by the terminal and the window.
//!
//! The list shows every plugin of the configured repositories with its
//! origin (Official or Unofficial), whether it is installed and whether a
//! newer version exists. Enter installs or updates, Delete uninstalls
//! (after a question), F7 opens the list of repositories, where F7 adds one
//! (after a question naming it) and Delete removes one. The official
//! repository cannot be removed, only switched off with Enter.

use pnxr_config::PluginRepository;

use super::{App, AppEvent};
use crate::dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, InputPurpose, Overlay,
};
use crate::input::KeyPress;
use pnxr_config::Key;

/// One row of the plugin list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRow {
    /// The plugin's name.
    pub name: String,
    /// What finds it again in the catalogue (`name@official`, or the
    /// repository that lists it).
    pub selector: String,
    /// The installed version, if it is installed.
    pub installed: Option<String>,
    /// A newer version is in the catalogue.
    pub update: bool,
}

/// One row of the list of repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRow {
    /// `url` or `url#reference`.
    pub label: String,
    /// The built-in official repository.
    pub official: bool,
}

/// The text of the plugin list rows.
pub fn store_lines(rows: &[(String, StoreRow)]) -> Vec<String> {
    rows.iter().map(|(line, _)| line.clone()).collect()
}

fn store_dialog(items: Vec<String>, rows: Vec<StoreRow>) -> ChooseDialog {
    let mut dialog = ChooseDialog::new("Plugins", items, ChoosePurpose::PluginStore(rows));
    dialog.footer = crate::hints::list(
        "install or update",
        &["Del: uninstall", "F5: refresh", "F7: repositories"],
        "close",
    );
    dialog
}

/// A message as rows of the list, short enough to be read whole in a small
/// window (rows are not wrapped); a longer word, such as an address, keeps a
/// row of its own.
fn message_rows(text: &str) -> Vec<String> {
    const WIDTH: usize = 44;
    let mut rows: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match rows.last_mut() {
            Some(row) if row.chars().count() + 1 + word.chars().count() <= WIDTH => {
                row.push(' ');
                row.push_str(word);
            }
            _ => rows.push(word.to_owned()),
        }
    }
    rows
}

#[cfg(feature = "scripting")]
mod live {
    use std::path::PathBuf;

    use pnxr_config::AppPaths;
    use pnxr_script::catalog::{self, Row};
    use pnxr_script::plugin::Grants;
    use pnxr_script::source::Lock;

    use super::*;

    fn specs(config: &pnxr_config::PluginSettings) -> Vec<catalog::RepositorySpec> {
        let extra: Vec<_> = config
            .repositories
            .iter()
            .map(|r| catalog::RepositorySpec { url: r.url.clone(), reference: r.reference.clone() })
            .collect();
        catalog::repositories(config.official, &extra)
    }

    fn line(row: &Row) -> String {
        let state = row.state();
        let state = if state.is_empty() { String::new() } else { format!("  [{state}]") };
        let origin = if row.entry.official { "Official" } else { "Unofficial" };
        format!(
            "{:<10} {} {}{state}  {}",
            origin, row.entry.name, row.entry.version, row.entry.description
        )
    }

    fn to_row(row: &Row) -> StoreRow {
        let selector = if row.entry.official {
            format!("{}@official", row.entry.name)
        } else {
            format!("{}@{}", row.entry.name, row.entry.listed_in)
        };
        StoreRow {
            name: row.entry.name.clone(),
            selector,
            installed: row.installed.clone(),
            update: row.update,
        }
    }

    /// The catalogue rows with their lines, and what went wrong.
    pub(super) fn load(
        config: &pnxr_config::PluginSettings,
        paths: &AppPaths,
        refresh: bool,
    ) -> (Vec<(String, StoreRow)>, Vec<String>) {
        let (entries, problems) =
            catalog::available(&paths.plugin_catalog_dir(), &specs(config), refresh);
        let rows = catalog::rows(&entries, "", &paths.plugins_dir());
        (rows.iter().map(|r| (line(r), to_row(r))).collect(), problems)
    }

    pub(super) fn install(
        config: &pnxr_config::PluginSettings,
        paths: &AppPaths,
        selector: &str,
    ) -> Result<String, String> {
        let (entries, _) = catalog::available(&paths.plugin_catalog_dir(), &specs(config), false);
        let entry = catalog::find(&entries, selector).map_err(|e| e.to_string())?;
        let mut lock = Lock::load(&paths.plugin_lock_file()).map_err(|e| e.to_string())?;
        let plugin =
            catalog::install(entry, &paths.plugins_dir(), &mut lock).map_err(|e| e.to_string())?;
        Ok(format!(
            "Installed {} {}. It asks to {}; you are asked at its first use",
            plugin.name(),
            plugin.manifest.version,
            plugin.manifest.permissions.describe().join(", ")
        ))
    }

    pub(super) fn uninstall(paths: &AppPaths, name: &str) -> Result<(), String> {
        let dir: PathBuf = paths.plugins_dir().join(name);
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{name}: {e}"))?;
        Grants::load(&paths.plugin_grants_file())
            .and_then(|mut g| g.revoke(name))
            .map_err(|e| e.to_string())?;
        Lock::load(&paths.plugin_lock_file())
            .and_then(|mut l| l.forget(name))
            .map_err(|e| e.to_string())
    }

    /// How many plugins the repository lists (it must have some).
    pub(super) fn check(paths: &AppPaths, repository: &PluginRepository) -> Result<usize, String> {
        let spec = catalog::RepositorySpec {
            url: repository.url.clone(),
            reference: repository.reference.clone(),
        };
        if spec.is_official() {
            return Err("the official repository is built in".into());
        }
        let (entries, problems) =
            catalog::refresh(&paths.plugin_catalog_dir(), &spec).map_err(|e| e.to_string())?;
        if entries.is_empty() {
            return Err(format!(
                "{} has no plugins in a plugins/ folder{}",
                spec.url,
                problems.first().map(|p| format!(" ({p})")).unwrap_or_default()
            ));
        }
        Ok(entries.len())
    }

    pub(super) fn parse(text: &str) -> Option<PluginRepository> {
        catalog::RepositorySpec::parse(text)
            .map(|s| PluginRepository { url: s.url, reference: s.reference })
    }
}

#[cfg(not(feature = "scripting"))]
mod live {
    use pnxr_config::AppPaths;

    use super::*;

    const OFF: &str = "Plugins are not part of this build";

    pub(super) fn load(
        _: &pnxr_config::PluginSettings,
        _: &AppPaths,
        _: bool,
    ) -> (Vec<(String, StoreRow)>, Vec<String>) {
        (Vec::new(), vec![OFF.into()])
    }
    pub(super) fn install(
        _: &pnxr_config::PluginSettings,
        _: &AppPaths,
        _: &str,
    ) -> Result<String, String> {
        Err(OFF.into())
    }
    pub(super) fn uninstall(_: &AppPaths, _: &str) -> Result<(), String> {
        Err(OFF.into())
    }
    pub(super) fn check(_: &AppPaths, _: &PluginRepository) -> Result<usize, String> {
        Err(OFF.into())
    }
    pub(super) fn parse(_: &str) -> Option<PluginRepository> {
        None
    }
}

impl App {
    /// Opens the plugin list, which says it is loading, and loads it (from
    /// the cache unless `refresh`) in the background.
    pub(super) fn open_plugin_store(&mut self, refresh: bool) {
        let loading = vec!["Loading the plugin list…".to_owned()];
        self.overlay = Some(Overlay::Choose(store_dialog(loading, Vec::new())));
        let config = self.context.config.plugins.clone();
        let paths = self.context.paths.clone();
        self.spawn_task(async move {
            let (rows, problems) =
                tokio::task::spawn_blocking(move || live::load(&config, &paths, refresh))
                    .await
                    .unwrap_or_else(|e| (Vec::new(), vec![e.to_string()]));
            AppEvent::PluginStore { rows, problems }
        });
    }

    /// The list arrived; it is dropped when the user has left the window.
    /// What went wrong is written in the list, where it can be read.
    pub(super) fn show_plugin_store(
        &mut self,
        rows: Vec<(String, StoreRow)>,
        problems: Vec<String>,
    ) {
        if !matches!(&self.overlay, Some(Overlay::Choose(d))
            if matches!(d.purpose, ChoosePurpose::PluginStore(_)))
        {
            return;
        }
        let mut items = store_lines(&rows);
        let empty = items.is_empty();
        if empty && problems.is_empty() {
            items.push("No plugin found".to_owned());
        }
        for problem in &problems {
            items.extend(message_rows(&format!("! {problem}")));
        }
        if empty {
            items.extend(message_rows("F7 shows the repositories, F5 tries again"));
        }
        let rows = rows.into_iter().map(|(_, r)| r).collect();
        self.overlay = Some(Overlay::Choose(store_dialog(items, rows)));
    }

    /// Enter on a row of the plugin list.
    pub(super) fn plugin_store_chosen(&mut self, rows: &[StoreRow], cursor: usize) {
        let Some(row) = rows.get(cursor) else { return };
        if row.installed.is_some() && !row.update {
            return self.set_status(format!("{} is installed and up to date", row.name));
        }
        let config = self.context.config.plugins.clone();
        let paths = self.context.paths.clone();
        let selector = row.selector.clone();
        self.set_status(format!("Installing {}…", row.name));
        self.spawn_task(async move {
            let result =
                tokio::task::spawn_blocking(move || live::install(&config, &paths, &selector))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
            match result {
                Ok(message) => {
                    AppEvent::TaskDone { message, error: false, reload: false, focus: None }
                }
                Err(message) => {
                    AppEvent::TaskDone { message, error: true, reload: false, focus: None }
                }
            }
        });
    }

    /// Keys of the plugin list and the repository list; `None` when the key
    /// is not theirs.
    pub(super) fn plugin_store_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let key = press.plain();
        match &mut dialog.purpose {
            ChoosePurpose::PluginStore(rows) => match key {
                Some(Key::F(5)) => {
                    self.open_plugin_store(true);
                    Some(false)
                }
                Some(Key::F(7)) => {
                    self.open_plugin_repos();
                    Some(false)
                }
                Some(Key::Delete) => {
                    match rows.get(dialog.cursor).filter(|r| r.installed.is_some()) {
                        Some(row) => {
                            self.overlay = Some(Overlay::Confirm(ConfirmDialog {
                                title: "Uninstall plugin".into(),
                                lines: vec![
                                    format!("Uninstall {}?", row.name),
                                    "Its permissions are forgotten.".into(),
                                ],
                                purpose: ConfirmPurpose::UninstallPlugin(row.name.clone()),
                            }));
                            Some(false)
                        }
                        None => {
                            self.set_error("Only an installed plugin can be uninstalled");
                            Some(true)
                        }
                    }
                }
                _ => None,
            },
            ChoosePurpose::PluginRepos(rows) => match key {
                Some(Key::F(7) | Key::Insert) => {
                    self.ask(
                        "Add plugin repository",
                        "Git address (github:owner/repo, a URL; #branch-or-tag at the end):",
                        "",
                        InputPurpose::AddPluginRepo,
                    );
                    Some(false)
                }
                Some(Key::Delete) => {
                    match rows.get(dialog.cursor).cloned() {
                        Some(row) if row.official => {
                            self.set_error(
                                "The official repository cannot be removed (Enter switches it off)",
                            );
                        }
                        Some(row) => {
                            self.context.config.plugins.remove(&row.label);
                            self.save_settings();
                            self.refresh_plugin_repos(dialog);
                        }
                        None => {}
                    }
                    Some(true)
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Enter on a repository: the official one is switched on or off.
    pub(super) fn plugin_repo_chosen(&mut self, rows: &[RepoRow], cursor: usize) {
        if rows.get(cursor).is_some_and(|r| r.official) {
            let on = !self.context.config.plugins.official;
            self.context.config.plugins.official = on;
            self.save_settings();
            self.set_status(if on {
                "The official repository is on"
            } else {
                "The official repository is off"
            });
        }
        self.open_plugin_repos();
    }

    fn repo_lines(&self) -> (Vec<String>, Vec<RepoRow>) {
        let settings = &self.context.config.plugins;
        let mut items = vec![format!(
            "Official (built in) {}",
            if settings.official { "[on]" } else { "[off]" }
        )];
        let mut rows = vec![RepoRow { label: "official".into(), official: true }];
        for repository in &settings.repositories {
            items.push(repository.label());
            rows.push(RepoRow { label: repository.label(), official: false });
        }
        (items, rows)
    }

    fn refresh_plugin_repos(&mut self, dialog: &mut ChooseDialog) {
        let (items, rows) = self.repo_lines();
        dialog.items = items;
        dialog.purpose = ChoosePurpose::PluginRepos(rows);
        dialog.move_cursor(0);
    }

    /// The list of plugin repositories.
    pub(super) fn open_plugin_repos(&mut self) {
        let (items, rows) = self.repo_lines();
        let mut dialog =
            ChooseDialog::new("Plugin repositories", items, ChoosePurpose::PluginRepos(rows));
        dialog.footer =
            crate::hints::list("switch the official one", &["F7: add", "Del: remove"], "close");
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// The typed address of a repository to add: ask first.
    pub(super) fn plugin_repo_typed(&mut self, text: &str) {
        match live::parse(text.trim()) {
            Some(repository) => {
                self.overlay = Some(Overlay::Confirm(ConfirmDialog {
                    title: "Add plugin repository".into(),
                    lines: vec![
                        format!("Add {}?", repository.label()),
                        "Its plugins run code on your files once you allow them.".into(),
                    ],
                    purpose: ConfirmPurpose::AddPluginRepo(repository),
                }));
            }
            None => self.set_error(format!("{text:?} is not a Git repository address")),
        }
    }

    /// The question about a new repository was answered yes: check that it
    /// has plugins, then add it.
    pub(super) fn add_plugin_repo(&mut self, repository: PluginRepository) {
        let paths = self.context.paths.clone();
        self.set_status(format!("Checking {}…", repository.url));
        self.spawn_task(async move {
            let again = repository.clone();
            let result = tokio::task::spawn_blocking(move || live::check(&paths, &again))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            AppEvent::PluginRepoChecked { repository, result }
        });
    }

    /// The repository was checked.
    pub(super) fn plugin_repo_checked(
        &mut self,
        repository: PluginRepository,
        result: Result<usize, String>,
    ) {
        match result {
            Ok(count) => {
                let label = repository.label();
                if self.context.config.plugins.add(repository) && self.save_settings() {
                    self.set_status(format!("Added {label}: {count} plugins"));
                }
                self.open_plugin_repos();
            }
            Err(error) => self.set_error(format!("Not added: {error}")),
        }
    }

    /// The uninstall question was answered yes.
    pub(super) fn uninstall_plugin(&mut self, name: &str) {
        match live::uninstall(&self.context.paths, name) {
            Ok(()) => {
                self.apply_key_overrides();
                self.set_status(format!("Uninstalled {name}"));
                self.open_plugin_store(false);
            }
            Err(error) => self.set_error(error),
        }
    }
}
