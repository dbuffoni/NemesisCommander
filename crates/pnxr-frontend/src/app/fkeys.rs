//! Giving a command a function key and a short name (Ctrl+F in the command
//! list): one list of the 30 slots of the bottom bar (F1 to F10, with Shift
//! and with Ctrl), who has each, and the name the button shows.

use pnxr_config::{Key, KeyChord};

use super::App;
use crate::dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, InputPurpose, Overlay,
};
use crate::input::KeyPress;
use crate::settings::{assign_fkey_to, clear_fkey_of, reset_keys, set_label_of};

/// The 30 slots in list order: plain F1-F10, Shift+F1-F10, Ctrl+F1-F10.
pub fn slot(index: usize) -> KeyChord {
    let number = (index % 10) as u8 + 1;
    KeyChord::normalized(Key::F(number), index / 10 == 2, false, index / 10 == 1)
}

impl App {
    /// Opens the list of slots for the command `id` (named `name` for the
    /// user), with the cursor on the slot it has, else on the first free one.
    pub(super) fn open_function_key_dialog(&mut self, id: &str, name: &str) -> Overlay {
        let mut dialog = ChooseDialog::new(
            &format!("Function key for {name}"),
            Vec::new(),
            ChoosePurpose::FunctionKey { id: id.to_owned(), name: name.to_owned() },
        );
        dialog.items = self.function_key_rows(id);
        let keymap = &self.context.keymap;
        dialog.cursor = (0..30)
            .find(|i| keymap.fkey_for_id(id) == Some(slot(*i)))
            .or_else(|| (0..30).find(|i| keymap.command_for(&slot(*i)).is_none()))
            .unwrap_or(0);
        dialog.footer = function_key_hint();
        Overlay::Choose(dialog)
    }

    /// The rows of the slot list: the slot, the name of its button and its owner.
    fn function_key_rows(&self, id: &str) -> Vec<String> {
        let keymap = &self.context.keymap;
        (0..30)
            .map(|i| {
                let chord = slot(i);
                let text = match keymap.command_for(&chord) {
                    Some(owner) if owner == id => format!(
                        "{}  (this command)",
                        keymap.label_for_id(&owner).unwrap_or_default()
                    ),
                    Some(owner) => format!(
                        "{}  ({})",
                        keymap.label_for_id(&owner).unwrap_or_default(),
                        self.command_label(&owner)
                    ),
                    None => "free".to_owned(),
                };
                format!("{:<11}{text}", chord.to_string())
            })
            .collect()
    }

    /// Keys of the slot list besides the usual ones; `Some(true)` keeps it open.
    pub(super) fn function_key_list_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let ChoosePurpose::FunctionKey { id, name } = &dialog.purpose else { return None };
        let (id, name) = (id.clone(), name.clone());
        let chord = press.chord;
        let key = press.plain();
        if key == Some(Key::Delete) {
            clear_fkey_of(&mut self.context.config, &id);
            self.settings_changed(format!("{name} has no function key now"));
            return Some(false);
        }
        if chord.ctrl && !chord.alt && chord.key == Key::Char('n') {
            // The short name is a secondary function: the default one is
            // used until it is changed here.
            let current = self.context.keymap.label_for_id(&id).unwrap_or_default();
            self.ask(
                "Short name",
                &format!(
                    "Name of the button of {name} (at most {} characters; empty restores the built-in name):",
                    pnxr_config::LABEL_MAX
                ),
                &current,
                InputPurpose::FunctionKeyName(id),
            );
            return Some(false);
        }
        if chord.ctrl && !chord.alt && chord.key == Key::Char('r') {
            match pnxr_config::Action::from_id(&id) {
                Some(action) => {
                    reset_keys(&mut self.context.config, action);
                    self.settings_changed(format!("{name}: built-in keys and name restored"));
                }
                None => {
                    clear_fkey_of(&mut self.context.config, &id);
                    self.settings_changed(format!("{name} has no function key now"));
                }
            }
            return Some(false);
        }
        None
    }

    /// Enter on a slot: gives it to the command, after asking when another
    /// command has it.
    pub(super) fn function_key_chosen(&mut self, id: String, name: String, cursor: usize) {
        let chord = slot(cursor);
        let owner = self.context.keymap.command_for(&chord).filter(|o| *o != id);
        match owner {
            Some(owner) => {
                self.overlay = Some(Overlay::Confirm(ConfirmDialog {
                    title: "Function key".into(),
                    lines: vec![format!(
                        "{chord} belongs to {} now. Give it to {name} instead?",
                        self.command_label(&owner)
                    )],
                    purpose: ConfirmPurpose::ReplaceFunctionKey {
                        id,
                        name,
                        chord: chord.to_string(),
                    },
                }));
            }
            None => self.give_function_key(&id, &name, chord),
        }
    }

    /// Gives `chord` to the command `id` and saves.
    pub(super) fn give_function_key(&mut self, id: &str, name: &str, chord: KeyChord) {
        let previous = assign_fkey_to(&mut self.context.config, id, chord);
        let mut message = format!("{name}: {chord}");
        if let Some(other) = previous {
            message.push_str(&format!(" (taken from {})", self.command_label(&other)));
        }
        self.settings_changed(message);
    }

    /// The typed short name of the command `id`.
    pub(super) fn function_key_name_typed(&mut self, id: &str, text: &str) {
        if set_label_of(&mut self.context.config, id, text) {
            let name = self.command_label(id);
            self.settings_changed(format!("{name}: button name \"{}\"", text.trim()));
        } else {
            self.set_error(format!(
                "A short name has at most {} characters",
                pnxr_config::LABEL_MAX
            ));
        }
    }
}

/// The keys shown under the slot list.
pub fn function_key_hint() -> String {
    crate::hints::list(
        "assign",
        &["Ctrl+N: short name", "Delete: remove", "Ctrl+R: built-in"],
        "cancel",
    )
}
