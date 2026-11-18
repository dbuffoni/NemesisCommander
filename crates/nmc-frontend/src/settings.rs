//! The settings screen (Ctrl+,): every option and every shortcut, changed
//! in place and saved at once. Both interfaces draw it from
//! [`SettingsScreen::rows`]; what keys do is decided in `app.rs`.
//!
//! The functions here only change a [`Config`]; the application saves it
//! and applies it (key map, transfer manager) afterwards.

use nmc_config::{Action, Config, KeyChord, Keymap};
use nmc_index::SortKey;
use nmc_ops::{ConflictPolicy, VerifyMode};

/// One option of the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Ask before deleting.
    ConfirmDelete,
    /// Delete to the recycle bin.
    UseTrash,
    /// Editor command for F4.
    Editor,
    /// Show hidden files.
    ShowHidden,
    /// Initial sort column.
    Sort,
    /// Initial sort direction.
    Descending,
    /// Sizes as `1.2 MiB`.
    HumanSizes,
    /// Left and Right move between folders.
    ArrowNavigation,
    /// Git marks and branch.
    GitStatus,
    /// Jobs running at once.
    ParallelJobs,
    /// Files copied at once within a job.
    ParallelFiles,
    /// Check after copying.
    Verify,
    /// What to do when a file exists.
    Conflict,
    /// Bandwidth limit in KiB/s.
    BandwidthLimit,
    /// Retries for network errors.
    Retries,
    /// Wrap long lines in the viewer.
    Wrap,
    /// Spaces per tab in the viewer.
    TabWidth,
    /// Files above this size open as hex.
    HexAbove,
    /// Icons for the kinds of files.
    Icons,
    /// Font family of the window.
    FontFamily,
    /// Font size of the window.
    FontSize,
    /// Fixed-width font family of the window.
    FixedFontFamily,
}

/// How a [`Field`] is changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// On or off: Enter, Space and the arrows switch it.
    Toggle,
    /// One of a few values: Enter and the arrows go through them.
    Choice,
    /// A whole number between the two limits: arrows step, Enter types.
    Number(u64, u64),
    /// Free text: Enter types.
    Text,
    /// A font family: Enter lists the installed fonts.
    Font,
}

const SORTS: [(SortKey, &str); 4] = [
    (SortKey::Name, "name"),
    (SortKey::Extension, "extension"),
    (SortKey::Size, "size"),
    (SortKey::Modified, "date"),
];
const VERIFY: [(VerifyMode, &str); 4] = [
    (VerifyMode::None, "no check"),
    (VerifyMode::Size, "same size"),
    (VerifyMode::Fast, "same content (XXH3)"),
    (VerifyMode::Full, "same content (BLAKE3)"),
];
const CONFLICTS: [(ConflictPolicy, &str); 7] = [
    (ConflictPolicy::Ask, "ask"),
    (ConflictPolicy::Overwrite, "overwrite"),
    (ConflictPolicy::OverwriteIfNewer, "overwrite if newer"),
    (ConflictPolicy::OverwriteIfDifferentSize, "overwrite if the size differs"),
    (ConflictPolicy::Skip, "skip"),
    (ConflictPolicy::KeepBoth, "keep both"),
    (ConflictPolicy::Fail, "stop with an error"),
];

impl Field {
    /// Every option, in screen order.
    pub const ALL: [Field; 22] = [
        Field::ConfirmDelete,
        Field::UseTrash,
        Field::Editor,
        Field::ShowHidden,
        Field::Sort,
        Field::Descending,
        Field::HumanSizes,
        Field::ArrowNavigation,
        Field::GitStatus,
        Field::Icons,
        Field::ParallelJobs,
        Field::ParallelFiles,
        Field::Verify,
        Field::Conflict,
        Field::BandwidthLimit,
        Field::Retries,
        Field::Wrap,
        Field::TabWidth,
        Field::HexAbove,
        Field::FontFamily,
        Field::FontSize,
        Field::FixedFontFamily,
    ];

