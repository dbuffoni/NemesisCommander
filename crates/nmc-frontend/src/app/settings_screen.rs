//! The settings screen (Ctrl+,): keys and the effects of changes.
//!
//! Every change is saved at once and applied: the key map, the transfer
//! manager (jobs at once, bandwidth) and what the panels show.

use nmc_app::sync::{self, SyncMode};
use nmc_config::{ExportBundle, Key};

use super::{App, AppEvent, External, home_file};
use crate::dialogs::{ChooseDialog, ChoosePurpose, InputDialog, InputPurpose, Overlay};
use crate::input::KeyPress;
use crate::settings::{Command, Field, FieldKind, Row, SettingsScreen};

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
        let typed = press.text.filter(|c| !press.chord.ctrl && !press.chord.alt && !c.is_control());
        match (key, typed) {
            // Esc clears what was typed first, then closes.
            (Some(Key::Esc), _) if !screen.filter.is_empty() => screen.set_filter(""),
            (Some(Key::Esc), _) => return None,
            (Some(Key::Up), _) => screen.move_cursor(-1),
            (Some(Key::Down), _) => screen.move_cursor(1),
            (Some(Key::PageUp), _) => screen.move_cursor(-10),
            (Some(Key::PageDown), _) => screen.move_cursor(10),
            (Some(Key::Home), _) => screen.move_cursor(-10_000),
            (Some(Key::End), _) => screen.move_cursor(10_000),
            (Some(Key::Left), _) => self.step_option(screen.current(), -1),
            (Some(Key::Right), _) => self.step_option(screen.current(), 1),
            (Some(Key::Enter), _) => return self.settings_enter(screen),
            (Some(Key::Space), _) if screen.filter.is_empty() => {
                return self.settings_enter(screen);
            }
            (Some(Key::Delete), _) => self.reset_row(screen.current()),
            (Some(Key::Backspace), _) => {
                let mut filter = screen.filter.clone();
                filter.pop();
                screen.set_filter(&filter);
            }
            (_, Some(c)) => {
                let filter = format!("{}{c}", screen.filter);
                screen.set_filter(&filter);
            }
            _ => {}
        }
        Some(Overlay::Settings(screen))
    }

    /// Enter on a row: change, type, record or run.
    fn settings_enter(&mut self, screen: SettingsScreen) -> Option<Overlay> {
        match screen.current() {
            Row::Header(_) => {}
            Row::Option(field) => match field.kind() {
                FieldKind::Toggle | FieldKind::Choice => self.step_option(Row::Option(field), 1),
                FieldKind::Font => return Some(self.font_dialog(field, screen)),
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
                let suggestion = home_file("nemesiscommander-settings.toml");
                return Some(Overlay::Input(InputDialog::new(
                    "Export settings",
                    "Write settings, bookmarks and connections to:",
                    &suggestion,
                    InputPurpose::ExportSettings,
                )));
            }
            Command::Import => {
                let suggestion = home_file("nemesiscommander-settings.toml");
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
        self.external = Some(External {
            command: self.editor_command(&file),
            reload_settings: true,
            rename: None,
        });
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
        if let Row::Option(field) = row {
            field.reset(&mut self.context.config);
            self.settings_changed(format!("{}: back to the default", field.label()));
        }
    }

    /// A typed value for `field`.
    /// The list of installed fonts for `field`, with "(system default)" on
    /// top; typing narrows it.
    fn font_dialog(&mut self, field: Field, screen: SettingsScreen) -> Overlay {
        if self.fonts.is_empty() {
            self.set_status("The terminal uses its own font; fonts are chosen in the window");
            return Overlay::Settings(screen);
        }
        let current = field.value(&self.context.config);
        let mut dialog = ChooseDialog::new(
            field.label(),
            Vec::new(),
            ChoosePurpose::Font { field, screen: Box::new(screen), names: Vec::new() },
        );
        dialog.query = Some(String::new());
        self.refresh_fonts(&mut dialog, &current);
        Overlay::Choose(dialog)
    }

    /// Rebuilds the font list from the query, with the cursor on `current`
    /// when it is there (else on the first row).
    fn refresh_fonts(&self, dialog: &mut ChooseDialog, current: &str) {
        let query = dialog.query.clone().unwrap_or_default();
        let needle = query.to_lowercase();
        let mut names = vec![String::new()];
        names.extend(self.fonts.iter().filter(|f| f.to_lowercase().contains(&needle)).cloned());
        if !query.is_empty() {
            names.remove(0);
        }
        dialog.items = names
            .iter()
            .map(|n| if n.is_empty() { "(system default)".to_owned() } else { n.clone() })
            .collect();
        dialog.cursor = names.iter().position(|n| n == current).unwrap_or(0);
        dialog.footer = if query.is_empty() {
            "Type to filter   Enter: use   Esc: back".into()
        } else {
            format!("Filter: {query}_   Enter: use   Backspace: erase")
        };
        if let ChoosePurpose::Font { names: list, .. } = &mut dialog.purpose {
            *list = names;
        }
    }

    /// Typing and Backspace in the font list.
    pub(super) fn fonts_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let mut query = dialog.query.clone().unwrap_or_default();
        match (press.plain(), press.text) {
            (Some(Key::Backspace), _) => {
                query.pop();
            }
            (_, Some(c)) if !press.chord.ctrl && !press.chord.alt && !c.is_control() => {
                query.push(c);
            }
            _ => return None,
        }
        dialog.query = Some(query);
        self.refresh_fonts(dialog, "");
        Some(true)
    }

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
    pub(super) fn settings_changed(&mut self, message: String) {
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

/// Reads what was typed in the "Install nmc serve" dialog: the server, then
/// the options of `nmc serve install` (`--root DIR`, `--read-only`,
/// `--port N`, `--renew`).
fn parse_install_serve(
    text: &str,
    tls_dir: std::path::PathBuf,
) -> Result<nmc_agent::provision::InstallOptions, String> {
    let mut words = text.split_whitespace();
    let host = words.next().ok_or("type the server, for example me@nas.lan")?;
    let mut options = nmc_agent::provision::InstallOptions::new(host, tls_dir);
    while let Some(word) = words.next() {
        match word {
            "--read-only" => options.read_only = true,
            "--renew" => options.renew = true,
            "--root" => options.roots.push(words.next().ok_or("--root needs a folder")?.to_owned()),
            "--port" => {
                options.port =
                    words.next().and_then(|p| p.parse().ok()).ok_or("--port needs a number")?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

impl App {
    /// Installs `nmc serve` on the server typed in the dialog (the same as
    /// `nmc serve install` on the command line).
    pub(super) fn install_serve(&mut self, text: &str) {
        let options = match parse_install_serve(text, self.context.paths.tls_dir()) {
            Ok(options) => options,
            Err(e) => return self.set_error(e),
        };
        self.set_status(format!("Installing nmc serve on {}…", options.authority));
        self.spawn_task(async move {
            let result = nmc_agent::provision::install_serve(&options, |_| {}).await;
            let (message, error) = match result {
                Ok(location) => (
                    format!(
                        "nmc serve is running: open {location} (F4 in the drive list saves it)"
                    ),
                    false,
                ),
                Err(e) => (format!("Installing nmc serve failed: {e}"), true),
            };
            AppEvent::TaskDone { message, error, reload: false, focus: None }
        });
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn the_install_dialog_reads_the_options() {
        let options = parse_install_serve(
            "me@nas.lan --root /srv --root /home --read-only --port 9000",
            "/tls".into(),
        )
        .unwrap();
        assert_eq!(options.authority, "me@nas.lan");
        assert_eq!(options.roots, ["/srv", "/home"]);
        assert!(options.read_only);
        assert_eq!(options.port, 9000);
        assert!(parse_install_serve("", "/tls".into()).is_err());
        assert!(parse_install_serve("h --bogus", "/tls".into()).is_err());
        assert!(parse_install_serve("h --port x", "/tls".into()).is_err());
    }
}
