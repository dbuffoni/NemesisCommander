//! Qt key events as [`KeyPress`]es.
//!
//! QML hands over `event.key`, `event.text` and `event.modifiers` unchanged;
//! the numbers are the ones of `Qt::Key` and `Qt::KeyboardModifier`.

use pnxr_config::{Key, KeyChord};
use pnxr_frontend::KeyPress;

/// `Qt::ShiftModifier`.
pub const SHIFT: i32 = 0x0200_0000;
/// `Qt::ControlModifier` (the Command key on macOS).
pub const CONTROL: i32 = 0x0400_0000;
/// `Qt::AltModifier`.
pub const ALT: i32 = 0x0800_0000;

/// First `Qt::Key` value that is not a character.
const SPECIAL: i32 = 0x0100_0000;
const F1: i32 = 0x0100_0030;
const F24: i32 = 0x0100_0047;

/// Converts a Qt key event. Returns `None` for keys the application does not
/// use, such as a modifier pressed alone.
pub fn key_press(key: i32, text: &str, modifiers: i32) -> Option<KeyPress> {
    let ctrl = modifiers & CONTROL != 0;
    let alt = modifiers & ALT != 0;
    let mut shift = modifiers & SHIFT != 0;
    let mut typed = typed_char(text);
    let key = match key {
        0x20 => {
            typed = Some(' ');
            Key::Space
        }
        0x21..SPECIAL => {
            // Prefer the typed character: it follows the keyboard layout and
            // the shift state (`é`, `+`). With Ctrl or Alt Qt types a control
            // character or nothing, so fall back to the key itself.
            let c = typed.or_else(|| {
                char::from_u32(key as u32).map(|c| c.to_lowercase().next().unwrap_or(c))
            })?;
            Key::Char(c)
        }
        F1..=F24 => Key::F((key - F1 + 1) as u8),
        0x0100_0000 => Key::Esc,
        0x0100_0001 => Key::Tab,
        0x0100_0002 => {
            shift = true;
            Key::Tab
        }
        0x0100_0003 => Key::Backspace,
        0x0100_0004 | 0x0100_0005 => Key::Enter,
        0x0100_0006 => Key::Insert,
        0x0100_0007 => Key::Delete,
        0x0100_0010 => Key::Home,
        0x0100_0011 => Key::End,
        0x0100_0012 => Key::Left,
        0x0100_0013 => Key::Up,
        0x0100_0014 => Key::Right,
        0x0100_0015 => Key::Down,
        0x0100_0016 => Key::PageUp,
        0x0100_0017 => Key::PageDown,
        _ => return None,
    };
    if ctrl || alt || !matches!(key, Key::Char(_) | Key::Space) {
        typed = None;
    }
    Some(KeyPress { chord: KeyChord::normalized(key, ctrl, alt, shift), text: typed })
}

/// The single printable character of `text`, if that is what it holds.
fn typed_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(key: i32, text: &str, modifiers: i32) -> (String, Option<char>) {
        let press = key_press(key, text, modifiers).expect("a key");
        (press.chord.to_string(), press.text)
    }

    #[test]
    fn function_and_navigation_keys() {
        assert_eq!(chord(F1, "", 0), ("F1".into(), None));
        assert_eq!(chord(F1 + 4, "", 0), ("F5".into(), None));
        assert_eq!(chord(F1 + 5, "", SHIFT), ("Shift+F6".into(), None));
        assert_eq!(chord(0x0100_0004, "\r", 0), ("Enter".into(), None), "Return");
        assert_eq!(chord(0x0100_0005, "\r", 0), ("Enter".into(), None), "keypad Enter");
        assert_eq!(chord(0x0100_0002, "", SHIFT), ("Shift+Tab".into(), None), "Backtab");
        assert_eq!(chord(0x0100_0016, "", 0), ("PageUp".into(), None));
        assert_eq!(chord(0x0100_0000, "\u{1b}", 0), ("Esc".into(), None));
    }

    #[test]
    fn characters_follow_what_was_typed() {
        assert_eq!(chord(0x41, "a", 0), ("A".into(), Some('a')));
        assert_eq!(chord(0x41, "A", SHIFT), ("Shift+A".into(), Some('A')));
        assert_eq!(chord(0x2b, "+", SHIFT), ("+".into(), Some('+')), "shift is part of +");
        assert_eq!(chord(0xc9, "é", 0), ("É".into(), Some('é')), "letters print uppercase");
        assert_eq!(chord(0x20, " ", 0), ("Space".into(), Some(' ')));
    }

    #[test]
    fn modifier_chords_do_not_type() {
        assert_eq!(chord(0x53, "\u{13}", CONTROL), ("Ctrl+S".into(), None));
        assert_eq!(chord(0x52, "", ALT), ("Alt+R".into(), None));
        assert_eq!(chord(0x52, "", CONTROL | SHIFT), ("Ctrl+Shift+R".into(), None));
    }

    #[test]
    fn modifiers_alone_are_ignored() {
        assert!(key_press(0x0100_0020, "", SHIFT).is_none(), "Shift");
        assert!(key_press(0x0100_0021, "", CONTROL).is_none(), "Control");
    }
}