    /// The section it is shown in.
    pub fn section(self) -> &'static str {
        use Field::*;
        match self {
            ConfirmDelete | UseTrash | Editor => "General",
            ShowHidden | Sort | Descending | HumanSizes | ArrowNavigation | GitStatus | Icons => {
                "Panels"
            }
            ParallelJobs | ParallelFiles | Verify | Conflict | BandwidthLimit | Retries => {
                "Transfers"
            }
            Wrap | TabWidth | HexAbove => "Viewer",
            FontFamily | FontSize | FixedFontFamily => "Window fonts (not the terminal)",
        }
    }

    /// What it is called on screen.
    pub fn label(self) -> &'static str {
        use Field::*;
        match self {
            ConfirmDelete => "Ask before deleting",
            UseTrash => "Delete to the recycle bin",
            Editor => "Editor (empty: $EDITOR or the system's)",
            ShowHidden => "Show hidden files",
            Sort => "Sort by",
            Descending => "Largest, latest or last first",
            HumanSizes => "Sizes as KiB, MiB, GiB",
            ArrowNavigation => "← goes up, → enters a folder",
            GitStatus => "Git marks and branch in repositories",
            Icons => "Icons for the kinds of files",
            ParallelJobs => "Jobs at the same time (1: one after the other)",
            ParallelFiles => "Files at the same time in a job",
            Verify => "Check after copying",
            Conflict => "When a file exists",
            BandwidthLimit => "Bandwidth limit in KiB/s (0: none)",
            Retries => "Retries after network errors",
            Wrap => "Wrap long lines",
            TabWidth => "Spaces per tab",
            HexAbove => "Open as hex above (MiB)",
            FontFamily => "Font",
            FontSize => "Font size in points (0: the system's)",
            FixedFontFamily => "Fixed-width font",
        }
    }

    /// How it is changed.
    pub fn kind(self) -> FieldKind {
        use Field::*;
        match self {
            ConfirmDelete | UseTrash | ShowHidden | Descending | HumanSizes | ArrowNavigation
            | GitStatus | Icons | Wrap => FieldKind::Toggle,
            Sort | Verify | Conflict => FieldKind::Choice,
            Editor => FieldKind::Text,
            FontFamily | FixedFontFamily => FieldKind::Font,
            FontSize => FieldKind::Number(0, 48),
            ParallelJobs => FieldKind::Number(1, 64),
            ParallelFiles => FieldKind::Number(1, 64),
            BandwidthLimit => FieldKind::Number(0, 10_000_000),
            Retries => FieldKind::Number(0, 100),
            TabWidth => FieldKind::Number(1, 16),
            HexAbove => FieldKind::Number(1, 1_000_000),
        }
    }

    /// Its value in `config`, as shown.
    pub fn value(self, config: &Config) -> String {
        use Field::*;
        let on = |b: bool| if b { "yes" } else { "no" }.to_owned();
        let (g, p, t, v) = (&config.general, &config.panels, &config.transfers, &config.viewer);
        match self {
            ConfirmDelete => on(g.confirm_delete),
            UseTrash => on(g.use_trash),
            Editor => g.editor.clone(),
            ShowHidden => on(p.show_hidden),
            Sort => name_of(&SORTS, p.sort),
            Descending => on(p.descending),
            HumanSizes => on(p.human_sizes),
            ArrowNavigation => on(p.arrow_navigation),
            GitStatus => on(p.git_status),
            Icons => on(p.icons),
            ParallelJobs => t.parallel_jobs.to_string(),
            ParallelFiles => t.parallel_files.to_string(),
            Verify => name_of(&VERIFY, t.verify),
            Conflict => name_of(&CONFLICTS, t.conflict),
            BandwidthLimit => t.bandwidth_limit_kib.to_string(),
            Retries => t.retries.to_string(),
            Wrap => on(v.wrap),
            TabWidth => v.tab_width.to_string(),
            HexAbove => v.hex_above_mib.to_string(),
            FontFamily => config.appearance.font_family.clone(),
            FontSize => config.appearance.font_size.to_string(),
            FixedFontFamily => config.appearance.fixed_font_family.clone(),
        }
    }

    /// Switches a toggle, steps a choice or a number by `delta`.
    pub fn step(self, config: &mut Config, delta: i64) {
        use Field::*;
        let (g, p, t, v) =
            (&mut config.general, &mut config.panels, &mut config.transfers, &mut config.viewer);
        match self {
            ConfirmDelete => g.confirm_delete ^= true,
            UseTrash => g.use_trash ^= true,
            ShowHidden => p.show_hidden ^= true,
            Descending => p.descending ^= true,
            HumanSizes => p.human_sizes ^= true,
            ArrowNavigation => p.arrow_navigation ^= true,
            GitStatus => p.git_status ^= true,
            Icons => p.icons ^= true,
            Wrap => v.wrap ^= true,
            Sort => p.sort = cycle(&SORTS, p.sort, delta),
            Verify => t.verify = cycle(&VERIFY, t.verify, delta),
            Conflict => t.conflict = cycle(&CONFLICTS, t.conflict, delta),
            Editor | FontFamily | FixedFontFamily => {}
            _ => {
                let current: u64 = self.value(config).parse().unwrap_or(0);
                let next = current.saturating_add_signed(delta);
                let _ = self.set(config, &next.to_string());
            }
        }
    }

    /// Sets a number or text from what was typed.
    pub fn set(self, config: &mut Config, text: &str) -> Result<(), String> {
        let text = text.trim();
        if let FieldKind::Number(min, max) = self.kind() {
            let number: u64 = text
                .parse()
                .map_err(|_| format!("{}: not a whole number: {text}", self.label()))?;
            let n = number.clamp(min, max);
            let t = &mut config.transfers;
            match self {
                Field::ParallelJobs => t.parallel_jobs = n as usize,
                Field::ParallelFiles => t.parallel_files = n as usize,
                Field::BandwidthLimit => t.bandwidth_limit_kib = n,
                Field::Retries => t.retries = n as u32,
                Field::TabWidth => config.viewer.tab_width = n as usize,
                Field::HexAbove => config.viewer.hex_above_mib = n,
                Field::FontSize => config.appearance.font_size = n as u32,
                _ => {}
            }
            return Ok(());
        }
        match self {
            Field::Editor => config.general.editor = text.to_owned(),
            Field::FontFamily => config.appearance.font_family = text.to_owned(),
            Field::FixedFontFamily => config.appearance.fixed_font_family = text.to_owned(),
            _ => {}
        }
        Ok(())
    }

    /// Puts the default value back.
    pub fn reset(self, config: &mut Config) {
        let defaults = Config::default();
        let value = self.value(&defaults);
        match self.kind() {
            FieldKind::Number(..) | FieldKind::Text | FieldKind::Font => {
                let _ = self.set(config, &value);
            }
            _ => {
                // Step until the shown value matches the default.
                for _ in 0..8 {
                    if self.value(config) == value {
                        break;
                    }
                    self.step(config, 1);
                }
            }
        }
    }
}

