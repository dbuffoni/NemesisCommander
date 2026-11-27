//! Commands, their shortcuts, and the help overlay.
//!
//! Pairnox has no toolbar or icons: every command is an [`Action`]
//! reachable from the menu and from a key chord. The same table drives the
//! TUI, the GUI, the menus and the help overlay (F1), so they never drift
//! apart.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::str::FromStr;

/// Menu groups, in menu-bar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    /// File operations.
    Files,
    /// Selection.
    Select,
    /// Moving around.
    Navigate,
    /// Panel display.
    View,
    /// Tools and remote.
    Tools,
    /// Application.
    App,
}

impl Category {
    /// Title of the menu.
    pub fn title(self) -> &'static str {
        match self {
            Category::Files => "Files",
            Category::Select => "Select",
            Category::Navigate => "Navigate",
            Category::View => "View",
            Category::Tools => "Tools",
            Category::App => "Pairnox",
        }
    }

    /// All categories in menu order.
    pub const ALL: [Category; 6] = [
        Category::Files,
        Category::Select,
        Category::Navigate,
        Category::View,
        Category::Tools,
        Category::App,
    ];
}

/// Declares the [`Action`] enum together with its metadata, so adding a
/// command is one line in one place.
macro_rules! actions {
    ($( $variant:ident = $id:literal, $category:ident, $essential:literal, $label:literal, $short:literal, [$($key:literal),*]; )*) => {
        /// Every command of the application.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action { $( #[doc = $label] $variant, )* }

        impl Action {
            /// All actions, in menu order.
            pub const ALL: &'static [Action] = &[ $( Action::$variant, )* ];

            /// Stable identifier used in the configuration file (`files.copy`).
            pub fn id(self) -> &'static str {
                match self { $( Action::$variant => $id, )* }
            }

            /// Human-readable label for menus and the help overlay.
            pub fn label(self) -> &'static str {
                match self { $( Action::$variant => $label, )* }
            }

            /// A name of at most 8 characters for a function-key button of
            /// the bottom bar (ADR 0055).
            pub fn short(self) -> &'static str {
                match self { $( Action::$variant => $short, )* }
            }

            /// The menu it belongs to.
            pub fn category(self) -> Category {
                match self { $( Action::$variant => Category::$category, )* }
            }

            /// Whether the help overlay lists it (the "main shortcuts").
            pub fn is_essential(self) -> bool {
                match self { $( Action::$variant => $essential, )* }
            }

            /// Built-in key chords, as text.
            pub fn default_keys(self) -> &'static [&'static str] {
                match self { $( Action::$variant => &[$($key),*], )* }
            }
        }
    };
}

actions! {
    // Files
    View          = "files.view",           Files, true,  "View file",                "View", ["F3"];
    Edit          = "files.edit",           Files, true,  "Edit file",                "Edit", ["F4"];
    Copy          = "files.copy",           Files, true,  "Copy",                     "Copy", ["F5"];
    Move          = "files.move",           Files, true,  "Move",                     "Move", ["F6"];
    Rename        = "files.rename",         Files, true,  "Rename",                   "Rename", ["F2"];
    MultiRename   = "files.multi-rename",   Files, false, "Multi-rename…",            "MultiRen", ["Shift+F2"];
    AiRename      = "files.ai-rename",      Files, false, "Multi-rename with AI…",    "AIRen", [];
    MakeDir       = "files.mkdir",          Files, true,  "New folder",               "Mkdir", ["F7"];
    Delete        = "files.delete",         Files, true,  "Delete (to trash)",        "Delete", ["F8", "Delete"];
    DeleteForever = "files.delete-forever", Files, false, "Delete permanently",       "Erase", ["Shift+F8", "Shift+Delete"];
    Pack          = "files.pack",           Files, true,  "Pack into archive…",       "Pack", ["Shift+F5"];
    Unpack        = "files.unpack",         Files, false, "Unpack archive…",          "Unpack", ["Shift+F6"];
    Properties    = "files.properties",     Files, false, "Properties…", "Props", ["Shift+F4", "Ctrl+E"];
    Undo          = "files.undo",           Files, false, "Undo rename, new folder, move", "Undo", ["Ctrl+Z"];
    Checksum      = "files.checksum",       Files, false, "Checksum",                 "Checksum", ["Ctrl+F6", "Ctrl+K"];
    CopyPath      = "files.copy-path",      Files, false, "Copy path to clipboard…",  "CopyPath", ["Ctrl+P"];
    Export        = "files.export",         Files, false, "Export listing…",          "Export", [];
    Link          = "files.link",           Files, false, "Create symlink or hard link…", "Link", ["Shift+F7"];
    BulkRename    = "files.bulk-rename",    Files, false, "Rename in the editor…",    "EdRename", [];
    OpenTerminal  = "files.open-terminal",  Files, false, "Open terminal here",       "Terminal", [];
    BasketAdd     = "files.basket-add",     Files, false, "Add to basket",            "ToBasket", ["Ctrl+G"];
    // Select
    ToggleSelect  = "select.toggle",        Select, true, "Select / unselect",        "Select", ["Insert", "Space"];
    SelectMask    = "select.mask",          Select, true, "Select by mask…",          "SelMask", ["+"];
    UnselectMask  = "select.unmask",        Select, false, "Unselect by mask…",       "UnsMask", ["-"];
    InvertSelect  = "select.invert",        Select, false, "Invert selection",        "Invert", ["*"];
    SelectAll     = "select.all",           Select, false, "Select all",              "SelAll", ["Ctrl+A"];
    // Navigate
    Open          = "nav.open",             Navigate, false, "Open / enter",          "Open", ["Enter"];
    Parent        = "nav.parent",           Navigate, false, "Parent folder",         "Up", ["Backspace"];
    SwitchPanel   = "nav.switch-panel",     Navigate, true,  "Other panel",           "Panel", ["Tab"];
    GoTo          = "nav.goto",             Navigate, true,  "Go to location…",       "GoTo", ["Ctrl+L"];
    Back          = "nav.back",             Navigate, false, "Back",                  "Back", ["Ctrl+O"];
    Forward       = "nav.forward",          Navigate, false, "Forward",               "Forward", ["Ctrl+Y"];
    Drives        = "nav.drives",           Navigate, true,  "Drives and connections…", "Drives", ["Shift+F1", "Ctrl+N", "d"];
    Bookmarks     = "nav.bookmarks",        Navigate, true,  "Bookmarks…",            "Bookmark", ["Ctrl+D"];
    AddBookmark   = "nav.add-bookmark",     Navigate, false, "Bookmark this folder",  "AddMark", ["Ctrl+Shift+D"];
    AddBookmarkTarget = "nav.add-bookmark-target", Navigate, false, "Bookmark this folder with the other panel's", "MarkPair", ["Ctrl+Shift+B"];
    NewTab        = "nav.new-tab",          Navigate, false, "New tab",               "NewTab", ["Ctrl+T"];
    CloseTab      = "nav.close-tab",        Navigate, false, "Close tab",             "CloseTab", ["Ctrl+W"];
    NextTab       = "nav.next-tab",         Navigate, false, "Next tab",              "NextTab", ["Ctrl+Tab", "Ctrl+PageDown"];
    PrevTab       = "nav.prev-tab",         Navigate, false, "Previous tab",          "PrevTab", ["Ctrl+Shift+Tab", "Ctrl+PageUp"];
    SamePath      = "nav.same-path",        Navigate, false, "Open this folder in other panel", "SamePath", ["Ctrl+Up"];
    OpenInLeft    = "nav.open-in-left",     Navigate, false, "Open folder in left panel", "ToLeft", ["Ctrl+Left"];
    OpenInRight   = "nav.open-in-right",    Navigate, false, "Open folder in right panel", "ToRight", ["Ctrl+Right"];
    FlatView      = "nav.flat-view",        Navigate, false, "Flat (branch) view",    "Flat", ["Ctrl+Shift+F"];
    Basket        = "nav.basket",           Navigate, false, "Basket…",               "Basket", ["Ctrl+B"];
    SwapPanels    = "nav.swap-panels",      Navigate, false, "Swap panels",           "Swap", ["Ctrl+Down", "Ctrl+U"];
    // View
    Refresh       = "view.refresh",         View, false, "Refresh",                   "Refresh", ["Ctrl+R"];
    ToggleHidden  = "view.hidden",          View, false, "Show hidden files",         "Hidden", ["Ctrl+H"];
    QuickFilter   = "view.filter",          View, true,  "Quick filter…",             "Filter", ["Ctrl+S"];
    SortName      = "view.sort-name",       View, false, "Sort by name",              "SortName", ["Ctrl+1"];
    SortExtension = "view.sort-ext",        View, false, "Sort by extension",         "SortExt", ["Ctrl+2"];
    SortModified  = "view.sort-date",       View, false, "Sort by date",              "SortDate", ["Ctrl+3"];
    QuickView     = "view.quick-view",      View, false, "Quick view in other panel", "QuickVw", ["Shift+F3", "Ctrl+Q"];
    BarRow        = "view.bar-row",         View, false, "Bottom bar: next row (Shift / Ctrl)", "BarRow", ["Ctrl+F"];
    // Tools
    Search        = "tools.search",         Tools, true,  "Search…",                  "Search", ["Ctrl+F2"];
    Compare       = "tools.compare",        Tools, false, "Compare files / folders",  "Compare", ["Ctrl+F3"];
    CompareHash   = "tools.compare-hash",   Tools, false, "Compare folders by content…", "CmpHash", [];
    Sync          = "tools.sync",           Tools, false, "Synchronize folders…",     "Sync", ["Ctrl+F4"];
    GenerateFiles = "tools.generate-files", Tools, false, "Generate random text files…", "Generate", [];
    Duplicates    = "tools.duplicates",     Tools, false, "Find duplicate files",     "Dupes", ["Ctrl+F5"];
    DiskUsage     = "tools.disk-usage",     Tools, false, "Disk usage of this folder", "DiskUse", ["Ctrl+F7"];
    Git           = "tools.git",            Tools, false, "Git actions…",             "Git", ["Ctrl+F11", "Ctrl+Shift+G"];
    Disks         = "tools.disks",          Tools, false, "Disks and partitions…",    "Disks", ["Ctrl+F8"];
    SavedSearches = "tools.saved-searches", Tools, false, "Saved searches…",          "Searches", [];
    SaveSearch    = "tools.save-search",    Tools, false, "Save the last search…",    "SaveSrch", [];
    Jobs          = "tools.jobs",           Tools, true,  "Transfer queue",           "Queue", ["Ctrl+J"];
    RunScript     = "tools.run-script",     Tools, false, "Plugins and scripts…",     "Plugins", ["Ctrl+F9", "Ctrl+X"];
    AiScript      = "tools.ai-script",      Tools, false, "Write a script with AI…",  "AIScript", [];
    InstallServe  = "tools.install-serve",  Tools, false, "Install pnxr serve on a server…", "Install", [];
    // App
    Help          = "app.help",             App, true,  "Show main shortcuts",        "Help", ["F1"];
    Menu          = "app.commands",         App, true,  "Commands",                   "Commands", ["F9"];
    Settings      = "app.settings",         App, true,  "Settings…",                  "Settings", ["Ctrl+,", "Ctrl+F1"];
    ExportConfig  = "app.export-config",    App, false, "Export settings to file…",   "ExpConf", [];
    ImportConfig  = "app.import-config",    App, false, "Import settings from file…", "ImpConf", [];
    Choose        = "app.choose",           App, false, "Choose and exit (pnxr pick)", "Choose", ["Ctrl+F10"];
    About         = "app.about",            App, false, "About Pairnox", "About", [];
    Quit          = "app.quit",             App, true,  "Quit",                       "Quit", ["F10"];
}

