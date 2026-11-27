//! Commands that work on lists of paths: copy a path,
//! export a listing, links, renaming in the editor, the bookmark list with recent
//! folders, saved searches, the basket, similar pictures, comparing by
//! content and synchronizing folders.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use pnxr_config::{Action, Key};
use pnxr_ops::{DeleteOptions, LinkKind, SyncAction, SyncMode, delete_paths};
use pnxr_panel::SideId;
use pnxr_transfer::{JobKind, JobSpec};
use pnxr_vfs::{Capabilities, HashAlgorithm, ProviderRegistry, VfsPath};

use super::{App, AppEvent, External, display_location, done, resolve_location};
use crate::dialogs::Forget;
use crate::dialogs::{ChooseDialog, ChoosePurpose, InputPurpose, JumpRow, Overlay};
use crate::export::{Format, PathVariant};
use crate::input::KeyPress;
use crate::similar::{self, Picture};
use crate::syncplan::SyncRun;
use crate::viewer::Viewer;

/// Picture files bigger than this are not looked at.
const PICTURE_LIMIT: u64 = 48 << 20;

/// Extensions of the pictures the similar-picture search reads.
const PICTURE_EXTENSIONS: [&str; 9] =
    ["jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "ico"];

/// Recent folders shown under the bookmarks.
const RECENT_LIMIT: usize = 5;

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `rel` (parts joined by `/`) below `base`.
fn join_relative(base: &VfsPath, rel: &str) -> VfsPath {
    rel.split('/').filter(|part| !part.is_empty()).fold(base.clone(), |path, part| path.join(part))
}

/// Makes sure `folder` exists, creating it and what is above it.
async fn ensure_folder(registry: &ProviderRegistry, folder: &VfsPath) -> pnxr_vfs::Result<()> {
    let provider = registry.provider_for(folder).await?;
    let mut missing = Vec::new();
    let mut path = folder.clone();
    while provider.stat(&path).await.is_err() {
        missing.push(path.clone());
        match path.parent() {
            Some(parent) if parent != path => path = parent,
            _ => break,
        }
    }
    for path in missing.into_iter().rev() {
        provider.create_dir(&path).await?;
    }
    Ok(())
}

impl App {
    // ------------------------------------------------------------ clipboard

    /// The text a command wants in the system clipboard, once: the front
    /// end hands it over (the terminal with an escape sequence, the window
    /// through Qt).
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    pub(super) fn copy_text(&mut self, text: String, what: &str) {
        self.clipboard = Some(text);
        self.set_status(format!("Copied to the clipboard: {what}"));
    }

    pub(super) fn ask_copy_path(&mut self) {
        let paths = self.targets();
        if paths.is_empty() {
            return self.set_error("Nothing to copy");
        }
        let items = PathVariant::ALL
            .iter()
            .map(|variant| {
                let sample = variant.text(&paths);
                let first = sample.lines().next().unwrap_or("");
                let more = if paths.len() > 1 && *variant != PathVariant::Folder {
                    format!(" (+{} more)", paths.len() - 1)
                } else {
                    String::new()
                };
                format!("{:<12}{first}{more}", variant.label())
            })
            .collect();
        let title = format!("Copy to the clipboard: {}", Self::count_text(&paths));
        self.overlay =
            Some(Overlay::Choose(ChooseDialog::new(&title, items, ChoosePurpose::CopyPath(paths))));
    }

    // --------------------------------------------------------------- export

    /// The shown folder and its entries: the selected ones, or all.
    fn listing_entries(&self, selection_only: bool) -> (VfsPath, Vec<pnxr_vfs::Entry>) {
        let panel = self.workspace.active();
        let entries = panel
            .entries()
            .iter()
            .filter(|e| e.name != pnxr_panel::PARENT)
            .filter(|e| !selection_only || panel.is_selected(&e.name))
            .cloned()
            .collect();
        (panel.location().clone(), entries)
    }

    pub(super) fn ask_export(&mut self) {
        let panel = self.workspace.active();
        let selection_only = panel.entries().iter().any(|e| panel.is_selected(&e.name));
        let (_, entries) = self.listing_entries(selection_only);
        if entries.is_empty() {
            return self.set_error("Nothing to export");
        }
        let what = if selection_only { "the selection" } else { "the folder listing" };
        let items = Format::ALL.iter().map(|f| f.label().to_owned()).collect();
        let title = format!("Export {what} ({} items)", entries.len());
        self.overlay = Some(Overlay::Choose(ChooseDialog::new(
            &title,
            items,
            ChoosePurpose::ExportFormat { selection_only },
        )));
    }

    pub(super) fn export_listing(&mut self, format: Format, selection_only: bool, to: &str) {
        let (folder, entries) = self.listing_entries(selection_only);
        let text = crate::export::render(&folder, &entries, format);
        if to.is_empty() {
            return self.copy_text(text, &format!("{} items as {}", entries.len(), format.label()));
        }
        let here = self.workspace.active().location().clone();
        let file = match resolve_location(to, &here) {
            Ok(path) => path,
            Err(e) => return self.set_error(e),
        };
        let Some(native) = file.to_local_path() else {
            return self.set_error("The listing can only be written to a local file");
        };
        let count = entries.len();
        self.spawn_task(async move {
            if native.exists() {
                return done(
                    Err(pnxr_vfs::VfsError::Other(format!(
                        "{} already exists: give another name",
                        native.display()
                    ))),
                    None,
                );
            }
            let result = std::fs::write(&native, text)
                .map(|()| format!("Wrote {count} items to {}", native.display()))
                .map_err(|e| pnxr_vfs::VfsError::Other(format!("{}: {e}", native.display())));
            done(result, None)
        });
    }

    // ---------------------------------------------------------------- links

    pub(super) fn ask_link(&mut self) {
        let paths = self.targets();
        if paths.is_empty() {
            return self.set_error("Nothing to link");
        }
        let folder = self.workspace.inactive().location().clone();
        if !folder.is_local() || paths.iter().any(|p| !p.is_local()) {
            return self.set_error("Links can only be made between local folders");
        }
        let items = vec![
            "Symbolic link, relative (survives moving both folders)".to_owned(),
            "Symbolic link, absolute".to_owned(),
            "Hard link (files only, same drive)".to_owned(),
        ];
        let title = format!("Link {} in {}", Self::count_text(&paths), display_location(&folder));
        self.overlay =
            Some(Overlay::Choose(ChooseDialog::new(&title, items, ChoosePurpose::LinkKind(paths))));
    }

    fn make_links(&mut self, sources: Vec<VfsPath>, kind: LinkKind) {
        let folder = self.workspace.inactive().location().clone();
        let registry = self.context.registry.clone();
        self.spawn_task(async move {
            match pnxr_ops::create_links(&registry, &sources, &folder, kind).await {
                Ok(report) if report.failed.is_empty() => done(
                    Ok(format!("Created {} link{}", report.created, plural(report.created))),
                    None,
                ),
                Ok(report) => AppEvent::TaskDone {
                    message: format!(
                        "Created {}, failed {}: {}",
                        report.created,
                        report.failed.len(),
                        report.failed.join("; ")
                    ),
                    error: true,
                    reload: true,
                    focus: None,
                },
                Err(e) => done(Err(e), None),
            }
        });
    }

    // ---------------------------------------------------- rename in editor

    pub(super) fn bulk_rename(&mut self) {
        let paths = self.targets();
        if paths.is_empty() {
            return self.set_error("Nothing to rename");
        }
        if self.graphical && self.context.config.general.editor.trim().is_empty() {
            return self.set_error(
                "Set an editor that waits until it is closed in Settings (for example: code --wait)",
            );
        }
        let names: Vec<&str> = paths.iter().filter_map(|p| p.file_name()).collect();
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let file =
            std::env::temp_dir().join(format!("pnxr-rename-{}-{nanos}.txt", std::process::id()));
        if let Err(e) = std::fs::write(&file, names.join("\n") + "\n") {
            return self.set_error(format!("{}: {e}", file.display()));
        }
        self.external = Some(External {
            command: self.editor_command(&file),
            reload_settings: false,
            rename: Some((file, paths)),
        });
    }

    /// The editor of "rename in the editor" ended: ask about the names in
    /// the file, or explain what is wrong with them.
    pub(super) fn finish_bulk_rename(&mut self, file: &Path, paths: Vec<VfsPath>) {
        let text = std::fs::read_to_string(file);
        let _ = std::fs::remove_file(file);
        let text = match text {
            Ok(text) => text,
            Err(e) => return self.set_error(format!("{}: {e}", file.display())),
        };
        let mut names: Vec<String> = text.lines().map(str::to_owned).collect();
        while names.len() > paths.len() && names.last().is_some_and(|n| n.trim().is_empty()) {
            names.pop();
        }
        if names.len() != paths.len() {
            return self.set_error(format!(
                "The file has {} lines for {} items: keep one name per line",
                names.len(),
                paths.len()
            ));
        }
        let mut seen = BTreeSet::new();
        for name in &names {
            if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
                return self.set_error(format!("Not a file name: \"{name}\""));
            }
            if !seen.insert(name.clone()) {
                return self.set_error(format!("Two items would be called \"{name}\""));
            }
        }
        self.confirm_new_names(paths, names, Vec::new());
    }

    // ----------------------------------------------------------------- jump

    /// Remembers the folders the panels just opened.
    pub(super) fn record_visits(&mut self, listed: &VfsPath) {
        let location = listed.to_string();
        for side in [SideId::Left, SideId::Right] {
            let panel = self.workspace.side(side).panel();
            let index = super::side_index(side);
            if panel.location() == listed
                && panel.error().is_none()
                && self.visited[index].as_deref() != Some(location.as_str())
            {
                self.visited[index] = Some(location.clone());
                self.frecency.record(&location, now_seconds());
            }
        }
    }

    /// Opens the bookmarks list: bookmarks first, then the recent folders;
    /// typing narrows both.
    pub(super) fn open_bookmarks(&mut self) {
        let (items, rows) = self.bookmark_rows("");
        let mut dialog = ChooseDialog::new("Bookmarks", items, ChoosePurpose::Bookmarks(rows));
        dialog.query = Some(String::new());
        self.refresh_bookmarks(&mut dialog);
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// The rows for `query`: the bookmarks whose name or folder holds it,
    /// then the most recent folders that do.
    fn bookmark_rows(&self, query: &str) -> (Vec<String>, Vec<JumpRow>) {
        let needle = query.to_lowercase();
        let mut items = Vec::new();
        let mut rows = Vec::new();
        for (index, bookmark) in self.context.config.bookmarks.iter().enumerate() {
            if bookmark.name.to_lowercase().contains(&needle)
                || bookmark.path.to_lowercase().contains(&needle)
            {
                items.push(super::bookmark_row(bookmark));
                rows.push(JumpRow::Bookmark(index));
            }
        }
        let here = self.workspace.active().location().to_string();
        let recent: Vec<String> = self
            .frecency
            .recent(query, RECENT_LIMIT + 1)
            .into_iter()
            .map(|v| v.path.clone())
            .filter(|path| *path != here)
            .take(RECENT_LIMIT)
            .collect();
        if !recent.is_empty() {
            items.push("── Recent folders ──".into());
            rows.push(JumpRow::Header);
            for uri in recent {
                items.push(
                    VfsPath::parse(&uri).map_or_else(|_| uri.clone(), |p| display_location(&p)),
                );
                rows.push(JumpRow::Recent(uri));
            }
        }
        if rows.is_empty() {
            items.push(if needle.is_empty() {
                "(no bookmarks yet)".into()
            } else {
                "(nothing matches)".into()
            });
            rows.push(JumpRow::Header);
        }
        (items, rows)
    }

    /// Whether the active folder is not a bookmark yet.
    fn here_can_be_bookmarked(&self) -> bool {
        let here = display_location(self.workspace.active().location());
        !self.context.config.bookmarks.iter().any(|b| b.path == here)
    }

    /// Rebuilds the rows of the bookmarks list from its query, with the
    /// cursor on the first row that can be chosen.
    fn refresh_bookmarks(&self, dialog: &mut ChooseDialog) {
        let query = dialog.query.clone().unwrap_or_default();
        let (items, rows) = self.bookmark_rows(&query);
        dialog.cursor = rows.iter().position(|r| *r != JumpRow::Header).unwrap_or(0);
        dialog.items = items;
        dialog.purpose = ChoosePurpose::Bookmarks(rows);
        // The command to bookmark this folder is offered only while the
        // folder is not a bookmark.
        let add = self.here_can_be_bookmarked().then_some("Ctrl+D: bookmark this folder");
        let mut keys = vec!["Delete: remove bookmark"];
        keys.extend(add);
        keys.push(if query.is_empty() { "Type: filter" } else { "Backspace: erase" });
        dialog.footer = crate::hints::list("open", &keys, "close");
        if !query.is_empty() {
            dialog.footer = format!("Filter: {query}_{}{}", crate::hints::SEP, dialog.footer);
        }
    }

    /// Typing, Backspace and Delete in the bookmarks list; `None` for the
    /// keys the list handles itself (arrows, Enter, Esc).
    pub(super) fn bookmarks_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let key = press.plain();
        let mut query = dialog.query.clone().unwrap_or_default();
        if press.chord.ctrl && !press.chord.alt && press.chord.key == Key::Char('d') {
            if self.here_can_be_bookmarked() {
                self.add_bookmark(false);
            }
            self.refresh_bookmarks(dialog);
            return Some(true);
        }
        match (key, press.text) {
            (Some(Key::Delete), _) => {
                let ChoosePurpose::Bookmarks(rows) = &dialog.purpose else { return None };
                if let Some(JumpRow::Bookmark(index)) = rows.get(dialog.cursor) {
                    let name = self.context.config.bookmarks[*index].name.clone();
                    self.overlay = Some(super::forget_question(
                        Forget::Bookmark(*index),
                        format!("Remove the bookmark \"{name}\"?"),
                    ));
                    return Some(false);
                }
                return Some(true);
            }
            (Some(Key::Backspace), _) => {
                query.pop();
            }
            (_, Some(c)) if !press.chord.ctrl && !press.chord.alt && !c.is_control() => {
                query.push(c);
            }
            _ => return None,
        }
        dialog.query = Some(query);
        self.refresh_bookmarks(dialog);
        Some(true)
    }

    pub(super) fn open_location(&mut self, uri: String) {
        let here = self.workspace.active().location().clone();
        match resolve_location(&uri, &here) {
            Ok(path) => {
                let request = self.workspace.active_mut().navigate(path);
                self.load(request);
            }
            Err(e) => self.set_error(e),
        }
    }

    // ------------------------------------------------------ saved searches

    pub(super) fn save_search_as(&mut self, name: &str) {
        let Some(mut search) = self.last_search.clone() else {
            return self.set_error("There is no search to save yet");
        };
        if name.is_empty() {
            return;
        }
        search.name = name.to_owned();
        let searches = &mut self.context.config.searches;
        match searches.iter_mut().find(|s| s.name == name) {
            Some(existing) => *existing = search,
            None => searches.push(search),
        }
        if self.save_settings() {
            let keys = self.context.keymap.shortcut_text(Action::Menu);
            self.set_status(format!(
                "Saved search \"{name}\": run it from the command list ({keys}) and give it a key there"
            ));
        }
    }

    pub(super) fn open_saved_searches(&mut self) {
        if self.context.config.searches.is_empty() {
            return self.set_error("No saved searches yet: search, then \"Save the last search\"");
        }
        let items = self.context.config.searches.iter().map(crate::searches::describe).collect();
        let mut dialog = ChooseDialog::new("Saved searches", items, ChoosePurpose::SavedSearches);
        dialog.footer = crate::hints::list("search", &["Delete: forget"], "close");
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// Runs the saved search `name` in the folder of the active panel.
    pub fn run_saved_search(&mut self, name: &str) {
        let Some(search) = self.context.config.searches.iter().find(|s| s.name == name).cloned()
        else {
            return self.set_error(format!("No saved search called \"{name}\""));
        };
        match crate::searches::saved_criteria(&search, SystemTime::now()) {
            Ok(criteria) => self.search_with(criteria, search),
            Err(e) => self.set_error(format!("{name}: {e}")),
        }
    }

    // --------------------------------------------------------------- basket

    /// What the search results list says about the basket.
    pub(super) fn basket_hint(&self) -> String {
        format!("{}: add all to the basket", self.context.keymap.shortcut_text(Action::BasketAdd))
    }

    pub(super) fn basket_add(&mut self, paths: Vec<VfsPath>) -> usize {
        let before = self.basket.len();
        for path in paths {
            if !self.basket.contains(&path) {
                self.basket.push(path);
            }
        }
        self.basket.len() - before
    }

    pub(super) fn add_selection_to_basket(&mut self) {
        let paths = self.targets();
        if paths.is_empty() {
            return self.set_error("Nothing to add");
        }
        let added = self.basket_add(paths);
        self.workspace.active_mut().clear_selection();
        let total = self.basket.len();
        self.set_status(format!("Basket: {added} added, {total} in all"));
    }

    /// Takes `paths` out of the basket (they were moved or deleted).
    pub(super) fn basket_forget(&mut self, paths: &[VfsPath]) {
        self.basket.retain(|p| !paths.iter().any(|gone| p == gone || p.starts_with(gone)));
    }

    pub(super) fn open_basket(&mut self) {
        if self.basket.is_empty() {
            let key = self.context.keymap.shortcut_text(Action::BasketAdd);
            return self.set_error(format!(
                "The basket is empty: add files with {key}, or the results of a search"
            ));
        }
        self.overlay = Some(Overlay::Choose(self.basket_dialog(0)));
    }

    fn basket_dialog(&self, cursor: usize) -> ChooseDialog {
        let keymap = &self.context.keymap;
        let key = |action| keymap.shortcut_text(action);
        let mut dialog = ChooseDialog::new(
            &format!("Basket ({})", self.basket.len()),
            self.basket.iter().map(display_location).collect(),
            ChoosePurpose::Basket(self.basket.clone()),
        );
        dialog.footer = [
            format!("{}: copy", key(Action::Copy)),
            format!("{}: move", key(Action::Move)),
            format!("{}: delete", key(Action::Delete)),
            format!("{}: pack", key(Action::Pack)),
            format!("{}: rename", key(Action::MultiRename)),
            "Delete: remove one".to_owned(),
            "Shift+Delete: empty".to_owned(),
            "Esc: close".to_owned(),
        ]
        .join(crate::hints::SEP);
        dialog.cursor = cursor.min(self.basket.len().saturating_sub(1));
        dialog
    }

    /// Keys that only some lists have. `Some(true)` keeps the list open,
    /// `Some(false)` closes it, `None`: not a key of this list.
    pub(super) fn choose_extra_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let key = press.plain();
        let shift_delete = press.chord.shift && press.chord.key == Key::Delete;
        let action = self.context.keymap.action_for(&press.chord);
        if let Some(handled) = self.plugin_store_key(dialog, press) {
            return Some(handled);
        }
        match &dialog.purpose {
            ChoosePurpose::Drives(drives) if key == Some(Key::F(2)) => {
                match drives.get(dialog.cursor).filter(|d| d.saved) {
                    Some(drive) => {
                        let (name, uri) = (drive.name.clone(), drive.location.clone());
                        self.ask(
                            "Rename connection",
                            "Name in the drive list:",
                            &name.clone(),
                            InputPurpose::RenameConnection { name, uri },
                        );
                        Some(false)
                    }
                    None => {
                        self.set_error("Only saved connections can be renamed");
                        Some(false)
                    }
                }
            }
            ChoosePurpose::Drives(drives) if key == Some(Key::F(4)) => {
                match drives.get(dialog.cursor).filter(|d| d.saved) {
                    Some(drive) => {
                        let (name, uri) = (drive.name.clone(), drive.location.clone());
                        self.ask_address(
                            "Edit connection",
                            &uri.clone(),
                            InputPurpose::EditConnection { name, uri },
                        );
                    }
                    None => self.set_error("Only saved connections can be edited"),
                }
                Some(false)
            }
            ChoosePurpose::Drives(drives)
                if press.chord.ctrl && press.chord.key == Key::Char('t') =>
            {
                match drives
                    .get(dialog.cursor)
                    .filter(|d| d.category == crate::drives::Category::Remote)
                {
                    Some(drive) => match VfsPath::parse(&drive.location) {
                        Ok(path) => self.open_connection(&drive.location.clone(), path, true),
                        Err(e) => self.set_error(e.to_string()),
                    },
                    None => self.set_error("Only a remote connection can be tested"),
                }
                Some(false)
            }
            ChoosePurpose::ConnectionLogin { .. }
                if press.chord.ctrl && press.chord.key == Key::Char('t') =>
            {
                self.test_login(dialog.clone());
                Some(false)
            }
            ChoosePurpose::Bookmarks(_) => self.bookmarks_key(dialog, press),
            ChoosePurpose::DiskUsage { .. } => self.disk_usage_key(dialog, press),
            ChoosePurpose::Font { .. } => self.fonts_key(dialog, press),
            ChoosePurpose::Properties(_) => self.properties_key(dialog, press),
            ChoosePurpose::SavedSearches if key == Some(Key::Delete) => {
                let index = dialog.cursor;
                match self.context.config.searches.get(index) {
                    Some(search) => {
                        self.overlay = Some(super::forget_question(
                            Forget::Search(index),
                            format!("Forget the saved search \"{}\"?", search.name),
                        ));
                        Some(false)
                    }
                    None => Some(true),
                }
            }
            ChoosePurpose::SearchResults(paths) if action == Some(Action::BasketAdd) => {
                let paths = paths.clone();
                if let Some(cancel) = self.search_cancel.take() {
                    cancel.cancel();
                }
                let added = self.basket_add(paths);
                let total = self.basket.len();
                self.set_status(format!("Basket: {added} added, {total} in all"));
                Some(false)
            }
            ChoosePurpose::Basket(_) if shift_delete => {
                let count = self.basket.len();
                let question =
                    format!("Empty the basket ({count} paths)? The files stay where they are.");
                self.overlay = Some(super::forget_question(Forget::Basket, question));
                Some(false)
            }
            ChoosePurpose::Basket(_) if key == Some(Key::Delete) => {
                if dialog.cursor < self.basket.len() {
                    self.basket.remove(dialog.cursor);
                }
                if self.basket.is_empty() {
                    self.set_status("The basket is empty");
                    return Some(false);
                }
                *dialog = self.basket_dialog(dialog.cursor);
                Some(true)
            }
            ChoosePurpose::Basket(paths) => {
                let acts_on_files = matches!(
                    action,
                    Some(
                        Action::Copy
                            | Action::Move
                            | Action::Delete
                            | Action::DeleteForever
                            | Action::Pack
                            | Action::MultiRename
                            | Action::AiRename
                    )
                );
                let action = action.filter(|_| acts_on_files)?;
                // The same commands as in a panel, on what is in the basket.
                self.target_override = Some(paths.clone());
                self.perform(action);
                self.target_override = None;
                Some(false)
            }
            _ => None,
        }
    }

    /// An item was chosen in a list that belongs to these commands.
    pub(super) fn chosen_feature(&mut self, purpose: ChoosePurpose, cursor: usize) {
        match purpose {
            ChoosePurpose::CopyPath(paths) => {
                if let Some(variant) = PathVariant::ALL.get(cursor) {
                    let text = variant.text(&paths);
                    self.copy_text(text, &Self::count_text(&paths));
                }
            }
            ChoosePurpose::ExportFormat { selection_only } => {
                if let Some(format) = Format::ALL.get(cursor).copied() {
                    self.ask(
                        &format!("Export as {}", format.label()),
                        "File to write (empty: copy to the clipboard):",
                        "",
                        InputPurpose::ExportTo { format, selection_only },
                    );
                }
            }
            ChoosePurpose::LinkKind(paths) => {
                let kind =
                    [LinkKind::RelativeSymlink, LinkKind::AbsoluteSymlink, LinkKind::HardLink]
                        .get(cursor)
                        .copied();
                if let Some(kind) = kind {
                    self.make_links(paths, kind);
                }
            }
            ChoosePurpose::SavedSearches => {
                if let Some(name) = self.context.config.searches.get(cursor).map(|s| s.name.clone())
                {
                    self.run_saved_search(&name);
                }
            }
            ChoosePurpose::Basket(paths) => {
                if let Some(path) = paths.get(cursor) {
                    let request = self.workspace.active_mut().reveal(path);
                    self.load_opt(request);
                }
            }
            ChoosePurpose::SyncMode => {
                let mode = [SyncMode::Mirror, SyncMode::Update, SyncMode::TwoWay].get(cursor);
                if let Some(mode) = mode {
                    self.start_sync(*mode);
                }
            }
            ChoosePurpose::Similar(groups) => {
                if cursor == 0 {
                    let extras: Vec<VfsPath> = groups
                        .iter()
                        .flat_map(|g| g.iter().skip(1).map(|p| p.path.clone()))
                        .collect();
                    let added = self.basket_add(extras);
                    let key = self.context.keymap.shortcut_text(Action::Basket);
                    self.set_status(format!(
                        "{added} extra copies are in the basket ({key}): delete them from there"
                    ));
                } else if let Some(best) = groups.get(cursor - 1).and_then(|g| g.first()) {
                    let request = self.workspace.active_mut().reveal(&best.path);
                    self.load_opt(request);
                }
            }
            _ => {}
        }
    }

    // ---------------------------------------------------- similar pictures

    pub(super) fn find_similar(&mut self) {
        let root = self.workspace.active().location().clone();
        let registry = self.context.registry.clone();
        self.set_status("Looking for similar pictures…");
        self.spawn_task(async move {
            let provider = match registry.provider_for(&root).await {
                Ok(provider) => provider,
                Err(e) => return done(Err(e), None),
            };
            let mut files = Vec::new();
            let mut items = pnxr_vfs::walk(provider.as_ref(), &root, None);
            while let Some(item) = items.next().await {
                let Ok(item) = item else { continue };
                let name = item.entry.name.to_lowercase();
                let is_picture =
                    name.rsplit_once('.').is_some_and(|(_, ext)| PICTURE_EXTENSIONS.contains(&ext));
                if item.entry.is_file() && is_picture && item.entry.size <= PICTURE_LIMIT {
                    files.push(item.path);
                }
            }
            drop(items);
            let total = files.len();
            let pictures: Vec<Picture> = futures::stream::iter(files)
                .map(|path| {
                    let registry = registry.clone();
                    async move {
                        let (data, cut) =
                            crate::viewer::load(&registry, &path, PICTURE_LIMIT).await.ok()?;
                        if cut {
                            return None;
                        }
                        tokio::task::spawn_blocking(move || Picture::from_bytes(path, &data))
                            .await
                            .ok()
                            .flatten()
                    }
                })
                .buffer_unordered(4)
                .filter_map(|p| async move { p })
                .collect()
                .await;
            let groups = similar::group(pictures, similar::MAX_DISTANCE);
            if groups.is_empty() {
                return done(Ok(format!("No similar pictures among {total} pictures")), None);
            }
            let extras: usize = groups.iter().map(|g| g.len() - 1).sum();
            let mut items = vec![format!(
                "Put {extras} extra cop{} in the basket (all but the best of each group)",
                if extras == 1 { "y" } else { "ies" }
            )];
            for group in &groups {
                let best = &group[0];
                let others: Vec<&str> =
                    group[1..].iter().map(|p| p.path.file_name().unwrap_or("")).collect();
                items.push(format!(
                    "{} similar: keep {} ({}), also {}",
                    group.len(),
                    best.path.file_name().unwrap_or(""),
                    best.describe(),
                    others.join(", ")
                ));
            }
            let mut dialog = ChooseDialog::new(
                &format!("Similar pictures: {} groups among {total}", groups.len()),
                items,
                ChoosePurpose::Similar(groups),
            );
            dialog.footer = crate::hints::list("show the best copy", &[], "close");
            AppEvent::Show(Box::new(Overlay::Choose(dialog)))
        });
    }

    // ----------------------------------------------------- compare by hash

    pub(super) fn compare_by_hash(&mut self) {
        let active = self.workspace.active_side();
        let (here, there) = (
            self.workspace.active().location().clone(),
            self.workspace.inactive().location().clone(),
        );
        let registry = self.context.registry.clone();
        self.set_status("Comparing contents…");
        self.spawn_task(async move {
            let result: pnxr_vfs::Result<AppEvent> = async {
                let (tree_a, tree_b) = (
                    pnxr_ops::list_tree(&registry, &here).await?,
                    pnxr_ops::list_tree(&registry, &there).await?,
                );
                let (provider_a, provider_b) =
                    (registry.provider_for(&here).await?, registry.provider_for(&there).await?);
                let on_server = provider_a.capabilities().contains(Capabilities::SERVER_HASH)
                    || provider_b.capabilities().contains(Capabilities::SERVER_HASH);
                let files = |tree: &pnxr_ops::Tree| -> BTreeMap<String, u64> {
                    tree.iter().filter(|(_, e)| e.is_file()).map(|(p, e)| (p.clone(), e.size)).collect()
                };
                let (files_a, files_b) = (files(&tree_a), files(&tree_b));
                let mut differ: BTreeSet<String> = BTreeSet::new();
                let mut only_a: BTreeSet<String> = BTreeSet::new();
                let mut only_b: BTreeSet<String> = BTreeSet::new();
                let mut to_hash = Vec::new();
                for (path, size) in &files_a {
                    match files_b.get(path) {
                        None => {
                            only_a.insert(path.clone());
                        }
                        Some(other) if other != size => {
                            differ.insert(path.clone());
                        }
                        Some(_) => to_hash.push(path.clone()),
                    }
                }
                only_b.extend(files_b.keys().filter(|p| !files_a.contains_key(*p)).cloned());
                let hashed = to_hash.len();
                let results: Vec<(String, bool)> = futures::stream::iter(to_hash)
                    .map(|path| {
                        let (pa, pb) = (provider_a.clone(), provider_b.clone());
                        let (a, b) = (join_relative(&here, &path), join_relative(&there, &path));
                        async move {
                            let (x, y) = futures::join!(
                                pnxr_ops::checksum(pa.as_ref(), &a, HashAlgorithm::Blake3),
                                pnxr_ops::checksum(pb.as_ref(), &b, HashAlgorithm::Blake3),
                            );
                            (path, matches!((x, y), (Ok(x), Ok(y)) if x == y))
                        }
                    })
                    .buffer_unordered(8)
                    .collect()
                    .await;
                let mut identical = 0;
                for (path, same) in results {
                    if same {
                        identical += 1;
                    } else {
                        differ.insert(path);
                    }
                }
                let top = |paths: &BTreeSet<String>| -> Vec<String> {
                    let names: BTreeSet<&str> =
                        paths.iter().filter_map(|p| p.split('/').next()).collect();
                    names.into_iter().map(str::to_owned).collect()
                };
                let (mut marks_a, mut marks_b) = (top(&only_a), top(&only_b));
                let different = top(&differ);
                marks_a.extend(different.iter().cloned());
                marks_b.extend(different.iter().cloned());
                // Folders that exist on one side only count as their files do.
                let message = format!(
                    "Compared by content: {identical} identical, {} different, {} only here, {} only there{}",
                    differ.len(),
                    only_a.len(),
                    only_b.len(),
                    if on_server && hashed > 0 {
                        " (hashes computed where the files are)"
                    } else {
                        ""
                    }
                );
                Ok(AppEvent::Compared {
                    marks: vec![(active, marks_a), (active.other(), marks_b)],
                    message,
                })
            }
            .await;
            result.unwrap_or_else(|e| done(Err(e), None))
        });
    }

    // ----------------------------------------------------------------- sync

    pub(super) fn ask_sync(&mut self) {
        let (here, there) =
            (self.workspace.active().location(), self.workspace.inactive().location());
        if here == there {
            return self.set_error("Both panels show the same folder");
        }
        let (from, to) = (display_location(here), display_location(there));
        let items = vec![
            format!("Mirror: make {to} a copy of {from} (deletes what only it has)"),
            format!("Update: copy new and newer files of {from} to {to}"),
            format!("Two-way: the newest copy wins, each gets what the other has"),
        ];
        self.overlay = Some(Overlay::Choose(ChooseDialog::new(
            "Synchronize the folders of the two panels",
            items,
            ChoosePurpose::SyncMode,
        )));
    }

    fn start_sync(&mut self, mode: SyncMode) {
        let left = self.workspace.active().location().clone();
        let right = self.workspace.inactive().location().clone();
        let registry = self.context.registry.clone();
        self.set_status("Comparing the folders…");
        self.spawn_task(async move {
            match pnxr_ops::plan_sync(&registry, &left, &right, mode).await {
                Ok(plan) => AppEvent::Show(Box::new(Overlay::Viewer(Viewer::for_sync(SyncRun {
                    left,
                    right,
                    mode,
                    plan,
                })))),
                Err(e) => done(Err(e), None),
            }
        });
    }

    /// The user approved a synchronization plan: delete what the mirror
    /// removes, create the folders, then queue the copies as jobs.
    pub(super) fn run_sync(&mut self, run: SyncRun) {
        let registry = self.context.registry.clone();
        let use_trash = self.context.config.general.use_trash;
        self.set_status("Synchronizing…");
        self.spawn_task(async move {
            let SyncRun { left, right, plan, .. } = run;
            let deletions: Vec<VfsPath> = plan
                .items
                .iter()
                .filter(|i| i.action == SyncAction::DeleteRight)
                .map(|i| join_relative(&right, &i.path))
                .collect();
            let mut error = None;
            let mut deleted = 0;
            if !deletions.is_empty() {
                let options = DeleteOptions { use_trash, observer: None };
                match delete_paths(&registry, &deletions, options).await {
                    Ok(report) => {
                        deleted = report.done;
                        error = report.failed.first().map(|(_, message)| message.clone());
                    }
                    Err(e) => error = Some(e.to_string()),
                }
            }
            // One copy job for each destination folder.
            let mut groups: BTreeMap<VfsPath, Vec<VfsPath>> = BTreeMap::new();
            for item in &plan.items {
                let (from, to) = match item.action {
                    SyncAction::CopyToRight => (&left, &right),
                    SyncAction::CopyToLeft => (&right, &left),
                    _ => continue,
                };
                let parent = item.path.rsplit_once('/').map_or("", |(parent, _)| parent);
                let destination = join_relative(to, parent);
                groups.entry(destination).or_default().push(join_relative(from, &item.path));
            }
            let mut copies = Vec::new();
            for (destination, sources) in groups {
                match ensure_folder(&registry, &destination).await {
                    Ok(()) => copies.push((sources, destination)),
                    Err(e) => error = Some(e.to_string()),
                }
            }
            AppEvent::SyncCopies { copies, deleted, error }
        });
    }

    /// Queues the copies of a synchronization, replacing what is there.
    pub(super) fn queue_sync_copies(&mut self, copies: Vec<(Vec<VfsPath>, VfsPath)>) -> usize {
        let mut files = 0;
        for (sources, destination) in copies {
            files += sources.len();
            let options = self
                .context
                .job_options_to(Some(pnxr_ops::ConflictPolicy::Overwrite), Some(&destination));
            let spec = JobSpec {
                kind: JobKind::Copy,
                sources,
                destination: Some(destination),
                options,
                priority: 0,
            };
            if let Err(e) = self.manager.submit(spec) {
                self.set_error(e.to_string());
            }
        }
        self.refresh_jobs();
        files
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
