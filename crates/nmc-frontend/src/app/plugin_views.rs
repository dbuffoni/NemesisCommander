//! Panel columns and viewers added by plugins (SC-30).
//!
//! A plugin declares `columns` and `previewers` in its manifest. They run
//! the plugin's script as the commands `column:ID` and `preview:ID` (see
//! `nmc_script::plugin`), and only for plugins the user has already
//! allowed: nothing runs by itself before that.

use std::collections::HashMap;

use nmc_vfs::VfsPath;

use super::App;
#[cfg(feature = "scripting")]
use super::AppEvent;
use nmc_panel::SideId;

use crate::app::side_index;

/// A plugin column of a panel: heading and width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnHead {
    /// The heading.
    pub title: String,
    /// Width in characters.
    pub width: usize,
}

/// What the plugin columns hold for one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnCells {
    /// The folder they belong to.
    pub folder: Option<VfsPath>,
    /// The columns, left to right.
    pub heads: Vec<ColumnHead>,
    /// The text of each column for each entry name.
    pub rows: HashMap<String, Vec<String>>,
}

impl App {
    /// The plugin columns to show in `side`'s panel (none while the folder
    /// is not the one they were computed for).
    pub fn plugin_columns(&self, side: SideId) -> &[ColumnHead] {
        let cells = &self.column_cells[side_index(side)];
        if cells.folder.as_ref() == Some(self.workspace.side(side).panel().location()) {
            &cells.heads
        } else {
            &[]
        }
    }

    /// The text of the plugin columns for the entry `name` of `side`.
    pub fn plugin_cells(&self, side: SideId, name: &str) -> Vec<String> {
        let heads = self.plugin_columns(side).len();
        let cells = &self.column_cells[side_index(side)];
        match cells.rows.get(name) {
            Some(texts) if heads > 0 => texts.clone(),
            _ => vec![String::new(); heads],
        }
    }

    /// A listing arrived: the folder's plugin columns are computed again.
    pub(super) fn refresh_plugin_columns(&mut self, folder: &VfsPath) {
        for side in [SideId::Left, SideId::Right] {
            if self.workspace.side(side).panel().location() == folder {
                let index = side_index(side);
                if self.column_cells[index].folder.as_ref() != Some(folder) {
                    self.column_cells[index] = ColumnCells::default();
                }
            }
        }
        #[cfg(feature = "scripting")]
        self.start_plugin_columns(folder);
    }

    /// The columns of a folder arrived.
    pub(super) fn set_plugin_cells(&mut self, cells: ColumnCells) {
        let Some(folder) = cells.folder.clone() else { return };
        for side in [SideId::Left, SideId::Right] {
            if self.workspace.side(side).panel().location() == &folder {
                self.column_cells[side_index(side)] = cells.clone();
            }
        }
    }
}

#[cfg(feature = "scripting")]
mod run {
    use std::sync::{Arc, Mutex};

    use nmc_script::plugin::{COLUMN_FILES, Grants, Plugin, discover, name_matches};

    use super::*;
    use crate::app::plugins::{PluginChoice, run_choice};

    /// The installed plugins the user has allowed.
    fn granted_plugins(paths: &nmc_config::AppPaths) -> Vec<Plugin> {
        let (plugins, _) = discover(&paths.plugins_dir());
        let Ok(grants) = Grants::load(&paths.plugin_grants_file()) else { return Vec::new() };
        plugins.into_iter().filter(|p| grants.is_granted(p)).collect()
    }