impl Action {
    /// The name for the narrow terminal bar (five columns): [`Action::short`]
    /// when that fits, else a shorter one.
    pub fn narrow(self) -> &'static str {
        match self {
            Action::Menu => "Cmds",
            Action::Rename => "Ren",
            Action::Delete => "Del",
            Action::DeleteForever => "Erase",
            Action::Search => "Find",
            Action::Unpack => "Unpk",
            Action::Drives => "Drive",
            Action::MultiRename => "MRen",
            Action::Settings => "Setup",
            Action::Compare => "Cmp",
            Action::SortName => "Name",
            Action::SortExtension => "Ext",
            Action::SortModified => "Date",
            Action::ToggleHidden => "Hide",
            Action::DiskUsage => "Usage",
            Action::Choose => "Pick",
            other => {
                let short = other.short();
                match short.char_indices().nth(5) {
                    Some((end, _)) => &short[..end],
                    None => short,
                }
            }
        }
    }

    /// What the command does, in a sentence or two, for the command list's
    /// help (F1 there).
    pub fn help(self) -> &'static str {
        match self {
            Action::View => {
                "Opens the file under the cursor in the viewer: text, hex, pictures, PDF, Office documents, Markdown and 3D models. Esc closes it."
            }
            Action::Edit => {
                "Opens the file under the cursor in your editor (set in Settings; $EDITOR by default)."
            }
            Action::Copy => {
                "Copies the selected items (or the one under the cursor) to the other panel's folder, or to the folder you type. Runs in the transfer queue."
            }
            Action::Move => {
                "Moves the selected items (or the one under the cursor) to the other panel's folder, or to the folder you type. Undo (Ctrl+Z) can bring them back."
            }
            Action::Rename => "Renames the item under the cursor.",
            Action::MultiRename => {
                "Renames many items at once with a template ([N] name, [E] extension, [C] counter...). The new names are shown live, and a full preview before anything changes."
            }
            Action::AiRename => {
                "Describe in words how the selected files should be renamed; the AI writes the rule and you see the preview before it is applied. Needs an Anthropic key."
            }
            Action::MakeDir => "Creates a new folder in the active panel.",
            Action::Delete => {
                "Deletes the selected items (or the one under the cursor), to the trash when the system has one."
            }
            Action::DeleteForever => "Deletes the selected items for good, without the trash.",
            Action::Pack => {
                "Packs the selected items into an archive (zip, 7z, tar.zst...); the type comes from the extension you type. Ctrl+O in the question opens more options: format, compression level and a password (zip and 7z, AES-256; 7z also hides the file names). Works on the basket too."
            }
            Action::Unpack => {
                "Unpacks the archive under the cursor into the other panel's folder. An encrypted archive asks for its password."
            }
            Action::Properties => {
                "Shows size, dates, owner and permissions of the selected items. It opens read-only; e or F2 turns on editing: a check box for each permission with the octal number kept in step, a checked date, and optionally everything inside folders. Ctrl+S applies."
            }
            Action::Undo => {
                "Undoes the last rename, new folder or move, up to 50 steps in this session."
            }
            Action::Checksum => {
                "Computes the checksum (SHA-256 and others) of the file under the cursor and can copy it or compare it with one you paste."
            }
            Action::CopyPath => {
                "Copies the path of the selected items to the clipboard, in the form you choose (full, name only, URL...)."
            }
            Action::Export => {
                "Writes the listing of the panel (or the selection) to a file or the clipboard as text, CSV, JSON or Markdown."
            }
            Action::Link => {
                "Creates a symbolic or hard link to the item under the cursor in the other panel's folder."
            }
            Action::OpenTerminal => {
                "Opens a terminal in the folder of the active panel (the command is set in Settings). In the terminal interface it quits and leaves the shell in the folder, once `pnxr shell-init` is set up; on a server it opens an SSH session in that folder."
            }
            Action::BulkRename => {
                "Opens the names of the selected items in your editor; save and close it to rename them."
            }
            Action::BasketAdd => {
                "Adds the selected items to the basket, a list that collects files from many folders so they can be copied, moved, deleted or packed together (Ctrl+B)."
            }
            Action::ToggleSelect => {
                "Selects or unselects the item under the cursor and moves down."
            }
            Action::SelectMask => "Selects the items whose names match a mask such as *.jpg.",
            Action::UnselectMask => "Unselects the items whose names match a mask.",
            Action::InvertSelect => "Selects what is not selected and unselects what is.",
            Action::SelectAll => "Selects every item of the active panel.",
            Action::Open => {
                "Enters the folder or archive under the cursor, or opens the file with the system's program."
            }
            Action::Parent => "Goes up to the parent folder.",
            Action::SwitchPanel => "Moves the focus to the other panel.",
            Action::GoTo => {
                "Goes to any location: a path, ~/..., or a URI such as sftp://host/dir."
            }
            Action::Back => "Goes back to the previous folder of this panel.",
            Action::Forward => "Goes forward again after Back.",
            Action::Drives => {
                "Lists the local drives and your saved connections (SFTP, agent, WebDAV, S3, SMB), and lets you add a new connection."
            }
            Action::Bookmarks => {
                "Lists your bookmarks and the recent folders; type to filter. A bookmark can also open the other panel."
            }
            Action::AddBookmark => "Bookmarks the folder of the active panel.",
            Action::AddBookmarkTarget => {
                "Bookmarks this folder together with the other panel's folder: opening it later opens both."
            }
            Action::NewTab => "Opens a new tab in the active panel.",
            Action::CloseTab => "Closes the current tab of the active panel.",
            Action::NextTab => "Goes to the next tab of the active panel.",
            Action::PrevTab => "Goes to the previous tab of the active panel.",
            Action::SamePath => {
                "Shows this folder (or the one under the cursor) in the other panel."
            }
            Action::OpenInLeft => "Shows the folder under the cursor in the left panel.",
            Action::OpenInRight => "Shows the folder under the cursor in the right panel.",
            Action::Basket => {
                "Opens the basket: the items collected from several folders. From there you can copy, move, delete, pack or rename them together."
            }
            Action::SwapPanels => {
                "Swaps the two panels. With the quick view open, the file panel and the quick view trade places."
            }
            Action::Refresh => "Reads the folders of both panels again.",
            Action::ToggleHidden => "Shows or hides hidden files.",
            Action::QuickFilter => {
                "Narrows the listing as you type (`*.pdf`, `report`). Esc clears the filter."
            }
            Action::SortName => "Sorts by name; pressing it again reverses the order.",
            Action::SortExtension => "Sorts by extension; pressing it again reverses the order.",
            Action::SortModified => {
                "Sorts by modification date; pressing it again reverses the order."
            }
            Action::BarRow => {
                "Switches the bottom bar between its three rows: F1-F10, Shift+F1-F10 and Ctrl+F1-F10. Terminals do not report a held Shift or Ctrl, so this key stands in for holding it; the window also switches while the key is held."
            }
            Action::QuickView => {
                "Shows a preview of the file under the cursor in the other panel. Shift+arrows and Shift+PgUp/PgDn scroll it."
            }
            Action::Search => {
                "Searches files by name and, if you want, by content, in this folder and below, also on remote locations."
            }
            Action::Compare => {
                "Compares the two files under the cursors (line by line), or the two folders by name, size and date: the differences are selected, and Esc clears them."
            }
            Action::CompareHash => {
                "Compares the two folders by the checksum of each file, so equal names with different content are found."
            }
            Action::Sync => {
                "Plans how to make one folder match the other (mirror, update or two-way), shows what would change, and runs it only when you confirm."
            }
            Action::Duplicates => {
                "Finds files with the same content in this folder and below; the extra copies can go to the basket."
            }
            Action::DiskUsage => {
                "Lists what takes the space in this folder, biggest first, and lets you go into the folders."
            }
            Action::Git => {
                "Basic Git actions on the repository of this folder: stage and unstage, diff, commit, discard changes (asks first), switch branch, log, pull and push. Works on the selected files; needs git installed."
            }
            Action::Disks => "Lists the disks and partitions with size, type, file system and use.",
            Action::SavedSearches => "Lists the searches you saved and runs the one you choose.",
            Action::SaveSearch => "Saves the last search under a name so you can run it again.",
            Action::Jobs => {
                "Shows the transfer queue: progress, speed, pause, resume, cancel and priority of each copy or move."
            }
            Action::RunScript => {
                "Lists the installed plugins and your scripts and runs the one you choose."
            }
            Action::AiScript => {
                "Describe what you want in words; the AI writes a TypeScript script, tests it in a scratch copy and saves it. Needs an Anthropic key."
            }
            Action::InstallServe => {
                "Installs `pnxr serve` on a server over SSH, with certificates, so that you can connect to it securely and copy straight between servers."
            }
            Action::Help => "Shows the main shortcuts.",
            Action::Menu => {
                "Lists every command with its shortcut; type to find one, Ctrl+K gives it a shortcut, F1 explains it."
            }
            Action::Settings => {
                "Opens the settings: panels, fonts, transfers, viewer and shortcuts. Type to find a setting."
            }
            Action::ExportConfig => {
                "Writes settings, bookmarks and connections to a file you can take to another machine."
            }
            Action::ImportConfig => {
                "Reads settings, bookmarks and connections from a file written by Export, replacing or merging."
            }
            Action::FlatView => {
                "Lists every file below this folder in one flat list, with its subfolder in a column. Press it again, or Backspace, to come back."
            }
            Action::Choose => {
                "Only in pnxr pick: gives the marked items, the item under the cursor or, when asked for a folder, the current folder to the program that started pnxr pick."
            }
            Action::About => {
                "Shows the logo, the version and the commit this build was made from, the licence and where the source is."
            }
            Action::GenerateFiles => {
                "Makes text files of random words in this folder, for trying transfers and speeds: asks the name, how many (a counter is added to the name when there are several) and the size of each (10k, 2.5M, 1G)."
            }
            Action::Quit => "Closes the application.",
        }
    }
}

