//! Drawing the [`App`] with ratatui. Nothing here changes application
//! state except the scroll offsets and page size the renderer measures.

use nmc_config::Category;
use nmc_panel::format::local_time;
use nmc_panel::{PARENT, SideId};
use nmc_transfer::JobState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use nmc_frontend::about;
use nmc_frontend::commands::CommandPalette;
use nmc_frontend::drives::drive_label;
use nmc_frontend::icons::icon_for;
use nmc_frontend::preview::Zoom;
use nmc_frontend::view::{
    column_headers, commands_view, describe, entry_details, git_mark, git_of, job_how, job_line,
    job_route, jobs_progress, jobs_summary, key_bar_action, panel_footer, scroll_offset, size_text,
    tab_title,
};
use nmc_frontend::{
    App, ChooseDialog, InputDialog, Overlay, TextField, Viewer, display_location, side_index,
};

const SIZE_WIDTH: usize = 9;
const DATE_WIDTH: usize = 16;

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

fn highlight() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}

/// Draws the whole screen.
pub fn draw(frame: &mut Frame, app: &mut App) {
    // Documents are laid out for the width they get: the whole screen in
    // the viewer, one panel in the quick view (both inside a border).
    let width = frame.area().width as usize;
    app.columns = width.saturating_sub(2);
    if let Some(Overlay::Viewer(viewer)) = &mut app.overlay {
        viewer.layout(width.saturating_sub(2));
    }
    if let Some(Some(viewer)) = &mut app.quick_view {
        viewer.layout((width / 2).saturating_sub(2));
    }
    // A line for the transfers, only while there are some.
    let progress = jobs_progress(&app.jobs, app);
    let [panels, transfers, status, keys] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(u16::from(progress.is_some())),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(panels);

    let active = app.workspace.active_side();
    for (side, area) in [(SideId::Left, left), (SideId::Right, right)] {
        if side != active && app.quick_view.is_some() {
            draw_quick_view(frame, app, area);
        } else {
            draw_panel(frame, app, side, area);
        }
    }
    if let Some(progress) = progress {
        draw_transfers(frame, app, progress, transfers);
    }
    draw_status(frame, app, status);
    draw_key_bar(frame, app, keys);

    if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Help => draw_help(frame, app),
            Overlay::Commands(palette) => draw_commands(frame, app, palette),
            Overlay::Input(dialog) => draw_input(frame, dialog, &app.with_back(dialog.hint())),
            Overlay::Confirm(dialog) => {
                let mut lines: Vec<Line> =
                    dialog.lines.iter().map(|l| Line::raw(l.as_str())).collect();
                lines.push(Line::raw(""));
                lines.push(Line::styled(app.with_back(dialog.hint()), dim()));
                draw_box(frame, &dialog.title, lines);
            }
            Overlay::Choose(dialog) => {
                let page = draw_choose(frame, dialog, &app.with_back(dialog.footer.clone()));
                app.page = page;
            }
            Overlay::Message(dialog) => {
                let mut lines: Vec<Line> =
                    dialog.lines.iter().map(|l| Line::raw(l.as_str())).collect();
                lines.push(Line::raw(""));
                lines.push(Line::styled(app.with_back(dialog.hint()), dim()));
                // A log keeps one size while lines are added to it.
                let log_rows = (dialog.busy || dialog.lines.len() > 10)
                    .then(|| frame.area().height.saturating_sub(6).min(22) as usize);
                if dialog.logo {
                    // The logo takes the first rows of the box.
                    let blank = (0..about::LOGO_ROWS).map(|_| Line::raw(""));
                    lines.splice(0..0, blank);
                    let area = draw_box(frame, &dialog.title, lines);
                    let top = Rect::new(
                        area.x + 1,
                        area.y + 1,
                        area.width.saturating_sub(2),
                        about::LOGO_ROWS,
                    );
                    draw_image(frame, about::logo(), Zoom::default(), top);
                } else {
                    draw_box_fixed(frame, &dialog.title, lines, log_rows);
                }
            }
            Overlay::Viewer(viewer) => {
                let page = draw_viewer(frame, viewer, frame.area());
                app.page = page;
            }
            Overlay::Jobs(view) => draw_jobs(frame, app, view.cursor),
            Overlay::Settings(screen) => draw_settings(frame, app, screen),
        }
    }
    if let Some(conflict) = app.pending_conflict() {
        let lines = vec![
            Line::raw(format!("{} already exists.", display_location(&conflict.destination))),
            Line::raw(""),
            Line::raw(format!("  Existing: {}", describe(&conflict.destination_entry))),
            Line::raw(format!("  New:      {}", describe(&conflict.source_entry))),
            Line::raw(""),
            Line::styled(
                shrink_hint(nmc_frontend::hints::CONFLICT, box_width(frame.area()) as usize - 4),
                dim(),
            ),
        ];
        draw_box(frame, "File exists", lines);
    }
}

