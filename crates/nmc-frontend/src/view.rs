//! What the screen shows, as plain data.
//!
//! [`Screen::of`] describes everything a front end draws: both panels (only
//! the rows that fit), the status line, the function key bar and the open
//! dialog. The desktop interface sends it to QML as JSON; the text helpers
//! are shared with the terminal interface, so both read the same.

use nmc_config::{Action, Key, KeyChord};
use nmc_panel::format::{human_size, local_time, permissions};
use nmc_panel::{PARENT, Panel, SideId};
use nmc_transfer::{JobInfo, JobKind, JobState};
use nmc_vfs::Entry;
use serde::Serialize;

use crate::app::{App, display_location, side_index};
use crate::commands::CommandPalette;
use crate::dialogs::Overlay;
use crate::settings::SettingsScreen;
use crate::viewer::Viewer;

/// The whole screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Screen {
    /// Left and right panel (or quick view).
    pub panels: [PanelView; 2],
    /// The status line: a message, or details of the entry under the cursor.
    pub status: StatusView,
    /// Progress of running transfers (`2 jobs 40%`), empty when idle.
    pub jobs: String,
    /// How far they are, 0 to 1; -1 when idle.
    pub progress: f64,
    /// Function keys F1–F10 with short labels.
    pub keys: Vec<KeyLabel>,
    /// The open dialog or window.
    pub overlay: Option<OverlayView>,
    /// A copy conflict waiting for an answer, shown above everything.
    pub conflict: Option<ConflictView>,
    /// The fonts of the window (the terminal ignores them).
    pub fonts: FontsView,
    /// The outlines of the icons the rows refer to (empty when icons are
    /// off); the terminal ignores them.
    pub icons: Vec<IconShape>,
}

/// The outline of an icon on a 16 × 16 grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IconShape {
    /// The name rows refer to.
    pub id: &'static str,
    /// Lines as `x, y, x, y, …`; one that ends where it began is closed.
    pub strokes: Vec<Vec<u8>>,
}

/// The fonts chosen in the settings; empty or 0 means the system's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FontsView {
    /// Family of the text.
    pub family: String,
    /// Size in points.
    pub size: u32,
    /// Family of the fixed-width text.
    pub fixed_family: String,
}

/// One side of the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PanelView {
    /// Location of the panel, as the user types it.
    pub title: String,
    /// Git branch and distance from the upstream (`main ↑1`), empty outside a
    /// repository.
    pub branch: String,
    /// The drive or connection the location is on (`Local`, `C:`, a name).
    pub drive: String,
    /// Tabs, when there is more than one.
    pub tabs: Vec<TabView>,
    /// This side has the keyboard focus.
    pub active: bool,
    /// The visible rows.
    pub rows: Vec<RowView>,
    /// Index of the first visible row in the whole listing.
    pub offset: usize,
    /// Number of entries in the listing.
    pub total: usize,
    /// Counts, selection and filter.
    pub footer: String,
    /// Why the listing failed, when it did.
    pub error: Option<String>,
    /// A listing is in progress.
    pub loading: bool,
    /// The quick view, when this side shows it instead of a listing.
    pub quick_view: Option<ViewerView>,
    /// Column titles (name, size, date), the sorted one with an arrow.
    pub headers: [String; 3],
    /// The panel shows the flat (branch) view: rows carry a folder column.
    pub flat: bool,
}

/// A tab of a panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TabView {
    /// Last name of the tab's location.
    pub label: String,
    /// The tab shown.
    pub active: bool,
}

/// One entry of a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowView {
    /// Name; folders end with `/`. In the flat view only the file name.
    pub name: String,
    /// In the flat view, the folder of the file inside the panel's folder
    /// (`sub/deeper`); empty elsewhere.
    pub folder: String,
    /// Size, `<DIR>`, or `UP` for the parent entry.
    pub size: String,
    /// Modification time.
    pub date: String,
    /// A folder (or the parent entry).
    pub dir: bool,
    /// A hidden entry.
    pub hidden: bool,
    /// Part of the selection.
    pub selected: bool,
    /// Under the cursor.
    pub cursor: bool,
    /// Git mark: `M` changed, `?` untracked, `!` ignored, empty for none.
    pub git: String,
    /// The shape of its icon (see [`crate::icons::Shape::id`]); empty when
    /// icons are off or for the parent entry.
    pub icon: String,
    /// The icon's colour (`#rrggbb`).
    pub icon_color: String,
    /// The file as a `file://` URL, for dragging it to other programs;
    /// empty for anything that is not on this disk.
    pub url: String,
}

/// The status line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusView {
    /// Text.
    pub text: String,
    /// A message (not entry details).
    pub message: bool,
    /// The message is an error.
    pub error: bool,
}

/// A function key and what it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyLabel {
    /// 1 to 10.
    pub number: u8,
    /// Short label (`Copy`), empty when unbound.
    pub label: String,
}

