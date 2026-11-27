//! The state of every overlay window: dialogs, menu, help, viewer, jobs.
//!
//! Overlays only hold data. What happens when one is confirmed is decided in
//! `app.rs`, by matching on the dialog's *purpose*.

use pnxr_archive::ArchiveFormat;
use pnxr_config::{Action, Category, ExportBundle};
use pnxr_ops::Conflict;
use pnxr_ops::Resolution;
use pnxr_vfs::VfsPath;
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

/// A password carried along while a dialog asks the next question. It never
/// shows in debug output.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// What typed S3 keys are used for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3Next {
    /// Open (or test) the saved connection at `uri` with them.
    Open {
        /// Its location.
        uri: String,
        /// Test it instead of opening it.
        test: bool,
    },
    /// Save them in `~/.aws/credentials` with the connection.
    Save {
        /// The name of the connection.
        name: String,
        /// Its location.
        uri: String,
        /// The connection being edited (name, location), if any.
        replace: Option<(String, String)>,
    },
}

/// What an [`InputDialog`] is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPurpose {
    /// The address of a plugin repository to add.
    AddPluginRepo,
    /// The short name of the bottom bar button of the command with this id.
    FunctionKeyName(String),
    /// Copy these items to the typed location.
    CopyTo(Vec<VfsPath>),
    /// Move these items to the typed location.
    MoveTo(Vec<VfsPath>),
    /// Create a folder with the typed name.
    MakeDir,
    /// Generate random text files: the name is asked.
    GenerateName,
    /// Generate random text files: how many (the name is known).
    GenerateCount(String),
    /// Generate random text files: the size (name and number are known).
    GenerateSize(String, u64),
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
    NameConnection {
        /// The location (without any password).
        uri: String,
        /// A password found in the typed address.
        secret: Secret,
    },
    /// Change the location of a saved connection (F4 in the drive list)...
    EditConnection {
        /// The name now.
        name: String,
        /// Its location now.
        uri: String,
    },
    /// ...then its name.
    EditConnectionName {
        /// The name now.
        name: String,
        /// Its location now.
        uri: String,
        /// The new location.
        new_uri: String,
        /// A password found in the typed address.
        secret: Secret,
    },
    /// The user name of a login that has none yet.
    ConnectionUser {
        /// The name of the connection.
        name: String,
        /// Its location.
        uri: String,
        /// The connection being edited (name, location), if any.
        replace: Option<(String, String)>,
        /// Ask for the password every time (else save it).
        ask: bool,
        /// A password found in the typed address.
        secret: Secret,
    },
    /// The password to save in the system keyring.
    ConnectionPassword {
        /// The name of the connection.
        name: String,
        /// Its location (with the user).
        uri: String,
        /// The connection being edited (name, location), if any.
        replace: Option<(String, String)>,
    },
    /// The access key ID of an S3 connection...
    S3AccessKey {
        /// What the keys are for.
        next: S3Next,
    },
    /// ...then its secret access key.
    S3Secret {
        /// What the keys are for.
        next: S3Next,
        /// The access key ID just typed.
        access: String,
    },
    /// The password of a connection that asks for it every time.
    ConnectionOpen {
        /// Its location.
        uri: String,
        /// Test it instead of opening it.
        test: bool,
    },
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
    /// Keep selected only the entries matching the typed mask.
    KeepMask,
    /// The message of a Git commit in this folder.
    GitCommit(std::path::PathBuf),
    /// Filter the panel while typing.
    QuickFilter,
    /// Pack these items into the typed archive.
    PackInto(Vec<VfsPath>),
    /// Advanced pack: the password to protect the archive with (empty:
    /// none)...
    PackPassword {
        /// What to pack.
        paths: Vec<VfsPath>,
        /// The format chosen.
        format: ArchiveFormat,
        /// The compression level chosen.
        level: u32,
    },
    /// ...typed again, to be sure...
    PackPasswordAgain {
        /// What to pack.
        paths: Vec<VfsPath>,
        /// The format chosen.
        format: ArchiveFormat,
        /// The compression level chosen.
        level: u32,
        /// The first one typed.
        first: String,
    },
    /// ...and last, where to write the archive.
    PackAdvanced {
        /// What to pack.
        paths: Vec<VfsPath>,
        /// The format chosen.
        format: ArchiveFormat,
        /// The compression level chosen.
        level: u32,
        /// The password, if any.
        password: Option<String>,
    },
    /// The password of an encrypted archive (the container file).
    ArchivePassword(VfsPath),
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
    /// A value typed for a row of the Properties dialog, which opens again.
    PropsEdit {
        /// The row.
        row: crate::properties::PropRow,
        /// The dialog's state.
        state: Box<crate::properties::PropsState>,
    },
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
    /// Install `pnxr serve` on the typed server.
    InstallServe,
    /// The folder to put the `pnxr` command in.
    InstallCommandFolder,
    /// The API key of this AI provider, and what to do once it is saved.
    AiKey {
        /// The provider.
        provider: String,
        /// What comes next.
        then: crate::app::AiThen,
    },
    /// A text of this AI provider.
    AiProviderField {
        /// The provider.
        provider: String,
        /// Which text.
        field: crate::app::AiField,
    },
    /// The `sudo` password of the server, to turn lingering on; the text is
    /// the install it belongs to (the install is run again with it).
    InstallLinger(String),
}

