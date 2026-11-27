//! Commands, their shortcuts, and the help overlay.
//!
//! NemesisCommander has no toolbar or icons: every command is an [`Action`]
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
            Category::App => "NemesisCommander",
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
    Properties    = "files.properties",     Files, false, "Properties and permissions", "Props", ["Shift+F4", "Ctrl+E"];
    Undo          = "files.undo",           Files, false, "Undo rename, new folder, move", "Undo", ["Ctrl+Z"];
    Attributes    = "files.attributes",     Files, false, "Change dates and permissions…", "Attribs", [];
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
    // Tools
    Search        = "tools.search",         Tools, true,  "Search…",                  "Search", ["Ctrl+F2"];
    Compare       = "tools.compare",        Tools, false, "Compare files / folders",  "Compare", ["Ctrl+F3"];
    CompareHash   = "tools.compare-hash",   Tools, false, "Compare folders by content…", "CmpHash", [];
    Sync          = "tools.sync",           Tools, false, "Synchronize folders…",     "Sync", ["Ctrl+F4"];
    GenerateFiles = "tools.generate-files", Tools, false, "Generate random text files…", "Generate", [];
    Duplicates    = "tools.duplicates",     Tools, false, "Find duplicate files",     "Dupes", ["Ctrl+F5"];
    DiskUsage     = "tools.disk-usage",     Tools, false, "Disk usage of this folder", "DiskUse", ["Ctrl+F7"];
    Disks         = "tools.disks",          Tools, false, "Disks and partitions…",    "Disks", ["Ctrl+F8"];
    SimilarImages = "tools.similar-images", Tools, false, "Find similar pictures",    "Similar", [];
    SavedSearches = "tools.saved-searches", Tools, false, "Saved searches…",          "Searches", [];
    SaveSearch    = "tools.save-search",    Tools, false, "Save the last search…",    "SaveSrch", [];
    Jobs          = "tools.jobs",           Tools, true,  "Transfer queue",           "Queue", ["Ctrl+J"];
    RunScript     = "tools.run-script",     Tools, false, "Plugins and scripts…",     "Plugins", ["Ctrl+F9", "Ctrl+X"];
    AiScript      = "tools.ai-script",      Tools, false, "Write a script with AI…",  "AIScript", [];
    InstallServe  = "tools.install-serve",  Tools, false, "Install nmc serve on a server…", "Install", [];
    // App
    Help          = "app.help",             App, true,  "Show main shortcuts",        "Help", ["F1"];
    Menu          = "app.commands",         App, true,  "Commands",                   "Commands", ["F9"];
    Settings      = "app.settings",         App, true,  "Settings…",                  "Settings", ["Ctrl+,", "Ctrl+F1"];
    ExportConfig  = "app.export-config",    App, false, "Export settings to file…",   "ExpConf", [];
    ImportConfig  = "app.import-config",    App, false, "Import settings from file…", "ImpConf", [];
    Choose        = "app.choose",           App, false, "Choose and exit (nmc pick)", "Choose", ["Ctrl+F10"];
    About         = "app.about",            App, false, "About NemesisCommander", "About", [];
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
                "Shows size, dates, owner and permissions of the item under the cursor, and lets you change the permissions."
            }
            Action::Undo => {
                "Undoes the last rename, new folder or move, up to 50 steps in this session."
            }
            Action::Attributes => {
                "Changes dates and permissions of the selected items: a date, a time, `now`, `mode=644`, `-r` for folders inside."
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
                "Opens a terminal in the folder of the active panel (the command is set in Settings). In the terminal interface it quits and leaves the shell in the folder, once `nmc shell-init` is set up; on a server it opens an SSH session in that folder."
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
            Action::Disks => "Lists the disks and partitions with size, type, file system and use.",
            Action::SimilarImages => {
                "Finds pictures that look alike, even when resized or saved in another format."
            }
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
                "Installs `nmc serve` on a server over SSH, with certificates, so that you can connect to it securely and copy straight between servers."
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
                "Only in nmc pick: gives the marked items, the item under the cursor or, when asked for a folder, the current folder to the program that started nmc pick."
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

/// The effective key bindings: defaults plus user overrides.
#[derive(Debug, Clone)]
pub struct Keymap {
    by_chord: HashMap<KeyChord, Action>,
    by_action: BTreeMap<Action, Vec<KeyChord>>,
    /// Plugin and script commands, by id.
    extra_by_chord: HashMap<KeyChord, String>,
    by_extra: BTreeMap<String, Vec<KeyChord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::with_overrides(&BTreeMap::new()).0
    }
}

/// Commands that were removed: a settings file that still binds a key to
/// one is not a mistake, so no warning is made.
const RETIRED: &[&str] = &["view.folder-sizes"];

impl Keymap {
    /// Builds the keymap from defaults and `overrides` (action id → chords).
    /// Overriding an action replaces all its default chords; a chord given to
    /// one action is taken away from any other. Problems (unknown actions,
    /// bad chords) are returned as warnings and otherwise ignored.
    pub fn with_overrides(overrides: &BTreeMap<String, Vec<String>>) -> (Keymap, Vec<String>) {
        let mut warnings = Vec::new();
        let mut by_action: BTreeMap<Action, Vec<KeyChord>> = BTreeMap::new();
        for &action in Action::ALL {
            let chords = action.default_keys().iter().map(|k| {
                k.parse::<KeyChord>()
                    .unwrap_or_else(|_| panic!("built-in key {k:?} for {} must parse", action.id()))
            });
            by_action.insert(action, chords.collect());
        }
        let mut by_extra: BTreeMap<String, Vec<KeyChord>> = BTreeMap::new();
        for (id, keys) in overrides {
            let action = Action::from_id(id);
            if action.is_none() && RETIRED.contains(&id.as_str()) {
                continue;
            }
            if action.is_none() && !is_extra_command(id) {
                warnings.push(format!("unknown action {id:?} in [keys]"));
                continue;
            }
            let mut chords = Vec::new();
            for key in keys {
                match key.parse::<KeyChord>() {
                    Ok(chord) => chords.push(chord),
                    Err(e) => warnings.push(format!("{e} for {id}")),
                }
            }
            for other in by_action.values_mut().chain(by_extra.values_mut()) {
                other.retain(|c| !chords.contains(c));
            }
            match action {
                Some(action) => {
                    by_action.insert(action, chords);
                }
                None => {
                    by_extra.insert(id.clone(), chords);
                }
            }
        }
        by_extra.retain(|_, chords| !chords.is_empty());
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
        (Keymap { by_chord, by_action, extra_by_chord, by_extra }, warnings)
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
        assert!(keymap.chords_for(Action::Move).is_empty(), "invalid override unbinds");
    }
}