fn name_of<T: PartialEq + Copy>(table: &[(T, &str)], value: T) -> String {
    table.iter().find(|(v, _)| *v == value).map_or("?", |(_, name)| name).to_owned()
}

fn cycle<T: PartialEq + Copy>(table: &[(T, &str)], value: T, delta: i64) -> T {
    let index = table.iter().position(|(v, _)| *v == value).unwrap_or(0) as i64;
    let len = table.len() as i64;
    let step = if delta < 0 { -1 } else { 1 };
    table[(index + step).rem_euclid(len) as usize].0
}

/// A command at the end of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Send these settings to another machine.
    SendTo,
    /// Replace these settings with another machine's.
    FetchFrom,
    /// Write all settings to a file.
    Export,
    /// Read settings from a file.
    Import,
    /// Open the settings file in the editor.
    EditFile,
}

impl Command {
    /// Every command, in screen order.
    pub const ALL: [Command; 5] =
        [Command::SendTo, Command::FetchFrom, Command::Export, Command::Import, Command::EditFile];

    /// What it is called on screen.
    pub fn label(self) -> &'static str {
        match self {
            Command::SendTo => "Send these settings to another machine…",
            Command::FetchFrom => "Get the settings of another machine…",
            Command::Export => "Export settings to a file…",
            Command::Import => "Import settings from a file…",
            Command::EditFile => "Edit the settings file",
        }
    }
}

/// A row of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A section title (the cursor skips it).
    Header(&'static str),
    /// An option.
    Option(Field),
    /// A command.
    Command(Command),
}