// ---------------------------------------------------------------- panels

fn draw_panel(frame: &mut Frame, app: &mut App, side: SideId, area: Rect) {
    let is_active = app.workspace.active_side() == side;
    let git = git_of(app, side);
    let side_state = app.workspace.side(side);
    let panel = side_state.panel();
    let border = if is_active { Style::new().fg(Color::Cyan) } else { dim() };

    let mut block = Block::bordered().border_style(border).title(Line::styled(
        format!(
            " [{}] {}{}{} ",
            drive_label(&app.context().config, panel.location()),
            git.as_ref().map(|g| format!("⎇ {}  ", g.summary())).unwrap_or_default(),
            nmc_frontend::drives::panel_location(&app.context().config, panel.location()),
            if panel.is_branch() { "  (flat)" } else { "" }
        ),
        bold(),
    ));
    if side_state.tabs().len() > 1 {
        let spans: Vec<Span> = side_state
            .tabs()
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let text = format!(" {}:{} ", i + 1, tab_title(tab));
                if i == side_state.active_tab() {
                    Span::styled(text, highlight())
                } else {
                    Span::raw(text)
                }
            })
            .collect();
        block = block.title_top(Line::from(spans).right_aligned());
    }
    block = block.title_bottom(Line::styled(format!(" {} ", panel_footer(panel)), dim()));

    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 2 || inner.width < 8 {
        return;
    }

    let width = inner.width as usize;
    let show_date = width >= 40;
    // Two columns for the git mark when the folder is in a repository.
    let mark_width = if git.is_some() { 2 } else { 0 };
    // Two more for the icon of the kind of file.
    let icon_width = if app.context().config.panels.icons { 2 } else { 0 };
    let mut name_width = width
        .saturating_sub(SIZE_WIDTH + 1 + mark_width + icon_width)
        .saturating_sub(if show_date { DATE_WIDTH + 1 } else { 0 });
    // The flat view gives a third of the name column to the folder.
    // Columns of plugins come out of the name column, which keeps
    // at least 16 characters; the ones that do not fit are left out.
    let mut plugin_heads: Vec<nmc_frontend::ColumnHead> = Vec::new();
    for head in app.plugin_columns(side) {
        let used: usize = plugin_heads.iter().map(|h| h.width + 1).sum();
        if name_width >= used + head.width + 1 + 16 {
            plugin_heads.push(head.clone());
        }
    }
    let plugin_width: usize = plugin_heads.iter().map(|h| h.width + 1).sum();
    name_width = name_width.saturating_sub(plugin_width);
    let folder_width = if panel.is_branch() { (name_width / 3).clamp(0, 28) } else { 0 };
    name_width = name_width.saturating_sub(folder_width + usize::from(folder_width > 0));
    let [name_title, size_title, date_title] = column_headers(panel.sort());
    let header = format!(
        "{}{} {}{}{:>SIZE_WIDTH$}{}",
        " ".repeat(mark_width),
        fit(&name_title, name_width),
        if folder_width > 0 { format!("{} ", fit("Folder", folder_width)) } else { String::new() },
        plugin_heads.iter().map(|h| format!("{} ", fit(&h.title, h.width))).collect::<String>(),
        size_title,
        if show_date { format!(" {:<DATE_WIDTH$}", date_title) } else { String::new() }
    );
    // The text of nmc_folder.md, at most a third of the panel.
    let note =
        nmc_frontend::view::folder_text_lines(app, side, width, (inner.height as usize / 3).max(2));
    let note_rows = if note.is_empty() { 0 } else { note.len() + 1 };
    app.note_rows[side_index(side)] = note_rows;
    let rows = (inner.height as usize - 1).saturating_sub(note_rows).max(1);
    let index = side_index(side);
    let cursor = panel.cursor();
    let offset = scroll_offset(app.scroll[index], cursor, rows, panel.entries().len());
    app.scroll[index] = offset;
    if is_active {
        app.page = rows;
    }

    let mut lines: Vec<Line> = note.iter().map(rich_line).collect();
    if !note.is_empty() {
        lines.push(Line::styled("─".repeat(width), dim()));
    }
    lines.push(Line::styled(header, dim().add_modifier(Modifier::UNDERLINED)));
    if let Some(error) = panel.error().filter(|_| panel.entries().is_empty()) {
        lines.push(Line::styled(error.to_owned(), Style::new().fg(Color::Red)));
    }
    for (i, entry) in panel.entries().iter().enumerate().skip(offset).take(rows) {
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
        let size = size_text(entry, app.context().config.panels.human_sizes);
        let date = if show_date {
            format!(" {:<DATE_WIDTH$}", entry.modified.map(local_time).unwrap_or_default())
        } else {
            String::new()
        };
        let mark = if mark_width > 0 {
            format!("{:<1} ", git_mark(git.as_deref(), &entry.name))
        } else {
            String::new()
        };
        let name_text = format!("{} ", fit(&name, name_width));
        // The deepest folders are the useful end of a long path.
        let folder_text = if folder_width > 0 {
            format!("{} ", fit_end(&folder, folder_width))
        } else {
            String::new()
        };
        let cells = app.plugin_cells(side, &entry.name);
        let plugin_text: String = plugin_heads
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{} ", fit(cells.get(i).map_or("", String::as_str), h.width)))
            .collect();
        let rest = format!("{plugin_text}{:>SIZE_WIDTH$}{date}", size);

        let mut style = Style::new();
        if entry.is_dir() {
            style = style.add_modifier(Modifier::BOLD);
        }
        if entry.is_hidden() {
            style = style.fg(Color::DarkGray);
        }
        if panel.is_selected(&entry.name) {
            style = style.fg(Color::Rgb(0x6f, 0xa3, 0xff)).add_modifier(Modifier::BOLD);
        }
        if i == cursor {
            style = if is_active {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style.add_modifier(Modifier::UNDERLINED)
            };
        }
        let mut spans = vec![Span::styled(mark, style)];
        if icon_width > 0 && entry.name != PARENT {
            let icon = icon_for(entry);
            let mut colour = style;
            if i != cursor && !panel.is_selected(&entry.name) {
                colour = colour.fg(hex_colour(icon.color));
            }
            // The icon sits after the git mark, before the name.
            spans.push(Span::styled(format!("{} ", icon.shape.glyph()), colour));
        } else if icon_width > 0 {
            spans.push(Span::styled("  ", style));
        }
        spans.push(Span::styled(name_text, style));
        if !folder_text.is_empty() {
            spans.push(Span::styled(folder_text, if i == cursor { style } else { dim() }));
        }
        spans.push(Span::styled(rest, style));
        let line = Line::from(spans);
        lines.push(line);
    }
    if panel.is_loading() && panel.entries().is_empty() {
        lines.push(Line::styled("loading…", dim()));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The terminal colour for a `#rrggbb` string.
fn hex_colour(hex: &str) -> Color {
    let channel = |range: std::ops::Range<usize>| {
        hex.get(range).and_then(|c| u8::from_str_radix(c, 16).ok()).unwrap_or(128)
    };
    Color::Rgb(channel(1..3), channel(3..5), channel(5..7))
}

/// Pads or cuts `text` to exactly `width` characters (cut text ends in `…`).
pub fn fit(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return format!("{text:<width$}");
    }
    if width == 0 {
        return String::new();
    }
    let mut cut: String = text.chars().take(width - 1).collect();
    cut.push('…');
    cut
}

