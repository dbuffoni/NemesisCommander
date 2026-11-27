//! The mouse in a terminal: click a row, double-click to open it, the wheel
//! to scroll, and dragging a row onto the other panel to copy it.

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use pnxr_config::Action;
use pnxr_frontend::{App, side_index};
use pnxr_panel::SideId;
use ratatui::layout::{Constraint, Layout, Rect};

/// Two clicks on the same row closer than this open it.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Lines the wheel moves per notch.
const WHEEL_LINES: isize = 3;

/// What the mouse did last, to tell a drag from a click.
#[derive(Debug, Default)]
pub struct Pointer {
    pressed: Option<(SideId, usize)>,
    last_click: Option<(SideId, usize, Instant)>,
}

/// Which panel and which row (counted from the first listed entry) is at
/// the terminal cell `(x, y)` of a screen `screen` big. The layout is the
/// one of `ui::draw`: two bordered panels above a status and a key line;
/// inside a panel the first line is the column header.
pub fn hit(screen: Rect, x: u16, y: u16) -> Option<(SideId, Option<usize>)> {
    let [panels, _, _] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1), Constraint::Length(1)])
            .areas(screen);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(panels);
    let (side, area) = if x >= left.x && x < left.right() && y >= left.y && y < left.bottom() {
        (SideId::Left, left)
    } else if x >= right.x && x < right.right() && y >= right.y && y < right.bottom() {
        (SideId::Right, right)
    } else {
        return None;
    };
    let first_row = area.y + 2;
    let last_row = area.bottom().saturating_sub(1);
    let row = (y >= first_row && y < last_row).then(|| usize::from(y - first_row));
    Some((side, row))
}

impl Pointer {
    /// Applies a mouse event on a screen `screen` big.
    pub fn handle(&mut self, app: &mut App, event: MouseEvent, screen: Rect) {
        let Some((side, row)) = hit(screen, event.column, event.row) else {
            self.pressed = None;
            return;
        };
        // The text of pnxr_folder.md pushes the rows down.
        let row = row.and_then(|row| row.checked_sub(app.note_rows[side_index(side)]));
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(row) = row else {
                    self.pressed = None;
                    return;
                };
                app.click_row(side, row);
                self.pressed = Some((side, row));
                let now = Instant::now();
                let double = matches!(
                    self.last_click,
                    Some((s, r, at)) if s == side && r == row && now.duration_since(at) < DOUBLE_CLICK
                );
                if double {
                    self.last_click = None;
                    if app.overlay().is_none() {
                        app.perform(Action::Open);
                    }
                } else {
                    self.last_click = Some((side, row, now));
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some((from, from_row)) = self.pressed.take()
                    && from != side
                {
                    app.drag_rows(from, from_row, side);
                    self.last_click = None;
                }
            }
            MouseEventKind::ScrollUp => app.scroll_rows(side, -WHEEL_LINES),
            MouseEventKind::ScrollDown => app.scroll_rows(side, WHEEL_LINES),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_map_to_panels_and_rows() {
        let screen = Rect::new(0, 0, 80, 24);
        // The border and the column header come first.
        assert_eq!(hit(screen, 3, 0), Some((SideId::Left, None)));
        assert_eq!(hit(screen, 3, 1), Some((SideId::Left, None)));
        assert_eq!(hit(screen, 3, 2), Some((SideId::Left, Some(0))));
        assert_eq!(hit(screen, 60, 5), Some((SideId::Right, Some(3))));
        // The status and key lines are not panels.
        assert_eq!(hit(screen, 3, 22), None);
        assert_eq!(hit(screen, 3, 23), None);
        // The bottom border is not a row.
        assert_eq!(hit(screen, 3, 21), Some((SideId::Left, None)));
        assert_eq!(hit(screen, 3, 20), Some((SideId::Left, Some(18))));
    }
}
