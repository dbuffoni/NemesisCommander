//! The settings screen (Ctrl+,): keys and the effects of changes.
//!
//! Every change is saved at once and applied: the key map, the transfer
//! manager (jobs at once, bandwidth) and what the panels show.

use std::sync::Arc;

use pnxr_app::sync::{self, SyncMode};
use pnxr_config::{ExportBundle, Key};

use super::{App, AppEvent, External, home_file};
use crate::dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, InputDialog, InputPurpose,
    MessageDialog, Overlay,
};
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
            (Some(Key::F(1)), _) => {
                let (title, lines) = crate::settings::help_for(screen.current());
                let mut help = MessageDialog::new(&title, lines);
                help.back_to_settings = Some(Box::new(screen));
                return Some(Overlay::Message(help));
            }
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
                FieldKind::Editor => return Some(self.editor_dialog(field, screen)),
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
            Command::AiProviders => {
                self.open_ai_providers();
                return self.overlay.take();
            }
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
                let suggestion = home_file("pairnox-settings.toml");
                return Some(Overlay::Input(InputDialog::new(
                    "Export settings",
                    "Write settings, bookmarks and connections to:",
                    &suggestion,
                    InputPurpose::ExportSettings,
                )));
            }
            Command::Import => {
                let suggestion = home_file("pairnox-settings.toml");
                return Some(Overlay::Input(InputDialog::new(
                    "Import settings",
                    "Read settings from:",
                    &suggestion,
                    InputPurpose::ImportSettings,
                )));
            }
            Command::ResetStats => {
                let count = self.manager.stats_count();
                self.manager.reset_stats();
                self.set_status(format!(
                    "Forgot what was remembered about {count} server{}",
                    if count == 1 { "" } else { "s" }
                ));
                return Some(Overlay::Settings(*screen));
            }
            Command::ResetShortcuts | Command::ResetFunctionKeys => {
                let functions = command == Command::ResetFunctionKeys;
                crate::settings::reset_all_keys(&mut self.context.config, functions);
                self.settings_changed(
                    if functions {
                        "Function keys and button names are the built-in ones again"
                    } else {
                        "Shortcuts are the built-in ones again"
                    }
                    .to_owned(),
                );
                return Some(Overlay::Settings(*screen));
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

    /// The list of editors for `field`: the default, the known editors (the
    /// installed ones first) and a row to type a command.
    fn editor_dialog(&mut self, field: Field, screen: SettingsScreen) -> Overlay {
        let window = !matches!(field, Field::TerminalEditor);
        let default = match field {
            Field::GuiEditor => "the shared one, or the desktop's",
            Field::TerminalEditor => "the shared one, $EDITOR, vi",
            _ => "$VISUAL / $EDITOR, then the system's",
        };
        let suggestions =
            crate::editors::suggestions(window, &crate::editors::installed(), default);
        let mut items: Vec<String> = suggestions.iter().map(|s| s.label.clone()).collect();
        let commands: Vec<String> = suggestions.into_iter().map(|s| s.command).collect();
        items.push("Type a command myself…".to_owned());
        let current = field.value(&self.context.config);
        let mut dialog = ChooseDialog::new(
            field.label(),
            items,
            ChoosePurpose::Editor { field, screen: Box::new(screen), commands: commands.clone() },
        );
        dialog.cursor = commands.iter().position(|c| *c == current).unwrap_or(0);
        dialog.footer = crate::hints::list("choose", &[], "back");
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
            crate::hints::list("choose", &["Type: filter"], "close")
        } else {
            let keys = crate::hints::list("choose", &["Backspace: erase"], "close");
            format!("Filter: {query}_{}{keys}", crate::hints::SEP)
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

    /// A server took fewer files at once than tried: keeps the number on its
    /// saved connection (`max_connections`), where it can be changed.
    pub(super) fn remember_endpoint_limit(&mut self, endpoint: &str, limit: usize) {
        let mut name = None;
        for connection in &mut self.context.config.connections {
            let Ok(path) = pnxr_vfs::VfsPath::parse(&connection.uri) else { continue };
            if pnxr_transfer::endpoint_key(&path).as_deref() == Some(endpoint)
                && connection.max_connections().is_none()
            {
                connection.options.insert("max_connections".into(), limit.to_string());
                name = Some(connection.name.clone());
            }
        }
        match name {
            Some(name) => {
                self.settings_changed(format!(
                    "{name} keeps refusing sessions: copies to it now use at most {limit} files at once (set in the connection)"
                ));
            }
            None => self.set_status(format!(
                "{endpoint} seems to take only {limit} files at once: copies to it keep to that until pnxr restarts"
            )),
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
        pnxr_panel::set_hide_ds_store(self.context.config.panels.hide_ds_store);
        self.apply_key_overrides();
        let transfers = &self.context.config.transfers;
        self.manager.set_max_concurrent_jobs(transfers.parallel_jobs);
        self.manager.set_global_bandwidth_limit(transfers.bandwidth_limit_kib * 1024);
        self.manager.set_profile(pnxr_app::to_profile(transfers.profile));
        self.manager.set_stats_enabled(transfers.keep_stats);
        pnxr_app::apply_endpoint_limits(&self.manager, &self.context.config);
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

impl App {
    /// Installs `pnxr serve` on the server typed in the dialog (the same as
    /// `pnxr serve install` on the command line), then shows the log.
    pub(super) fn install_serve(&mut self, text: &str) {
        let options =
            match crate::install::parse(text, self.context.paths.tls_dir(), &*self.context.remotes)
            {
                Ok(options) => options,
                Err(e) => return self.set_error(e),
            };
        // Nothing is touched on the server before the user has seen what
        // will be done and said yes.
        self.overlay = Some(Overlay::Confirm(ConfirmDialog {
            title: "Install pnxr serve?".into(),
            lines: crate::install::plan(&options),
            purpose: ConfirmPurpose::InstallServe(text.to_owned()),
        }));
    }

    /// The user said yes: installs, showing every step as it happens, then
    /// says plainly whether it worked and offers to save the connection.
    pub(super) fn start_install(&mut self, text: &str) {
        self.start_install_with(text, None);
    }

    /// [`start_install`](Self::start_install), with the `sudo` password of
    /// the server to turn lingering on if the install finds it off.
    pub(super) fn start_install_with(&mut self, text: &str, sudo: Option<String>) {
        let mut options =
            match crate::install::parse(text, self.context.paths.tls_dir(), &*self.context.remotes)
            {
                Ok(options) => options,
                Err(e) => return self.set_error(e),
            };
        options.sudo_password = sudo.map(pnxr_app::SudoPassword);
        let text = text.to_owned();
        let title = "Install pnxr serve";
        let mut window = MessageDialog::new(title, vec![crate::install::summary(&options)]);
        window.busy = true;
        self.overlay = Some(Overlay::Message(window));
        let events = self.events.clone();
        let remotes = Arc::clone(&self.context.remotes);
        self.spawn_task(async move {
            let live = events.clone();
            let (lines, result) = crate::install::run_install(&*remotes, options, move |line| {
                let _ =
                    live.send(AppEvent::Progress { title: title.into(), line: line.to_owned() });
            })
            .await;
            AppEvent::Show(Box::new(Overlay::Message(crate::install::finished(
                lines,
                result,
                &text,
                remotes.linger_off(),
            ))))
        });
    }

    /// A saved `scs://` server does not answer: offers to install it.
    pub(super) fn offer_install(&mut self, host: &str, reason: &str) {
        self.overlay = Some(Overlay::Confirm(ConfirmDialog {
            title: "Install pnxr serve?".into(),
            lines: vec![
                format!("{host} does not answer as an pnxr serve server:"),
                format!("  {reason}"),
                String::new(),
                "pnxr serve may not be installed there, or its service may be stopped (after a restart,".into(),
                "for example). Installing again starts it; certificates are kept. Install now over SSH?".into(),
                "(You will see what is done and can change the options before it starts.)".into(),
            ],
            purpose: ConfirmPurpose::OfferInstall(host.to_owned()),
        }));
    }

    /// Ctrl+T in the install question: tests the server without installing,
    /// then comes back to the question.
    pub(super) fn test_install(&mut self, dialog: InputDialog) {
        let options = match crate::install::parse(
            dialog.field.text(),
            self.context.paths.tls_dir(),
            &*self.context.remotes,
        ) {
            Ok(options) => options,
            Err(e) => return self.ask_again(dialog, e),
        };
        let title = "Test install";
        let mut window = MessageDialog::new(title, vec![crate::install::summary(&options)]);
        window.busy = true;
        window.then = Some(dialog.clone());
        self.overlay = Some(Overlay::Message(window));
        let events = self.events.clone();
        let remotes = Arc::clone(&self.context.remotes);
        self.spawn_task(async move {
            let lines = crate::install::run_check(&*remotes, options, move |line| {
                let _ =
                    events.send(AppEvent::Progress { title: title.into(), line: line.to_owned() });
            })
            .await;
            let mut window = MessageDialog::new(title, lines);
            window.then = Some(dialog);
            AppEvent::Show(Box::new(Overlay::Message(window)))
        });
    }

    /// F1 in the install question: what the install does.
    pub(super) fn explain_install(&mut self, dialog: InputDialog) {
        self.overlay = Some(Overlay::Message(MessageDialog {
            title: "Install pnxr serve: what it does".into(),
            lines: crate::install::explanation(),
            then: Some(dialog),
            then_choose: None,
            back_to_settings: None,
            back: false,
            logo: false,
            busy: false,
            fixed: false,
        }));
    }
}