    /// What `command` of `plugin` prints for `selection`.
    async fn print_of(
        plugin: &Plugin,
        command: String,
        folder: &VfsPath,
        selection: Vec<VfsPath>,
        registry: Arc<nmc_vfs::ProviderRegistry>,
        paths: &nmc_config::AppPaths,
    ) -> Result<Vec<String>, String> {
        let printed = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = printed.clone();
        let output: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line| {
            sink.lock().unwrap_or_else(|e| e.into_inner()).push(line.to_owned())
        });
        let choice = PluginChoice::Command { dir: plugin.dir.clone(), command };
        let (_, result) =
            run_choice(choice, folder.clone(), folder.clone(), selection, registry, output, paths)
                .await;
        result?;
        let lines = printed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Ok(lines)
    }

    impl App {
        pub(super) fn start_plugin_columns(&mut self, folder: &VfsPath) {
            let Some(panel) = [SideId::Left, SideId::Right]
                .into_iter()
                .map(|side| self.workspace.side(side).panel())
                .find(|panel| panel.location() == folder)
            else {
                return;
            };
            let names: Vec<String> = panel
                .entries()
                .iter()
                .filter(|e| !e.is_dir() && e.name != nmc_panel::PARENT)
                .map(|e| e.name.clone())
                .collect();
            if names.is_empty() || folder.container().is_some() {
                return;
            }
            let paths = self.context.paths.clone();
            let registry = self.context.registry.clone();
            let folder = folder.clone();
            self.spawn_task(async move {
                let mut cells =
                    ColumnCells { folder: Some(folder.clone()), ..ColumnCells::default() };
                for plugin in granted_plugins(&paths) {
                    for column in &plugin.manifest.columns {
                        let files: Vec<String> = names
                            .iter()
                            .filter(|n| name_matches(&column.patterns, n))
                            .take(COLUMN_FILES)
                            .cloned()
                            .collect();
                        if files.is_empty() {
                            continue;
                        }
                        let selection: Vec<VfsPath> =
                            files.iter().map(|n| folder.join(n)).collect();
                        let lines = print_of(
                            &plugin,
                            format!("column:{}", column.id),
                            &folder,
                            selection,
                            registry.clone(),
                            &paths,
                        )
                        .await
                        .unwrap_or_else(|e| vec![format!("! {e}")]);
                        let index = cells.heads.len();
                        cells.heads.push(ColumnHead {
                            title: column.title.clone(),
                            width: usize::from(column.width.unwrap_or(12)).clamp(3, 40),
                        });
                        for row in cells.rows.values_mut() {
                            row.push(String::new());
                        }
                        for (position, name) in files.iter().enumerate() {
                            let text = lines.get(position).cloned().unwrap_or_default();
                            let row = cells
                                .rows
                                .entry(name.clone())
                                .or_insert_with(|| vec![String::new(); index + 1]);
                            row.resize(index + 1, String::new());
                            row[index] = text;
                        }
                    }
                }
                AppEvent::PluginCells(Box::new(cells))
            });
        }
    }

    /// The text a previewer plugin makes for `path`: `Some(markdown)` when
    /// an allowed plugin has a previewer for its name.
    pub(crate) async fn preview_text(
        registry: Arc<nmc_vfs::ProviderRegistry>,
        paths: &nmc_config::AppPaths,
        path: &VfsPath,
    ) -> Option<Result<String, String>> {
        let name = path.file_name()?.to_owned();
        let folder = path.parent()?;
        for plugin in granted_plugins(paths) {
            for previewer in &plugin.manifest.previewers {
                if !name_matches(&previewer.patterns, &name) {
                    continue;
                }
                let command = format!("preview:{}", previewer.id);
                let result =
                    print_of(&plugin, command, &folder, vec![path.clone()], registry, paths).await;
                return Some(result.map(|lines| lines.join("\n")));
            }
        }
        None
    }
}

#[cfg(feature = "scripting")]
pub(crate) use run::preview_text;

/// Without scripting there are no previewers.
#[cfg(not(feature = "scripting"))]
pub(crate) async fn preview_text(
    _registry: std::sync::Arc<nmc_vfs::ProviderRegistry>,
    _paths: &nmc_config::AppPaths,
    _path: &VfsPath,
) -> Option<Result<String, String>> {
    None
}
