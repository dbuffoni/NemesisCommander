//! Drawing the [`App`] with ratatui. Nothing here changes application
//! state except the scroll offsets and page size the renderer measures.

use nmc_config::{Category, Key, KeyChord};
use nmc_panel::format::local_time;
use nmc_panel::{PARENT, SideId};
use nmc_transfer::JobState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use nmc_frontend::commands::CommandPalette;
use nmc_frontend::drives::drive_label;
use nmc_frontend::icons::icon_for;
use nmc_frontend::preview::Zoom;
use nmc_frontend::view::{
    column_headers, commands_view, counted_size, describe, entry_details, git_mark, git_of,
    job_line, jobs_progress, jobs_summary, panel_footer, scroll_offset, short_label,
    size_text_counted, tab_title,
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
            Overlay::Input(dialog) => draw_input(frame, dialog),
            Overlay::Confirm(dialog) => {
                let mut lines: Vec<Line> =
                    dialog.lines.iter().map(|l| Line::raw(l.as_str())).collect();
                lines.push(Line::raw(""));
                lines.push(Line::styled("Enter / y: yes    Esc / n: no", dim()));
                draw_box(frame, &dialog.title, lines);
            }
            Overlay::Choose(dialog) => {
                let page = draw_choose(frame, dialog);
                app.page = page;
            }
            Overlay::Message(dialog) => {
                let mut lines: Vec<Line> =
                    dialog.lines.iter().map(|l| Line::raw(l.as_str())).collect();
                lines.push(Line::raw(""));
                lines.push(Line::styled(dialog.hint(), dim()));
                draw_box(frame, &dialog.title, lines);
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
            Line::raw("o: overwrite   s: skip   r: keep both   a / Esc: abort"),
            Line::styled("Capital letter (O, S, R): same answer for all", dim()),
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
            " [{}] {}{} ",
            drive_label(&app.context().config, panel.location()),
            git.as_ref().map(|g| format!("⎇ {}  ", g.summary())).unwrap_or_default(),
            display_location(panel.location())
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
    let name_width = width
        .saturating_sub(SIZE_WIDTH + 1 + mark_width + icon_width)
        .saturating_sub(if show_date { DATE_WIDTH + 1 } else { 0 });
    let [name_title, size_title, date_title] = column_headers(panel.sort());
    let header = format!(
        "{}{} {:>SIZE_WIDTH$}{}",
        " ".repeat(mark_width),
        fit(&name_title, name_width),
        size_title,
        if show_date { format!(" {:<DATE_WIDTH$}", date_title) } else { String::new() }
    );
    let rows = inner.height as usize - 1;
    let index = side_index(side);
    let cursor = panel.cursor();
    let offset = scroll_offset(app.scroll[index], cursor, rows, panel.entries().len());
    app.scroll[index] = offset;
    if is_active {
        app.page = rows;
    }

    let mut lines = vec![Line::styled(header, dim().add_modifier(Modifier::UNDERLINED))];
    if let Some(error) = panel.error().filter(|_| panel.entries().is_empty()) {
        lines.push(Line::styled(error.to_owned(), Style::new().fg(Color::Red)));
    }
    for (i, entry) in panel.entries().iter().enumerate().skip(offset).take(rows) {
        let mut name = entry.name.clone();
        if entry.is_dir() && entry.name != PARENT {
            name.push('/');
        }
        let counted = counted_size(app.folder_sizes(), panel.location(), entry);
        let size = size_text_counted(entry, app.context().config.panels.human_sizes, counted);
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
        let text = format!("{mark}{} {:>SIZE_WIDTH$}{date}", fit(&name, name_width), size);

        let mut style = Style::new();
        if entry.is_dir() {
            style = style.add_modifier(Modifier::BOLD);
        }
        if entry.is_hidden() {
            style = style.fg(Color::DarkGray);
        }
        if panel.is_selected(&entry.name) {
            style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
        }
        if i == cursor {
            style = if is_active {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style.add_modifier(Modifier::UNDERLINED)
            };
        }
        let line = if icon_width > 0 && entry.name != PARENT {
            let icon = icon_for(entry);
            let mut colour = style;
            if i != cursor && !panel.is_selected(&entry.name) {
                colour = colour.fg(hex_colour(icon.color));
            }
            // The icon sits after the git mark, before the name.
            let (mark_text, rest) =
                text.split_at(text.char_indices().nth(mark_width).map_or(text.len(), |(b, _)| b));
            Line::from(vec![
                Span::styled(mark_text.to_owned(), style),
                Span::styled(format!("{} ", icon.shape.glyph()), colour),
                Span::styled(rest.to_owned(), style),
            ])
        } else if icon_width > 0 {
            let (mark_text, rest) =
                text.split_at(text.char_indices().nth(mark_width).map_or(text.len(), |(b, _)| b));
            Line::styled(format!("{mark_text}  {rest}"), style)
        } else {
            Line::styled(text, style)
        };
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
        None => (entry_details(app.workspace.active()), dim()),
    };
    let line = Line::from(Span::styled(fit(&text, area.width as usize), style));
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_key_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    for n in 1..=10u8 {
        let label = app
            .context()
            .keymap
            .action_for(&KeyChord::plain(Key::F(n)))
            .map(short_label)
            .unwrap_or("");
        // With the "F" in front, ten labels fit 80 columns at five letters.
        let width = if area.width < 90 { 5 } else { 6 };
        let label = match label {
            "Commands" if width < 6 => "Cmds",
            other => other,
        };
        let label: String = label.chars().take(width).collect();
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

fn draw_box(frame: &mut Frame, title: &str, lines: Vec<Line>) {
    let content_width = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = (content_width + 4).max(title.chars().count() as u16 + 6).max(30);
    let area = centered(
        frame.area(),
        width.min(frame.area().width.saturating_sub(2)),
        lines.len() as u16 + 2,
    );
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(Line::styled(format!(" {title} "), bold()));
    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_help(frame: &mut Frame, app: &App) {
    let groups = app.context().keymap.help();
    let column = |groups: &[(Category, Vec<(String, &'static str)>)]| -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for (category, rows) in groups {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(category.title().to_owned(), bold().fg(Color::Cyan)));
            for (keys, label) in rows {
                lines.push(Line::raw(format!("  {:<16} {label}", fit(keys, 16))));
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
    let left = column(first);
    let right = column(second);
    let height = left.len().max(right.len()) as u16 + 4;
    let area = centered(frame.area(), 84, height);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::styled(" Main shortcuts ", bold()))
        .title_bottom(Line::styled(" Any key closes · F9: every command in the menu ", dim()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [a, b] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .areas(inner.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 }));
    frame.render_widget(Paragraph::new(left), a);
    frame.render_widget(Paragraph::new(right), b);
}

/// The command list: category buttons, what was typed, then the commands
/// as `Category/Command` with their shortcuts on the right.
fn draw_commands(frame: &mut Frame, app: &App, palette: &CommandPalette) {
    let view = commands_view(app, palette);
    let area = frame.area();
    let width = 96.min(area.width.saturating_sub(4)).max(20);
    let height = (view.rows.len() as u16 + 6).clamp(8, area.height.saturating_sub(2).max(8));
    let rect = Rect { y: area.y + 1, ..centered(area, width, height) };
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Line::styled(" Commands ", bold()))
        .title_bottom(Line::styled(format!(" {} ", view.hint), dim()));
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
    let skip = (view.item + 1).saturating_sub(visible);
    let mut lines = vec![Line::from(buttons), typed, Line::raw("")];
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
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The visible part of a text field and the cursor column within it.
pub fn field_window(field: &TextField, width: usize) -> (String, usize) {
    let chars: Vec<char> = field.text().chars().collect();
    let width = width.max(1);
    let start = (field.cursor() + 1).saturating_sub(width);
    let visible: String = chars.iter().skip(start).take(width).collect();
    (visible, field.cursor() - start)
}

fn draw_input(frame: &mut Frame, dialog: &InputDialog) {
    let width = 72.min(frame.area().width.saturating_sub(2));
    // The preview (multi-rename) adds a blank line and its own lines.
    let preview_rows = if dialog.preview.is_empty() { 0 } else { dialog.preview.len() + 1 };
    let height = (6 + preview_rows as u16).min(frame.area().height.saturating_sub(2));
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(Line::styled(format!(" {} ", dialog.title), bold()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let field_width = inner.width.saturating_sub(2) as usize;
    let (visible, column) = field_window(&dialog.field, field_width);
    let mut lines = vec![
        Line::raw(fit(&dialog.prompt, inner.width as usize)),
        Line::styled(format!(" {visible:<field_width$} "), Style::new().bg(Color::DarkGray)),
    ];
    if !dialog.preview.is_empty() {
        lines.push(Line::raw(""));
        lines.extend(dialog.preview.iter().map(|l| Line::raw(fit(l, inner.width as usize))));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("Enter: OK    Esc: cancel    Ctrl+U: clear", dim()));
    frame.render_widget(Paragraph::new(lines), inner);
    frame.set_cursor_position(Position { x: inner.x + 1 + column as u16, y: inner.y + 1 });
}

/// Draws a list dialog; returns its page height.
fn draw_choose(frame: &mut Frame, dialog: &ChooseDialog) -> usize {
    let area = frame.area();
    let width = (dialog.items.iter().map(|i| i.chars().count()).max().unwrap_or(20) + 4)
        .max(dialog.title.chars().count() + 6)
        .max(40)
        .min(area.width.saturating_sub(2) as usize) as u16;
    let height = (dialog.items.len() as u16 + 3).clamp(5, area.height.saturating_sub(2));
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Line::styled(format!(" {} ", dialog.title), bold()))
        .title_bottom(Line::styled(format!(" {} ", dialog.footer), dim()));
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
    let lines: Vec<Line> = viewer.visible_rows(page).into_iter().map(Line::raw).collect();
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
            " p: pause  r: resume  c: cancel  +/-: priority  [/]: files at once  a: auto  x: clear  Esc ",
            dim(),
        ),
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let lines: Vec<Line> = if app.jobs.is_empty() {
        vec![Line::styled("No transfers", dim())]
    } else {
        app.jobs
            .iter()
            .enumerate()
            .map(|(i, job)| {
                let text = job_line(job, app);
                let text = fit(&text, inner.width as usize);
                let style = match job.state {
                    JobState::Failed => Style::new().fg(Color::Red),
                    JobState::Completed | JobState::Cancelled => dim(),
                    _ => Style::new(),
                };
                if i == cursor {
                    Line::styled(text, style.add_modifier(Modifier::REVERSED))
                } else {
                    Line::styled(text, style)
                }
            })
            .collect()
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
