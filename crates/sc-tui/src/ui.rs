//! Drawing the [`App`] with ratatui. Nothing here changes application
//! state except the scroll offsets and page size the renderer measures.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use sc_config::{Action, Category, Key, KeyChord};
use sc_panel::format::{human_size, local_time, permissions};
use sc_panel::{PARENT, Panel, SideId};
use sc_transfer::{JobInfo, JobState};
use sc_vfs::Entry;

use crate::app::{App, display_location, side_index};
use crate::dialogs::{ChooseDialog, InputDialog, MenuState, Overlay};
use crate::input::TextField;
use crate::viewer::Viewer;

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
    let [panels, status, keys] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1), Constraint::Length(1)])
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
    draw_status(frame, app, status);
    draw_key_bar(frame, app, keys);

    if let Some(overlay) = &app.overlay {
        match overlay {
            Overlay::Help => draw_help(frame, app),
            Overlay::Menu(menu) => draw_menu(frame, app, *menu),
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
                if dialog.then.is_none() {
                    lines.push(Line::raw(""));
                    lines.push(Line::styled("Esc: close", dim()));
                }
                draw_box(frame, &dialog.title, lines);
            }
            Overlay::Viewer(viewer) => {
                let page = draw_viewer(frame, viewer, frame.area());
                app.page = page;
            }
            Overlay::Jobs(view) => draw_jobs(frame, app, view.cursor),
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

fn describe(entry: &Entry) -> String {
    let date = entry.modified.map(local_time).unwrap_or_default();
    format!("{} ({} bytes), {date}", human_size(entry.size), entry.size)
}

// ---------------------------------------------------------------- panels

fn tab_title(panel: &Panel) -> String {
    panel.location().file_name().unwrap_or("/").to_owned()
}

fn draw_panel(frame: &mut Frame, app: &mut App, side: SideId, area: Rect) {
    let is_active = app.workspace.active_side() == side;
    let side_state = app.workspace.side(side);
    let panel = side_state.panel();
    let border = if is_active { Style::new().fg(Color::Cyan) } else { dim() };

    let mut block = Block::bordered()
        .border_style(border)
        .title(Line::styled(format!(" {} ", display_location(panel.location())), bold()));
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
    let name_width = width.saturating_sub(SIZE_WIDTH + 1).saturating_sub(if show_date {
        DATE_WIDTH + 1
    } else {
        0
    });
    let header = format!(
        "{:<name_width$} {:>SIZE_WIDTH$}{}",
        "Name",
        "Size",
        if show_date { format!(" {:<DATE_WIDTH$}", "Modified") } else { String::new() }
    );
    let rows = inner.height as usize - 1;
    let index = side_index(side);
    let cursor = panel.cursor();
    let mut offset = app.scroll[index];
    if cursor < offset {
        offset = cursor;
    } else if cursor >= offset + rows {
        offset = cursor + 1 - rows;
    }
    offset = offset.min(panel.entries().len().saturating_sub(rows));
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
        let size = if entry.name == PARENT {
            "UP".to_owned()
        } else if entry.is_dir() {
            "<DIR>".to_owned()
        } else if app.context().config.panels.human_sizes {
            human_size(entry.size)
        } else {
            entry.size.to_string()
        };
        let date = if show_date {
            format!(" {:<DATE_WIDTH$}", entry.modified.map(local_time).unwrap_or_default())
        } else {
            String::new()
        };
        let text = format!("{} {:>SIZE_WIDTH$}{date}", fit(&name, name_width), size);

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
        lines.push(Line::styled(text, style));
    }
    if panel.is_loading() && panel.entries().is_empty() {
        lines.push(Line::styled("loading…", dim()));
    }
    frame.render_widget(Paragraph::new(lines), inner);
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

fn panel_footer(panel: &Panel) -> String {
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
        text.push_str(&format!("  filter: {}", panel.filter()));
    }
    if panel.is_loading() {
        text.push_str("  loading…");
    }
    text
}

