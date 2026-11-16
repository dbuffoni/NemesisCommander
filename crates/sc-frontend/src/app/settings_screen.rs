//! The settings screen (Ctrl+,): keys and the effects of changes.
//!
//! Every change is saved at once and applied: the key map, the transfer
//! manager (jobs at once, bandwidth) and what the panels show.

use sc_app::sync::{self, SyncMode};
use sc_config::{ExportBundle, Key};

use super::{App, AppEvent, External, home_file};
use crate::dialogs::{ChooseDialog, ChoosePurpose, InputDialog, InputPurpose, Overlay};
use crate::input::KeyPress;
use crate::settings::{
    Command, Field, FieldKind, Recording, Row, SettingsScreen, assign_key, clear_keys, reset_keys,
};

impl App {
    /// Opens the settings screen.
    pub(super) fn open_settings(&mut self) {
        self.overlay = Some(Overlay::Settings(SettingsScreen::new()));
    }

    /// Handles a key on the settings screen; returns it to keep it open.
    pub(super) fn settings_key(
        &mut self,
        mut screen: SettingsScreen,
        press: KeyPress,
    ) -> Option<Overlay> {
        let key = press.plain();
        if let Some(recording) = screen.recording.take() {
            if key != Some(Key::Esc) {
                self.record_key(recording, press);
            }
            return Some(Overlay::Settings(screen));
        }
        match (key, press.text) {
            (Some(Key::Esc), _) => return None,
            (Some(Key::Up), _) => screen.move_cursor(-1),
            (Some(Key::Down), _) => screen.move_cursor(1),
            (Some(Key::PageUp), _) => screen.move_cursor(-10),
            (Some(Key::PageDown), _) => screen.move_cursor(10),
            (Some(Key::Home), _) => screen.move_cursor(-10_000),
            (Some(Key::End), _) => screen.move_cursor(10_000),
            (Some(Key::Left), _) => self.step_option(screen.current(), -1),
            (Some(Key::Right), _) => self.step_option(screen.current(), 1),
            (Some(Key::Enter | Key::Space), _) => return self.settings_enter(screen),
            (Some(Key::Delete | Key::Backspace), _) => {
                if let Row::Shortcut(action) = screen.current() {
                    clear_keys(&mut self.context.config, action);
                    self.settings_changed(format!("{} has no shortcut now", action.label()));
                }
            }
            (_, Some('a')) => {
                if let Row::Shortcut(action) = screen.current() {
                    screen.recording = Some(Recording { action, add: true });
                }
            }
            (_, Some('r')) => self.reset_row(screen.current()),
            _ => {}
        }
        Some(Overlay::Settings(screen))
    }

    /// Enter on a row: change, type, record or run.
    fn settings_enter(&mut self, mut screen: SettingsScreen) -> Option<Overlay> {
        match screen.current() {
            Row::Header(_) => {}
            Row::Option(field) => match field.kind() {
                FieldKind::Toggle | FieldKind::Choice => self.step_option(Row::Option(field), 1),
                FieldKind::Number(..) | FieldKind::Text => {
                    let value = field.value(&self.context.config);
                    return Some(Overlay::Input(InputDialog::new(
                        "Settings",
                        &format!("{}:", field.label()),
                        &value,
                        InputPurpose::SettingValue { field, screen: Box::new(screen) },
                    )));
                }
            },
            Row::Shortcut(action) => screen.recording = Some(Recording { action, add: false }),
            Row::Command(command) => return self.settings_command(command, screen),
        }
        Some(Overlay::Settings(screen))
    }

    fn settings_command(&mut self, command: Command, screen: SettingsScreen) -> Option<Overlay> {
        let screen = Box::new(screen);
        let (title, prompt, purpose) = match command {
            Command::SendTo => (
                "Send settings",
                "Machine (agent://host, scs://host) or settings file or folder:",
                InputPurpose::SendSettings(screen),
            ),
            Command::FetchFrom => (
                "Get settings",
                "Machine (agent://host, scs://host), settings file or exported file:",
                InputPurpose::FetchSettings(screen),
            ),
            Command::Export => {
                let suggestion = home_file("supercommander-settings.toml");
                return Some(Overlay::Input(InputDialog::new(
                    "Export settings",
                    "Write settings, bookmarks and connections to:",
                    &suggestion,
                    InputPurpose::ExportSettings,
                )));
            }
            Command::Import => {
                let suggestion = home_file("supercommander-settings.toml");
                return Some(Overlay::Input(InputDialog::new(
                    "Import settings",
                    "Read settings from:",
                    &suggestion,
                    InputPurpose::ImportSettings,
                )));
            }
            Command::EditFile => {
                self.edit_settings_file();
                return None;
            }
        };
        Some(Overlay::Input(InputDialog::new(title, prompt, "agent://", purpose)))
    }