/// The open dialog or window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OverlayView {
    /// F1: the main shortcuts, by category.
    Help {
        /// Groups of `(keys, label)` rows.
        groups: Vec<HelpGroup>,
    },
    /// F9: the command list.
    Commands(CommandsView),
    /// A one-line question.
    Input {
        /// Window title.
        title: String,
        /// Question.
        prompt: String,
        /// Current text.
        text: String,
        /// Cursor position in characters.
        cursor: usize,
        /// Lines below the field (multi-rename: the new names).
        preview: Vec<String>,
        /// The keys shown under the field.
        hint: String,
    },
    /// A yes/no question.
    Confirm {
        /// Window title.
        title: String,
        /// Text.
        lines: Vec<String>,
    },
    /// A list to pick from.
    Choose {
        /// Window title.
        title: String,
        /// Items.
        items: Vec<String>,
        /// Highlighted item.
        cursor: usize,
        /// Text below the list.
        footer: String,
    },
    /// Information to read.
    Message {
        /// Window title.
        title: String,
        /// Text.
        lines: Vec<String>,
        /// Enter opens a follow-up question.
        has_next: bool,
        /// The keys line below the text.
        hint: String,
        /// The logo to show above the text (About): a `file://` URL of
        /// a PNG for a window.
        logo_url: Option<String>,
        /// The same for a light theme.
        logo_url_light: Option<String>,
    },
    /// F3: the viewer.
    Viewer(ViewerView),
    /// Ctrl+J: the transfer queue.
    Jobs {
        /// One line per job.
        rows: Vec<JobRow>,
        /// Highlighted job.
        cursor: usize,
    },
    /// Ctrl+,: options and shortcuts.
    Settings(SettingsView),
}

/// The settings screen, cut to the rows that fit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsView {
    /// The visible rows.
    pub rows: Vec<SettingsRowView>,
    /// Index of the highlighted row in `rows`.
    pub cursor: usize,
    /// Position in the whole list, in percent.
    pub percent: usize,
    /// The keys of the highlighted row, or what to do while recording.
    pub hint: String,
    /// Waiting for a new shortcut.
    pub recording: bool,
}

/// One row of the settings screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsRowView {
    /// `header`, `option`, `shortcut` or `command`.
    pub kind: String,
    /// What the row is.
    pub label: String,
    /// Its value (shortcuts: the keys, `none` without any).
    pub value: String,
    /// Differs from the built-in value.
    pub changed: bool,
}

/// A category of the help window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HelpGroup {
    /// Category title.
    pub title: String,
    /// `(keys, label)` rows.
    pub rows: Vec<(String, String)>,
}

/// The command list (F9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandsView {
    /// Category buttons, `All` first.
    pub categories: Vec<String>,
    /// The chosen button.
    pub category: usize,
    /// What was typed.
    pub filter: String,
    /// The commands listed.
    pub rows: Vec<CommandRow>,
    /// Highlighted row.
    pub item: usize,
    /// While waiting for a key to give: the command's name.
    pub recording: Option<String>,
    /// The keys of the moment.
    pub hint: String,
    /// What the highlighted command does, while F1 shows it; else empty.
    pub help: String,
}

/// A row of the command list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandRow {
    /// Command id; empty when it cannot have shortcuts.
    pub id: String,
    /// `Category/Command`.
    pub name: String,
    /// Its shortcuts (`Ctrl+K / F5`), empty when none.
    pub keys: String,
}

/// The viewer or quick view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewerView {
    /// File shown.
    pub title: String,
    /// `text`, `hex` or `image`.
    pub mode: String,
    /// Visible lines (text or hex mode).
    pub lines: Vec<String>,
    /// Scroll position in percent.
    pub percent: usize,
    /// Only the start of the file was loaded.
    pub truncated: bool,
    /// The file could not be read.
    pub error: Option<String>,
    /// Still loading.
    pub loading: bool,
    /// The picture as a `file://` URL a desktop interface can show at full
    /// quality: the file itself for pictures on this disk, otherwise a PNG
    /// rendered for it (PDF pages, models, remote pictures).
    pub image_url: Option<String>,
    /// Formatted lines (documents), from the scroll position.
    pub rich: Vec<RichLineView>,
    /// The text `lines` with colours (source and data files): same rows,
    /// pieces with a `keyword`, `string`, `number`, `comment` or `name`
    /// word. Empty when the text has no colours.
    pub tinted: Vec<RichLineView>,
    /// One line about what is shown (picture size, page, zoom).
    pub info: String,
    /// The keys of this mode, for the window's header.
    pub keys: String,
    /// Picture zoom: 1 fits the picture.
    pub zoom: f64,
    /// The point of the picture in the middle, as fractions.
    pub center: [f64; 2],
    /// A new picture is being drawn.
    pub rendering: bool,
}

