//! The Properties dialog (see [`crate::properties`]): opening it, its keys
//! and applying the change.

use nmc_config::Key;

use super::App;
use crate::dialogs::{ChooseDialog, ChoosePurpose, InputDialog, InputPurpose, Overlay};
use crate::input::KeyPress;
use crate::properties::{PropRow, PropsState};

impl App {
    /// Opens the Properties dialog for the selection, or the item under the
    /// cursor.
    pub(super) fn open_properties(&mut self) {
        let panel = self.workspace.active();
        let targets = panel.targets();
        if targets.is_empty() {
            return self.set_error("Nothing to show");
        }
        let entries: Vec<_> = targets
            .iter()
            .filter_map(|t| {
                let name = t.file_name()?;
                panel.entries().iter().find(|e| e.name == name).cloned()
            })
            .collect();
        if entries.len() != targets.len() {
            return self.set_error("Properties of this item are not known");
        }
        let location = super::display_location(panel.location());
        let state = PropsState::new(targets, entries, location);
        self.overlay = Some(Overlay::Choose(Self::properties_dialog(state, 0)));
    }

    /// The list for `state`, the cursor on row `cursor`.
    pub(super) fn properties_dialog(mut state: PropsState, cursor: usize) -> ChooseDialog {
        let items = state.items();
        let first =
            if state.edit { state.rows.iter().position(|r| *r != PropRow::Info) } else { None };
        let mut dialog =
            ChooseDialog::new("Properties", items, ChoosePurpose::Properties(Box::new(state)));
        let editing = matches!(&dialog.purpose, ChoosePurpose::Properties(s) if s.edit);
        dialog.cursor = if editing && cursor == 0 { first.unwrap_or(0) } else { cursor };
        dialog.footer = if editing {
            crate::hints::list(
                "choose",
                &["Space: switch a permission", "Enter: change", "Ctrl+S: apply"],
                "back to reading",
            )
        } else {
            crate::hints::list("choose", &["e or F2: edit"], "close")
        };
        dialog
    }

    /// Keys of the Properties dialog. `Some(true)` keeps it open.
    pub(super) fn properties_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let ChoosePurpose::Properties(state) = &dialog.purpose else { return None };
        let mut state = (**state).clone();
        let key = press.plain();
        let row = state.rows.get(dialog.cursor).copied().unwrap_or(PropRow::Info);
        let reopen = |state: PropsState, cursor: usize| Self::properties_dialog(state, cursor);
        if !state.edit {
            if matches!(key, Some(Key::F(2))) || press.text == Some('e') {
                state.edit = true;
                *dialog = reopen(state, 0);
                return Some(true);
            }
            return None;
        }
        let apply = (press.chord.ctrl && press.chord.key == Key::Char('s'))
            || (key == Some(Key::Enter) && row == PropRow::Apply);
        if apply {
            return Some(self.apply_properties(&state));
        }
        match (key, row) {
            (Some(Key::Esc), _) => {
                state.edit = false;
                state.mode = None;
                state.modified = None;
                state.recursive = false;
                *dialog = reopen(state, 0);
            }
            (Some(Key::Enter | Key::Space), PropRow::Bit(mask)) => {
                state.toggle_bit(mask);
                *dialog = reopen(state, dialog.cursor);
            }
            (Some(Key::Enter | Key::Space), PropRow::Recursive) => {
                state.recursive ^= true;
                *dialog = reopen(state, dialog.cursor);
            }
            (Some(Key::Enter), PropRow::Modified) => {
                let text = state.modified.clone().unwrap_or_else(|| "now".to_owned());
                self.overlay = Some(Overlay::Input(InputDialog::new(
                    "Properties",
                    "Modified: now, a date 2026-11-17, a time 10:30 or both (empty: leave as is):",
                    &text,
                    InputPurpose::PropsEdit { row, state: Box::new(state) },
                )));
                return Some(false);
            }
            (Some(Key::Enter), PropRow::Octal) => {
                let text = state.shown_mode().map(|m| format!("{m:o}")).unwrap_or_default();
                self.overlay = Some(Overlay::Input(InputDialog::new(
                    "Properties",
                    "Permissions as an octal number (644, 755, 4755):",
                    &text,
                    InputPurpose::PropsEdit { row, state: Box::new(state) },
                )));
                return Some(false);
            }
            // F1 is for the general help of the row.
            (Some(Key::F(1)), _) => return None,
            (Some(Key::Up), _) => dialog.move_cursor(-1),
            (Some(Key::Down), _) => dialog.move_cursor(1),
            (Some(Key::PageUp), _) => dialog.move_cursor(-10),
            (Some(Key::PageDown), _) => dialog.move_cursor(10),
            (Some(Key::Home), _) => dialog.cursor = 0,
            (Some(Key::End), _) => dialog.move_cursor(isize::MAX),
            _ => {}
        }
        Some(true)
    }

    /// Does the change; the dialog closes when something is changed.
    fn apply_properties(&mut self, state: &PropsState) -> bool {
        let Some(text) = state.change_text() else {
            self.set_error("Nothing is changed yet");
            return true;
        };
        let dialog = InputDialog::new(
            "Properties",
            "",
            &text,
            InputPurpose::Attributes(state.targets.clone()),
        );
        self.submit_input(dialog);
        false
    }

    /// A value typed for a row of the Properties dialog; the dialog opens
    /// again (with the value, or the reason it was refused).
    pub(super) fn properties_typed(&mut self, row: PropRow, mut state: PropsState, text: &str) {
        let result = match row {
            PropRow::Modified => state.set_modified(text),
            PropRow::Octal => state.set_octal(text),
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.set_error(e);
        }
        self.overlay = Some(Overlay::Choose(Self::properties_dialog(state, 0)));
    }
}