impl Action {
    /// Finds an action by its [`id`](Self::id).
    pub fn from_id(id: &str) -> Option<Action> {
        // Renamed when the connection list became the drive list (ADR 0019).
        let id = match id {
            "tools.connect" => "nav.drives",
            "nav.jump" => "nav.bookmarks",
            other => other,
        };
        Action::ALL.iter().copied().find(|a| a.id() == id)
    }
}

/// A key without modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// A printable character. Letters are stored lowercase; use
    /// [`KeyChord::shift`] for capitals.
    Char(char),
    /// Function key F1–F24.
    F(u8),
    /// Enter / Return.
    Enter,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Space bar.
    Space,
}

const NAMED_KEYS: &[(&str, Key)] = &[
    ("Enter", Key::Enter),
    ("Esc", Key::Esc),
    ("Tab", Key::Tab),
    ("Backspace", Key::Backspace),
    ("Delete", Key::Delete),
    ("Insert", Key::Insert),
    ("Home", Key::Home),
    ("End", Key::End),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
    ("Up", Key::Up),
    ("Down", Key::Down),
    ("Left", Key::Left),
    ("Right", Key::Right),
    ("Space", Key::Space),
];

/// A key plus modifiers, e.g. `Ctrl+Shift+F5`.
///
/// Front ends normalize their native key events into chords: an uppercase
/// letter becomes the lowercase letter with `shift`, and `shift` is ignored
/// for other characters (`+` is just `+`, whatever the keyboard layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyChord {
    /// Control (Command is *not* mapped here; see the GUI docs).
    pub ctrl: bool,
    /// Alt / Option.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// The key.
    pub key: Key,
}