/// A piece of formatted text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpanView {
    /// The text.
    pub text: String,
    /// How it looks, as space-separated words: `h1`-`h6`, `bold`,
    /// `italic`, `code`, `link`, `mark` (decorations), `keyword`, `string`,
    /// `number`, `comment`, `name` (source code); empty for plain.
    pub style: String,
}

/// A line of a formatted document, with what a window needs to draw it
/// better than a terminal can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RichLineView {
    /// Its pieces of text.
    pub spans: Vec<SpanView>,
    /// Heading level 1-6, 0 for other lines.
    pub heading: u8,
    /// Only a line of `─` or `═` (under a heading, or a rule): a window
    /// draws a thin line instead.
    pub rule: bool,
    /// A line of a code block, drawn on a shaded background.
    pub code: bool,
}

impl RichLineView {
    fn of(line: &nmc_preview::rich::Line) -> Self {
        let rule = !line.is_empty()
            && line.iter().all(|s| {
                s.style.mark && !s.text.is_empty() && s.text.chars().all(|c| c == '─' || c == '═')
            });
        RichLineView {
            spans: line.iter().map(SpanView::of).collect(),
            heading: line.iter().map(|s| s.style.heading).max().unwrap_or(0),
            rule,
            code: !line.is_empty() && line.iter().all(|s| s.style.code),
        }
    }
}

impl SpanView {
    fn of(span: &nmc_preview::rich::Span) -> Self {
        let s = span.style;
        let mut words: Vec<String> = Vec::new();
        if s.heading > 0 {
            words.push(format!("h{}", s.heading));
        }
        if s.tone != nmc_preview::rich::Tone::Plain {
            words.push(s.tone.word().to_owned());
        }
        for (on, word) in [
            (s.bold, "bold"),
            (s.italic, "italic"),
            (s.code, "code"),
            (s.link, "link"),
            (s.mark, "mark"),
            (s.removed, "removed"),
        ] {
            if on {
                words.push(word.to_owned());
            }
        }
        SpanView { text: span.text.clone(), style: words.join(" ") }
    }
}

/// A job of the transfer queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobRow {
    /// The job as one line.
    pub text: String,
    /// `queued`, `running`, `paused`, `done`, `failed` or `cancelled`.
    pub state: String,
}

/// A copy conflict question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConflictView {
    /// What exists already.
    pub destination: String,
    /// The existing file.
    pub existing: String,
    /// The file being copied.
    pub incoming: String,
}

impl Screen {
    /// Describes the screen of `app`, with `rows` visible rows per panel.
    ///
    /// Like drawing, this keeps each panel scrolled so the cursor stays
    /// visible (`app.scroll`) and records `rows` as the page size.
    pub fn of(app: &mut App, rows: usize, viewer_rows: usize) -> Screen {
        let active = app.workspace.active_side();
        app.page = rows;
        let columns = app.columns;
        if let Some(Overlay::Viewer(viewer)) = &mut app.overlay {
            viewer.layout(columns);
        }
        if let Some(Some(viewer)) = &mut app.quick_view {
            // The quick view is half the window wide.
            viewer.layout(columns / 2);
        }
        let panels = [SideId::Left, SideId::Right].map(|side| {
            if side != active && app.quick_view.is_some() {
                quick_view_panel(app, viewer_rows)
            } else {
                panel_view(app, side, rows)
            }
        });
        let status = match app.status() {
            Some(status) => {
                StatusView { text: status.text.clone(), message: true, error: status.error }
            }
            None => StatusView {
                text: entry_details(
                    app.workspace.active(),
                    git_of(app, app.workspace.active_side()).as_deref(),
                ),
                message: false,
                error: false,
            },
        };
        let keys = (1..=10u8)
            .map(|number| KeyLabel { number, label: key_bar_label(app, number).to_owned() })
            .collect();
        let overlay = overlay_view(app, viewer_rows);
        let conflict = app.pending_conflict().map(|c| ConflictView {
            destination: display_location(&c.destination),
            existing: describe(&c.destination_entry),
            incoming: describe(&c.source_entry),
        });
        Screen {
            panels,
            status,
            jobs: jobs_summary(&app.jobs, app).unwrap_or_default(),
            progress: jobs_progress(&app.jobs, app).unwrap_or(-1.0),
            keys,
            overlay,
            conflict,
            icons: if app.context().config.panels.icons {
                crate::icons::Shape::ALL
                    .iter()
                    .map(|s| IconShape { id: s.id(), strokes: s.strokes() })
                    .collect()
            } else {
                Vec::new()
            },
            fonts: FontsView {
                family: app.context().config.appearance.font_family.clone(),
                size: app.context().config.appearance.font_size,
                fixed_family: app.context().config.appearance.fixed_font_family.clone(),
            },
        }
    }
}