impl InputPurpose {
    /// Whether the answer is the address of a connection.
    pub fn is_address(&self) -> bool {
        matches!(self, InputPurpose::NewConnection | InputPurpose::EditConnection { .. })
    }

    /// Whether Ctrl+T can test the connection being set up.
    pub fn testable(&self) -> bool {
        self.is_address()
            || matches!(
                self,
                InputPurpose::NameConnection { .. }
                    | InputPurpose::EditConnectionName { .. }
                    | InputPurpose::ConnectionUser { .. }
                    | InputPurpose::ConnectionPassword { .. }
                    | InputPurpose::S3AccessKey { next: S3Next::Save { .. } }
                    | InputPurpose::S3Secret { next: S3Next::Save { .. }, .. }
            )
    }

    /// How many rows under the field the question reserves for what is
    /// written there while typing (examples, what was understood, the new
    /// names): the window keeps its size whatever is shown. At least one,
    /// for the error that may come after Enter.
    pub fn preview_rows(&self) -> usize {
        match self {
            InputPurpose::InstallServe => 10,
            InputPurpose::AiKey { .. } => 11,
            purpose if purpose.is_address() => 8,
            InputPurpose::MultiRename(_) | InputPurpose::AiRename(_) => 8,
            InputPurpose::SelectMask | InputPurpose::UnselectMask | InputPurpose::KeepMask => 6,
            _ => 1,
        }
    }

    /// Whether the answer is a place (a path or an address), shown whole
    /// under the field when it does not fit in it.
    pub fn is_path(&self) -> bool {
        matches!(
            self,
            InputPurpose::CopyTo(_)
                | InputPurpose::MoveTo(_)
                | InputPurpose::GoTo
                | InputPurpose::PackInto(_)
                | InputPurpose::PackAdvanced { .. }
                | InputPurpose::ExportTo { .. }
                | InputPurpose::ExportSettings
                | InputPurpose::ImportSettings
                | InputPurpose::NewConnection
                | InputPurpose::EditConnection { .. }
        )
    }

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
    /// A password: shown as dots.
    pub secret: bool,
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
            secret: false,
        }
    }

    /// The keys shown under the field: the usual ones, then those this
    /// question adds (kept out of the prompt text).
    pub fn hint(&self) -> String {
        match &self.purpose {
            InputPurpose::PackInto(_) => crate::hints::question(&["Ctrl+O: more options"]),
            InputPurpose::SelectMask | InputPurpose::UnselectMask | InputPurpose::KeepMask => {
                crate::hints::question(&[
                    "F2: select / unselect / keep only",
                    "Ctrl+O: presets",
                    "F1: help",
                ])
            }
            InputPurpose::MultiRename(_) => {
                crate::hints::question(&["Ctrl+O: ready-made", "F1: help"])
            }
            InputPurpose::InstallServe => crate::hints::question(&["Ctrl+T: test", "F1: help"]),
            purpose if purpose.testable() => crate::hints::question(&["Ctrl+T: test"]),
            _ => crate::hints::question(&[]),
        }
    }

    /// The field as it is drawn: dots for a password.
    pub fn shown(&self) -> TextField {
        if !self.secret {
            return self.field.clone();
        }
        let mut shown = TextField::new(&"•".repeat(self.field.text().chars().count()));
        shown.set_cursor(self.field.cursor());
        shown
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
    /// Uninstall the plugin with this name.
    UninstallPlugin(String),
    /// Add this plugin repository.
    AddPluginRepo(pnxr_config::PluginRepository),
    /// Give a function key that another command has to this one.
    ReplaceFunctionKey {
        /// The command's id.
        id: String,
        /// Its name for the user.
        name: String,
        /// The key, as text (`Shift+F5`).
        chord: String,
    },
    /// Install `pnxr serve` as typed in the question (server and options).
    InstallServe(String),
    /// Forget something the user saved (the list it came from opens again).
    Forget(Forget),
    /// Replace the checksum files that exist and write them all.
    ChecksumOverwrite(Box<crate::app::ChecksumJob>),
    /// Discard the changes of these files (names relative to the folder).
    GitDiscard(std::path::PathBuf, Vec<String>),
    /// A server of a saved `scs://` location does not answer: offer to
    /// install `pnxr serve` there (its host).
    OfferInstall(String),
}

