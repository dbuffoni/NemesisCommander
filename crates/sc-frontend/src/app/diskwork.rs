//! What takes the space on disks: folder sizes in the panel, the disk
//! usage list, the duplicate finder and the table of disks.

use std::collections::{BTreeMap, HashSet};
use std::time::SystemTime;

use futures::StreamExt;
use sc_config::{Action, Key};
use sc_panel::format::human_size;
use sc_vfs::{HashAlgorithm, VfsPath};

use super::{App, AppEvent, display_location, done};
use crate::dialogs::{ChooseDialog, ChoosePurpose, Overlay};
use crate::input::KeyPress;
use crate::usage::{self, Usage};

/// A file's path and when it was last changed.
type Copy = (VfsPath, Option<SystemTime>);

impl App {
    // --------------------------------------------------------- folder sizes

    /// Counts the folders of the active panel (the selected ones, else all
    /// of them) and shows their sizes in the size column.
    pub(super) fn count_folder_sizes(&mut self) {
        let panel = self.workspace.active();
        let folder = panel.location().clone();
        let selected: Vec<String> = panel
            .entries()
            .iter()
            .filter(|e| e.is_dir() && e.name != ".." && panel.is_selected(&e.name))
            .map(|e| e.name.clone())
            .collect();
        let names: Vec<String> = if selected.is_empty() {
            panel
                .entries()
                .iter()
                .filter(|e| e.is_dir() && e.name != "..")
                .map(|e| e.name.clone())
                .collect()
        } else {
            selected
        };
        if names.is_empty() {
            return self.set_status("No folders to count");
        }
        let registry = self.context.registry.clone();
        self.set_status("Counting folders…");
        self.spawn_task(async move {
            let mut sizes = Vec::new();
            for name in names {
                let path = folder.join(&name);
                if let Ok(found) = usage::measure_total(&registry, &path).await {
                    sizes.push((path, found));
                }
            }
            AppEvent::FolderSizes { sizes }
        });
    }

    /// Remembers the counted sizes (shown until the folder is listed again).
    pub(super) fn folder_sizes_counted(&mut self, sizes: Vec<(VfsPath, (u64, u64))>) {
        let total: u64 = sizes.iter().map(|(_, (bytes, _))| bytes).sum();
        let count = sizes.len();
        for (path, found) in sizes {
            self.folder_sizes.insert(path, found);
        }
        self.set_status(format!(
            "{count} folder{}: {} in all",
            if count == 1 { "" } else { "s" },
            human_size(total)
        ));
    }

    // ----------------------------------------------------------- disk usage

    /// Lists the items of `folder` by size, biggest first, in a window.
    pub(super) fn disk_usage(&mut self, folder: VfsPath) {
        let registry = self.context.registry.clone();
        self.set_status("Measuring…");
        self.spawn_task(async move {
            match usage::measure(&registry, &folder).await {
                Ok(usages) => {
                    let total: u64 = usages.iter().map(|u| u.bytes).sum();
                    let items = if usages.is_empty() {
                        vec!["(empty)".to_owned()]
                    } else {
                        usage::rows(&usages)
                    };
                    let names = usages.iter().map(|u: &Usage| (u.name.clone(), u.dir)).collect();
                    let mut dialog = ChooseDialog::new(
                        &format!(
                            "Disk usage: {} ({})",
                            display_location(&folder),
                            human_size(total)
                        ),
                        items,
                        ChoosePurpose::DiskUsage { folder, names },
                    );
                    dialog.footer = "Enter: look into a folder   Backspace: up   Esc: close".into();
                    AppEvent::Show(Box::new(Overlay::Choose(dialog)))
                }
                Err(e) => done(Err(e), None),
            }
        });
    }

    /// Enter and Backspace in the disk usage list.
    pub(super) fn disk_usage_key(
        &mut self,
        dialog: &ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let ChoosePurpose::DiskUsage { folder, names } = &dialog.purpose else { return None };
        if press.plain() == Some(Key::Backspace) {
            let parent = folder.parent()?;
            self.disk_usage(parent);
            return Some(false);
        }
        if press.plain() == Some(Key::Enter) {
            let (name, dir) = names.get(dialog.cursor)?;
            if *dir {
                let inside = folder.join(name);
                self.disk_usage(inside);
            } else {
                let file = folder.join(name);
                let request = self.workspace.active_mut().reveal(&file);
                self.load_opt(request);
            }
            return Some(false);
        }
        None
    }

    // ------------------------------------------------------------ duplicates