    /// Opens the settings file in the editor; it is read again afterwards.
    pub(super) fn edit_settings_file(&mut self) {
        let file = self.context.paths.config_file();
        if !file.exists() && !self.save_settings() {
            return;
        }
        self.external =
            Some(External { command: self.editor_command(&file), reload_settings: true });
    }

    fn step_option(&mut self, row: Row, delta: i64) {
        if let Row::Option(field) = row
            && field.kind() != FieldKind::Text
        {
            field.step(&mut self.context.config, delta);
            let value = field.value(&self.context.config);
            self.settings_changed(format!("{}: {value}", field.label()));
        }
    }

    fn reset_row(&mut self, row: Row) {
        match row {
            Row::Option(field) => {
                field.reset(&mut self.context.config);
                self.settings_changed(format!("{}: back to the default", field.label()));
            }
            Row::Shortcut(action) => {
                let losers = reset_keys(&mut self.context.config, action);
                let mut message = format!("{}: built-in shortcut again", action.label());
                for other in losers {
                    message.push_str(&format!("; taken from {}", other.label()));
                }
                self.settings_changed(message);
            }
            _ => {}
        }
    }

    fn record_key(&mut self, recording: Recording, press: KeyPress) {
        let chord = press.chord;
        let previous = assign_key(&mut self.context.config, recording.action, chord, recording.add);
        let mut message = format!("{}: {chord}", recording.action.label());
        if let Some(other) = previous {
            message.push_str(&format!(" (taken from {})", other.label()));
        }
        self.settings_changed(message);
    }

    /// A typed value for `field`.
    pub(super) fn setting_typed(&mut self, field: Field, text: &str) {
        match field.set(&mut self.context.config, text) {
            Ok(()) => {
                let value = field.value(&self.context.config);
                self.settings_changed(format!("{}: {value}", field.label()));
            }
            Err(e) => self.set_error(e),
        }
    }

    /// Saves and applies the settings after a change, showing `message`.
    fn settings_changed(&mut self, message: String) {
        if !self.save_settings() {
            return;
        }
        self.apply_settings();
        if self.status.as_ref().is_none_or(|s| !s.error) {
            self.set_status(message);
        }
    }

    /// Applies the current settings to what is running.
    pub(super) fn apply_settings(&mut self) {
        self.apply_key_overrides();
        let transfers = &self.context.config.transfers;
        self.manager.set_max_concurrent_jobs(transfers.parallel_jobs);
        self.manager.set_global_bandwidth_limit(transfers.bandwidth_limit_kib * 1024);
    }

    /// Sends the settings to the machine typed in the dialog.
    pub(super) fn send_settings(&mut self, text: &str) {
        let location = match super::resolve_location(text, self.workspace.active().location()) {
            Ok(location) => location,
            Err(e) => return self.set_error(e),
        };
        let registry = self.context.registry.clone();
        let config = self.context.config.clone();
        self.set_status(format!("Sending settings to {text}…"));
        self.spawn_task(async move {
            let result = sync::push(&registry, &config, &location, SyncMode::Replace).await;
            let (message, error) = match result {
                Ok(file) => {
                    (format!("Settings sent to {file} (the old file is config.toml.bak)"), false)
                }
                Err(e) => (e, true),
            };
            AppEvent::TaskDone { message, error, reload: false, focus: None }
        });
    }

    /// Fetches the settings of the machine typed in the dialog, then asks
    /// whether to replace or merge.
    pub(super) fn fetch_settings(&mut self, text: &str) {
        let location = match super::resolve_location(text, self.workspace.active().location()) {
            Ok(location) => location,
            Err(e) => return self.set_error(e),
        };
        let registry = self.context.registry.clone();
        self.set_status(format!("Getting settings from {text}…"));
        self.spawn_task(async move {
            match sync::pull(&registry, &location).await {
                Ok((config, file)) => {
                    let items = vec![
                        "Replace all settings".to_owned(),
                        "Merge: add bookmarks, connections and key overrides".to_owned(),
                    ];
                    AppEvent::Show(Box::new(Overlay::Choose(ChooseDialog::new(
                        &format!("Settings from {file}"),
                        items,
                        ChoosePurpose::ImportMode(Box::new(ExportBundle::new(&config))),
                    ))))
                }
                Err(message) => {
                    AppEvent::TaskDone { message, error: true, reload: false, focus: None }
                }
            }
        });
    }
}
