//! Commands, their shortcuts, and the help overlay.
//!
//! SuperCommander has no toolbar or icons: every command is an [`Action`]
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
            Category::App => "SuperCommander",
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
    ($( $variant:ident = $id:literal, $category:ident, $essential:literal, $label:literal, [$($key:literal),*]; )*) => {
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
    View          = "files.view",           Files, true,  "View file",                ["F3"];
    Edit          = "files.edit",           Files, true,  "Edit file",                ["F4"];
    Copy          = "files.copy",           Files, true,  "Copy",                     ["F5"];
    Move          = "files.move",           Files, true,  "Move",                     ["F6"];
    Rename        = "files.rename",         Files, true,  "Rename",                   ["Shift+F6", "F2"];
    MultiRename   = "files.multi-rename",   Files, false, "Multi-rename…",            ["Ctrl+M"];
    MakeDir       = "files.mkdir",          Files, true,  "New folder",               ["F7"];
    Delete        = "files.delete",         Files, true,  "Delete (to trash)",        ["F8", "Delete"];
    DeleteForever = "files.delete-forever", Files, false, "Delete permanently",       ["Shift+F8", "Shift+Delete"];
    Pack          = "files.pack",           Files, true,  "Pack into archive…",       ["Alt+F5"];
    Unpack        = "files.unpack",         Files, false, "Unpack archive…",          ["Alt+F9"];
    Properties    = "files.properties",     Files, false, "Properties and permissions", ["Alt+Enter"];
    Checksum      = "files.checksum",       Files, false, "Checksum",                 ["Ctrl+K"];
    // Select
    ToggleSelect  = "select.toggle",        Select, true, "Select / unselect",        ["Insert", "Space"];
    SelectMask    = "select.mask",          Select, true, "Select by mask…",          ["+"];
    UnselectMask  = "select.unmask",        Select, false, "Unselect by mask…",       ["-"];
    InvertSelect  = "select.invert",        Select, false, "Invert selection",        ["*"];
    SelectAll     = "select.all",           Select, false, "Select all",              ["Ctrl+A"];
    // Navigate
    Open          = "nav.open",             Navigate, false, "Open / enter",          ["Enter"];
    Parent        = "nav.parent",           Navigate, false, "Parent folder",         ["Backspace"];
    SwitchPanel   = "nav.switch-panel",     Navigate, true,  "Other panel",           ["Tab"];
    GoTo          = "nav.goto",             Navigate, true,  "Go to location…",       ["Ctrl+L"];
    Back          = "nav.back",             Navigate, false, "Back",                  ["Alt+Left"];
    Forward       = "nav.forward",          Navigate, false, "Forward",               ["Alt+Right"];
    Bookmarks     = "nav.bookmarks",        Navigate, true,  "Bookmarks…",            ["Ctrl+D"];
    AddBookmark   = "nav.add-bookmark",     Navigate, false, "Bookmark this folder",  ["Ctrl+Shift+D"];
    NewTab        = "nav.new-tab",          Navigate, false, "New tab",               ["Ctrl+T"];
    CloseTab      = "nav.close-tab",        Navigate, false, "Close tab",             ["Ctrl+W"];
    NextTab       = "nav.next-tab",         Navigate, false, "Next tab",              ["Ctrl+Tab", "Ctrl+PageDown"];
    PrevTab       = "nav.prev-tab",         Navigate, false, "Previous tab",          ["Ctrl+Shift+Tab", "Ctrl+PageUp"];
    SamePath      = "nav.same-path",        Navigate, false, "Open this folder in other panel", ["Alt+O"];
    SwapPanels    = "nav.swap-panels",      Navigate, false, "Swap panels",           ["Ctrl+U"];
    // View
    Refresh       = "view.refresh",         View, false, "Refresh",                   ["Ctrl+R"];
    ToggleHidden  = "view.hidden",          View, false, "Show hidden files",         ["Ctrl+H"];
    QuickFilter   = "view.filter",          View, true,  "Quick filter…",             ["Ctrl+S"];
    SortName      = "view.sort-name",       View, false, "Sort by name",              ["Ctrl+F3"];
    SortExtension = "view.sort-ext",        View, false, "Sort by extension",         ["Ctrl+F4"];
    SortSize      = "view.sort-size",       View, false, "Sort by size",              ["Ctrl+F6"];
    SortModified  = "view.sort-date",       View, false, "Sort by date",              ["Ctrl+F5"];
    QuickView     = "view.quick-view",      View, false, "Quick view in other panel", ["Ctrl+Q"];
    // Tools
    Search        = "tools.search",         Tools, true,  "Search…",                  ["Alt+F7"];
    Compare       = "tools.compare",        Tools, false, "Compare files / folders",  ["Alt+C"];
    Jobs          = "tools.jobs",           Tools, true,  "Transfer queue",           ["Ctrl+J"];
    Connect       = "tools.connect",        Tools, true,  "Connect…",                 ["Ctrl+N"];
    RunScript     = "tools.run-script",     Tools, false, "Plugins and scripts…",     ["Alt+X"];
    // App
    Help          = "app.help",             App, true,  "Show main shortcuts",        ["F1"];
    Menu          = "app.menu",             App, true,  "Menu",                       ["F9"];
    Settings      = "app.settings",         App, true,  "Settings…",                  ["Ctrl+,"];
    ExportConfig  = "app.export-config",    App, false, "Export settings to file…",   [];
    ImportConfig  = "app.import-config",    App, false, "Import settings from file…", [];
    Quit          = "app.quit",             App, true,  "Quit",                       ["F10"];
}

impl Action {
    /// Finds an action by its [`id`](Self::id).
    pub fn from_id(id: &str) -> Option<Action> {
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

/// The effective key bindings: defaults plus user overrides.
#[derive(Debug, Clone)]
pub struct Keymap {
    by_chord: HashMap<KeyChord, Action>,
    by_action: BTreeMap<Action, Vec<KeyChord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::with_overrides(&BTreeMap::new()).0
    }
}

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
        for (id, keys) in overrides {
            let Some(action) = Action::from_id(id) else {
                warnings.push(format!("unknown action {id:?} in [keys]"));
                continue;
            };
            let mut chords = Vec::new();
            for key in keys {
                match key.parse::<KeyChord>() {
                    Ok(chord) => chords.push(chord),
                    Err(e) => warnings.push(format!("{e} for {id}")),
                }
            }
            for other in by_action.values_mut() {
                other.retain(|c| !chords.contains(c));
            }
            by_action.insert(action, chords);
        }
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
        (Keymap { by_chord, by_action }, warnings)
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

    fn chord(s: &str) -> KeyChord {
        s.parse().unwrap()
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