/// What the user saved that a list can remove. Policy for destructive
/// questions: whatever cannot be undone asks first, and Enter means no when
/// it also touches files or secrets (permanent delete, a saved connection,
/// which may hold a login); for the rest Enter means yes. Taking one path out
/// of the basket asks nothing (the files are untouched, adding it again is
/// one key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forget {
    /// A bookmark, by its index in the settings.
    Bookmark(usize),
    /// A saved search, by its index in the settings.
    Search(usize),
    /// A saved connection (name and location).
    Connection(String, String),
    /// Every path in the basket.
    Basket,
}

impl ConfirmDialog {
    /// Enter says no: what cannot be undone is never one key away.
    pub fn default_no(&self) -> bool {
        matches!(
            self.purpose,
            ConfirmPurpose::Delete { trash: false, .. }
                | ConfirmPurpose::Forget(Forget::Connection(..))
        )
    }

    /// The keys shown under the question.
    pub fn hint(&self) -> String {
        crate::hints::confirm(self.default_no())
    }
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
#[derive(Debug, Clone)]
pub enum ChoosePurpose {
    /// Bookmarks, then recent folders, filtered by what is typed (Enter
    /// opens the row, Delete removes a bookmark).
    Bookmarks(Vec<JumpRow>),
    /// Open one of a bookmark's target folders in the other panel.
    BookmarkTarget(Vec<String>),
    /// Where the login of a connection with a password is kept.
    ConnectionLogin {
        /// The name of the connection.
        name: String,
        /// Its location.
        uri: String,
        /// The connection being edited (name, location), if any.
        replace: Option<(String, String)>,
        /// A password found in the typed address.
        secret: Secret,
    },
    /// The slots of the function key bar for a command (Ctrl+F in the
    /// command list); Enter gives it the highlighted slot.
    FunctionKey {
        /// The command's id.
        id: String,
        /// The command's name for the user.
        name: String,
    },
    /// Open a drive or saved connection, one per row (the last row asks
    /// for a new connection; Delete removes a saved one).
    Drives(Vec<crate::drives::Drive>),
    /// Go to a search hit.
    SearchResults(Vec<VfsPath>),
    /// Replace or merge imported settings.
    ImportMode(Box<ExportBundle>),
    /// Run one of these plugin commands or scripts.
    Plugin(Vec<crate::app::PluginChoice>),
    /// The plugins of the repositories (Enter installs or updates).
    PluginStore(Vec<crate::app::StoreRow>),
    /// The AI providers (their names).
    AiProviders(Vec<String>),
    /// The ready-made providers to add.
    AiPresets(Vec<pnxr_config::AiProvider>),
    /// The rows of one AI provider (its name).
    AiProvider(String),
    /// The rows of "Install the pnxr command".
    InstallCommand(Vec<crate::app::InstallRow>),
    /// The messages of the status line (c copies one, a copies all): the
    /// texts, newest first.
    MessageLog(Vec<String>),
    /// The plugin repositories (Enter switches the official one).
    PluginRepos(Vec<crate::app::RepoRow>),
    /// Copy these items' paths in the chosen form.
    CopyPath(Vec<VfsPath>),
    /// Advanced pack: the format.
    PackFormat {
        /// What to pack.
        paths: Vec<VfsPath>,
        /// The rows' formats.
        formats: Vec<ArchiveFormat>,
    },
    /// Advanced pack: the compression level (1, 3, 6 or 9).
    PackLevel {
        /// What to pack.
        paths: Vec<VfsPath>,
        /// The format chosen.
        format: ArchiveFormat,
    },
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
    /// What a plugin command found (`pnxr.showResults`): rows to sort and
    /// mark, then trash, or give to one of the plugin's commands.
    PluginResults {
        /// The rows, marks and sort.
        results: Box<crate::results::Results>,
        /// The plugin whose commands the actions run.
        plugin: Option<std::path::PathBuf>,
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
    /// Ready-made masks; the choice opens the selection question for
    /// `purpose` with the mask in it.
    MaskPresets(InputPurpose),
    /// The Git actions for the folder `dir` and the files `files` (names
    /// relative to `dir`).
    Git {
        /// The folder of the panel.
        dir: std::path::PathBuf,
        /// The selected files.
        files: Vec<String>,
    },
    /// Branches to switch to.
    GitBranches {
        /// The folder of the panel.
        dir: std::path::PathBuf,
        /// The branch of each row.
        names: Vec<String>,
    },
    /// The ready-made multi-rename rules; the files they are for.
    RenamePresets(Vec<VfsPath>),
    /// Checksum files to write for `sources` (relative to `base`), and, when
    /// the cursor is on a checksum file, to check it (the first row).
    Checksums {
        /// The folder the names are relative to.
        base: VfsPath,
        /// The files and folders to hash.
        sources: Vec<VfsPath>,
        /// A checksum file that can be checked.
        verify: Option<VfsPath>,
    },
    /// The Properties of the items: read-only until `e` or F2.
    Properties(Box<crate::properties::PropsState>),
    /// An editor command for a setting; the first row is the default and the
    /// last lets the user type a command, back to the settings afterwards.
    Editor {
        /// The option.
        field: Field,
        /// The screen to go back to.
        screen: Box<SettingsScreen>,
        /// The command of each row except the last ("" is the default).
        commands: Vec<String>,
    },
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
#[derive(Debug, Clone)]
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
            footer: crate::hints::list("choose", &[], "cancel"),
            query: None,
        }
    }

