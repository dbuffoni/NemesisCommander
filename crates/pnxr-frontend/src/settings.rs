//! The settings screen (Ctrl+,): every option and every shortcut, changed
//! in place and saved at once. Both interfaces draw it from
//! [`SettingsScreen::rows`]; what keys do is decided in `app.rs`.
//!
//! The functions here only change a [`Config`]; the application saves it
//! and applies it (key map, transfer manager) afterwards.

use pnxr_config::{Action, Config, KeyChord, Keymap, TransferProfile};
use pnxr_index::SortKey;
use pnxr_ops::{ConflictPolicy, VerifyMode};

/// The text of the help window (F1 on the settings screen) for the
/// highlighted row: the full help of the option, then the overview.
pub fn help_for(row: Row) -> (String, Vec<String>) {
    let mut lines = Vec::new();
    let title = match row {
        Row::Option(field) => {
            lines.push(field.label().to_owned());
            lines.push(String::new());
            lines.push(field.help().0.to_owned());
            lines.push(String::new());
            lines.extend(wrap(field.help().1, 76));
            lines.push(String::new());
            lines.push("─────".to_owned());
            "Setting help".to_owned()
        }
        _ => "Settings help".to_owned(),
    };
    lines.extend(help_lines());
    (title, lines)
}

/// `text` in lines of at most `width` characters, cut at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The overview that follows the help of an option.
pub fn help_lines() -> Vec<String> {
    [
        "Up/Down move, Left/Right/Enter change, Del = default, type to filter.",
        "Every change is saved and applied at once.",
        "",
        "Parallelism of transfers",
        "  automatic  (default) starts with 2 files at a time, adds files while",
        "             they make the copy clearly faster, goes back when they do",
        "             not help, and lowers the number if the speed drops.",
        "  fixed      always the number in \"Files at the same time\".",
        "Automatic profile",
        "  balanced   up to 8 files; fast without overloading the server.",
        "  max speed  up to 16 files, tries more often, keeps small gains.",
        "  discreet   up to 3 files, only for clear gains; leaves room.",
        "On errors the automatic mode uses fewer files; if one file at a time",
        "still fails it waits for the server (5 s, 10 s, 20 s ... 1.5 min) and",
        "tries again. A server that keeps refusing sessions gets a limit saved",
        "on its connection (max_connections); jobs on one server share it, and",
        "the next copy there starts from the level that worked.",
        "In the transfer queue (Ctrl+J): [ and ] set files by hand, a returns to",
        "automatic, p pauses (never resumed by itself).",
        "Bandwidth limit (KiB/s) caps all transfers together; 0 = no limit.",
        "\"Remember what works for each server\" keeps a small file with the level",
        "and limit that worked per server, so known servers start well. It drops",
        "deleted or renamed connections and old servers itself; off deletes it.",
    ]
    .iter()
    .map(|l| (*l).to_owned())
    .collect()
}

/// One option of the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Ask before deleting.
    ConfirmDelete,
    /// Delete to the recycle bin.
    UseTrash,
    /// Editor command for F4, for both interfaces.
    Editor,
    /// Editor command for F4 in the window.
    GuiEditor,
    /// Editor command for F4 in the terminal interface.
    TerminalEditor,
    /// Command that opens a terminal in the folder.
    Terminal,
    /// Show hidden files.
    ShowHidden,
    /// Never list `.DS_Store`.
    HideDsStore,
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
    /// The `git` program.
    GitExecutable,
    /// Jobs running at once.
    ParallelJobs,
    /// Files copied at once within a job.
    ParallelFiles,
    /// Automatic or fixed number of files at once.
    AutoParallelism,
    /// How boldly the automatic mode works.
    Profile,
    /// Remember what works for each server.
    KeepStats,
    /// Check after copying.
    Verify,
    /// Content check on agent and `pnxr serve` servers.
    VerifyRemoteFast,
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
    /// An editor: Enter lists known editors (typing a command is possible).
    Editor,
}

