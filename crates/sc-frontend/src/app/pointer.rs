//! The mouse: clicking rows, the wheel, and dragging files between the
//! panels or in from other programs. The terminal and the window both call
//! these; what a click means is decided here.

use std::path::PathBuf;

use sc_panel::SideId;
use sc_vfs::VfsPath;

use super::{App, InputPurpose, display_location, side_index};

impl App {
    /// A click on visible row `row` (0 is the first row shown) of `side`:
    /// that side gets the focus and the cursor moves to the row.
    pub fn click_row(&mut self, side: SideId, row: usize) {
        if self.overlay.is_some() {
            return;
        }
        self.workspace.set_active(side);
        let offset = self.scroll[side_index(side)];
        let panel = self.workspace.side_mut(side).panel_mut();
        if panel.entries().is_empty() {
            return;
        }
        panel.set_cursor(offset + row);
        self.update_quick_view();
    }

    /// The wheel over `side`: moves the cursor by `lines` rows.
    pub fn scroll_rows(&mut self, side: SideId, lines: isize) {
        if self.overlay.is_some() {
            return;
        }
        self.workspace.set_active(side);
        self.workspace.side_mut(side).panel_mut().move_cursor(lines);
        self.update_quick_view();
    }

    /// Row `row` of `from` was dragged onto `to`: asks to copy it (or the
    /// selection, when the row is part of it) into the folder shown there.
    pub fn drag_rows(&mut self, from: SideId, row: usize, to: SideId) {
        if from == to || self.overlay.is_some() {
            return;
        }
        let offset = self.scroll[side_index(from)];
        let panel = self.workspace.side(from).panel();
        let Some(entry) = panel.entries().get(offset + row) else { return };
        let paths = if panel.is_selected(&entry.name) {
            panel.targets()
        } else if entry.name == sc_panel::PARENT {
            return;
        } else {
            vec![panel.location().join(&entry.name)]
        };
        self.drop_paths(to, paths);
    }

    /// Files from another program were dropped on `side`.
    pub fn drop_files(&mut self, side: SideId, files: &[PathBuf]) {
        let paths: Vec<VfsPath> =
            files.iter().filter_map(|file| VfsPath::from_local(file.clone()).ok()).collect();
        self.drop_paths(side, paths);
    }

    /// Asks whether to copy `paths` into the folder `side` shows.
    pub fn drop_paths(&mut self, side: SideId, paths: Vec<VfsPath>) {
        if paths.is_empty() || self.overlay.is_some() {
            return;
        }
        let destination = display_location(self.workspace.side(side).panel().location());
        let prompt = format!("Copy {} to:", Self::count_text(&paths));
        self.ask("Copy", &prompt, &destination, InputPurpose::CopyTo(paths));
    }
}