/// Describes the command list.
pub fn commands_view(app: &App, palette: &CommandPalette) -> CommandsView {
    let keymap = &app.context().keymap;
    let recording = palette.recording.then(|| palette.selected().map(|e| e.name())).flatten();
    let hint = if let Some(name) = &recording {
        format!("Press the new shortcut for {name}    Esc: cancel")
    } else {
        "Type to find    Left Right: category    Enter: run    F1: help    \
         Ctrl+K: set shortcut    Del: remove shortcut    Esc: close"
            .to_owned()
    };
    CommandsView {
        categories: palette.categories.clone(),
        category: palette.category,
        filter: palette.filter.clone(),
        rows: palette
            .shown()
            .into_iter()
            .map(|e| CommandRow { id: e.id.clone(), name: e.name(), keys: keymap.keys_text(&e.id) })
            .collect(),
        item: palette.item,
        recording,
        hint,
        help: if palette.help {
            palette
                .selected()
                .map(|e| format!("{}: {}", e.name(), e.help()))
                .unwrap_or_else(|| "No command to explain.".to_owned())
        } else {
            String::new()
        },
    }
}

/// The first visible row of a list of `total` rows showing `rows` rows, so
/// that `cursor` stays visible, moving as little as possible from `offset`.
pub fn scroll_offset(offset: usize, cursor: usize, rows: usize, total: usize) -> usize {
    let rows = rows.max(1);
    let offset = if cursor < offset {
        cursor
    } else if cursor >= offset + rows {
        cursor + 1 - rows
    } else {
        offset
    };
    offset.min(total.saturating_sub(rows))
}

