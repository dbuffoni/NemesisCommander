//! Keyboard input: terminal key events become [`KeyPress`]es, and
//! [`TextField`] is the one-line editor used by every dialog.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use sc_config::{Key, KeyChord};

/// One key press as the application sees it.
///
/// `chord` is what the key map looks up; `text` is the character to insert
/// when a text field has the focus (`None` for keys that do not type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    /// The normalized chord (`Ctrl+S`, `F5`, `Shift+F6`, ...).
    pub chord: KeyChord,
    /// The typed character, exactly as entered (`A`, `+`, `é`).
    pub text: Option<char>,
}

impl KeyPress {
    /// A key without modifiers (`F5`, `Enter`, ...).
    pub fn key(key: Key) -> Self {
        KeyPress { chord: KeyChord::plain(key), text: None }
    }

    /// A chord described in key-map syntax, e.g. `"Ctrl+S"`.
    ///
    /// # Panics
    /// If `text` is not a valid chord; meant for tests and fixed tables.
    pub fn chord(text: &str) -> Self {
        let chord: KeyChord = text.parse().expect("valid key chord");
        KeyPress { chord, text: None }
    }

    /// A typed character.
    pub fn char(c: char) -> Self {
        let key = if c == ' ' { Key::Space } else { Key::Char(c) };
        let shift = c.is_uppercase();
        KeyPress { chord: KeyChord::normalized(key, false, false, shift), text: Some(c) }
    }

    /// Converts a terminal key event. Returns `None` for key releases and
    /// keys the application does not use.
    pub fn from_event(event: &KeyEvent) -> Option<Self> {
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

    /// The key without modifiers, if no modifier is held.
    pub fn plain(&self) -> Option<Key> {
        let KeyChord { ctrl, alt, shift, key } = self.chord;
        (!ctrl && !alt && !shift).then_some(key)
    }
}

/// A one-line text editor with a cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    /// Cursor position in characters (not bytes).
    cursor: usize,
}

impl TextField {
    /// A field holding `text`, with the cursor at the end.
    pub fn new(text: &str) -> Self {
        TextField { text: text.to_owned(), cursor: text.chars().count() }
    }

    /// The current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Cursor position in characters.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Puts the cursor at character `position` (clamped).
    pub fn set_cursor(&mut self, position: usize) {
        self.cursor = position.min(self.text.chars().count());
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map_or(self.text.len(), |(i, _)| i)
    }

    /// Applies an editing key. Returns `true` if the text changed.
    pub fn handle(&mut self, press: &KeyPress) -> bool {
        if let Some(c) = press.text {
            let at = self.byte_index(self.cursor);
            self.text.insert(at, c);
            self.cursor += 1;
            return true;
        }
        let chord = press.chord;
        if chord.ctrl && chord.key == Key::Char('u') {
            let changed = !self.text.is_empty();
            self.text.clear();
            self.cursor = 0;
            return changed;
        }
        match chord.key {
            Key::Backspace if self.cursor > 0 => {
                let at = self.byte_index(self.cursor - 1);
                self.text.remove(at);
                self.cursor -= 1;
                true
            }
            Key::Delete if self.cursor < self.text.chars().count() => {
                let at = self.byte_index(self.cursor);
                self.text.remove(at);
                true
            }
            Key::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                false
            }
            Key::Right => {
                self.set_cursor(self.cursor + 1);
                false
            }
            Key::Home => {
                self.cursor = 0;
                false
            }
            Key::End => {
                self.cursor = self.text.chars().count();
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn terminal_events_become_chords() {
        let press = KeyPress::from_event(&event(KeyCode::F(5), KeyModifiers::NONE)).unwrap();
        assert_eq!(press.chord.to_string(), "F5");
        assert_eq!(press.text, None);

        let press =
            KeyPress::from_event(&event(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(press.chord.to_string(), "Ctrl+S");
        assert_eq!(press.text, None, "control keys do not type");

        let press = KeyPress::from_event(&event(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(press.chord.to_string(), "Shift+A");
        assert_eq!(press.text, Some('A'));

        let press = KeyPress::from_event(&event(KeyCode::Char('+'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(press.chord.to_string(), "+");

        let press = KeyPress::from_event(&event(KeyCode::BackTab, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(press.chord.to_string(), "Shift+Tab");
    }

    #[test]
    fn releases_are_ignored() {
        let mut release = event(KeyCode::Enter, KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(KeyPress::from_event(&release), None);
    }

    #[test]
    fn helpers_match_terminal_events() {
        let typed = KeyPress::from_event(&event(KeyCode::Char('x'), KeyModifiers::NONE)).unwrap();
        assert_eq!(KeyPress::char('x'), typed);
        assert_eq!(KeyPress::chord("Ctrl+S").chord.to_string(), "Ctrl+S");
        assert_eq!(KeyPress::char(' ').chord.key, Key::Space);
    }

    #[test]
    fn text_field_edits_by_character() {
        let mut field = TextField::new("héllo");
        assert_eq!(field.cursor(), 5);
        field.handle(&KeyPress::key(Key::Backspace));
        assert_eq!(field.text(), "héll");
        field.handle(&KeyPress::key(Key::Home));
        field.handle(&KeyPress::key(Key::Right));
        field.handle(&KeyPress::key(Key::Delete));
        assert_eq!(field.text(), "hll");
        field.handle(&KeyPress::char('E'));
        assert_eq!(field.text(), "hEll");
        assert!(field.handle(&KeyPress::chord("Ctrl+U")));
        assert_eq!(field.text(), "");
        assert!(!field.handle(&KeyPress::key(Key::Backspace)));
    }
}