fn draw_quick_view(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().border_style(dim()).title(" Quick view ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines: Vec<Line> = match &app.quick_view {
        Some(Some(viewer)) => match (&viewer.error, &viewer.data) {
            (Some(error), _) => vec![Line::styled(error.clone(), Style::new().fg(Color::Red))],
            (None, None) => vec![Line::styled("loading…", dim())],
            (None, Some(_)) => {
                viewer.visible_rows(inner.height as usize).into_iter().map(Line::raw).collect()
            }
        },
        _ => vec![Line::styled("Move the cursor to a file to preview it", dim())],
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

// ------------------------------------------------------- status and keys

/// `done` as a percentage of `total` (`None` when the total is unknown).
fn percent(done: u64, total: u64) -> Option<u64> {
    (done.min(total) * 100).checked_div(total)
}

fn jobs_summary(jobs: &[JobInfo], app: &App) -> Option<String> {
    let active: Vec<&JobInfo> =
        jobs.iter().filter(|j| matches!(j.state, JobState::Queued | JobState::Running)).collect();
    if active.is_empty() {
        return None;
    }
    let total: u64 = active.iter().map(|j| j.total_bytes).sum();
    let done: u64 =
        active.iter().map(|j| j.done_bytes + app.live_bytes.get(&j.id).copied().unwrap_or(0)).sum();
    let percent = percent(done, total).map_or("…".into(), |p| format!("{p}%"));
    Some(format!("{} jobs {percent}", active.len()))
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let right = jobs_summary(&app.jobs, app).unwrap_or_default();
    let (text, style) = match app.status() {
        Some(status) if status.error => (status.text.clone(), Style::new().fg(Color::Red)),
        Some(status) => (status.text.clone(), Style::new()),
        None => (entry_details(app.workspace.active()), dim()),
    };
    let width = area.width as usize;
    let left_width = width.saturating_sub(right.chars().count() + 1);
    let line = Line::from(vec![
        Span::styled(fit(&text, left_width), style),
        Span::raw(" "),
        Span::styled(right, Style::new().fg(Color::Cyan)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn entry_details(panel: &Panel) -> String {
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
    parts.join("  ")
}

/// A short label for the function key bar.
fn short_label(action: Action) -> &'static str {
    match action {
        Action::Help => "Help",
        Action::Rename => "Rename",
        Action::View => "View",
        Action::Edit => "Edit",
        Action::Copy => "Copy",
        Action::Move => "Move",
        Action::MakeDir => "Mkdir",
        Action::Delete => "Delete",
        Action::Menu => "Menu",
        Action::Quit => "Quit",
        other => other.label().split_whitespace().next().unwrap_or("").trim_end_matches('…'),
    }
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
        spans.push(Span::raw(format!("{n}")));
        spans.push(Span::styled(
            format!("{label:<6}"),
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

fn draw_menu(frame: &mut Frame, app: &App, menu: MenuState) {
    let area = frame.area();
    let bar = Rect { height: 1, ..area };
    let mut spans = Vec::new();
    let mut x_of_open = 0u16;
    let mut x = 0u16;
    for (i, category) in Category::ALL.iter().enumerate() {
        let text = format!("  {}  ", category.title());
        if i == menu.menu {
            x_of_open = x;
            spans.push(Span::styled(text.clone(), highlight()));
        } else {
            spans.push(Span::raw(text.clone()));
        }
        x += text.chars().count() as u16;
    }
    frame.render_widget(Clear, bar);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(Color::DarkGray)),
        bar,
    );

    let items = MenuState::items(menu.menu);
    let rows: Vec<(String, String)> = items
        .iter()
        .map(|&a| (a.label().to_owned(), app.context().keymap.shortcut_text(a)))
        .collect();
    let label_width = rows.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
    let key_width = rows.iter().map(|(_, k)| k.chars().count()).max().unwrap_or(0);
    let width = (label_width + key_width + 6) as u16;
    let drop = Rect {
        x: x_of_open.min(area.width.saturating_sub(width)),
        y: 1,
        width: width.min(area.width),
        height: (rows.len() as u16 + 2).min(area.height.saturating_sub(1)),
    };
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(i, (label, keys))| {
            let text = format!(" {label:<label_width$}  {keys:>key_width$} ");
            if i == menu.item { Line::styled(text, highlight()) } else { Line::raw(text) }
        })
        .collect();
    frame.render_widget(Clear, drop);
    frame.render_widget(Paragraph::new(lines).block(Block::bordered()), drop);
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
    let area = centered(frame.area(), width, 6);
    frame.render_widget(Clear, area);
    let block = Block::bordered().title(Line::styled(format!(" {} ", dialog.title), bold()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let field_width = inner.width.saturating_sub(2) as usize;
    let (visible, column) = field_window(&dialog.field, field_width);
    let lines = vec![
        Line::raw(fit(&dialog.prompt, inner.width as usize)),
        Line::styled(format!(" {visible:<field_width$} "), Style::new().bg(Color::DarkGray)),
        Line::raw(""),
        Line::styled("Enter: OK    Esc: cancel    Ctrl+U: clear", dim()),
    ];
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
    let mode = if viewer.hex { "hex" } else { "text" };
    let rows = viewer.row_count();
    let percent = if rows == 0 { 100 } else { (viewer.scroll * 100 / rows.max(1)).min(100) };
    let mut title = format!(" {} [{mode}] {percent}% ", display_location(&viewer.path));
    if viewer.truncated {
        title.push_str("(first 16 MiB) ");
    }
    let block = Block::bordered().title(Line::styled(title, bold())).title_bottom(Line::styled(
        " Esc: close   F4 / h: hex   F2 / w: wrap   PgUp PgDn Home End ",
        dim(),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let page = inner.height as usize;
    let lines: Vec<Line> = match (&viewer.error, &viewer.data) {
        (Some(error), _) => vec![Line::styled(error.clone(), Style::new().fg(Color::Red))],
        (None, None) => vec![Line::styled("loading…", dim())],
        (None, Some(_)) => viewer.visible_rows(page).into_iter().map(Line::raw).collect(),
    };
    let mut paragraph = Paragraph::new(lines);
    if viewer.wrap && !viewer.hex {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }
    frame.render_widget(paragraph, inner);
    page
}

fn job_state(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Paused => "paused",
        JobState::Completed => "done",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
    }
}

fn draw_jobs(frame: &mut Frame, app: &App, cursor: usize) {
    let area = frame.area();
    let rect =
        centered(area, area.width.saturating_sub(6).max(40), area.height.saturating_sub(4).max(8));
    frame.render_widget(Clear, rect);
    let block = Block::bordered().title(Line::styled(" Transfer queue ", bold())).title_bottom(
        Line::styled(
            " p: pause  r: resume/retry  c: cancel  +/-: priority  x: remove finished  Esc: close ",
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
                let done = job.done_bytes + app.live_bytes.get(&job.id).copied().unwrap_or(0);
                let progress = percent(done, job.total_bytes)
                    .map_or_else(|| "   -".to_owned(), |p| format!("{p:>3}%"));
                let mut text = format!(
                    "#{:<4} {:<9} {progress} {:>9}  p{:<3} {}",
                    job.id.0,
                    job_state(job.state),
                    human_size(job.total_bytes),
                    job.spec.priority,
                    job.spec.title()
                );
                if let Some(error) = job.errors.first() {
                    text.push_str(&format!("  ({error})"));
                }
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