impl KeyChord {
    /// A chord without modifiers.
    pub fn plain(key: Key) -> Self {
        KeyChord { ctrl: false, alt: false, shift: false, key }
    }

    /// Builds a chord from raw front-end data, applying the normalization
    /// rules described on the type.
    pub fn normalized(key: Key, ctrl: bool, alt: bool, shift: bool) -> Self {
        match key {
            Key::Char(c) if c.is_alphabetic() => {
                let lower = c.to_lowercase().next().unwrap_or(c);
                KeyChord { ctrl, alt, shift: shift || lower != c, key: Key::Char(lower) }
            }
            Key::Char(' ') => KeyChord { ctrl, alt, shift, key: Key::Space },
            Key::Char(_) => KeyChord { ctrl, alt, shift: false, key },
            _ => KeyChord { ctrl, alt, shift, key },
        }
    }
}

/// Error for an unparsable chord.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid key {0:?}")]
pub struct KeyParseError(pub String);

impl FromStr for KeyChord {
    type Err = KeyParseError;

    /// Parses `Ctrl+Alt+Shift+Key`. Modifier names are case-insensitive;
    /// the key is the last part (`Ctrl++` is Ctrl and the plus key).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let err = || KeyParseError(text.to_owned());
        let (mods, key) = if let Some(rest) = text.strip_suffix("++") {
            (rest, "+")
        } else if text == "+" {
            ("", "+")
        } else {
            match text.rsplit_once('+') {
                Some((mods, key)) => (mods, key),
                None => ("", text),
            }
        };
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                _ => return Err(err()),
            }
        }
        let key = if let Some((_, k)) =
            NAMED_KEYS.iter().find(|(name, _)| name.eq_ignore_ascii_case(key))
        {
            *k
        } else if let Some(n) = key.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u8>().ok()) {
            if !(1..=24).contains(&n) {
                return Err(err());
            }
            Key::F(n)
        } else {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                // In chord text letters are case-insensitive: "Ctrl+D" is
                // Ctrl and the D key; Shift must be written explicitly.
                (Some(c), None) => Key::Char(c.to_lowercase().next().unwrap_or(c)),
                _ => return Err(err()),
            }
        };
        Ok(KeyChord::normalized(key, ctrl, alt, shift))
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.key {
            Key::Char(c) => write!(f, "{}", c.to_uppercase()),
            Key::F(n) => write!(f, "F{n}"),
            named => {
                let name =
                    NAMED_KEYS.iter().find(|(_, k)| *k == named).map(|(n, _)| *n).unwrap_or("?");
                f.write_str(name)
            }
        }
    }
}

/// Ids in `[keys]` that name commands of plugins (`plugin.NAME.COMMAND`)
/// or user scripts (`script.NAME`) rather than built-in actions. They
/// have no built-in keys.
pub const EXTRA_PREFIXES: [&str; 3] = ["plugin.", "script.", "search."];

/// Whether `id` names a plugin or script command (see [`EXTRA_PREFIXES`]).
pub fn is_extra_command(id: &str) -> bool {
    EXTRA_PREFIXES.iter().any(|prefix| id.starts_with(prefix))
}

/// The shortcut model written by this version (ADR 0059): standard shortcuts
/// in `keys`, one function key per command in `fkeys`, short names in `labels`.
pub const SHORTCUT_MODEL: u32 = 2;

/// The longest short name of a function key, in characters.
pub const LABEL_MAX: usize = 12;

/// Whether `chord` is a function key of the bottom bar: F1 to F10, bare or
/// with Shift or Ctrl (never Alt).
pub fn is_function_key(chord: &KeyChord) -> bool {
    matches!(chord.key, Key::F(n) if (1..=10).contains(&n))
        && !chord.alt
        && !(chord.ctrl && chord.shift)
}

/// [`is_function_key`] for chord text; text that is no chord is not one.
pub fn is_function_key_text(text: &str) -> bool {
    text.parse::<KeyChord>().is_ok_and(|c| is_function_key(&c))
}

/// The effective key bindings: defaults plus user overrides.
#[derive(Debug, Clone)]
pub struct Keymap {
    /// Every chord that runs a built-in action (shortcuts and function keys).
    by_chord: HashMap<KeyChord, Action>,
    /// The chords of an action: its function key first, then its shortcuts.
    by_action: BTreeMap<Action, Vec<KeyChord>>,
    /// Plugin and script commands, by id.
    extra_by_chord: HashMap<KeyChord, String>,
    by_extra: BTreeMap<String, Vec<KeyChord>>,
    /// The function key of each command (built-in or extra), by id.
    fkey_by_id: BTreeMap<String, KeyChord>,
    /// The custom short names, by id.
    labels: BTreeMap<String, String>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::with_overrides(&BTreeMap::new()).0
    }
}

/// Commands that were removed: a settings file that still binds a key to
/// one is not a mistake, so no warning is made.
const RETIRED: &[&str] = &["view.folder-sizes"];

/// Commands that moved to a plugin: a key given to the old id is given to the
/// plugin's command, and works once the plugin is installed (ADR 0060).
pub const MOVED_TO_PLUGIN: &[(&str, &str)] =
    &[("tools.similar-images", "plugin.image-duplicates.find")];

/// The id a command has now (`id` itself unless it moved to a plugin).
pub fn current_id(id: &str) -> &str {
    MOVED_TO_PLUGIN.iter().find(|(old, _)| *old == id).map_or(id, |(_, new)| new)
}

/// What a plugin declares for one of its commands in `plugin.json`: the
/// defaults of the shortcut, the function key and the button name. They sit
/// below everything the user sets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginKeyDefault {
    /// The command id, `plugin.NAME.COMMAND`.
    pub id: String,
    /// A standard shortcut (not a function key).
    pub shortcut: Option<String>,
    /// A function key (F1 to F10, with Shift or Ctrl).
    pub fkey: Option<String>,
    /// The short name of the button.
    pub label: Option<String>,
}

/// A command's two slots while the keymap is built.
#[derive(Debug, Clone, Default)]
struct Slots {
    shortcuts: Vec<KeyChord>,
    fkey: Option<KeyChord>,
}

impl Slots {
    fn drop_chord(&mut self, chord: &KeyChord) {
        self.shortcuts.retain(|c| c != chord);
        if self.fkey.as_ref() == Some(chord) {
            self.fkey = None;
        }
    }
}

impl Action {
    /// The built-in standard shortcuts: the default chords that are not
    /// function keys.
    pub fn default_shortcuts(self) -> Vec<KeyChord> {
        self.default_keys()
            .iter()
            .filter(|k| !is_function_key_text(k))
            .map(|k| k.parse().unwrap_or_else(|_| panic!("built-in key {k:?} must parse")))
            .collect()
    }

    /// The built-in function key, if it has one.
    pub fn default_fkey(self) -> Option<KeyChord> {
        self.default_keys().iter().find(|k| is_function_key_text(k)).and_then(|k| k.parse().ok())
    }
}

