//! The command list (F9): finding a command, running it, and giving it a
//! shortcut.

use pnxr_config::{Action, Key};
use pnxr_panel::SideId;

use super::App;
use crate::commands::CommandPalette;
use crate::dialogs::Overlay;
use crate::input::KeyPress;
use crate::settings::{assign_key_to, clear_keys_of};

impl App {
    /// Opens the command list.
    pub(super) fn open_commands(&mut self) {
        self.overlay = Some(Overlay::Commands(CommandPalette::new(self.commands())));
    }

    /// Handles a key in the command list; returns it to keep it open.
    pub(super) fn palette_key(
        &mut self,
        mut palette: CommandPalette,
        press: KeyPress,
    ) -> Option<Overlay> {
        let chord = press.chord;
        let key = press.plain();
        if palette.recording {
            palette.recording = false;
            if key != Some(Key::Esc) {
                self.give_key(&palette, press);
            }
            return Some(Overlay::Commands(palette));
        }
        let typed = press.text.filter(|_| !chord.ctrl && !chord.alt);
        match (key, typed) {
            // F1 explains the highlighted command; F1 or Esc hides it again.
            (Some(Key::F(1)), _) => palette.help = !palette.help,
            (Some(Key::Esc), _) if palette.help => palette.help = false,
            (Some(Key::Esc), _) if !palette.filter.is_empty() => palette.clear_filter(),
            (Some(Key::Esc | Key::F(9) | Key::F(10)), _) => return None,
            (Some(Key::Left), _) => palette.move_category(-1),
            (Some(Key::Right | Key::Tab), _) => palette.move_category(1),
            (Some(Key::Up), _) => palette.move_item(-1),
            (Some(Key::Down), _) => palette.move_item(1),
            (Some(Key::PageUp), _) => palette.move_item(-10),
            (Some(Key::PageDown), _) => palette.move_item(10),
            (Some(Key::Home), _) => palette.move_item(-10_000),
            (Some(Key::End), _) => palette.move_item(10_000),
            (Some(Key::Backspace), _) => palette.erase(),
            (Some(Key::Delete), _) => self.remove_keys(&palette),
            (Some(Key::Enter), _) => return self.run_selected(palette),
            _ if chord.shift && !chord.ctrl && !chord.alt && chord.key == Key::Tab => {
                palette.move_category(-1)
            }
            _ if chord.ctrl && !chord.alt && chord.key == Key::Char('u') => palette.clear_filter(),
            _ if chord.ctrl && !chord.alt && chord.key == Key::Char('k') => {
                match palette.selected() {
                    Some(entry) if entry.can_have_keys() => palette.recording = true,
                    Some(entry) => {
                        self.set_error(format!("{} cannot have a shortcut", entry.name()))
                    }
                    None => {}
                }
            }
            (_, Some(' ')) if palette.filter.is_empty() => {}
            (_, Some(c)) => palette.type_char(c),
            _ => {}
        }
        Some(Overlay::Commands(palette))
    }

    /// A click in the command list: on category button `index`
    /// (`"category"`) or on listed row `index` (`"command"`, which runs it).
    pub fn pick(&mut self, what: &str, index: usize) {
        // A click on the drive above a panel: that panel takes the list.
        if what == "drive" {
            let side = if index == 0 { SideId::Left } else { SideId::Right };
            self.workspace.set_active(side);
            self.perform(Action::Drives);
            return;
        }
        let Some(Overlay::Commands(mut palette)) = self.overlay.take() else {
            return;
        };
        match what {
            "category" => palette.set_category(index),
            "command" if index < palette.shown().len() => {
                palette.set_item(index);
                self.overlay = self.run_selected(palette);
                return;
            }
            _ => {}
        }
        self.overlay = Some(Overlay::Commands(palette));
    }

    /// Runs the highlighted command; the list stays open when none is.
    fn run_selected(&mut self, palette: CommandPalette) -> Option<Overlay> {
        let Some(entry) = palette.selected().cloned() else {
            return Some(Overlay::Commands(palette));
        };
        self.run_entry(entry.target);
        // A command may open a window of its own; that one stays.
        self.overlay.take()
    }

    /// Gives the pressed key to the highlighted command.
    fn give_key(&mut self, palette: &CommandPalette, press: KeyPress) {
        let Some(entry) = palette.selected() else { return };
        let previous = assign_key_to(&mut self.context.config, &entry.id, press.chord, false);
        let mut message = format!("{}: {}", entry.name(), press.chord);
        if let Some(other) = previous {
            message.push_str(&format!(" (taken from {})", self.command_label(&other)));
        }
        self.settings_changed(message);
    }

    /// Removes the shortcuts of the highlighted command.
    fn remove_keys(&mut self, palette: &CommandPalette) {
        let Some(entry) = palette.selected().filter(|e| e.can_have_keys()) else { return };
        clear_keys_of(&mut self.context.config, &entry.id);
        self.settings_changed(format!("{} has no shortcut now", entry.name()));
    }
}
