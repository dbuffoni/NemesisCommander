//! Terminal key events (crossterm) as [`KeyPress`]es.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use pnxr_config::{Key, KeyChord};
use pnxr_frontend::KeyPress;

/// Converts a terminal key event. Returns `None` for key releases and keys
/// the application does not use.
pub fn key_press(event: &KeyEvent) -> Option<KeyPress> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let alt = event.modifiers.contains(KeyModifiers::ALT);
    let mut shift = event.modifiers.contains(KeyModifiers::SHIFT);
    let mut text = None;
    let key = match event.code {
        KeyCode::Char(' ') => {
            text = Some(' ');
            Key::Space
        }
        KeyCode::Char(c) => {
            text = Some(c);
            Key::Char(c)
        }
        KeyCode::F(n) => Key::F(n),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => {
            shift = true;
            Key::Tab
        }
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        _ => return None,
    };
    if ctrl || alt {
        text = None;
    }
    Some(KeyPress { chord: KeyChord::normalized(key, ctrl, alt, shift), text })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn terminal_events_become_chords() {
        let press = key_press(&event(KeyCode::F(5), KeyModifiers::NONE)).unwrap();
        assert_eq!(press.chord.to_string(), "F5");
        assert_eq!(press.text, None);

        let press = key_press(&event(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(press.chord.to_string(), "Ctrl+S");
        assert_eq!(press.text, None, "control keys do not type");

        let press = key_press(&event(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(press.chord.to_string(), "Shift+A");
        assert_eq!(press.text, Some('A'));

        let press = key_press(&event(KeyCode::Char('+'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(press.chord.to_string(), "+");

        let press = key_press(&event(KeyCode::BackTab, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(press.chord.to_string(), "Shift+Tab");
    }

    #[test]
    fn releases_are_ignored() {
        let mut release = event(KeyCode::Enter, KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(key_press(&release), None);
    }

    #[test]
    fn helpers_match_terminal_events() {
        let typed = key_press(&event(KeyCode::Char('x'), KeyModifiers::NONE)).unwrap();
        assert_eq!(KeyPress::char('x'), typed);
    }
}