const SORTS: [(SortKey, &str); 4] = [
    (SortKey::Name, "name"),
    (SortKey::Extension, "extension"),
    (SortKey::Size, "size"),
    (SortKey::Modified, "date"),
];
const PROFILES: [(TransferProfile, &str); 3] = [
    (TransferProfile::Balanced, "balanced"),
    (TransferProfile::MaxSpeed, "max speed"),
    (TransferProfile::Discreet, "discreet"),
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
    pub const ALL: [Field; 30] = [
        Field::ConfirmDelete,
        Field::UseTrash,
        Field::Editor,
        Field::GuiEditor,
        Field::TerminalEditor,
        Field::Terminal,
        Field::ShowHidden,
        Field::HideDsStore,
        Field::Sort,
        Field::Descending,
        Field::HumanSizes,
        Field::ArrowNavigation,
        Field::Icons,
        Field::AutoParallelism,
        Field::Profile,
        Field::KeepStats,
        Field::ParallelFiles,
        Field::Verify,
        Field::VerifyRemoteFast,
        Field::Conflict,
        Field::BandwidthLimit,
        Field::Retries,
        Field::Wrap,
        Field::TabWidth,
        Field::HexAbove,
        Field::FontFamily,
        Field::FontSize,
        Field::FixedFontFamily,
        Field::GitStatus,
        Field::GitExecutable,
    ];

    /// The section it is shown in.
    pub fn section(self) -> &'static str {
        use Field::*;
        match self {
            ConfirmDelete | UseTrash | Editor | GuiEditor | TerminalEditor | Terminal => "General",
            ShowHidden | HideDsStore | Sort | Descending | HumanSizes | ArrowNavigation | Icons => {
                "Panels"
            }
            GitStatus | GitExecutable => "Git",
            ParallelJobs | ParallelFiles | AutoParallelism | Profile | KeepStats | Verify
            | VerifyRemoteFast | Conflict | BandwidthLimit | Retries => "Transfers",
            Wrap | TabWidth | HexAbove => "Viewer",
            FontFamily | FontSize | FixedFontFamily => "Window fonts (not the terminal)",
        }
    }

    /// The help of the option: a short line (always shown under the list)
    /// and the full text (F1). Written here, next to the option, and used by
    /// the screen, the search and the manual.
    pub fn help(self) -> (&'static str, &'static str) {
        use Field::*;
        match self {
            ConfirmDelete => (
                "Ask before anything is deleted.",
                "On: Delete (F8) asks first and lists what goes. Off: it deletes at once. Default: yes. Deleting permanently (Shift+F8) always asks.",
            ),
            UseTrash => (
                "Delete to the recycle bin when the place has one.",
                "On: F8 moves items to the recycle bin (trash) so they can be taken back. Off, or on a place that has no bin (servers), they are deleted for good. Default: yes.",
            ),
            Editor => (
                "The editor for F4, in the window and in the terminal.",
                "A command, for example `codium --wait`. {file} and {line} may be used; without {file} the file is added at the end. Empty: $VISUAL, then $EDITOR, then the system's. The two settings below override it for one interface. Enter lists known editors.",
            ),
            GuiEditor => (
                "The editor for F4 in the window.",
                "Used only by the window. A terminal editor (nvim, nano...) set here opens in a terminal window. Empty: the shared editor if it is a window editor, else the desktop's text editor. Enter lists known editors, installed ones first.",
            ),
            TerminalEditor => (
                "The editor for F4 in the terminal interface.",
                "Used only by the terminal interface, which gives the terminal to the editor and takes it back afterwards. A window editor set here starts detached and the interface stays usable. Empty: the shared editor if it is a terminal editor, then $VISUAL, $EDITOR, then vi.",
            ),
            Terminal => (
                "The command that opens a terminal in the folder.",
                "{dir} is the folder, {dir_url} the same as a URL; quotes keep spaces in one word. The process also starts in the folder, so a command with no {dir} works for terminals that follow it (Warp: /path/warp.appimage). Empty: the terminal of the desktop (xdg-terminal-exec, $TERMINAL, then known terminals). If the terminal fails, the message says what it printed.",
            ),
            ShowHidden => (
                "Show files whose name starts with a dot.",
                "Also Ctrl+H in the panels. Default: no.",
            ),
            HideDsStore => (
                "Never list the .DS_Store files of macOS.",
                "Even with hidden files shown. Default: no.",
            ),
            Sort => (
                "How new panels are sorted.",
                "name, extension, size or date. Ctrl+1, Ctrl+2 and Ctrl+3 change it for one panel.",
            ),
            Descending => (
                "Reverse the order: largest, latest or last first.",
                "Applies to the sort above. Default: no.",
            ),
            HumanSizes => ("Show sizes as 1.2 MiB instead of exact bytes.", "Default: yes."),
            ArrowNavigation => (
                "Left goes up a folder and Right enters the folder under the cursor.",
                "The \"Lynx-like\" movement of Double Commander. Default: no.",
            ),
            Icons => (
                "Draw a small icon for the kind of each file.",
                "Outline icons for folders, pictures, archives, code... Default: yes.",
            ),
            ParallelJobs => (
                "How many transfers run at the same time.",
                "1 runs them one after the other. The others wait in the queue (Ctrl+J).",
            ),
            ParallelFiles => (
                "Files copied at the same time within one job, when parallelism is fixed.",
                "Used only when \"Parallelism of transfers\" is fixed.",
            ),
            AutoParallelism => (
                "Automatic: the program finds how many files at once copy fastest.",
                "automatic starts with 2 files at a time, adds files while they make the copy clearly faster, goes back when they do not help and lowers the number if the speed drops. fixed always uses \"Files at the same time\".",
            ),
            Profile => (
                "How boldly the automatic mode adds files.",
                "balanced: up to 8 files. max speed: up to 16, tries more often. discreet: up to 3, only for clear gains, leaves room for other work.",
            ),
            KeepStats => (
                "Remember the level that worked for each server.",
                "A small file keeps what worked and the limit per server so known servers start well. Deleted or renamed connections and old servers are dropped by themselves; off deletes it.",
            ),
            Verify => (
                "Check copies after they are written.",
                "no check, same size, same content (XXH3, fast) or same content (BLAKE3, full). More thorough checks take longer.",
            ),
            VerifyRemoteFast => (
                "Ask agent and pnxr serve servers to check content themselves.",
                "The server hashes its copy, so the file is not read back over the network.",
            ),
            Conflict => (
                "What to do when the destination file exists.",
                "ask, overwrite, overwrite if newer, overwrite if the size differs, skip, keep both or stop with an error.",
            ),
            BandwidthLimit => {
                ("Cap all transfers together, in KiB per second.", "0 means no limit.")
            }
            Retries => (
                "How many times a network error is retried.",
                "Each retry waits a little longer (5 s, 10 s, 20 s... up to 1.5 minutes).",
            ),
            Wrap => ("Wrap long lines in the viewer.", "Default: no (long lines scroll sideways)."),
            TabWidth => ("Spaces a tab stands for in the viewer.", "Between 1 and 16."),
            HexAbove => (
                "Files bigger than this open as hex, in MiB.",
                "Big files are not read as text. Default: 50 MiB.",
            ),
            FontFamily => (
                "The font of all text in the window.",
                "Default: JetBrainsMono Nerd Font Mono. If it is not installed the system's font is used. Enter lists the installed fonts; Delete returns to the default. The terminal interface uses the terminal's font.",
            ),
            FontSize => (
                "The size of the text, in points.",
                "Default: 11. 0 uses the size of the system. The window never shows text bigger than it can hold.",
            ),
            FixedFontFamily => (
                "The font of columns, viewers and code.",
                "A fixed-width font. Default: JetBrainsMono Nerd Font Mono; if it is not installed the system's monospace font is used.",
            ),
            GitStatus => (
                "Mark changed files and show the branch inside Git repositories.",
                "M changed, ? not tracked, ! ignored; the branch and how far it is from the upstream are in the panel title. Default: yes.",
            ),
            GitExecutable => (
                "The git program to run.",
                "A name on PATH or a full path. Empty: git. Git actions are disabled with a message when it is not found.",
            ),
        }
    }

    /// What it is called on screen.
    pub fn label(self) -> &'static str {
        use Field::*;
        match self {
            ConfirmDelete => "Ask before deleting",
            UseTrash => "Delete to the recycle bin",
            Editor => "Editor, both interfaces (empty: $EDITOR or the system's)",
            GuiEditor => "Editor in the window (empty: the shared one, or the desktop's)",
            TerminalEditor => "Editor in the terminal (empty: the shared one, $EDITOR, vi)",
            Terminal => "Terminal command, {dir} is the folder (empty: the system's)",
            ShowHidden => "Show hidden files",
            HideDsStore => "Always hide .DS_Store files",
            Sort => "Sort by",
            Descending => "Largest, latest or last first",
            HumanSizes => "Sizes as KiB, MiB, GiB",
            ArrowNavigation => "← goes up, → enters a folder",
            GitStatus => "Git marks and branch in repositories",
            GitExecutable => "Git program (empty: git)",
            Icons => "Icons for the kinds of files",
            ParallelJobs => "Jobs at the same time (1: one after the other)",
            ParallelFiles => "Files at the same time when Parallelism is fixed",
            AutoParallelism => "Parallelism of transfers",
            Profile => "Automatic profile",
            KeepStats => "Remember what works for each server (small file)",
            Verify => "Check after copying",
            VerifyRemoteFast => "Content check on agent / pnxr serve servers",
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
            ConfirmDelete | UseTrash | ShowHidden | HideDsStore | Descending | HumanSizes
            | ArrowNavigation | GitStatus | Icons | Wrap | VerifyRemoteFast | KeepStats => {
                FieldKind::Toggle
            }
            Sort | Verify | Conflict | AutoParallelism | Profile => FieldKind::Choice,
            Terminal | GitExecutable => FieldKind::Text,
            Editor | GuiEditor | TerminalEditor => FieldKind::Editor,
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
            GuiEditor => g.gui_editor.clone(),
            TerminalEditor => g.terminal_editor.clone(),
            Terminal => g.terminal.clone(),
            ShowHidden => on(p.show_hidden),
            HideDsStore => on(p.hide_ds_store),
            Sort => name_of(&SORTS, p.sort),
            Descending => on(p.descending),
            HumanSizes => on(p.human_sizes),
            ArrowNavigation => on(p.arrow_navigation),
            GitStatus => on(p.git_status),
            GitExecutable => config.git.executable.clone(),
            Icons => on(p.icons),
            ParallelJobs => t.parallel_jobs.to_string(),
            ParallelFiles => t.parallel_files.to_string(),
            AutoParallelism => if t.auto_parallelism { "automatic" } else { "fixed" }.to_owned(),
            Profile => name_of(&PROFILES, t.profile),
            KeepStats => on(t.keep_stats),
            Verify => name_of(&VERIFY, t.verify),
            VerifyRemoteFast => on(t.verify_remote_fast),
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
            HideDsStore => p.hide_ds_store ^= true,
            Descending => p.descending ^= true,
            HumanSizes => p.human_sizes ^= true,
            ArrowNavigation => p.arrow_navigation ^= true,
            GitStatus => p.git_status ^= true,
            AutoParallelism => t.auto_parallelism ^= true,
            KeepStats => t.keep_stats ^= true,
            Profile => t.profile = cycle(&PROFILES, t.profile, delta),
            VerifyRemoteFast => t.verify_remote_fast ^= true,
            Icons => p.icons ^= true,
            Wrap => v.wrap ^= true,
            Sort => p.sort = cycle(&SORTS, p.sort, delta),
            Verify => t.verify = cycle(&VERIFY, t.verify, delta),
            Conflict => t.conflict = cycle(&CONFLICTS, t.conflict, delta),
            Editor | GuiEditor | TerminalEditor | Terminal | GitExecutable | FontFamily
            | FixedFontFamily => {}
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
            Field::GuiEditor => config.general.gui_editor = text.to_owned(),
            Field::TerminalEditor => config.general.terminal_editor = text.to_owned(),
            Field::Terminal => config.general.terminal = text.to_owned(),
            Field::GitExecutable => config.git.executable = text.to_owned(),
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
            FieldKind::Number(..) | FieldKind::Text | FieldKind::Font | FieldKind::Editor => {
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
    /// Forget what is remembered about servers.
    ResetStats,
    /// Give every command its built-in standard shortcuts again.
    ResetShortcuts,
    /// Give every command its built-in function key and button name again.
    ResetFunctionKeys,
    /// Open the settings file in the editor.
    EditFile,
}

impl Command {
    /// Every command, in screen order.
    pub const ALL: [Command; 8] = [
        Command::SendTo,
        Command::FetchFrom,
        Command::Export,
        Command::Import,
        Command::ResetStats,
        Command::ResetShortcuts,
        Command::ResetFunctionKeys,
        Command::EditFile,
    ];

    /// What it is called on screen.
    pub fn label(self) -> &'static str {
        match self {
            Command::SendTo => "Send these settings to another machine…",
            Command::FetchFrom => "Get the settings of another machine…",
            Command::Export => "Export settings to a file…",
            Command::Import => "Import settings from a file…",
            Command::ResetStats => "Forget what is remembered about servers",
            Command::ResetShortcuts => "Reset all shortcuts to the built-in ones",
            Command::ResetFunctionKeys => "Reset all function keys and button names",
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
    /// The rows shown, headers included: all of them, or those that match
    /// the filter.
    pub rows: Vec<Row>,
    /// The highlighted row (never a header, unless nothing matches).
    pub cursor: usize,
    /// What was typed to find a setting.
    pub filter: String,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// Every row of the screen, in order, headers included.
fn all_rows() -> Vec<Row> {
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
    rows
}

impl SettingsScreen {
    /// The screen with the cursor on the first option.
    pub fn new() -> Self {
        SettingsScreen { rows: all_rows(), cursor: 1, filter: String::new() }
    }

    /// The highlighted row.
    pub fn current(&self) -> Row {
        self.rows[self.cursor]
    }

    /// Shows only the rows whose section or label holds every word of
    /// `filter` (ignoring case and small spelling mistakes), each under its header; the cursor goes to
    /// the first one. An empty filter shows everything.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_owned();
        let mut found: Vec<(Option<Row>, Row, usize)> = Vec::new();
        let mut header: Option<Row> = None;
        for row in all_rows() {
            let text = match row {
                Row::Header(_) => {
                    header = Some(row);
                    continue;
                }
                Row::Option(field) => {
                    format!("{} {} {}", field.section(), field.label(), field.help().0)
                }
                Row::Command(command) => format!("share settings {}", command.label()),
            };
            if let Some(score) = crate::fuzzy::score(filter, &text) {
                found.push((header, row, score));
            }
        }
        // Spelling mistakes are forgiven only when nothing fits as typed.
        if found.iter().any(|(_, _, s)| crate::fuzzy::is_exact(filter, *s)) {
            found.retain(|(_, _, s)| crate::fuzzy::is_exact(filter, *s));
        }
        let mut rows: Vec<Row> = Vec::new();
        let mut last_header: Option<Row> = None;
        for (header, row, _) in found {
            if header != last_header {
                rows.extend(header);
                last_header = header;
            }
            rows.push(row);
        }
        if rows.is_empty() {
            rows.push(Row::Header("No setting matches"));
        }
        self.cursor = rows.iter().position(|r| !matches!(r, Row::Header(_))).unwrap_or(0);
        self.rows = rows;
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

/// The effective keymap of `config`.
fn keymap_of(config: &Config) -> Keymap {
    Keymap::from_config(config).0
}

/// The keys of `action` under `config`'s overrides: its function key first,
/// then its standard shortcuts.
pub fn keys_of(config: &Config, action: Action) -> Vec<KeyChord> {
    keymap_of(config).chords_for(action).to_vec()
}

/// Gives `chord` to `action` (replacing its keys, or added with `add`). A
/// function key goes to the function-key slot, any other chord to the
/// shortcuts. Another command that had the chord loses it; its id is returned.
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
    let keymap = keymap_of(config);
    let previous = take_chord(config, &keymap, id, chord);
    let mut keys = if add { keymap.chords_for_id(id).to_vec() } else { Vec::new() };
    if !keys.contains(&chord) {
        keys.push(chord);
    }
    set_keys(config, id, &keys);
    previous
}

/// Gives `chord`, which is no function key, to the command `id` as its only
/// standard shortcut; its function key stays. Another command that had the
/// chord loses it; its id is returned.
pub fn assign_shortcut_to(config: &mut Config, id: &str, chord: KeyChord) -> Option<String> {
    let keymap = keymap_of(config);
    let previous = take_chord(config, &keymap, id, chord);
    let mut keys: Vec<KeyChord> = keymap.fkey_for_id(id).into_iter().collect();
    keys.push(chord);
    set_keys(config, id, &keys);
    previous
}

/// Gives the function key `chord` (F1 to F10, bare, with Shift or Ctrl) to
/// the command `id`; its standard shortcuts stay. Returns the id of the
/// command that had it, which loses it.
pub fn assign_fkey_to(config: &mut Config, id: &str, chord: KeyChord) -> Option<String> {
    let keymap = keymap_of(config);
    let previous = take_chord(config, &keymap, id, chord);
    let mut keys = vec![chord];
    keys.extend(keymap.shortcuts_for_id(id));
    set_keys(config, id, &keys);
    previous
}

/// Removes the function key of the command `id`; the shortcuts stay.
pub fn clear_fkey_of(config: &mut Config, id: &str) {
    let shortcuts = keymap_of(config).shortcuts_for_id(id);
    set_keys(config, id, &shortcuts);
}

/// Removes the standard shortcuts of the command `id`; the function key stays.
pub fn clear_shortcuts_of(config: &mut Config, id: &str) {
    let keys: Vec<KeyChord> = keymap_of(config).fkey_for_id(id).into_iter().collect();
    set_keys(config, id, &keys);
}

/// Gives the command `id` the short name `name` for its bottom bar button;
/// empty text (or the built-in name) restores the built-in one. Returns
/// `false` when the name is longer than [`pnxr_config::LABEL_MAX`].
pub fn set_label_of(config: &mut Config, id: &str, name: &str) -> bool {
    let name = name.trim();
    if name.chars().count() > pnxr_config::LABEL_MAX {
        return false;
    }
    let built_in = Action::from_id(id).map(|a| a.short());
    if name.is_empty() || Some(name) == built_in {
        config.labels.remove(id);
    } else {
        config.labels.insert(id.to_owned(), name.to_owned());
    }
    config.shortcut_model = Some(pnxr_config::keymap::SHORTCUT_MODEL);
    true
}

/// Removes the key `chord` from the command that has it (not `id`); returns
/// that command's id.
fn take_chord(config: &mut Config, keymap: &Keymap, id: &str, chord: KeyChord) -> Option<String> {
    let previous = keymap.command_for(&chord).filter(|other| other != id);
    if let Some(other) = &previous {
        let remaining: Vec<KeyChord> =
            keymap.chords_for_id(other).iter().copied().filter(|c| *c != chord).collect();
        set_keys(config, other, &remaining);
    }
    previous
}

/// Removes every key of `action`.
pub fn clear_keys(config: &mut Config, action: Action) {
    clear_keys_of(config, action.id());
}

/// Removes every key of the command `id`: shortcuts and function key.
pub fn clear_keys_of(config: &mut Config, id: &str) {
    set_keys(config, id, &[]);
}

/// Gives `action` its built-in keys and short name again, taking the keys from
/// commands that have them now; returns the ids of those commands.
pub fn reset_keys(config: &mut Config, action: Action) -> Vec<String> {
    config.keys.remove(action.id());
    config.fkeys.remove(action.id());
    config.labels.remove(action.id());
    let mut losers = Vec::new();
    for chord in default_keys(action.id()) {
        let keymap = keymap_of(config);
        if let Some(other) = take_chord(config, &keymap, action.id(), chord) {
            losers.push(other);
        }
    }
    losers
}

/// Gives every command its built-in keys and names again (`functions`: the
/// function keys and short names; otherwise the standard shortcuts).
pub fn reset_all_keys(config: &mut Config, functions: bool) {
    if functions {
        config.fkeys.clear();
        config.labels.clear();
    } else {
        config.keys.clear();
    }
}

/// The built-in keys of command `id` (none for plugins and scripts): its
/// function key first, then its shortcuts.
fn default_keys(id: &str) -> Vec<KeyChord> {
    Action::from_id(id)
        .map(|action| {
            action.default_fkey().into_iter().chain(action.default_shortcuts()).collect()
        })
        .unwrap_or_default()
}

/// Stores `keys` for command `id`: the first function key as its `fkeys`
/// entry and the other chords as its shortcuts, each as an override only when
/// it differs from the built-in one.
fn set_keys(config: &mut Config, id: &str, keys: &[KeyChord]) {
    let fkey = keys.iter().copied().find(pnxr_config::is_function_key);
    let shortcuts: Vec<KeyChord> =
        keys.iter().copied().filter(|c| !pnxr_config::is_function_key(c)).collect();
    let action = Action::from_id(id);
    let default_shortcuts = action.map(Action::default_shortcuts).unwrap_or_default();
    let default_fkey = action.and_then(Action::default_fkey);
    if shortcuts == default_shortcuts {
        config.keys.remove(id);
    } else {
        config.keys.insert(id.to_owned(), shortcuts.iter().map(ToString::to_string).collect());
    }
    if fkey == default_fkey {
        config.fkeys.remove(id);
    } else {
        config.fkeys.insert(id.to_owned(), fkey.map(|c| c.to_string()).unwrap_or_default());
    }
    config.shortcut_model = Some(pnxr_config::keymap::SHORTCUT_MODEL);
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
        Field::ParallelJobs.reset(&mut config); // not on the screen any more
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
        assert_eq!(keys_of(&config, Action::Refresh), [chord("F5"), chord("Ctrl+R")]);
        assert!(keys_of(&config, Action::Copy).is_empty());
        // Resetting Copy takes F5 back.
        assert_eq!(reset_keys(&mut config, Action::Copy), [Action::Refresh.id()]);
        assert_eq!(keys_of(&config, Action::Copy), [chord("F5")]);
        assert_eq!(keys_of(&config, Action::Refresh), [chord("Ctrl+R")]);
        assert!(config.keys.is_empty(), "back to the built-in keys: no overrides");
        assert!(config.fkeys.is_empty(), "and no function-key overrides");
        // Clearing Quit removes its function key (F10): it has no shortcut.
        clear_keys(&mut config, Action::Quit);
        assert_eq!(config.fkeys["app.quit"], "");
        assert!(keys_of(&config, Action::Quit).is_empty());
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
        screen.move_cursor(7);
        assert_eq!(screen.current(), Row::Option(Field::ShowHidden), "over the Panels title");
        screen.move_cursor(1000);
        assert_eq!(screen.current(), Row::Command(Command::EditFile));
    }

    #[test]
    fn every_option_has_help_and_a_title_finds_its_whole_section() {
        for field in Field::ALL {
            let (short, long) = field.help();
            assert!(!short.is_empty() && !long.is_empty(), "{field:?} has no help");
        }
        let mut screen = SettingsScreen::new();
        screen.set_filter("git");
        let options: Vec<Field> = screen
            .rows
            .iter()
            .filter_map(|r| if let Row::Option(f) = r { Some(*f) } else { None })
            .collect();
        assert_eq!(options, [Field::GitStatus, Field::GitExecutable], "the Git section");
        screen.set_filter("transfers");
        assert!(screen.rows.contains(&Row::Option(Field::Retries)), "all of Transfers");
        screen.set_filter("popular tools unknown");
        screen.set_filter("mark changed files");
        assert!(screen.rows.contains(&Row::Option(Field::GitStatus)), "the help text is searched");
    }

    #[test]
    fn typing_finds_settings_under_their_headers() {
        let mut screen = SettingsScreen::new();
        screen.set_filter("font");
        let options: Vec<Row> =
            screen.rows.iter().copied().filter(|r| matches!(r, Row::Option(_))).collect();
        assert_eq!(
            options,
            [
                Row::Option(Field::FontFamily),
                Row::Option(Field::FontSize),
                Row::Option(Field::FixedFontFamily)
            ]
        );
        assert!(matches!(screen.rows[0], Row::Header(_)), "under its header");
        assert_eq!(screen.current(), Row::Option(Field::FontFamily));
        screen.set_filter("FIXED  font");
        assert_eq!(screen.current(), Row::Option(Field::FixedFontFamily));
        screen.set_filter("fnot");
        assert_eq!(screen.current(), Row::Option(Field::FontFamily), "a swapped pair of letters");
        screen.set_filter("zzzz");
        assert_eq!(screen.rows, [Row::Header("No setting matches")]);
        assert!(matches!(screen.current(), Row::Header(_)));
        screen.move_cursor(5);
        screen.set_filter("");
        assert_eq!(screen.rows, SettingsScreen::new().rows);
    }
}