/// The state of the settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsScreen {
    /// Every row, headers included.
    pub rows: Vec<Row>,
    /// The highlighted row (never a header).
    pub cursor: usize,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsScreen {
    /// The screen with the cursor on the first option.
    pub fn new() -> Self {
        let mut rows = Vec::new();
        let mut section = "";
        for field in Field::ALL {
            if field.section() != section {
                section = field.section();
                rows.push(Row::Header(section));
            }
            rows.push(Row::Option(field));
        }
        rows.push(Row::Header("Share settings"));
        rows.extend(Command::ALL.map(Row::Command));
        SettingsScreen { rows, cursor: 1 }
    }

    /// The highlighted row.
    pub fn current(&self) -> Row {
        self.rows[self.cursor]
    }

    /// Moves the cursor by `delta` rows, skipping headers.
    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.rows.len() - 1;
        let target = self.cursor.saturating_add_signed(delta).min(last);
        let step: isize = if delta < 0 { -1 } else { 1 };
        let selectable = |i: &usize| !matches!(self.rows[*i], Row::Header(_));
        // The first row from the target onwards, else back towards the cursor.
        let ahead = std::iter::successors(Some(target), |i| i.checked_add_signed(step))
            .take_while(|i| *i <= last)
            .find(selectable);
        let behind = || {
            std::iter::successors(Some(target), |i| i.checked_add_signed(-step))
                .take_while(|i| *i <= last)
                .find(selectable)
        };
        if let Some(row) = ahead.or_else(behind) {
            self.cursor = row;
        }
    }
}

/// The keys of `action` under `config`'s overrides.
pub fn keys_of(config: &Config, action: Action) -> Vec<KeyChord> {
    let (keymap, _) = Keymap::with_overrides(&config.keys);
    keymap.chords_for(action).to_vec()
}

/// Gives `chord` to `action` (replacing its keys, or added with `add`).
/// Another command that had the chord loses it; its id is returned.
pub fn assign_key(
    config: &mut Config,
    action: Action,
    chord: KeyChord,
    add: bool,
) -> Option<String> {
    assign_key_to(config, action.id(), chord, add)
}

/// Like [`assign_key`] for any command id: a built-in action, or a plugin
/// or script command (`plugin.NAME.COMMAND`, `script.NAME`).
pub fn assign_key_to(config: &mut Config, id: &str, chord: KeyChord, add: bool) -> Option<String> {
    let (keymap, _) = Keymap::with_overrides(&config.keys);
    let previous = keymap.command_for(&chord).filter(|other| other != id);
    if let Some(other) = &previous {
        let remaining: Vec<KeyChord> =
            keymap.chords_for_id(other).iter().copied().filter(|c| *c != chord).collect();
        set_keys(config, other, &remaining);
    }
    let mut keys = if add { keymap.chords_for_id(id).to_vec() } else { Vec::new() };
    if !keys.contains(&chord) {
        keys.push(chord);
    }
    set_keys(config, id, &keys);
    previous
}

/// Removes every key of `action`.
pub fn clear_keys(config: &mut Config, action: Action) {
    clear_keys_of(config, action.id());
}

/// Removes every key of the command `id`.
pub fn clear_keys_of(config: &mut Config, id: &str) {
    set_keys(config, id, &[]);
}

/// Gives `action` its built-in keys again, taking them from commands that
/// have them now; returns the ids of those commands.
pub fn reset_keys(config: &mut Config, action: Action) -> Vec<String> {
    config.keys.remove(action.id());
    let mut losers = Vec::new();
    for chord in default_keys(action.id()) {
        let (keymap, _) = Keymap::with_overrides(&config.keys);
        if let Some(other) = keymap.command_for(&chord).filter(|o| o != action.id()) {
            let remaining: Vec<KeyChord> =
                keymap.chords_for_id(&other).iter().copied().filter(|c| *c != chord).collect();
            set_keys(config, &other, &remaining);
            losers.push(other);
        }
    }
    losers
}

/// The built-in keys of command `id` (none for plugins and scripts).
fn default_keys(id: &str) -> Vec<KeyChord> {
    Action::from_id(id)
        .map(|action| action.default_keys().iter().filter_map(|k| k.parse().ok()).collect())
        .unwrap_or_default()
}