/// A line of key hints (`a: one    b: two    Esc: close`, items apart by two
/// or more spaces) that fits `width` columns: when it is too long, items
/// from the middle go, the first and the last (the way out) stay.
pub fn shrink_hint(hint: &str, width: usize) -> String {
    let items: Vec<&str> = hint
        .split(nmc_frontend::hints::SEP)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect();
    let join = |items: &[&str]| items.join(nmc_frontend::hints::SEP);
    if join(&items).chars().count() <= width || items.len() < 3 {
        return join(&items);
    }
    let last = items[items.len() - 1];
    let mut kept: Vec<&str> = vec![items[0]];
    for item in &items[1..items.len() - 1] {
        let mut trial = kept.clone();
        trial.push(item);
        trial.push(last);
        if join(&trial).chars().count() > width {
            break;
        }
        kept.push(item);
    }
    kept.push(last);
    join(&kept)
}

/// `text` padded to `width`, or cut at the start with `…` when longer (the
/// end of a folder path is the part that tells folders apart).
fn fit_end(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return format!("{text:<width$}");
    }
    if width == 0 {
        return String::new();
    }
    let tail: String = text.chars().skip(count - (width - 1)).collect();
    format!("…{tail}")
}

fn draw_quick_view(frame: &mut Frame, app: &App, area: Rect) {
    let mut block = Block::bordered().border_style(dim()).title(" Quick view ");
    if let Some(Some(viewer)) = &app.quick_view {
        let info = viewer.info();
        if !info.is_empty() {
            block = block.title_bottom(Line::styled(format!(" {info} "), dim()));
        }
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match &app.quick_view {
        Some(Some(viewer)) => draw_content(frame, viewer, inner),
        _ => frame.render_widget(
            Paragraph::new(Line::styled("Move the cursor to a file to preview it", dim())),
            inner,
        ),
    }
}

// ------------------------------------------------------- status and keys

/// A thin bar and the numbers of the running transfers.
fn draw_transfers(frame: &mut Frame, app: &App, progress: f64, area: Rect) {
    let width = (area.width as usize / 4).clamp(10, 40);
    let filled = ((progress * width as f64).round() as usize).min(width);
    let line = Line::from(vec![
        Span::styled("━".repeat(filled), Style::new().fg(Color::Cyan)),
        Span::styled("─".repeat(width - filled), dim()),
        Span::raw("  "),
        Span::styled(
            jobs_summary(&app.jobs, app).unwrap_or_default(),
            Style::new().fg(Color::Cyan),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let (text, style) = match app.status() {
        Some(status) if status.error => (status.text.clone(), Style::new().fg(Color::Red)),
        Some(status) => (status.text.clone(), Style::new()),
        None => (
            entry_details(
                app.workspace.active(),
                git_of(app, app.workspace.active_side()).as_deref(),
            ),
            dim(),
        ),
    };
    let line = Line::from(Span::styled(fit(&text, area.width as usize), style));
    frame.render_widget(Paragraph::new(line), area);
}

/// The columns of each label on the key bar: as many as fit (up to the 8 of
/// the longest short name), five on a narrow screen where the shorter
/// `narrow` names are used. With the "F" and the number in front, ten
/// labels of six letters fit 91 columns.
fn key_bar_width(columns: usize) -> usize {
    let fits = |width: usize| 9 * (2 + width + 1) + (3 + width + 1) <= columns;
    (6..=8).rev().find(|w| fits(*w)).unwrap_or(5)
}

fn draw_key_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    for n in 1..=10u8 {
        let width = key_bar_width(area.width as usize);
        let label = match key_bar_action(app, n) {
            Some(action) if width < 6 => action.narrow(),
            Some(action) => action.short(),
            None => "",
        };
        let label = fit(label, width).trim_end().to_owned();
        spans.push(Span::raw(format!("F{n}")));
        spans.push(Span::styled(
            format!("{label:<width$}"),
            Style::new().fg(Color::Black).bg(Color::Cyan),
        ));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// --------------------------------------------------------------- overlays

/// A `width` × `height` rectangle centered in `area` (clamped to it).
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// The width of a message or question: most of the screen, so a long path
/// or log line fits and the window does not change size with its text.
fn box_width(screen: Rect) -> u16 {
    let wide = screen.width.saturating_sub(2);
    (screen.width * 9 / 10).clamp(60.min(wide), 120.min(wide))
}

/// `text` cut into rows of at most `width` characters, at any character
/// (paths have no spaces to break at). A continued row is indented.
pub fn hard_wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return vec![text.to_owned()];
    }
    let indent = chars.iter().take_while(|c| **c == ' ').count().min(width / 2);
    let mut rows = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let room = if rows.is_empty() { width } else { width - indent - 2 };
        let end = (at + room).min(chars.len());
        let part: String = chars[at..end].iter().collect();
        rows.push(if rows.is_empty() { part } else { format!("{}{part}", " ".repeat(indent + 2)) });
        at = end;
    }
    rows
}

/// A window with `lines` (wrapped to its width) and a key line under them.
/// With too many lines for the screen, the last ones are shown (what a
/// running log has just said). Returns the area.
fn draw_box(frame: &mut Frame, title: &str, lines: Vec<Line>) -> Rect {
    draw_box_fixed(frame, title, lines, None)
}

/// [`draw_box`] with the height of the content always `rows` (blank rows
/// below it), so the window does not change size while its content does.
fn draw_box_fixed(frame: &mut Frame, title: &str, lines: Vec<Line>, rows: Option<usize>) -> Rect {
    let screen = frame.area();
    let width = box_width(screen);
    let inner_width = width.saturating_sub(4) as usize;
    let mut wrapped: Vec<Line> = Vec::new();
    for line in lines {
        let style = line.spans.first().map(|s| s.style).unwrap_or_default();
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        for part in hard_wrap(&text, inner_width) {
            wrapped.push(Line::styled(part, style));
        }
    }
    let room = screen.height.saturating_sub(4) as usize;
    let height = rows.unwrap_or(wrapped.len()).min(room);
    if wrapped.len() > height {
        // The last rows matter (the verdict, the key line).
        let cut = wrapped.len() - height + 1;
        wrapped.drain(..cut);
        wrapped.insert(0, Line::styled(format!("… ({cut} earlier lines)"), dim()));
    }
    while wrapped.len() < height {
        wrapped.push(Line::raw(""));
    }
    let area = centered(screen, width, height as u16 + 2);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(Line::styled(format!(" {title} "), bold()));
    frame.render_widget(Paragraph::new(wrapped).block(block), area);
    area
}

/// The first two shortcuts of `a / b / c`: the help shows the main ones,
/// the F9 list has all of them.
fn first_keys(keys: &str) -> String {
    keys.split(" / ").take(2).collect::<Vec<_>>().join(" / ")
}

fn draw_help(frame: &mut Frame, app: &App) {
    let groups = app.context().keymap.help();
    let screen = frame.area();
    // As wide as the screen allows (up to 100), so labels are seldom cut.
    let width = screen.width.clamp(20, 100);
    let column_width = (width as usize).saturating_sub(4) / 2;
    let column = |groups: &[(Category, Vec<(String, &'static str)>)]| -> Vec<Line<'static>> {
        // The keys column is as wide as its longest keys, within a share
        // of the column, so a short column does not waste room.
        let longest = groups
            .iter()
            .flat_map(|(_, rows)| rows)
            .map(|(k, _)| first_keys(k).chars().count())
            .max();
        let key_width = longest.unwrap_or(0).min(column_width / 2).max(4);
        let label_width = column_width.saturating_sub(key_width + 5);
        let mut lines = Vec::new();
        for (category, rows) in groups {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(category.title().to_owned(), bold().fg(Color::Cyan)));
            for (keys, label) in rows {
                let label = fit(label, label_width);
                lines.push(Line::raw(format!(
                    "  {} {}",
                    fit(&first_keys(keys), key_width),
                    label.trim_end()
                )));
            }
        }
        lines
    };
    // Split the groups into two columns of similar height.
    let total: usize = groups.iter().map(|(_, rows)| rows.len() + 2).sum();
    let mut split = 0;
    let mut running = 0;
    for (i, (_, rows)) in groups.iter().enumerate() {
        if running >= total / 2 {
            break;
        }
        running += rows.len() + 2;
        split = i + 1;
    }
    let (first, second) = groups.split_at(split);
    let mut left = column(first);
    let mut right = column(second);
    // On a short screen the lists end in "…" instead of running over the
    // border; every command stays reachable in the F9 list.
    let room = screen.height.saturating_sub(4) as usize;
    for lines in [&mut left, &mut right] {
        if lines.len() > room && room > 0 {
            lines.truncate(room - 1);
            lines.push(Line::styled("  …", dim()));
        }
    }
    let height = left.len().max(right.len()) as u16 + 4;
    let area = centered(screen, width, height);
    frame.render_widget(Clear, area);
    let hint = shrink_hint(nmc_frontend::hints::HELP, area.width.saturating_sub(4) as usize);
    let block = Block::bordered()
        .title(Line::styled(" Main shortcuts ", bold()))
        .title_bottom(Line::styled(format!(" {hint} "), dim()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [a, b] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .areas(inner.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 }));
    frame.render_widget(Paragraph::new(left), a);
    frame.render_widget(Paragraph::new(right), b);
}

/// Rows of the command list window, borders included (it never resizes).
const COMMANDS_HEIGHT: u16 = 24;

/// The command list: category buttons, what was typed, then the commands
/// as `Category/Command` with their shortcuts on the right.
fn draw_commands(frame: &mut Frame, app: &App, palette: &CommandPalette) {
    let view = commands_view(app, palette);
    let area = frame.area();
    let width = 96.min(area.width.saturating_sub(4)).max(20);
    // A fixed height: the window must not grow and shrink as the
    // typed filter narrows the list.
    let height = COMMANDS_HEIGHT.min(area.height.saturating_sub(2)).max(8);
    let rect = Rect { y: area.y + 1, ..centered(area, width, height) };
    frame.render_widget(Clear, rect);
    let block =
        Block::bordered().title(Line::styled(" Commands ", bold())).title_bottom(Line::styled(
            format!(" {} ", shrink_hint(&view.hint, rect.width.saturating_sub(6) as usize)),
            dim(),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut buttons = Vec::new();
    for (i, name) in view.categories.iter().enumerate() {
        let text = format!(" {name} ");
        buttons.push(if i == view.category {
            Span::styled(text, highlight())
        } else {
            Span::raw(text)
        });
        buttons.push(Span::raw(" "));
    }
    let typed = match &view.recording {
        Some(name) => Line::styled(format!("Press the new shortcut for {name}"), bold()),
        None => Line::from(vec![Span::styled("> ", dim()), Span::raw(format!("{}_", view.filter))]),
    };
    let width = inner.width as usize;
    let visible = inner.height.saturating_sub(3) as usize;
    let mut lines = vec![Line::from(buttons), typed, Line::raw("")];
    // F1: what the highlighted command does, in up to three lines.
    let help = wrap_text(&view.help, width.saturating_sub(2), 3);
    let visible = visible.saturating_sub(if help.is_empty() { 0 } else { help.len() + 1 });
    let skip = (view.item + 1).saturating_sub(visible);
    if view.rows.is_empty() {
        lines.push(Line::styled(" Nothing matches", dim()));
    }
    for (i, row) in view.rows.iter().enumerate().skip(skip).take(visible) {
        let keys_width = row.keys.chars().count();
        let name = fit(&row.name, width.saturating_sub(keys_width + 3));
        let gap = width.saturating_sub(name.chars().count() + keys_width + 2);
        let selected = i == view.item;
        let text_style = if selected { highlight() } else { Style::new() };
        let keys_style = if selected { highlight() } else { Style::new().fg(Color::Cyan) };
        lines.push(Line::from(vec![
            Span::styled(format!(" {name}{}", " ".repeat(gap)), text_style),
            Span::styled(format!("{} ", row.keys), keys_style),
        ]));
    }
    if !help.is_empty() {
        lines.push(Line::raw(""));
        lines.extend(
            help.into_iter().map(|l| Line::styled(format!(" {l}"), Style::new().fg(Color::Yellow))),
        );
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// `text` broken into lines of at most `width` characters at spaces, at
/// most `rows` of them (the last one ends in `…` when the text goes on).
fn wrap_text(text: &str, width: usize, rows: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    if lines.len() > rows {
        lines.truncate(rows);
        if let Some(last) = lines.last_mut() {
            *last = fit(&format!("{last} …"), width);
        }
    }
    lines
}

/// The visible part of a text field and the cursor column within it.
pub fn field_window(field: &TextField, width: usize) -> (String, usize) {
    let chars: Vec<char> = field.text().chars().collect();
    let width = width.max(1);
    let start = (field.cursor() + 1).saturating_sub(width);
    let visible: String = chars.iter().skip(start).take(width).collect();
    (visible, field.cursor() - start)
}

/// `text` cut into lines of at most `width` characters, at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn draw_input(frame: &mut Frame, dialog: &InputDialog, hint: &str) {
    let screen = frame.area();
    let width = box_width(screen);
    let inner_width = width.saturating_sub(2) as usize;
    // The prompt may need several lines. The lines under the field have a
    // reserved room, so the window keeps its size while they change.
    let prompt: Vec<String> =
        wrap(&dialog.prompt, inner_width).iter().flat_map(|l| hard_wrap(l, inner_width)).collect();
    let reserved = dialog.purpose.preview_rows();
    let preview: Vec<String> = dialog
        .preview
        .iter()
        .flat_map(|l| hard_wrap(l, inner_width))
        .take(reserved.max(1))
        .collect();
    // A path longer than the field is shown whole below it.
    let field_width = inner_width.saturating_sub(2);
    let overflow = dialog.shown().text().chars().count() > field_width;
    let full: Vec<String> = if overflow {
        hard_wrap(dialog.shown().text(), inner_width.saturating_sub(2))
    } else {
        Vec::new()
    };
    let full_rows = if dialog.purpose.is_path() { full.len().max(1) } else { 0 };
    let preview_rows = reserved + 1;
    let height = (5 + prompt.len() + full_rows + preview_rows) as u16;
    let height = height.min(screen.height.saturating_sub(2));
    let area = centered(screen, width, height);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(Line::styled(format!(" {} ", dialog.title), bold()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let (visible, column) = field_window(&dialog.shown(), field_width);
    let mut lines: Vec<Line> = prompt.iter().map(|l| Line::raw(l.clone())).collect();
    lines
        .push(Line::styled(format!(" {visible:<field_width$} "), Style::new().bg(Color::DarkGray)));
    if full_rows > 0 {
        for i in 0..full_rows {
            let row = full.get(i).cloned().unwrap_or_default();
            lines.push(Line::styled(format!(" {row}"), dim()));
        }
    }
    lines.push(Line::raw(""));
    for i in 0..reserved {
        lines.push(Line::raw(preview.get(i).cloned().unwrap_or_default()));
    }
    lines.push(Line::raw(""));
    for row in hard_wrap(hint, inner_width) {
        lines.push(Line::styled(row, dim()));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    frame.set_cursor_position(Position {
        x: inner.x + 1 + column as u16,
        y: inner.y + prompt.len() as u16,
    });
}

/// Draws a list dialog; returns its page height.
fn draw_choose(frame: &mut Frame, dialog: &ChooseDialog, footer: &str) -> usize {
    let area = frame.area();
    let longest = dialog.items.iter().map(|i| i.chars().count()).max().unwrap_or(20);
    let filtering = dialog.keeps_size();
    // A list that is filtered while typing keeps one size whatever is left
    // in it; others fit their rows, but never less than half the screen.
    let width = if filtering {
        box_width(area) as usize
    } else {
        (longest + 4)
            .max(dialog.title.chars().count() + 6)
            .max(footer.chars().count() + 4)
            .max(40)
            .max(area.width as usize / 2)
    }
    .min(area.width.saturating_sub(2) as usize) as u16;
    let room = area.height.saturating_sub(2);
    let height = if filtering {
        (area.height * 7 / 10).max(8).min(room)
    } else {
        (dialog.items.len() as u16 + 3).clamp(5, room)
    };
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let footer = shrink_hint(footer, rect.width.saturating_sub(4) as usize);
    let block = Block::bordered()
        .title(Line::styled(format!(" {} ", dialog.title), bold()))
        .title_bottom(Line::styled(format!(" {footer} "), dim()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let rows = inner.height as usize;
    let offset = (dialog.cursor + 1).saturating_sub(rows);
    let lines: Vec<Line> = if dialog.items.is_empty() {
        vec![Line::styled("(nothing yet)", dim())]
    } else {
        dialog
            .items
            .iter()
            .enumerate()
            .skip(offset)
            .take(rows)
            .map(|(i, item)| {
                let text = fit(item, inner.width as usize);
                if i == dialog.cursor { Line::styled(text, highlight()) } else { Line::raw(text) }
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), inner);
    rows
}

/// Draws the full-screen viewer; returns its page height.
fn draw_viewer(frame: &mut Frame, viewer: &Viewer, area: Rect) -> usize {
    frame.render_widget(Clear, area);
    let view = nmc_frontend::view::viewer_view(viewer, area.height.saturating_sub(2) as usize);
    let mut title = format!(" {} [{}] ", view.title, view.mode);
    if !viewer.shows_image() {
        title.push_str(&format!("{}% ", view.percent));
    }
    if view.truncated {
        title.push_str("(first 16 MiB) ");
    }
    let mut block = Block::bordered()
        .title(Line::styled(title, bold()))
        .title_bottom(Line::styled(format!(" {} ", view.keys), dim()));
    if !view.info.is_empty() {
        block = block.title_bottom(Line::styled(format!(" {} ", view.info), dim()).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    draw_content(frame, viewer, inner);
    inner.height as usize
}

/// Draws what a viewer shows (text, hex, a document or a picture) in `area`.
fn draw_content(frame: &mut Frame, viewer: &Viewer, area: Rect) {
    let page = area.height as usize;
    if let Some(error) = &viewer.error {
        let line = Line::styled(error.clone(), Style::new().fg(Color::Red));
        frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: false }), area);
        return;
    }
    if viewer.data.is_none() {
        frame.render_widget(Paragraph::new(Line::styled("loading…", dim())), area);
        return;
    }
    if viewer.shows_image() {
        match &viewer.image {
            Some(picture) => draw_image(frame, picture, viewer.zoom, area),
            None => frame.render_widget(Paragraph::new(Line::styled("drawing…", dim())), area),
        }
        return;
    }
    if viewer.rich.is_some() && !viewer.hex {
        let lines: Vec<Line> = viewer.visible_rich(page).iter().map(rich_line).collect();
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }
    let tinted = viewer.visible_tinted(page);
    let lines: Vec<Line> = if tinted.is_empty() {
        viewer.visible_rows(page).into_iter().map(Line::raw).collect()
    } else {
        tinted.iter().map(rich_line).collect()
    };
    let mut paragraph = Paragraph::new(lines);
    if viewer.wrap && !viewer.hex {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }
    frame.render_widget(paragraph, area);
}

/// A line of a formatted document in terminal colours.
fn rich_line(line: &nmc_preview::rich::Line) -> Line<'static> {
    Line::from(
        line.iter()
            .map(|span| Span::styled(span.text.clone(), rich_style(&span.style)))
            .collect::<Vec<_>>(),
    )
}

/// How a document style looks in a terminal.
fn rich_style(style: &nmc_preview::rich::Style) -> Style {
    use nmc_preview::rich::Tone;
    let mut out = Style::new();
    match style.heading {
        0 => {}
        1 => out = out.fg(Color::Yellow).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => out = out.fg(Color::Yellow).add_modifier(Modifier::BOLD),
        _ => out = out.fg(Color::Cyan).add_modifier(Modifier::BOLD),
    }
    if style.bold {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.code {
        out = out.fg(Color::Green);
    }
    if style.link {
        out = out.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
    }
    if style.mark {
        out = out.fg(Color::DarkGray);
    }
    if style.removed {
        out = out.fg(Color::Red);
    }
    match style.tone {
        Tone::Plain => {}
        Tone::Keyword => out = out.fg(Color::Magenta),
        Tone::Str => out = out.fg(Color::Green),
        Tone::Number => out = out.fg(Color::Yellow),
        Tone::Comment => out = out.fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        Tone::Name => out = out.fg(Color::Cyan),
    }
    out
}

/// Draws the zoomed part of a picture centered in `area` with half-block
/// characters.
fn draw_image(frame: &mut Frame, picture: &image::RgbaImage, zoom: Zoom, area: Rect) {
    let (x, y, w, h) = zoom.region(picture.width(), picture.height());
    let cropped;
    let shown = if (w, h) == picture.dimensions() {
        picture
    } else {
        cropped = image::imageops::crop_imm(picture, x, y, w, h).to_image();
        &cropped
    };
    let cells = nmc_frontend::viewer::image_cells(shown, area.width, area.height);
    let height = cells.len() as u16;
    let width = cells.first().map_or(0, |row| row.len() as u16);
    let left = area.x + (area.width - width) / 2;
    let top = area.y + (area.height - height) / 2;
    let buffer = frame.buffer_mut();
    for (y, row) in cells.iter().enumerate() {
        for (x, (upper, lower)) in row.iter().enumerate() {
            let cell = &mut buffer[(left + x as u16, top + y as u16)];
            cell.set_char('▀');
            cell.set_fg(Color::Rgb(upper[0], upper[1], upper[2]));
            cell.set_bg(Color::Rgb(lower[0], lower[1], lower[2]));
        }
    }
}

/// Draws the settings screen (Ctrl+,): options, shortcuts and commands.
fn draw_settings(frame: &mut Frame, app: &App, screen: &nmc_frontend::settings::SettingsScreen) {
    let area = frame.area();
    let rect =
        centered(area, area.width.saturating_sub(4).min(100), area.height.saturating_sub(2).max(8));
    frame.render_widget(Clear, rect);
    let rows = rect.height.saturating_sub(2) as usize;
    let view = nmc_frontend::view::settings_view(app, screen, rows);
    let hint_style = if view.recording { Style::new().fg(Color::Yellow) } else { dim() };
    let block = Block::bordered()
        .title(Line::styled(format!(" Settings {}% ", view.percent), bold()))
        .title_bottom(Line::styled(format!(" {} ", view.hint), hint_style));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let width = inner.width as usize;
    let value_width = (width / 3).clamp(12, 40);
    let label_width = width.saturating_sub(value_width + 3);
    let lines: Vec<Line> = view
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            if row.kind == "header" {
                return Line::styled(
                    fit(&row.label, width),
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                );
            }
            let mark = if row.changed { "*" } else { " " };
            let text = format!(
                " {} {mark} {}",
                fit(&row.label, label_width),
                fit(&row.value, value_width)
            );
            let style = if i == view.cursor {
                highlight()
            } else if row.kind == "command" {
                Style::new().fg(Color::Cyan)
            } else {
                Style::new()
            };
            Line::styled(text, style)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_jobs(frame: &mut Frame, app: &App, cursor: usize) {
    let area = frame.area();
    let rect =
        centered(area, area.width.saturating_sub(6).max(40), area.height.saturating_sub(4).max(8));
    frame.render_widget(Clear, rect);
    let block = Block::bordered().title(Line::styled(" Transfer queue ", bold())).title_bottom(
        Line::styled(
            format!(
                " {} ",
                shrink_hint(nmc_frontend::hints::JOBS, rect.width.saturating_sub(6) as usize)
            ),
            dim(),
        ),
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let lines: Vec<Line> = if app.jobs.is_empty() {
        vec![Line::styled("No transfers", dim())]
    } else {
        let mut lines = Vec::new();
        // Three rows a job; the highlighted one stays in sight.
        let visible = (inner.height as usize / 3).max(1);
        let offset = (cursor + 1).saturating_sub(visible);
        for (i, job) in app.jobs.iter().enumerate().skip(offset) {
            let text = fit(&job_line(job, app), inner.width as usize);
            let width = inner.width as usize;
            let route = fit(&format!("      {}", job_route(job, app)), width);
            let how = fit(&format!("      {}", job_how(job, app)), width);
            let style = match job.state {
                JobState::Failed => Style::new().fg(Color::Red),
                JobState::Completed | JobState::Cancelled => dim(),
                _ => Style::new(),
            };
            let (first, rest) = if i == cursor {
                (style.add_modifier(Modifier::REVERSED), style.add_modifier(Modifier::REVERSED))
            } else {
                (style, dim())
            };
            lines.push(Line::styled(text, first));
            lines.push(Line::styled(route, rest));
            lines.push(Line::styled(how, rest));
        }
        lines
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_pads_and_cuts() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("città", 5), "città");
        assert_eq!(fit("x", 0), "");
    }

    #[test]
    fn centered_rect_stays_inside() {
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(centered(area, 20, 10), Rect::new(30, 7, 20, 10));
        assert_eq!(centered(area, 200, 100), area);
    }

    #[test]
    fn field_window_follows_the_cursor() {
        let field = TextField::new("abcdefghij");
        assert_eq!(field_window(&field, 4), ("hij".to_owned(), 3));
        let mut field = TextField::new("abcdefghij");
        field.set_cursor(0);
        assert_eq!(field_window(&field, 4), ("abcd".to_owned(), 0));
    }
}