    /// Whether the rows change while the list is open (filtered while typing,
    /// search results arriving): the window then keeps one size.
    pub fn keeps_size(&self) -> bool {
        self.query.is_some()
            || matches!(
                self.purpose,
                ChoosePurpose::SearchResults(_) | ChoosePurpose::PluginResults { .. }
            )
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
    /// Or Enter goes back to this list.
    pub then_choose: Option<Box<ChooseDialog>>,
    /// Enter goes back to the dialog it came from (a help window).
    pub back: bool,
    /// Enter or Esc goes back to the settings screen.
    pub back_to_settings: Option<Box<crate::settings::SettingsScreen>>,
    /// The logo is shown above the text (About).
    pub logo: bool,
    /// The work behind it is still going: lines are added as it goes.
    pub busy: bool,
    /// The text is output with columns (checksums, git): a window shows it
    /// in the fixed-width font.
    pub fixed: bool,
}

impl MessageDialog {
    /// A plain message.
    pub fn new(title: &str, lines: Vec<String>) -> Self {
        MessageDialog {
            title: title.to_owned(),
            lines,
            then: None,
            then_choose: None,
            back: false,
            back_to_settings: None,
            logo: false,
            busy: false,
            fixed: false,
        }
    }

    /// The same text, in the fixed-width font of a window.
    pub fn with_fixed_font(mut self) -> Self {
        self.fixed = true;
        self
    }

    /// A log (the steps of an install, a connection test) is read from its
    /// end; any other text from its start, and it scrolls with the keys.
    pub fn is_log(&self) -> bool {
        self.busy || self.lines.first().is_some_and(|l| l.starts_with('['))
    }

    /// The hint below the text: `Enter: <next dialog>   Esc: close`.
    pub fn hint(&self) -> String {
        let copy = if self.can_copy() { "c: copy the log    " } else { "" };
        if self.busy {
            return format!("Working…    {copy}Esc: close");
        }
        if self.back {
            return format!("Enter: back    {copy}Esc: close");
        }
        match (&self.then, &self.then_choose) {
            (Some(next), _) => {
                format!("Enter: {}    {copy}Esc: close", next.title.to_lowercase())
            }
            (None, Some(next)) => {
                format!("Enter: {}    {copy}Esc: close", next.title.to_lowercase())
            }
            (None, None) => format!("{copy}Esc: close"),
        }
    }

    /// Whether `c` copies this window's text to the clipboard: a log (an
    /// install, a test), not a short message or the About window.
    pub fn can_copy(&self) -> bool {
        !self.logo && self.lines.len() >= 3
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
