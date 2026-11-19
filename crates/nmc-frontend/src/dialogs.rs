//! The state of every overlay window: dialogs, menu, help, viewer, jobs.
//!
//! Overlays only hold data. What happens when one is confirmed is decided in
//! `app.rs`, by matching on the dialog's *purpose*.

use nmc_config::{Action, Category, ExportBundle};
use nmc_ops::Conflict;
use nmc_ops::Resolution;
use nmc_vfs::VfsPath;
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
    /// Have Claude write the multi-rename rule for these items from the
    /// typed description.
    AiRename(Vec<VfsPath>),
    /// Open the typed location.
    GoTo,
    /// A new connection: first its location...
    NewConnection,
    /// ...then the name it is saved under.
    NameConnection(String),
    /// A new name for a saved connection (F2 in the drive list).
    RenameConnection {
        /// The name now.
        name: String,
        /// Its location.
        uri: String,
    },
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
    /// Third search question: size and age conditions.
    SearchFilters {
        /// Mask from the first question.
        mask: String,
        /// Text from the second question.
        content: String,
    },
    /// The name a search is saved under.
    NameSearch,
    /// Where to write the listing (empty: the clipboard).
    ExportTo {
        /// The format chosen before.
        format: crate::export::Format,
        /// Only the selected items, not the whole listing.
        selection_only: bool,
    },
    /// Change the dates and permissions of these items.
    Attributes(Vec<VfsPath>),
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
    /// Have Claude write a script that does what was typed.
    AiScript,
    /// Install `nmc serve` on the typed server.
    InstallServe,
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
    /// Lines shown below the field, updated while typing (the new names
    /// of a multi-rename).
    pub preview: Vec<String>,
}

impl InputDialog {
    /// A dialog with `initial` text in the field.
    pub fn new(title: &str, prompt: &str, initial: &str, purpose: InputPurpose) -> Self {
        InputDialog {
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            field: TextField::new(initial),
            purpose,
            preview: Vec::new(),
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
    /// Bookmarks, then recent folders, filtered by what is typed (Enter
    /// opens the row, Delete removes a bookmark).
    Bookmarks(Vec<JumpRow>),
    /// Open one of a bookmark's target folders in the other panel.
    BookmarkTarget(Vec<String>),
    /// Open a drive or saved connection, one per row (the last row asks
    /// for a new connection; Delete removes a saved one).
    Drives(Vec<crate::drives::Drive>),
    /// Go to a search hit.
    SearchResults(Vec<VfsPath>),
    /// Replace or merge imported settings.
    ImportMode(Box<ExportBundle>),
    /// Run one of these plugin commands or scripts.
    Plugin(Vec<crate::app::PluginChoice>),
    /// Copy these items' paths in the chosen form.
    CopyPath(Vec<VfsPath>),
    /// Export the listing in the chosen format.
    ExportFormat {
        /// Only the selected items, not the whole listing.
        selection_only: bool,
    },
    /// Make the chosen kind of link to these items in the other panel.
    LinkKind(Vec<VfsPath>),
    /// Run a saved search (Delete forgets it).
    SavedSearches,
    /// The basket: Enter shows the item in the panel.
    Basket(Vec<VfsPath>),
    /// How to synchronize the two folders.
    SyncMode,
    /// The items of a folder by size: Enter looks into a folder.
    DiskUsage {
        /// The folder listed.
        folder: VfsPath,
        /// The rows' names, and whether each is a folder.
        names: Vec<(String, bool)>,
    },
    /// Groups of identical files (the first of each is the one kept), after
    /// a row that puts the extra copies in the basket.
    Duplicates(Vec<Vec<VfsPath>>),
    /// The disks and partitions; where each is mounted.
    Disks(Vec<Option<String>>),
    /// A font family for a setting (the first name may be empty: the
    /// system's), back to the settings screen afterwards.
    Font {
        /// The option.
        field: Field,
        /// The screen to go back to.
        screen: Box<SettingsScreen>,
        /// The rows' font families.
        names: Vec<String>,
    },
    /// Groups of similar pictures, after a row that puts the extra copies
    /// in the basket.
    Similar(Vec<Vec<crate::similar::Picture>>),
}

/// What a row of the bookmarks list stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JumpRow {
    /// The bookmark at this index of the configuration.
    Bookmark(usize),
    /// A recently opened folder (as a VFS URI).
    Recent(String),
    /// The title of the recent folders; cannot be chosen.
    Header,
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
    /// What was typed to filter the rows, for lists that filter.
    pub query: Option<String>,
}

impl ChooseDialog {
    /// A list with the cursor on the first row.
    pub fn new(title: &str, items: Vec<String>, purpose: ChoosePurpose) -> Self {
        ChooseDialog {
            title: title.to_owned(),
            items,
            cursor: 0,
            purpose,
            footer: String::new(),
            query: None,
        }
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
    /// The logo is shown above the text (About).
    pub logo: bool,
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
        let mut dialog = ChooseDialog::new(
            "t",
            vec!["a".into(), "b".into()],
            ChoosePurpose::Bookmarks(Vec::new()),
        );
        dialog.move_cursor(5);
        assert_eq!(dialog.cursor, 1);
        dialog.move_cursor(-5);
        assert_eq!(dialog.cursor, 0);
    }
}