/// Stores `keys` for command `id` as an override, or no override when they
/// are the built-in ones.
fn set_keys(config: &mut Config, id: &str, keys: &[KeyChord]) {
    if keys == default_keys(id).as_slice() {
        config.keys.remove(id);
    } else {
        config.keys.insert(id.to_owned(), keys.iter().map(ToString::to_string).collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> KeyChord {
        text.parse().unwrap()
    }

    #[test]
    fn options_toggle_cycle_and_take_numbers() {
        let mut config = Config::default();
        Field::ShowHidden.step(&mut config, 1);
        assert!(config.panels.show_hidden);
        Field::Verify.step(&mut config, 1);
        assert_eq!(Field::Verify.value(&config), "same content (XXH3)");
        Field::Verify.step(&mut config, -1);
        Field::Verify.step(&mut config, -1);
        assert_eq!(config.transfers.verify, VerifyMode::None);
        Field::Conflict.step(&mut config, -1);
        assert_eq!(config.transfers.conflict, ConflictPolicy::Fail, "wraps around");
        Field::ParallelJobs.set(&mut config, "1").unwrap();
        assert_eq!(config.transfers.parallel_jobs, 1);
        Field::ParallelJobs.step(&mut config, -1);
        assert_eq!(config.transfers.parallel_jobs, 1, "never below 1");
        assert!(Field::TabWidth.set(&mut config, "wide").is_err());
        Field::TabWidth.set(&mut config, "99").unwrap();
        assert_eq!(config.viewer.tab_width, 16, "clamped");
        for field in Field::ALL {
            field.reset(&mut config);
        }
        assert_eq!(config, Config::default());
    }

    #[test]
    fn keys_move_between_commands() {
        let mut config = Config::default();
        // F5 goes from Copy to Refresh; Copy is left without keys.
        let previous = assign_key(&mut config, Action::Refresh, chord("F5"), true);
        assert_eq!(previous.as_deref(), Some("files.copy"));
        assert_eq!(keys_of(&config, Action::Refresh), [chord("Ctrl+R"), chord("F5")]);
        assert!(keys_of(&config, Action::Copy).is_empty());
        // Resetting Copy takes F5 back.
        assert_eq!(reset_keys(&mut config, Action::Copy), [Action::Refresh.id()]);
        assert_eq!(keys_of(&config, Action::Copy), [chord("F5")]);
        assert_eq!(keys_of(&config, Action::Refresh), [chord("Ctrl+R")]);
        assert!(config.keys.is_empty(), "back to the built-in keys: no overrides");
        clear_keys(&mut config, Action::Quit);
        assert_eq!(config.keys["app.quit"], Vec::<String>::new());
    }

    #[test]
    fn the_window_fonts_are_options() {
        let mut config = Config::default();
        Field::FontFamily.set(&mut config, " Fira Sans ").unwrap();
        Field::FontSize.set(&mut config, "200").unwrap();
        Field::FixedFontFamily.set(&mut config, "Fira Code").unwrap();
        assert_eq!(config.appearance.font_family, "Fira Sans");
        assert_eq!(config.appearance.font_size, 48, "clamped");
        assert_eq!(Field::FixedFontFamily.value(&config), "Fira Code");
        let again = Config::from_toml(&config.to_toml(), "test").unwrap();
        assert_eq!(again.appearance, config.appearance);
        for field in [Field::FontFamily, Field::FontSize, Field::FixedFontFamily] {
            field.reset(&mut config);
        }
        assert_eq!(config.appearance, Default::default());
    }

    #[test]
    fn the_cursor_skips_headers() {
        let mut screen = SettingsScreen::new();
        assert_eq!(screen.current(), Row::Option(Field::ConfirmDelete));
        screen.move_cursor(-1);
        assert_eq!(screen.current(), Row::Option(Field::ConfirmDelete), "stays on the first");
        screen.move_cursor(3);
        assert_eq!(screen.current(), Row::Option(Field::ShowHidden), "over the Panels title");
        screen.move_cursor(1000);
        assert_eq!(screen.current(), Row::Command(Command::EditFile));
    }
}