fn panel_view(app: &mut App, side: SideId, rows: usize) -> PanelView {
    let index = side_index(side);
    let human = app.context().config.panels.human_sizes;
    let icons = app.context().config.panels.icons;
    let active = app.workspace.active_side() == side;
    let side_state = app.workspace.side(side);
    let panel = side_state.panel();
    let cursor = panel.cursor();
    let total = panel.entries().len();
    let offset = scroll_offset(app.scroll[index], cursor, rows, total);
    let tabs = if side_state.tabs().len() > 1 {
        side_state
            .tabs()
            .iter()
            .enumerate()
            .map(|(i, tab)| TabView {
                label: format!("{}:{}", i + 1, tab_title(tab)),
                active: i == side_state.active_tab(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let git = git_of(app, side);
    let panel = app.workspace.side(side).panel();
    let row_views = panel
        .entries()
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
        .map(|(i, entry)| row_view(panel, entry, i == cursor, human, git.as_deref(), icons))
        .collect();
    let view = PanelView {
        title: if panel.is_branch() {
            format!("{}  (every file below)", display_location(panel.location()))
        } else {
            display_location(panel.location())
        },
        flat: panel.is_branch(),
        drive: {
            let label = crate::drives::drive_label(&app.context().config, panel.location());
            let state =
                crate::drives::connection_state(app.connection_lost[index], panel.is_loading());
            match crate::drives::connection_kind(&panel.location().to_string()) {
                Some(kind) => format!("{label} · {kind}{state}"),
                None => label,
            }
        },
        branch: git.as_ref().map(|g| g.summary()).unwrap_or_default(),
        tabs,
        active,
        rows: row_views,
        offset,
        total,
        footer: panel_footer(panel),
        error: panel.error().filter(|_| panel.entries().is_empty()).map(str::to_owned),
        loading: panel.is_loading(),
        quick_view: None,
        headers: column_headers(panel.sort()),
    };
    app.scroll[index] = offset;
    view
}

/// The git status of what `side` shows, if it is known for that folder.
pub fn git_of(app: &App, side: SideId) -> Option<std::sync::Arc<crate::git::GitStatus>> {
    let panel = app.workspace.side(side).panel();
    match &app.git[side_index(side)] {
        Some((path, status)) if path == panel.location() => Some(status.clone()),
        _ => None,
    }
}

/// The mark of `name`: one character, or empty.
pub fn git_mark(git: Option<&crate::git::GitStatus>, name: &str) -> String {
    let name = name.trim_end_matches('/');
    git.and_then(|g| g.marks.get(name)).map(|m| m.to_string()).unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn row_view(
    panel: &Panel,
    entry: &Entry,
    cursor: bool,
    human: bool,
    git: Option<&crate::git::GitStatus>,
    icons: bool,
) -> RowView {
    let icon = (icons && entry.name != PARENT).then(|| crate::icons::icon_for(entry));
    let mut name = entry.name.clone();
    let mut folder = String::new();
    if panel.is_branch()
        && entry.name != PARENT
        && let Some((parents, file)) = entry.name.rsplit_once('/')
    {
        folder = parents.to_owned();
        name = file.to_owned();
    }
    if entry.is_dir() && entry.name != PARENT {
        name.push('/');
    }
    RowView {
        name,
        folder,
        size: size_text(entry, human),
        date: entry.modified.map(local_time).unwrap_or_default(),
        dir: entry.is_dir(),
        hidden: entry.is_hidden(),
        selected: panel.is_selected(&entry.name),
        cursor,
        git: git_mark(git, &entry.name),
        icon: icon.map(|i| i.shape.id().to_owned()).unwrap_or_default(),
        icon_color: icon.map(|i| i.color.to_owned()).unwrap_or_default(),
        url: if entry.name == PARENT {
            String::new()
        } else {
            panel
                .location()
                .join(&entry.name)
                .to_local_path()
                .map(|p| file_url(&p))
                .unwrap_or_default()
        },
    }
}

fn quick_view_panel(app: &App, rows: usize) -> PanelView {
    let quick_view = match &app.quick_view {
        Some(Some(viewer)) => viewer_view(viewer, rows),
        _ => {
            let panel = app.workspace.active();
            let lines = match panel.current_entry() {
                Some(entry) if entry.is_dir() && entry.name != PARENT => vec![
                    format!("{}/", entry.name),
                    String::new(),
                    "A folder.".into(),
                    format!("Modified: {}", entry.modified.map(local_time).unwrap_or_default()),
                    String::new(),
                    "F3 counts its size; Enter opens it.".into(),
                ],
                _ => vec!["Move the cursor to a file to preview it.".into()],
            };
            ViewerView {
                title: "Quick view".into(),
                mode: "text".into(),
                lines,
                percent: 100,
                truncated: false,
                error: None,
                loading: false,
                image_url: None,
                rich: Vec::new(),
                tinted: Vec::new(),
                info: String::new(),
                keys: String::new(),
                zoom: 1.0,
                center: [0.5, 0.5],
                rendering: false,
            }
        }
    };
    let title = match app.workspace.active().current_entry() {
        Some(entry) if entry.name != PARENT => format!("Quick view: {}", entry.name),
        _ => "Quick view".into(),
    };
    PanelView {
        title,
        drive: String::new(),
        branch: String::new(),
        tabs: Vec::new(),
        active: false,
        rows: Vec::new(),
        offset: 0,
        total: 0,
        footer: format!(
            "Shift+Up/Down/PgUp/PgDn scroll, {} closes",
            app.context().keymap.shortcut_text(Action::QuickView)
        ),
        error: None,
        loading: false,
        quick_view: Some(quick_view),
        headers: Default::default(),
        flat: false,
    }
}

fn overlay_view(app: &App, viewer_rows: usize) -> Option<OverlayView> {
    Some(match app.overlay()? {
        Overlay::Help => OverlayView::Help {
            groups: app
                .context()
                .keymap
                .help()
                .into_iter()
                .map(|(category, rows)| HelpGroup {
                    title: category.title().to_owned(),
                    rows: rows.into_iter().map(|(keys, label)| (keys, label.to_owned())).collect(),
                })
                .collect(),
        },
        Overlay::Commands(palette) => OverlayView::Commands(commands_view(app, palette)),
        Overlay::Input(dialog) => OverlayView::Input {
            title: dialog.title.clone(),
            prompt: dialog.prompt.clone(),
            text: dialog.shown().text().to_owned(),
            cursor: dialog.shown().cursor(),
            preview: dialog.preview.clone(),
            hint: dialog.hint(),
        },
        Overlay::Confirm(dialog) => {
            OverlayView::Confirm { title: dialog.title.clone(), lines: dialog.lines.clone() }
        }
        Overlay::Choose(dialog) => OverlayView::Choose {
            title: dialog.title.clone(),
            items: dialog.items.clone(),
            cursor: dialog.cursor,
            footer: dialog.footer.clone(),
        },
        Overlay::Message(dialog) => OverlayView::Message {
            title: dialog.title.clone(),
            lines: dialog.lines.clone(),
            has_next: dialog.then.is_some() || dialog.then_choose.is_some(),
            hint: dialog.hint(),
            logo_url: dialog.logo.then(|| crate::about::logo_urls().map(|u| u.0)).flatten(),
            logo_url_light: dialog.logo.then(|| crate::about::logo_urls().map(|u| u.1)).flatten(),
        },
        Overlay::Viewer(viewer) => OverlayView::Viewer(viewer_view(viewer, viewer_rows)),
        Overlay::Jobs(view) => OverlayView::Jobs {
            rows: app
                .jobs
                .iter()
                .map(|job| JobRow {
                    text: format!("{}   {}", job_line(job, app), job_route(job, app)),
                    state: job_state(job.state).into(),
                })
                .collect(),
            cursor: view.cursor,
        },
        // The window's title and hint lines take some of the rows.
        Overlay::Settings(screen) => {
            OverlayView::Settings(settings_view(app, screen, viewer_rows.saturating_sub(7)))
        }
    })
}

/// Describes the settings screen with `rows` visible rows.
pub fn settings_view(app: &App, screen: &SettingsScreen, rows: usize) -> SettingsView {
    use crate::settings::{FieldKind, Row};
    let config = &app.context().config;
    let defaults = nmc_config::Config::default();
    let rows = rows.max(3);
    let total = screen.rows.len();
    // Keep the cursor away from the edges, like a list that scrolls.
    let offset = screen.cursor.saturating_sub(rows / 2).min(total.saturating_sub(rows));
    let row_view = |row: &Row| match *row {
        Row::Header(title) => SettingsRowView {
            kind: "header".into(),
            label: title.into(),
            value: String::new(),
            changed: false,
        },
        Row::Option(field) => SettingsRowView {
            kind: "option".into(),
            label: field.label().into(),
            value: field.value(config),
            changed: field.value(config) != field.value(&defaults),
        },
        Row::Command(command) => SettingsRowView {
            kind: "command".into(),
            label: command.label().into(),
            value: String::new(),
            changed: false,
        },
    };
    let find = if screen.filter.is_empty() {
        "Type to find   ".to_owned()
    } else {
        format!("Find: {}_   ", screen.filter)
    };
    let esc = if screen.filter.is_empty() { "Esc: close" } else { "Esc: clear" };
    let hint = match screen.current() {
        Row::Option(field) => match field.kind() {
            FieldKind::Toggle | FieldKind::Choice => {
                format!("{find}Enter / ← →: change   Del: default   {esc}")
            }
            FieldKind::Number(..) => {
                format!("{find}Enter: type a value   ← →: one less / more   Del: default   {esc}")
            }
            FieldKind::Text => format!("{find}Enter: type a value   Del: default   {esc}"),
            FieldKind::Font => {
                format!("{find}Enter: choose an installed font   Del: default   {esc}")
            }
        },
        _ => format!("{find}Enter: run   {esc}"),
    };
    SettingsView {
        rows: screen.rows.iter().skip(offset).take(rows).map(row_view).collect(),
        cursor: screen.cursor - offset,
        percent: if total <= rows { 100 } else { (offset + rows) * 100 / total },
        hint,
        recording: false,
    }
}

/// Describes the viewer with `rows` visible lines.
pub fn viewer_view(viewer: &Viewer, rows: usize) -> ViewerView {
    let total = viewer.row_count();
    let image_url = if viewer.shows_image() {
        let plain_picture = viewer.pdf.is_none() && viewer.model.is_none();
        let file = viewer
            .picture_file
            .clone()
            .or_else(|| plain_picture.then(|| viewer.path.to_local_path()).flatten());
        file.map(|path| file_url(&path))
    } else {
        None
    };
    let rich: Vec<RichLineView> = viewer.visible_rich(rows).iter().map(RichLineView::of).collect();
    let lines = if viewer.shows_image() || !rich.is_empty() {
        Vec::new()
    } else {
        viewer.visible_rows(rows)
    };
    let tinted: Vec<RichLineView> =
        viewer.visible_tinted(rows).iter().map(RichLineView::of).collect();
    ViewerView {
        title: viewer.title.clone().unwrap_or_else(|| display_location(&viewer.path)),
        mode: viewer.mode().into(),
        lines,
        percent: if total <= rows { 100 } else { ((viewer.scroll + rows) * 100 / total).min(100) },
        truncated: viewer.truncated,
        error: viewer.error.clone(),
        loading: viewer.data.is_none() && viewer.error.is_none(),
        image_url,
        rich,
        tinted,
        info: viewer.info(),
        keys: viewer_keys(viewer).into(),
        zoom: f64::from(viewer.zoom.factor),
        center: [f64::from(viewer.zoom.center.0), f64::from(viewer.zoom.center.1)],
        rendering: viewer.rendering,
    }
}

/// A `file://` URL of a local path, as Qt and browsers read it.
fn file_url(path: &std::path::Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    if text.starts_with('/') { format!("file://{text}") } else { format!("file:///{text}") }
}

/// The keys that matter in the viewer's current mode.
pub fn viewer_keys(viewer: &Viewer) -> &'static str {
    if viewer.diff.is_some() {
        return "n/p: next/previous change   PgUp/PgDn: scroll   Esc: close";
    }
    match viewer.mode() {
        "pdf" => "PgUp/PgDn: page   +/-: zoom   0: fit   F4: hex   Esc: close",
        "model" => "Arrows: turn   +/-: zoom   0: reset   F4: hex   Esc: close",
        "image" => "+/-: zoom   0: fit   arrows: move   F4: hex   Esc: close",
        "document" => "PgUp/PgDn: scroll   F4: hex   Esc: close",
        _ => "F2: wrap   F4: hex   Esc: close",
    }
}

// ------------------------------------------------------ shared text helpers

/// Column titles of a listing (name, size, date); the sorted column ends
/// with ↑ (ascending) or ↓.
pub fn column_headers(sort: nmc_index::SortSpec) -> [String; 3] {
    use nmc_index::SortKey;
    let arrow = if sort.descending { " ↓" } else { " ↑" };
    let mark = |key: SortKey, title: &str| {
        if sort.key == key { format!("{title}{arrow}") } else { title.to_owned() }
    };
    let name = if sort.key == SortKey::Extension {
        format!("Name (by type){arrow}")
    } else {
        mark(SortKey::Name, "Name")
    };
    [name, mark(SortKey::Size, "Size"), mark(SortKey::Modified, "Modified")]
}

/// Last name of a tab's location.
pub fn tab_title(panel: &Panel) -> String {
    panel.location().file_name().unwrap_or("/").to_owned()
}

/// The size column: `UP`, `<DIR>`, or the size.
pub fn size_text(entry: &Entry, human: bool) -> String {
    if entry.name == PARENT {
        "UP".to_owned()
    } else if entry.is_dir() {
        "<DIR>".to_owned()
    } else if human {
        human_size(entry.size)
    } else {
        entry.size.to_string()
    }
}

/// Size and date of an entry, for conflict questions.
pub fn describe(entry: &Entry) -> String {
    let date = entry.modified.map(local_time).unwrap_or_default();
    format!("{} ({} bytes), {date}", human_size(entry.size), entry.size)
}

/// The line below a panel: item count or selection, filter, loading.
pub fn panel_footer(panel: &Panel) -> String {
    let summary = panel.selection_summary();
    let mut text = if summary.files + summary.dirs > 0 {
        format!(
            "{} selected: {} files, {} folders, {}",
            summary.files + summary.dirs,
            summary.files,
            summary.dirs,
            human_size(summary.bytes)
        )
    } else {
        let count = panel.entries().iter().filter(|e| e.name != PARENT).count();
        format!("{count} items")
    };
    if !panel.filter().is_empty() {
        text.push_str(&format!("  filter: {} (Esc clears)", panel.filter()));
    }
    if panel.branch_truncated() {
        text.push_str(&format!("  (first {} files only)", nmc_panel::BRANCH_LIMIT));
    }
    if panel.is_loading() {
        text.push_str("  loading…");
    }
    text
}

/// Name, size, date, permissions and link target of the entry under the
/// cursor.
pub fn entry_details(panel: &Panel, git: Option<&crate::git::GitStatus>) -> String {
    let Some(entry) = panel.current_entry().filter(|e| e.name != PARENT) else {
        return String::new();
    };
    let mut parts = vec![entry.name.clone()];
    if !entry.is_dir() {
        parts.push(human_size(entry.size));
    }
    if let Some(modified) = entry.modified {
        parts.push(local_time(modified));
    }
    if let Some(mode) = entry.permissions {
        parts.push(permissions(mode));
    }
    if let Some(target) = &entry.symlink_target {
        parts.push(format!("→ {target}"));
    }
    // What a git mark in front of the name means.
    if let Some(mark) = git.and_then(|g| g.marks.get(entry.name.as_str())) {
        parts.push(format!("git {}", crate::git::legend(*mark)));
    }
    parts.join("  ")
}

/// What F`number` does with the modifiers held now, for the function key
/// bar: empty when nothing is bound to that combination.
pub fn key_bar_label(app: &App, number: u8) -> &'static str {
    // Alt is left out: desktops use it themselves (GNOME shows its own
    // window menu and moves windows with it), and no command is on Alt+F.
    let (ctrl, _alt, shift) = app.modifiers();
    app.context()
        .keymap
        .action_for(&KeyChord::normalized(Key::F(number), ctrl, false, shift))
        .map(short_label)
        .unwrap_or("")
}

/// A short label for the function key bar.
pub fn short_label(action: Action) -> &'static str {
    match action {
        Action::Help => "Help",
        Action::Rename => "Rename",
        Action::View => "View",
        Action::Edit => "Edit",
        Action::Copy => "Copy",
        Action::Move => "Move",
        Action::MakeDir => "Mkdir",
        Action::Delete => "Delete",
        Action::Menu => "Commands",
        Action::Quit => "Quit",
        other => other.label().split_whitespace().next().unwrap_or("").trim_end_matches('…'),
    }
}

/// `done` as a percentage of `total` (`None` when the total is unknown).
pub fn percent(done: u64, total: u64) -> Option<u64> {
    (done.min(total) * 100).checked_div(total)
}

/// The running and queued transfers, with what the totals say.
struct Active {
    running: usize,
    queued: usize,
    total: u64,
    done: u64,
}

fn active_jobs(jobs: &[JobInfo], app: &App) -> Option<Active> {
    let active: Vec<&JobInfo> =
        jobs.iter().filter(|j| matches!(j.state, JobState::Queued | JobState::Running)).collect();
    if active.is_empty() {
        return None;
    }
    Some(Active {
        running: active.iter().filter(|j| j.state == JobState::Running).count(),
        queued: active.iter().filter(|j| j.state == JobState::Queued).count(),
        total: active.iter().map(|j| j.total_bytes).sum(),
        done: active
            .iter()
            .map(|j| j.done_bytes + app.live_bytes.get(&j.id).copied().unwrap_or(0))
            .sum(),
    })
}

/// How far the running and queued transfers are, from 0 to 1 (`None` when
/// idle; 0 while the sizes are not known yet).
pub fn jobs_progress(jobs: &[JobInfo], app: &App) -> Option<f64> {
    let active = active_jobs(jobs, app)?;
    Some(if active.total == 0 {
        0.0
    } else {
        active.done.min(active.total) as f64 / active.total as f64
    })
}

/// `2 jobs · 40% · 12.4 MiB/s · 0:42 left · 3 queued` for queued and
/// running transfers.
pub fn jobs_summary(jobs: &[JobInfo], app: &App) -> Option<String> {
    let active = active_jobs(jobs, app)?;
    let count = active.running + active.queued;
    let mut parts = vec![format!("{count} job{}", if count == 1 { "" } else { "s" })];
    parts.push(percent(active.done, active.total).map_or("…".into(), |p| format!("{p}%")));
    if let Some(speed) = app.speed.speed(std::time::Instant::now()).filter(|s| *s >= 1.0) {
        parts.push(format!("{}/s", human_size(speed as u64)));
        let left = active.total.saturating_sub(active.done);
        if active.total > 0 {
            parts
                .push(format!("{} left", crate::speed::clock((left as f64 / speed).ceil() as u64)));
        }
    }
    if active.queued > 0 && active.running > 0 {
        parts.push(format!("{} queued", active.queued));
    }
    Some(parts.join(" · "))
}

/// A place for the queue: the system it is on (the saved connection, or
/// Local, Archive…) and the path in it: `nas: /backup`.
fn job_place(app: &App, path: &nmc_vfs::VfsPath) -> String {
    let system = crate::drives::drive_label(&app.context().config, path);
    let inside =
        if path.scheme() == "file" { display_location(path) } else { path.path().to_owned() };
    format!("{system}: {inside}")
}

/// Where a job takes its items from and puts them: `from Local: ~/docs  to
/// nas: /backup` (deletes only say where from).
pub fn job_route(job: &JobInfo, app: &App) -> String {
    let from = job.spec.source_folder().map(|p| job_place(app, &p)).unwrap_or_default();
    match (&job.spec.destination, job.spec.kind) {
        (Some(to), JobKind::Copy | JobKind::Move) => {
            format!("from {from}  →  to {}", job_place(app, to))
        }
        _ => format!("in {from}"),
    }
}

/// A job's state in words.
pub fn job_state(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Paused => "paused",
        JobState::Completed => "done",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
    }
}

/// A job of the transfer queue as one line.
pub fn job_line(job: &JobInfo, app: &App) -> String {
    let done = job.done_bytes + app.live_bytes.get(&job.id).copied().unwrap_or(0);
    let progress =
        percent(done, job.total_bytes).map_or_else(|| "   -".to_owned(), |p| format!("{p:>3}%"));
    let mut text = format!(
        "#{:<4} {:<9} {progress} {:>9}  p{:<3} {}",
        job.id.0,
        job_state(job.state),
        human_size(job.total_bytes),
        job.spec.priority,
        job.spec.what()
    );
    if matches!(job.state, JobState::Queued | JobState::Running | JobState::Paused)
        && job.spec.kind != JobKind::Delete
        && let Ok(files) = app.manager.job_parallelism(job.id)
    {
        let mode = if job.spec.options.auto_parallelism { "auto" } else { "fixed" };
        text.push_str(&format!("  [{files} at once, {mode}]"));
    }
    if let Some(error) = job.errors.first() {
        text.push_str(&format!("  ({error})"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_keeps_the_cursor_visible() {
        assert_eq!(scroll_offset(0, 3, 10, 100), 0);
        assert_eq!(scroll_offset(0, 12, 10, 100), 3, "cursor at the last visible row");
        assert_eq!(scroll_offset(20, 5, 10, 100), 5, "cursor above: scroll up to it");
        assert_eq!(scroll_offset(95, 99, 10, 100), 90, "never past the end");
        assert_eq!(scroll_offset(4, 2, 10, 5), 0, "short lists start at the top");
    }

    #[test]
    fn percentages() {
        assert_eq!(percent(5, 10), Some(50));
        assert_eq!(percent(20, 10), Some(100));
        assert_eq!(percent(1, 0), None);
    }
}
