//! The state of every overlay window: dialogs, menu, help, viewer, jobs.
//!
//! Overlays only hold data. What happens when one is confirmed is decided in
//! `app.rs`, by matching on the dialog's *purpose*.

use sc_config::{Action, Category, ExportBundle};
use sc_ops::Conflict;
use sc_ops::Resolution;
use sc_vfs::VfsPath;
use tokio::sync::oneshot;

use crate::input::TextField;
use crate::viewer::Viewer;

/// The window drawn over the panels, if any.
#[derive(Debug)]
pub enum Overlay {
    /// F1: the main shortcuts.
    Help,
    /// F9: the menu bar.
    Menu(MenuState),
    /// A one-line question.
    Input(InputDialog),
    /// A yes/no question.
    Confirm(ConfirmDialog),
    /// Pick one item from a list.
    Choose(ChooseDialog),
    /// Information to read and dismiss.
    Message(MessageDialog),
    /// F3: the file viewer.
    Viewer(Viewer),
    /// Ctrl+J: the transfer queue.
    Jobs(JobsView),
}

/// What an [`InputDialog`] is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPurpose {
    /// Copy these items to the typed location.
    CopyTo(Vec<VfsPath>),
    /// Move these items to the typed location.
    MoveTo(Vec<VfsPath>),
    /// Create a folder with the typed name.
    MakeDir,
    /// Rename this item.
    Rename(VfsPath),
    /// Multi-rename these files with the typed template.
    MultiRename(Vec<VfsPath>),
    /// Open the typed location.
    GoTo,
    /// Select entries matching the typed mask.
    SelectMask,
    /// Unselect entries matching the typed mask.
    UnselectMask,
    /// Filter the panel while typing.
    QuickFilter,
    /// Pack these items into the typed archive.
    PackInto(Vec<VfsPath>),
    /// First search question: the name mask.
    SearchName,
    /// Second search question: the text to find.
    SearchContent {
        /// Mask from the first question.
        mask: String,
    },
    /// New octal permissions for this item.
    Permissions(VfsPath),
    /// Write the settings to the typed file.
    ExportSettings,
    /// Read settings from the typed file.
    ImportSettings,
}

/// A one-line question.
#[derive(Debug, Clone)]
pub struct InputDialog {
    /// Window title.
    pub title: String,
    /// Text above the field.
    pub prompt: String,
    /// The answer being typed.
    pub field: TextField,
    /// What the answer is for.
    pub purpose: InputPurpose,
}

impl InputDialog {
    /// A dialog with `initial` text in the field.
    pub fn new(title: &str, prompt: &str, initial: &str, purpose: InputPurpose) -> Self {
        InputDialog {
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            field: TextField::new(initial),
            purpose,
        }
    }
}

/// What a [`ConfirmDialog`] is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmPurpose {
    /// Delete these paths, to the trash or permanently.
    Delete {
        /// Items to delete.
        paths: Vec<VfsPath>,
        /// Use the recycle bin.
        trash: bool,
    },
    /// Rename `paths` to `names`.
    MultiRename {
        /// Files to rename.
        paths: Vec<VfsPath>,
        /// Their new names.
        names: Vec<String>,
    },
    /// Run a plugin command or a script (allowing it first).
    RunPlugin(crate::app::PluginChoice),
}

/// A yes/no question.
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    /// Window title.
    pub title: String,
    /// The question and any details, one string per line.
    pub lines: Vec<String>,
    /// What "yes" does.
    pub purpose: ConfirmPurpose,
}

/// What a [`ChooseDialog`] is for.
#[derive(Debug)]
pub enum ChoosePurpose {
    /// Open a bookmark (Delete removes it).
    Bookmarks,
    /// Open a saved connection (the last item asks for a new location).
    Connections,
    /// Go to a search hit.
    SearchResults(Vec<VfsPath>),
    /// Replace or merge imported settings.
    ImportMode(Box<ExportBundle>),
    /// Run one of these plugin commands or scripts.
    Plugin(Vec<crate::app::PluginChoice>),
}

/// Pick one item from a list.
#[derive(Debug)]
pub struct ChooseDialog {
    /// Window title.
    pub title: String,
    /// The rows.
    pub items: Vec<String>,
    /// Highlighted row.
    pub cursor: usize,
    /// What choosing does.
    pub purpose: ChoosePurpose,
    /// Shown below the list (e.g. "searching…").
    pub footer: String,
}

impl ChooseDialog {
    /// A list with the cursor on the first row.
    pub fn new(title: &str, items: Vec<String>, purpose: ChoosePurpose) -> Self {
        ChooseDialog { title: title.to_owned(), items, cursor: 0, purpose, footer: String::new() }
    }

    /// Moves the cursor by `delta`, clamped to the list.
    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.items.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }
}

/// Information to read and dismiss.
#[derive(Debug, Clone)]
pub struct MessageDialog {
    /// Window title.
    pub title: String,
    /// Text, one string per line.
    pub lines: Vec<String>,
    /// Pressing Enter opens this follow-up dialog (e.g. "change permissions").
    pub then: Option<InputDialog>,
}

/// The menu bar: one menu per [`Category`], items from the action table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MenuState {
    /// Open menu (index into [`Category::ALL`]).
    pub menu: usize,
    /// Highlighted item.
    pub item: usize,
}

impl MenuState {
    /// The actions of menu `index`, in table order.
    pub fn items(index: usize) -> Vec<Action> {
        let category = Category::ALL[index];
        Action::ALL.iter().copied().filter(|a| a.category() == category).collect()
    }

    /// Opens the previous or next menu.
    pub fn move_menu(&mut self, delta: isize) {
        let count = Category::ALL.len() as isize;
        self.menu = (self.menu as isize + delta).rem_euclid(count) as usize;
        self.item = 0;
    }

    /// Moves within the open menu, wrapping around.
    pub fn move_item(&mut self, delta: isize) {
        let count = Self::items(self.menu).len() as isize;
        self.item = (self.item as isize + delta).rem_euclid(count) as usize;
    }

    /// The highlighted action.
    pub fn selected(&self) -> Action {
        Self::items(self.menu)[self.item]
    }
}

/// The transfer queue window.
#[derive(Debug, Clone, Copy, Default)]
pub struct JobsView {
    /// Highlighted job.
    pub cursor: usize,
}

/// A copy collision waiting for the user's answer.
#[derive(Debug)]
pub struct PendingConflict {
    /// What collides.
    pub conflict: Conflict,
    /// Where the answer goes.
    pub reply: oneshot::Sender<Resolution>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_cover_every_action() {
        let total: usize = (0..Category::ALL.len()).map(|i| MenuState::items(i).len()).sum();
        assert_eq!(total, Action::ALL.len());
    }

    #[test]
    fn menu_navigation_wraps() {
        let mut menu = MenuState::default();
        menu.move_menu(-1);
        assert_eq!(menu.menu, Category::ALL.len() - 1);
        menu.move_item(-1);
        assert_eq!(menu.selected(), *MenuState::items(menu.menu).last().unwrap());
    }

    #[test]
    fn choose_cursor_is_clamped() {
        let mut dialog =
            ChooseDialog::new("t", vec!["a".into(), "b".into()], ChoosePurpose::Bookmarks);
        dialog.move_cursor(5);
        assert_eq!(dialog.cursor, 1);
        dialog.move_cursor(-5);
        assert_eq!(dialog.cursor, 0);
    }
}
