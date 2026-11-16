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
use crate::settings::{Field, SettingsScreen};
use crate::viewer::Viewer;

/// The window drawn over the panels, if any.
#[derive(Debug)]
pub enum Overlay {
    /// F1: the main shortcuts.
    Help,
    /// F9: the menu bar.
    Commands(crate::commands::CommandPalette),
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
    /// Ctrl+,: options and shortcuts.
    Settings(SettingsScreen),
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
    /// A new connection: first its location...
    NewConnection,
    /// ...then the name it is saved under.
    NameConnection(String),
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
    /// A typed value for an option of the settings screen, which opens
    /// again afterwards.
    SettingValue {
        /// The option.
        field: Field,
        /// The screen to go back to.
        screen: Box<SettingsScreen>,
    },
    /// Send the settings to the typed machine (from the settings screen).
    SendSettings(Box<SettingsScreen>),
    /// Fetch the settings of the typed machine (from the settings screen).
    FetchSettings(Box<SettingsScreen>),
}

impl InputPurpose {
    /// The settings screen a dialog opened from, to go back to.
    pub fn settings_screen(&self) -> Option<&SettingsScreen> {
        match self {
            InputPurpose::SettingValue { screen, .. }
            | InputPurpose::SendSettings(screen)
            | InputPurpose::FetchSettings(screen) => Some(screen),
            _ => None,
        }
    }
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
    /// Open a drive or saved connection, one per row (the last row asks
    /// for a new connection; Delete removes a saved one).
    Drives(Vec<crate::drives::Drive>),
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

impl MessageDialog {
    /// The hint below the text: `Enter: <next dialog>   Esc: close`.
    pub fn hint(&self) -> String {
        match &self.then {
            Some(next) => format!("Enter: {}   Esc: close", next.title.to_lowercase()),
            None => "Esc: close".into(),
        }
    }
}

/// The built-in commands of menu `category`, in table order (the GUI's
/// menu bar).
pub fn menu_actions(category: Category) -> Vec<Action> {
    Action::ALL.iter().copied().filter(|a| a.category() == category).collect()
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
        let total: usize = Category::ALL.iter().map(|&c| menu_actions(c).len()).sum();
        assert_eq!(total, Action::ALL.len());
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