impl Keymap {
    /// Builds the keymap from the defaults and the `keys` overrides alone
    /// (command id → standard shortcuts).
    pub fn with_overrides(overrides: &BTreeMap<String, Vec<String>>) -> (Keymap, Vec<String>) {
        Keymap::build(overrides, &BTreeMap::new(), &BTreeMap::new())
    }

    /// Builds the keymap from the defaults and the overrides of `config`.
    pub fn from_config(config: &crate::Config) -> (Keymap, Vec<String>) {
        Keymap::build_with(&config.keys, &config.fkeys, &config.labels, &config.plugin_defaults)
    }

    /// Builds the keymap. `keys` replace the shortcuts of a command, `fkeys`
    /// its function key (empty text: none) and `labels` its short name. A
    /// chord given to one command is taken away from any other. Problems
    /// (unknown commands, bad chords) are returned as warnings and otherwise
    /// ignored.
    pub fn build(
        keys: &BTreeMap<String, Vec<String>>,
        fkeys: &BTreeMap<String, String>,
        labels: &BTreeMap<String, String>,
    ) -> (Keymap, Vec<String>) {
        Keymap::build_with(keys, fkeys, labels, &[])
    }

    /// Like [`build`](Keymap::build), with the defaults that plugins declare
    /// for their commands. A plugin default never takes a chord that a
    /// built-in command or an earlier plugin has, and everything the user
    /// sets (`keys`, `fkeys`, `labels`, an empty text for none) wins.
    pub fn build_with(
        keys: &BTreeMap<String, Vec<String>>,
        fkeys: &BTreeMap<String, String>,
        labels: &BTreeMap<String, String>,
        defaults: &[PluginKeyDefault],
    ) -> (Keymap, Vec<String>) {
        let mut warnings = Vec::new();
        let mut slots: BTreeMap<String, Slots> = BTreeMap::new();
        for &action in Action::ALL {
            slots.insert(
                action.id().to_owned(),
                Slots { shortcuts: action.default_shortcuts(), fkey: action.default_fkey() },
            );
        }
        let mut default_labels: BTreeMap<String, String> = BTreeMap::new();
        for default in defaults {
            if !is_extra_command(&default.id) || slots.contains_key(&default.id) {
                continue;
            }
            let taken = |slots: &BTreeMap<String, Slots>, chord: &KeyChord| {
                slots.iter().find_map(|(other, slot)| {
                    (slot.fkey.as_ref() == Some(chord) || slot.shortcuts.contains(chord))
                        .then(|| other.clone())
                })
            };
            let mut slot = Slots::default();
            for (text, function) in [(&default.shortcut, false), (&default.fkey, true)] {
                let Some(text) = text.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
                    continue;
                };
                let chord = match text.parse::<KeyChord>() {
                    Ok(chord) => chord,
                    Err(e) => {
                        warnings.push(format!("{e} in the plugin default of {}", default.id));
                        continue;
                    }
                };
                if is_function_key(&chord) != function {
                    warnings.push(format!(
                        "{chord} is {} in the plugin default of {}",
                        if function { "not a function key" } else { "a function key" },
                        default.id
                    ));
                } else if let Some(owner) = taken(&slots, &chord) {
                    warnings.push(format!(
                        "{chord} wanted by {} is already used by {owner}; not set",
                        default.id
                    ));
                } else if function {
                    slot.fkey = Some(chord);
                } else {
                    slot.shortcuts.push(chord);
                }
            }
            slots.insert(default.id.clone(), slot);
            if let Some(label) = default.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
                default_labels.insert(default.id.clone(), label.to_owned());
            }
        }
        // "Change dates and permissions" became part of Properties (0.5.0):
        // a key given to it now opens Properties.
        let mut keys = keys.clone();
        if let Some(list) = keys.remove("files.attributes") {
            keys.entry("files.properties".to_owned()).or_insert(list);
        }
        // Keys of a command that moved to a plugin follow it.
        let rename = |map: &mut BTreeMap<String, Vec<String>>| {
            for (old, new) in MOVED_TO_PLUGIN {
                if let Some(list) = map.remove(*old) {
                    map.entry((*new).to_owned()).or_insert(list);
                }
            }
        };
        rename(&mut keys);
        let mut fkeys = fkeys.clone();
        let mut labels = labels.clone();
        for (id, label) in default_labels {
            labels.entry(id).or_insert(label);
        }
        for (old, new) in MOVED_TO_PLUGIN {
            if let Some(v) = fkeys.remove(*old) {
                fkeys.entry((*new).to_owned()).or_insert(v);
            }
            if let Some(v) = labels.remove(*old) {
                labels.entry((*new).to_owned()).or_insert(v);
            }
        }
        let (fkeys, labels) = (&fkeys, &labels);
        let known = |id: &str, warnings: &mut Vec<String>| {
            let action = Action::from_id(id);
            if action.is_none() && RETIRED.contains(&id) {
                return false;
            }
            if action.is_none() && !is_extra_command(id) {
                warnings.push(format!("unknown action {id:?} in [keys]"));
                return false;
            }
            true
        };
        // What each overridden command ends up with, applied in id order.
        let mut overridden: BTreeMap<String, Slots> = BTreeMap::new();
        for (id, list) in &keys {
            if !known(id, &mut warnings) {
                continue;
            }
            let entry = overridden.entry(id.clone()).or_default();
            for key in list {
                match key.parse::<KeyChord>() {
                    // A function key in the shortcut list: accepted when the
                    // command has no `fkeys` entry of its own.
                    Ok(chord) if is_function_key(&chord) => {
                        if !fkeys.contains_key(id) && entry.fkey.is_none() {
                            entry.fkey = Some(chord);
                        } else {
                            warnings.push(format!(
                                "{chord} is a function key: it belongs in [fkeys], not in [keys] ({id})"
                            ));
                        }
                    }
                    Ok(chord) => entry.shortcuts.push(chord),
                    Err(e) => warnings.push(format!("{e} for {id}")),
                }
            }
        }
        for (id, text) in fkeys {
            if !known(id, &mut warnings) {
                continue;
            }
            let entry = overridden.entry(id.clone()).or_default();
            entry.fkey = None;
            if text.trim().is_empty() {
                continue;
            }
            match text.parse::<KeyChord>() {
                Ok(chord) if is_function_key(&chord) => entry.fkey = Some(chord),
                Ok(chord) => warnings.push(format!(
                    "{chord} cannot be a function key (F1 to F10, with Shift or Ctrl) for {id}"
                )),
                Err(e) => warnings.push(format!("{e} for {id}")),
            }
        }
        // A shortcut override keeps the function key of the defaults, and the
        // other way round.
        for (id, entry) in &mut overridden {
            let base = slots.get(id).cloned().unwrap_or_default();
            if !keys.contains_key(id) {
                entry.shortcuts = base.shortcuts;
            }
            if !fkeys.contains_key(id) && entry.fkey.is_none() {
                entry.fkey = base.fkey;
            }
        }
        for (id, entry) in &overridden {
            let mine: Vec<KeyChord> = entry.shortcuts.iter().copied().chain(entry.fkey).collect();
            for (other, slot) in slots.iter_mut() {
                if other != id {
                    for chord in &mine {
                        slot.drop_chord(chord);
                    }
                }
            }
        }
        for (id, entry) in overridden {
            slots.insert(id, entry);
        }
        let mut by_action: BTreeMap<Action, Vec<KeyChord>> = BTreeMap::new();
        let mut by_extra: BTreeMap<String, Vec<KeyChord>> = BTreeMap::new();
        let mut fkey_by_id = BTreeMap::new();
        for (id, slot) in &slots {
            let chords: Vec<KeyChord> =
                slot.fkey.iter().copied().chain(slot.shortcuts.iter().copied()).collect();
            if let Some(chord) = slot.fkey {
                fkey_by_id.insert(id.clone(), chord);
            }
            match Action::from_id(id) {
                Some(action) => {
                    by_action.insert(action, chords);
                }
                None if !chords.is_empty() => {
                    by_extra.insert(id.clone(), chords);
                }
                None => {}
            }
        }
        let mut short_names = BTreeMap::new();
        for (id, name) in labels {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            if name.chars().count() > LABEL_MAX {
                warnings
                    .push(format!("the short name of {id} is longer than {LABEL_MAX} characters"));
                continue;
            }
            short_names.insert(id.clone(), name.to_owned());
        }
        let extra_by_chord = by_extra
            .iter()
            .flat_map(|(id, chords)| chords.iter().map(move |&chord| (chord, id.clone())))
            .collect();
        let mut by_chord = HashMap::new();
        for (&action, chords) in &by_action {
            for &chord in chords {
                if let Some(previous) = by_chord.insert(chord, action) {
                    warnings.push(format!(
                        "{chord} is bound to both {} and {}",
                        previous.id(),
                        action.id()
                    ));
                }
            }
        }
        (
            Keymap {
                by_chord,
                by_action,
                extra_by_chord,
                by_extra,
                fkey_by_id,
                labels: short_names,
            },
            warnings,
        )
    }

    /// The standard shortcuts of the command `id` (no function key).
    pub fn shortcuts_for_id(&self, id: &str) -> Vec<KeyChord> {
        self.chords_for_id(id).iter().copied().filter(|c| !is_function_key(c)).collect()
    }

    /// The function key of the command `id`, if it has one.
    pub fn fkey_for_id(&self, id: &str) -> Option<KeyChord> {
        self.fkey_by_id.get(id).copied()
    }

    /// The short name the bottom bar shows for `id`: the custom one, else the
    /// command's built-in name (extra commands have none: `None`).
    pub fn label_for_id(&self, id: &str) -> Option<String> {
        self.labels.get(id).cloned().or_else(|| Action::from_id(id).map(|a| a.short().to_owned()))
    }

    /// Whether `id` has a short name of its own (not the built-in one).
    pub fn has_custom_label(&self, id: &str) -> bool {
        self.labels.contains_key(id)
    }

    /// What the bottom bar says for the function key `chord`: the label of
    /// the command bound to it (empty names are never returned).
    pub fn bar_label(&self, chord: &KeyChord) -> Option<String> {
        let id = self.command_for(chord)?;
        if self.fkey_for_id(&id) != Some(*chord) {
            return None;
        }
        self.label_for_id(&id)
            .or_else(|| Some(id.rsplit('.').next().unwrap_or(&id).chars().take(8).collect()))
    }

    /// The plugin or script command bound to `chord`, if any.
    pub fn extra_for(&self, chord: &KeyChord) -> Option<&str> {
        self.extra_by_chord.get(chord).map(String::as_str)
    }

    /// The command (built-in action id, or plugin or script id) bound to
    /// `chord`, if any.
    pub fn command_for(&self, chord: &KeyChord) -> Option<String> {
        self.action_for(chord)
            .map(|a| a.id().to_owned())
            .or_else(|| self.extra_for(chord).map(str::to_owned))
    }

    /// The chords of the command `id` (an action id, or a plugin or script id).
    pub fn chords_for_id(&self, id: &str) -> &[KeyChord] {
        match Action::from_id(id) {
            Some(action) => self.chords_for(action),
            None => self.by_extra.get(id).map(Vec::as_slice).unwrap_or(&[]),
        }
    }

    /// The chords of command `id` as text (`Ctrl+K / F5`), or an empty string.
    pub fn keys_text(&self, id: &str) -> String {
        self.chords_for_id(id).iter().map(ToString::to_string).collect::<Vec<_>>().join(" / ")
    }

    /// The action bound to `chord`, if any.
    pub fn action_for(&self, chord: &KeyChord) -> Option<Action> {
        self.by_chord.get(chord).copied()
    }

    /// The chords bound to `action` (first one is shown in menus).
    pub fn chords_for(&self, action: Action) -> &[KeyChord] {
        self.by_action.get(&action).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The first chord of `action` as text, or an empty string.
    pub fn shortcut_text(&self, action: Action) -> String {
        self.chords_for(action).first().map(ToString::to_string).unwrap_or_default()
    }

    /// The main shortcuts for the help overlay, grouped by menu.
    pub fn help(&self) -> Vec<(Category, Vec<(String, &'static str)>)> {
        Category::ALL
            .iter()
            .map(|&category| {
                let rows = Action::ALL
                    .iter()
                    .filter(|a| a.category() == category && a.is_essential())
                    .filter_map(|&a| {
                        let keys =
                            self.chords_for(a).iter().map(ToString::to_string).collect::<Vec<_>>();
                        (!keys.is_empty()).then(|| (keys.join(" / "), a.label()))
                    })
                    .collect::<Vec<_>>();
                (category, rows)
            })
            .filter(|(_, rows)| !rows.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_short_name_for_the_bottom_bar() {
        let mut seen = std::collections::HashMap::new();
        for action in Action::ALL {
            let short = action.short();
            assert!(
                !short.is_empty() && short.chars().count() <= 8,
                "{:?}: {short:?}",
                action.id()
            );
            assert_eq!(short, short.trim(), "{}", action.id());
            assert!(!short.contains('…') && !short.contains(' '), "{}", action.id());
            if let Some(other) = seen.insert(short, action.id()) {
                panic!("{} and {} share the short name {short:?}", other, action.id());
            }
        }
    }

    #[test]
    fn the_narrow_bar_names_fit_five_columns_and_stay_apart_per_layer() {
        let mut layers: std::collections::HashMap<(String, String), &str> = Default::default();
        for action in Action::ALL {
            assert!(action.narrow().chars().count() <= 5, "{}: {}", action.id(), action.narrow());
            for key in action.default_keys() {
                let Some((modifiers, f)) = key.rsplit_once('+').or(Some(("", key))) else {
                    continue;
                };
                if !f.starts_with('F') || f.len() > 3 || !f[1..].chars().all(|c| c.is_ascii_digit())
                {
                    continue;
                }
                if let Some(other) =
                    layers.insert((modifiers.to_owned(), action.narrow().to_owned()), action.id())
                    && other != action.id()
                {
                    panic!(
                        "{other} and {} are both {:?} in layer {modifiers:?}",
                        action.id(),
                        action.narrow()
                    );
                }
            }
        }
    }

    #[test]
    fn the_old_connect_id_reaches_the_drive_list() {
        assert_eq!(Action::from_id("tools.connect"), Some(Action::Drives));
        assert_eq!(Action::from_id("nav.drives"), Some(Action::Drives));
    }

    fn chord(s: &str) -> KeyChord {
        s.parse().unwrap()
    }

    #[test]
    fn plugin_and_script_commands_can_have_keys() {
        let mut overrides = BTreeMap::new();
        overrides.insert("plugin.tidy.run".to_owned(), vec!["Ctrl+K".to_owned()]);
        overrides.insert("script.backup".to_owned(), vec!["Alt+B".to_owned()]);
        overrides.insert("bogus.thing".to_owned(), vec!["Alt+Q".to_owned()]);
        let (keymap, warnings) = Keymap::with_overrides(&overrides);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let ctrl_k: KeyChord = "Ctrl+K".parse().unwrap();
        assert_eq!(keymap.extra_for(&ctrl_k), Some("plugin.tidy.run"));
        assert_eq!(keymap.action_for(&ctrl_k), None, "taken from Checksum");
        assert_eq!(keymap.command_for(&"Alt+B".parse().unwrap()).unwrap(), "script.backup");
        assert_eq!(keymap.keys_text("plugin.tidy.run"), "Ctrl+K");
        assert_eq!(keymap.keys_text("files.copy"), "F5");
    }

    #[test]
    fn no_default_key_uses_alt_or_is_shared() {
        let mut seen = std::collections::HashMap::new();
        for action in Action::ALL {
            for text in action.default_keys() {
                let key: KeyChord = text.parse().unwrap();
                assert!(!key.alt, "{text} of {} uses Alt", action.id());
                if let Some(other) = seen.insert(key, action.id()) {
                    panic!("{text} is bound to {other} and {}", action.id());
                }
            }
        }
    }

    #[test]
    fn parse_and_display() {
        for text in [
            "F5",
            "Shift+F6",
            "Ctrl+Shift+D",
            "Alt+Enter",
            "Ctrl+,",
            "+",
            "Ctrl++",
            "Ctrl+PageDown",
            "Space",
        ] {
            assert_eq!(chord(text).to_string(), text, "{text}");
        }
        assert_eq!(chord("ctrl+a"), chord("Ctrl+A"));
        assert_eq!(chord("Ctrl+A").key, Key::Char('a'));
        assert!("Hyper+X".parse::<KeyChord>().is_err());
        assert!("F25".parse::<KeyChord>().is_err());
        assert!("Ctrl+".parse::<KeyChord>().is_err());
    }

    #[test]
    fn normalization() {
        assert_eq!(KeyChord::normalized(Key::Char('A'), false, false, false), chord("Shift+A"));
        assert_eq!(KeyChord::normalized(Key::Char('+'), false, false, true), chord("+"));
        assert_eq!(KeyChord::normalized(Key::Char(' '), false, false, false), chord("Space"));
    }

    #[test]
    fn ids_are_unique_and_defaults_are_conflict_free() {
        let mut ids: Vec<&str> = Action::ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Action::ALL.len());
        let (_, warnings) = Keymap::with_overrides(&BTreeMap::new());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn f1_shows_help() {
        let keymap = Keymap::default();
        assert_eq!(keymap.action_for(&chord("F1")), Some(Action::Help));
        let help = keymap.help();
        let all_rows: Vec<&str> =
            help.iter().flat_map(|(_, rows)| rows.iter().map(|(_, label)| *label)).collect();
        assert!(all_rows.contains(&"Copy"));
        assert!(all_rows.contains(&"Show main shortcuts"));
        assert!(!all_rows.contains(&"Invert selection"), "only essential actions");
    }

    #[test]
    fn overrides_replace_and_steal() {
        let overrides = BTreeMap::from([
            ("app.help".to_owned(), vec!["Ctrl+F1".to_owned()]),
            ("files.copy".to_owned(), vec!["F3".to_owned()]), // steals F3 from View
            ("no.such".to_owned(), vec!["F12".to_owned()]),
            ("files.move".to_owned(), vec!["Bogus+1".to_owned()]),
        ]);
        let (keymap, warnings) = Keymap::with_overrides(&overrides);
        assert_eq!(keymap.action_for(&chord("F1")), None);
        assert_eq!(keymap.action_for(&chord("Ctrl+F1")), Some(Action::Help));
        assert_eq!(keymap.action_for(&chord("F3")), Some(Action::Copy));
        assert!(keymap.chords_for(Action::View).is_empty());
        assert_eq!(keymap.shortcut_text(Action::Copy), "F3");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        // An invalid shortcut list leaves no shortcut, and the function key stays.
        assert_eq!(keymap.chords_for(Action::Move), [chord("F6")]);
    }

    #[test]
    fn a_command_has_at_most_one_function_key_by_default() {
        for action in Action::ALL {
            let functions =
                action.default_keys().iter().filter(|k| is_function_key_text(k)).count();
            assert!(functions <= 1, "{} has {functions} function keys", action.id());
            assert_eq!(
                action.default_shortcuts().len() + functions,
                action.default_keys().len(),
                "{}",
                action.id()
            );
        }
        assert_eq!(Action::Rename.default_fkey(), Some(chord("F2")));
        assert_eq!(Action::Properties.default_shortcuts(), [chord("Ctrl+E")]);
        assert_eq!(Action::Properties.default_fkey(), Some(chord("Shift+F4")));
    }

    #[test]
    fn function_keys_are_f1_to_f10_without_alt() {
        for yes in ["F1", "F10", "Shift+F5", "Ctrl+F9"] {
            assert!(is_function_key(&chord(yes)), "{yes}");
        }
        // Ctrl+Shift+F5 is no slot of the bar: it counts as a shortcut chord.
        for no in ["F11", "Ctrl+Shift+F5", "Alt+F4", "Ctrl+E", "Delete"] {
            assert!(!is_function_key(&chord(no)), "{no}");
        }
    }

    fn built(
        keys: &[(&str, &[&str])],
        fkeys: &[(&str, &str)],
        labels: &[(&str, &str)],
    ) -> (Keymap, Vec<String>) {
        Keymap::build(
            &keys
                .iter()
                .map(|(id, list)| {
                    ((*id).to_owned(), list.iter().map(|k| (*k).to_owned()).collect())
                })
                .collect(),
            &fkeys.iter().map(|(id, k)| ((*id).to_owned(), (*k).to_owned())).collect(),
            &labels.iter().map(|(id, k)| ((*id).to_owned(), (*k).to_owned())).collect(),
        )
    }

    fn with_defaults(
        keys: &[(&str, &[&str])],
        fkeys: &[(&str, &str)],
        labels: &[(&str, &str)],
        defaults: &[PluginKeyDefault],
    ) -> (Keymap, Vec<String>) {
        Keymap::build_with(
            &keys
                .iter()
                .map(|(id, list)| {
                    ((*id).to_owned(), list.iter().map(|k| (*k).to_owned()).collect())
                })
                .collect(),
            &fkeys.iter().map(|(id, k)| ((*id).to_owned(), (*k).to_owned())).collect(),
            &labels.iter().map(|(id, k)| ((*id).to_owned(), (*k).to_owned())).collect(),
            defaults,
        )
    }

    fn plugin_default(id: &str, shortcut: &str, fkey: &str, label: &str) -> PluginKeyDefault {
        let some = |t: &str| (!t.is_empty()).then(|| t.to_owned());
        PluginKeyDefault {
            id: id.to_owned(),
            shortcut: some(shortcut),
            fkey: some(fkey),
            label: some(label),
        }
    }

    #[test]
    fn a_plugin_declares_defaults_and_the_user_wins() {
        let tidy = plugin_default("plugin.tidy.run", "Ctrl+Shift+I", "Shift+F9", "Tidy");
        let (keymap, warnings) = with_defaults(&[], &[], &[], std::slice::from_ref(&tidy));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(keymap.fkey_for_id("plugin.tidy.run"), Some(chord("Shift+F9")));
        assert_eq!(keymap.shortcuts_for_id("plugin.tidy.run"), [chord("Ctrl+Shift+I")]);
        assert_eq!(keymap.extra_for(&chord("Shift+F9")), Some("plugin.tidy.run"));
        assert_eq!(keymap.bar_label(&chord("Shift+F9")).as_deref(), Some("Tidy"));
        // The user's own function key, shortcut and name replace the defaults.
        let (keymap, _) = with_defaults(
            &[("plugin.tidy.run", &["Ctrl+J"])],
            &[("plugin.tidy.run", "Shift+F10")],
            &[("plugin.tidy.run", "Clean")],
            std::slice::from_ref(&tidy),
        );
        assert_eq!(keymap.fkey_for_id("plugin.tidy.run"), Some(chord("Shift+F10")));
        assert_eq!(keymap.shortcuts_for_id("plugin.tidy.run"), [chord("Ctrl+J")]);
        assert_eq!(keymap.extra_for(&chord("Shift+F9")), None);
        assert_eq!(keymap.label_for_id("plugin.tidy.run").as_deref(), Some("Clean"));
        // An empty text removes the default function key only.
        let (keymap, _) =
            with_defaults(&[], &[("plugin.tidy.run", "")], &[], std::slice::from_ref(&tidy));
        assert_eq!(keymap.fkey_for_id("plugin.tidy.run"), None);
        assert_eq!(keymap.shortcuts_for_id("plugin.tidy.run"), [chord("Ctrl+Shift+I")]);
    }

    #[test]
    fn a_plugin_default_never_takes_a_chord_that_is_in_use() {
        let greedy = plugin_default("plugin.greedy.go", "Ctrl+D", "F5", "Go");
        let (keymap, warnings) = with_defaults(&[], &[], &[], &[greedy]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert_eq!(keymap.action_for(&chord("F5")), Some(Action::Copy));
        assert_eq!(keymap.action_for(&chord("Ctrl+D")), Some(Action::Bookmarks));
        assert_eq!(keymap.fkey_for_id("plugin.greedy.go"), None);
        // The first plugin keeps a chord two of them want; wrong kinds are told.
        let first = plugin_default("plugin.a.x", "", "Shift+F9", "A");
        let second = plugin_default("plugin.b.y", "F6", "Shift+F9", "B");
        let (keymap, warnings) = with_defaults(&[], &[], &[], &[first, second]);
        assert_eq!(keymap.extra_for(&chord("Shift+F9")), Some("plugin.a.x"));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn shortcuts_and_the_function_key_are_independent() {
        // Copy gets a shortcut: its F5 stays.
        let (keymap, warnings) = built(&[("files.copy", &["Ctrl+C"])], &[], &[]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(keymap.fkey_for_id("files.copy"), Some(chord("F5")));
        assert_eq!(keymap.shortcuts_for_id("files.copy"), [chord("Ctrl+C")]);
        // Copy gets another function key: its shortcuts stay.
        let (keymap, _) = built(&[], &[("files.properties", "Ctrl+F4")], &[]);
        assert_eq!(keymap.fkey_for_id("files.properties"), Some(chord("Ctrl+F4")));
        assert_eq!(keymap.shortcuts_for_id("files.properties"), [chord("Ctrl+E")]);
        assert_eq!(keymap.action_for(&chord("Shift+F4")), None, "the old key is free");
        // An empty text removes the function key only.
        let (keymap, _) = built(&[], &[("files.copy", "")], &[]);
        assert_eq!(keymap.fkey_for_id("files.copy"), None);
        assert_eq!(keymap.action_for(&chord("F5")), None);
    }

    #[test]
    fn a_function_key_taken_by_another_command_is_removed_from_its_owner() {
        let (keymap, warnings) = built(&[], &[("files.copy", "F8")], &[]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(keymap.action_for(&chord("F8")), Some(Action::Copy));
        assert_eq!(keymap.fkey_for_id("files.delete"), None);
        assert_eq!(
            keymap.shortcuts_for_id("files.delete"),
            [chord("Delete")],
            "its shortcut stays"
        );
        assert_eq!(keymap.bar_label(&chord("F8")).as_deref(), Some("Copy"));
        assert_eq!(keymap.bar_label(&chord("F5")), None);
    }

    #[test]
    fn a_shortcut_taken_by_another_command_is_removed_from_its_owner() {
        let (keymap, _) = built(&[("files.copy", &["Ctrl+E"])], &[], &[]);
        assert_eq!(keymap.action_for(&chord("Ctrl+E")), Some(Action::Copy));
        assert_eq!(keymap.shortcuts_for_id("files.properties"), Vec::<KeyChord>::new());
        assert_eq!(keymap.fkey_for_id("files.properties"), Some(chord("Shift+F4")));
    }

    #[test]
    fn bad_function_keys_and_labels_are_warned_about_and_ignored() {
        let (keymap, warnings) = built(
            &[],
            &[("files.copy", "Alt+F5"), ("files.move", "Ctrl+Shift+F6"), ("files.view", "F11")],
            &[("files.copy", "A name that is too long"), ("files.move", "  ")],
        );
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert_eq!(keymap.fkey_for_id("files.copy"), None);
        assert_eq!(keymap.label_for_id("files.copy").as_deref(), Some("Copy"));
        assert_eq!(keymap.label_for_id("files.move").as_deref(), Some("Move"));
    }

    #[test]
    fn a_short_name_changes_the_bar_label_and_the_default_comes_back_without_it() {
        let (keymap, _) = built(&[], &[], &[("files.copy", "Duplicate")]);
        assert_eq!(keymap.bar_label(&chord("F5")).as_deref(), Some("Duplicate"));
        assert!(keymap.has_custom_label("files.copy"));
        let (keymap, _) = built(&[], &[], &[]);
        assert_eq!(keymap.bar_label(&chord("F5")).as_deref(), Some("Copy"));
        assert!(!keymap.has_custom_label("files.copy"));
    }

    #[test]
    fn plugin_commands_can_have_a_function_key_and_a_name() {
        let (keymap, warnings) = built(
            &[("plugin.tidy.run", &["Ctrl+K"])],
            &[("plugin.tidy.run", "Ctrl+F10")],
            &[("plugin.tidy.run", "Tidy")],
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(keymap.extra_for(&chord("Ctrl+F10")), Some("plugin.tidy.run"));
        assert_eq!(keymap.bar_label(&chord("Ctrl+F10")).as_deref(), Some("Tidy"));
        assert_eq!(keymap.shortcuts_for_id("plugin.tidy.run"), [chord("Ctrl+K")]);
    }

    #[test]
    fn keys_of_a_command_that_moved_to_a_plugin_follow_it() {
        let (keymap, warnings) = built(
            &[("tools.similar-images", &["Ctrl+Alt+S"])],
            &[("tools.similar-images", "Ctrl+F10")],
            &[("tools.similar-images", "Pics")],
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(keymap.extra_for(&chord("Ctrl+Alt+S")), Some("plugin.image-duplicates.find"));
        assert_eq!(keymap.extra_for(&chord("Ctrl+F10")), Some("plugin.image-duplicates.find"));
        assert_eq!(keymap.bar_label(&chord("Ctrl+F10")).as_deref(), Some("Pics"));
        assert_eq!(current_id("tools.similar-images"), "plugin.image-duplicates.find");
        assert_eq!(current_id("files.copy"), "files.copy");
        assert!(Action::from_id("tools.similar-images").is_none());
    }

    #[test]
    fn a_function_key_in_the_shortcut_list_is_read_as_the_function_key() {
        // A hand-written settings file of the first model.
        let (keymap, warnings) = built(&[("files.copy", &["Shift+F5", "Ctrl+C"])], &[], &[]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(keymap.fkey_for_id("files.copy"), Some(chord("Shift+F5")));
        assert_eq!(keymap.shortcuts_for_id("files.copy"), [chord("Ctrl+C")]);
        // With an [fkeys] entry the list may not hold one.
        let (_, warnings) = built(&[("files.copy", &["F4"])], &[("files.copy", "F5")], &[]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