    /// Looks for files with the same content in both panels' folders (and
    /// below): by size first, then by hash.
    pub(super) fn find_duplicates(&mut self) {
        let mut roots = vec![self.workspace.active().location().clone()];
        let other = self.workspace.inactive().location().clone();
        if !roots.contains(&other) {
            roots.push(other);
        }
        let registry = self.context.registry.clone();
        self.set_status("Looking for duplicates…");
        self.spawn_task(async move {
            let mut seen = HashSet::new();
            let mut by_size: BTreeMap<u64, Vec<Copy>> = BTreeMap::new();
            let mut files = 0usize;
            for root in &roots {
                let provider = match registry.provider_for(root).await {
                    Ok(provider) => provider,
                    Err(e) => return done(Err(e), None),
                };
                let mut items = sc_vfs::walk(provider.as_ref(), root, None);
                while let Some(item) = items.next().await {
                    let Ok(item) = item else { continue };
                    if item.entry.is_file() && item.entry.size > 0 && seen.insert(item.path.clone())
                    {
                        files += 1;
                        by_size
                            .entry(item.entry.size)
                            .or_default()
                            .push((item.path, item.entry.modified));
                    }
                }
            }
            let candidates: Vec<(u64, VfsPath, Option<SystemTime>)> = by_size
                .into_iter()
                .filter(|(_, same)| same.len() > 1)
                .flat_map(|(size, same)| same.into_iter().map(move |(p, m)| (size, p, m)))
                .collect();
            let hashed: Vec<(u64, String, VfsPath, Option<SystemTime>)> =
                futures::stream::iter(candidates)
                    .map(|(size, path, modified)| {
                        let registry = registry.clone();
                        async move {
                            let provider = registry.provider_for(&path).await.ok()?;
                            let hash =
                                sc_ops::checksum(provider.as_ref(), &path, HashAlgorithm::Blake3)
                                    .await
                                    .ok()?;
                            Some((size, hash, path, modified))
                        }
                    })
                    .buffer_unordered(8)
                    .filter_map(|found| async move { found })
                    .collect()
                    .await;
            let mut grouped: BTreeMap<(u64, String), Vec<Copy>> = BTreeMap::new();
            for (size, hash, path, modified) in hashed {
                grouped.entry((size, hash)).or_default().push((path, modified));
            }
            let mut groups: Vec<(u64, Vec<VfsPath>)> = grouped
                .into_iter()
                .filter(|(_, copies)| copies.len() > 1)
                .map(|((size, _), mut copies)| {
                    // The oldest copy is the one that is kept.
                    copies.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
                    (size, copies.into_iter().map(|(p, _)| p).collect())
                })
                .collect();
            if groups.is_empty() {
                return done(Ok(format!("No duplicates among {files} files")), None);
            }
            groups
                .sort_by_key(|(size, copies)| std::cmp::Reverse(size * (copies.len() as u64 - 1)));
            let wasted: u64 =
                groups.iter().map(|(size, copies)| size * (copies.len() as u64 - 1)).sum();
            let extras: usize = groups.iter().map(|(_, c)| c.len() - 1).sum();
            let mut items = vec![format!(
                "Put {extras} extra cop{} in the basket ({} to win back)",
                if extras == 1 { "y" } else { "ies" },
                human_size(wasted)
            )];
            for (size, copies) in &groups {
                let also: Vec<String> = copies[1..].iter().map(display_location).collect();
                items.push(format!(
                    "{} copies of {}: keep {}, also {}",
                    copies.len(),
                    human_size(*size),
                    display_location(&copies[0]),
                    also.join(", ")
                ));
            }
            let mut dialog = ChooseDialog::new(
                &format!("Duplicates: {} groups among {files} files", groups.len()),
                items,
                ChoosePurpose::Duplicates(groups.into_iter().map(|(_, copies)| copies).collect()),
            );
            dialog.footer = "Enter on a group shows the copy that is kept".into();
            AppEvent::Show(Box::new(Overlay::Choose(dialog)))
        });
    }

    // ----------------------------------------------------------------- disks

    /// Shows the disks and partitions of this computer.
    pub(super) fn show_disks(&mut self) {
        self.set_status("Reading the disks…");
        self.spawn_task(async move {
            let listed = tokio::task::spawn_blocking(crate::disks::list)
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            match listed {
                Ok(rows) if !rows.is_empty() => {
                    let mut dialog = ChooseDialog::new(
                        "Disks",
                        crate::disks::lines(&rows),
                        ChoosePurpose::Disks(rows.into_iter().map(|r| r.mount).collect()),
                    );
                    dialog.footer = "Enter: open where it is mounted   Esc: close".into();
                    AppEvent::Show(Box::new(Overlay::Choose(dialog)))
                }
                Ok(_) => AppEvent::TaskDone {
                    message: "No disks found".into(),
                    error: true,
                    reload: false,
                    focus: None,
                },
                Err(e) => AppEvent::TaskDone {
                    message: format!("Disks: {e}"),
                    error: true,
                    reload: false,
                    focus: None,
                },
            }
        });
    }

    /// What choosing a row of these lists does.
    pub(super) fn chosen_disk_tool(&mut self, purpose: ChoosePurpose, cursor: usize) {
        match purpose {
            ChoosePurpose::Duplicates(groups) => {
                if cursor == 0 {
                    let extras: Vec<VfsPath> =
                        groups.iter().flat_map(|g| g.iter().skip(1).cloned()).collect();
                    let added = self.basket_add(extras);
                    let key = self.context.keymap.shortcut_text(Action::Basket);
                    self.set_status(format!(
                        "{added} extra copies are in the basket ({key}): delete them from there"
                    ));
                } else if let Some(kept) = groups.get(cursor - 1).and_then(|g| g.first()) {
                    let request = self.workspace.active_mut().reveal(kept);
                    self.load_opt(request);
                }
            }
            ChoosePurpose::Disks(mounts) => match mounts.get(cursor) {
                Some(Some(mount)) => self.open_location(mount.clone()),
                _ => self.set_status("Not mounted"),
            },
            _ => {}
        }
    }
}
