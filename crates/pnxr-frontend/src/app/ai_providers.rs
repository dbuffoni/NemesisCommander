//! AI providers (ADR 0063): the list in Settings, the window that edits one,
//! and the question that asks for a key when the provider has none.

use pnxr_config::ai::{self, AiKind, AiProvider, KeySource};

use super::*;

/// What goes on after the key was saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiThen {
    /// Nothing: the key was asked for from the providers list.
    Nothing,
    /// Ask what the script should do.
    Script,
    /// Ask how to rename these.
    Rename(Vec<VfsPath>),
}

/// A text of a provider typed in a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiField {
    /// Its name.
    Name,
    /// The address of the API.
    Address,
    /// The model.
    Model,
    /// The variable that may hold the key.
    KeyEnv,
    /// The key itself.
    Key,
}

/// The rows of the window that edits one provider, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiRow {
    /// Name.
    Name,
    /// Anthropic or OpenAI-compatible.
    Kind,
    /// Address.
    Address,
    /// Model.
    Model,
    /// The key.
    Key,
    /// The environment variable.
    KeyEnv,
    /// Make it the default.
    MakeDefault,
    /// Remove it (Anthropic: put back its defaults).
    Remove,
}

const ROWS: [AiRow; 8] = [
    AiRow::Name,
    AiRow::Kind,
    AiRow::Address,
    AiRow::Model,
    AiRow::Key,
    AiRow::KeyEnv,
    AiRow::MakeDefault,
    AiRow::Remove,
];

/// How the key of a provider stands, for the lists.
fn key_state(provider: &AiProvider, config_dir: &std::path::Path) -> String {
    if !provider.needs_key() {
        return "no key needed".into();
    }
    match ai::find_key(provider, config_dir) {
        Some((_, KeySource::Environment(name))) => format!("key from {name}"),
        Some((_, KeySource::File(_))) => "key saved".into(),
        None => "NO KEY".into(),
    }
}

impl App {
    /// "AI providers…": the list.
    pub(super) fn open_ai_providers(&mut self) {
        self.show_ai_providers(0);
    }

    fn show_ai_providers(&mut self, cursor: usize) {
        let dir = self.context.paths.config_dir.clone();
        let settings = &self.context.config.ai;
        let providers = settings.all();
        let mut items: Vec<String> = providers
            .iter()
            .map(|p| {
                let star = if settings.is_default(&p.name) { "*" } else { " " };
                format!(
                    "{star} {:<12} {:<18} {}  ({})",
                    p.name,
                    p.kind.label(),
                    if p.model.is_empty() { "(no model)" } else { &p.model },
                    key_state(p, &dir)
                )
            })
            .collect();
        items.push("  Add a provider…".into());
        let names = providers.into_iter().map(|p| p.name).collect();
        let mut dialog =
            ChooseDialog::new("AI providers", items, ChoosePurpose::AiProviders(names));
        dialog.footer = crate::hints::list("edit", &["F7: add", "Del: remove"], "close");
        dialog.cursor = cursor;
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// Keys of the providers list.
    pub(super) fn ai_providers_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let ChoosePurpose::AiProviders(names) = &dialog.purpose else { return None };
        match press.plain() {
            Some(Key::F(7) | Key::Insert) => {
                self.ai_presets();
                Some(false)
            }
            Some(Key::Delete) => {
                match names.get(dialog.cursor).cloned() {
                    Some(name) => self.remove_ai_provider(&name),
                    None => self.set_status("Nothing to remove here"),
                }
                Some(false)
            }
            _ => None,
        }
    }

    /// Enter on the providers list.
    pub(super) fn ai_provider_chosen(&mut self, names: &[String], cursor: usize) {
        match names.get(cursor) {
            Some(name) => self.edit_ai_provider(name, 0),
            None => self.ai_presets(),
        }
    }

    /// The ready-made providers to start from.
    fn ai_presets(&mut self) {
        let presets: Vec<AiProvider> =
            AiProvider::presets().into_iter().filter(|p| p.name != ai::ANTHROPIC).collect();
        let items = presets
            .iter()
            .map(|p| match p.name.as_str() {
                "openrouter" => "OpenRouter (many models behind one key)".to_owned(),
                "openai" => "OpenAI".to_owned(),
                "local" => "A local model (Ollama, LM Studio…) on this computer".to_owned(),
                _ => "Another service that speaks the OpenAI API".to_owned(),
            })
            .collect();
        let mut dialog =
            ChooseDialog::new("Add an AI provider", items, ChoosePurpose::AiPresets(presets));
        dialog.footer = crate::hints::list("add", &[], "back");
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// A preset was chosen: add it (with a free name) and edit it.
    pub(super) fn ai_preset_chosen(&mut self, presets: &[AiProvider], cursor: usize) {
        let Some(mut provider) = presets.get(cursor).cloned() else { return };
        let base = provider.name.clone();
        let mut n = 1;
        while self.context.config.ai.get(&provider.name).is_some() {
            n += 1;
            provider.name = format!("{base}{n}");
        }
        let name = provider.name.clone();
        self.context.config.ai.set(provider);
        self.settings_changed(format!("Added the AI provider {name}"));
        self.edit_ai_provider(&name, 0);
    }

    fn remove_ai_provider(&mut self, name: &str) {
        if name == ai::ANTHROPIC {
            self.context.config.ai.remove(name);
            self.settings_changed(
                "Anthropic is built in; its address and model are the standard ones again".into(),
            );
        } else if self.context.config.ai.remove(name) {
            self.settings_changed(format!(
                "Removed the AI provider {name} (its saved key stays in the settings folder)"
            ));
        }
        self.show_ai_providers(0);
    }

    /// The window that edits one provider.
    pub(super) fn edit_ai_provider(&mut self, name: &str, cursor: usize) {
        let Some(provider) = self.context.config.ai.get(name) else {
            return self.show_ai_providers(0);
        };
        let dir = self.context.paths.config_dir.clone();
        let default = self.context.config.ai.is_default(name);
        let items: Vec<String> = ROWS
            .iter()
            .map(|row| match row {
                AiRow::Name => format!("Name: {}", provider.name),
                AiRow::Kind => format!("Kind: {} (Enter changes it)", provider.kind.label()),
                AiRow::Address => format!("Address: {}", provider.base_url),
                AiRow::Model => format!(
                    "Model: {}",
                    if provider.model.is_empty() { "(type one)" } else { &provider.model }
                ),
                AiRow::Key => format!("API key: {}", key_state(&provider, &dir)),
                AiRow::KeyEnv => format!(
                    "Environment variable of the key: {}",
                    if provider.key_env.is_empty() { "(none)" } else { &provider.key_env }
                ),
                AiRow::MakeDefault => {
                    if default {
                        "This is the default provider".to_owned()
                    } else {
                        "Make this the default provider".to_owned()
                    }
                }
                AiRow::Remove => {
                    if provider.name == ai::ANTHROPIC {
                        "Back to the standard address and model".to_owned()
                    } else {
                        "Remove this provider".to_owned()
                    }
                }
            })
            .collect();
        let mut dialog = ChooseDialog::new(
            &format!("AI provider {}", provider.name),
            items,
            ChoosePurpose::AiProvider(provider.name.clone()),
        );
        dialog.footer = crate::hints::list("change", &[], "back to the list");
        dialog.cursor = cursor.min(ROWS.len() - 1);
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// Enter on a row of the provider window.
    pub(super) fn ai_provider_row(&mut self, name: &str, cursor: usize) {
        let Some(provider) = self.context.config.ai.get(name) else { return };
        let Some(row) = ROWS.get(cursor).copied() else { return };
        let ask =
            |app: &mut App, title: &str, prompt: &str, text: &str, field: AiField, secret: bool| {
                let mut dialog = InputDialog::new(
                    title,
                    prompt,
                    text,
                    InputPurpose::AiProviderField { provider: provider.name.clone(), field },
                );
                dialog.secret = secret;
                app.overlay = Some(Overlay::Input(dialog));
            };
        match row {
            AiRow::Name if provider.name == ai::ANTHROPIC => {
                self.set_error("The built-in provider keeps its name");
                self.edit_ai_provider(name, cursor);
            }
            AiRow::Name => ask(self, "AI provider", "Name:", &provider.name, AiField::Name, false),
            AiRow::Kind => {
                let mut changed = provider.clone();
                changed.kind = provider.kind.other();
                self.context.config.ai.set(changed);
                self.settings_changed(format!(
                    "{}: {}",
                    provider.name,
                    provider.kind.other().label()
                ));
                self.edit_ai_provider(name, cursor);
            }
            AiRow::Address => ask(
                self,
                "AI provider",
                "Address of the API (for example https://api.openai.com/v1):",
                &provider.base_url,
                AiField::Address,
                false,
            ),
            AiRow::Model => {
                ask(self, "AI provider", "Model:", &provider.model, AiField::Model, false)
            }
            AiRow::Key => ask(
                self,
                "AI provider",
                &format!("API key of {} (empty forgets the saved one):", provider.name),
                "",
                AiField::Key,
                true,
            ),
            AiRow::KeyEnv => ask(
                self,
                "AI provider",
                "Environment variable that may hold the key (empty: none):",
                &provider.key_env,
                AiField::KeyEnv,
                false,
            ),
            AiRow::MakeDefault => {
                self.context.config.ai.default = provider.name.clone();
                self.settings_changed(format!("{} is the default AI provider", provider.name));
                self.edit_ai_provider(name, cursor);
            }
            AiRow::Remove => {
                self.remove_ai_provider(name);
            }
        }
    }

    /// A text of a provider was typed.
    pub(super) fn ai_provider_typed(&mut self, name: &str, field: AiField, text: &str) {
        let Some(mut provider) = self.context.config.ai.get(name) else { return };
        let text = text.trim();
        let mut now = provider.name.clone();
        match field {
            AiField::Name => {
                if text.is_empty() {
                    return self.set_error("A provider needs a name");
                }
                let old = provider.name.clone();
                provider.name = text.to_owned();
                if !self.context.config.ai.replace(&old, provider) {
                    self.set_error(format!("There is already a provider called {text}"));
                    return self.edit_ai_provider(name, 0);
                }
                now = text.to_owned();
                self.settings_changed(format!("Renamed to {text}"));
            }
            AiField::Address => {
                provider.base_url = text.trim_end_matches('/').to_owned();
                self.context.config.ai.set(provider);
                self.settings_changed("AI provider address set".into());
            }
            AiField::Model => {
                provider.model = text.to_owned();
                self.context.config.ai.set(provider);
                self.settings_changed("AI provider model set".into());
            }
            AiField::KeyEnv => {
                provider.key_env = text.to_owned();
                self.context.config.ai.set(provider);
                self.settings_changed("AI provider key variable set".into());
            }
            AiField::Key => {
                let dir = self.context.paths.config_dir.clone();
                if text.is_empty() {
                    let had = ai::forget_key(&provider, &dir);
                    self.set_status(if had {
                        "The saved key is forgotten"
                    } else {
                        "No saved key"
                    });
                } else {
                    match ai::save_key(&provider, &dir, text) {
                        Ok(file) => self.set_status(format!("Key saved in {}", file.display())),
                        Err(e) => self.set_error(format!("Cannot save the key: {e}")),
                    }
                }
            }
        }
        self.edit_ai_provider(&now, 4);
    }

    // ------------------------------------------------------------- the key

    /// Whether the default provider can be used; when it needs a key and has
    /// none, asks for it (with the way to get one) and goes on with `then`
    /// once it is saved.
    pub(super) fn ai_ready(&mut self, then: AiThen) -> bool {
        let provider = self.context.config.ai.default_provider();
        if !provider.needs_key()
            || ai::find_key(&provider, &self.context.paths.config_dir).is_some()
        {
            return true;
        }
        self.ask_ai_key(provider, then);
        false
    }

    /// The question for a key, with the guidance a first-time user needs.
    pub(super) fn ask_ai_key(&mut self, provider: AiProvider, then: AiThen) {
        let file = provider.key_file(&self.context.paths.config_dir);
        let mut dialog = InputDialog::new(
            "AI key needed",
            &format!("Paste the API key of {} here (Enter saves it, Esc cancels):", provider.name),
            "",
            InputPurpose::AiKey { provider: provider.name.clone(), then },
        );
        dialog.secret = true;
        dialog.preview = key_guidance(&provider, &file);
        self.overlay = Some(Overlay::Input(dialog));
    }

    /// The key was typed.
    pub(super) fn ai_key_typed(&mut self, name: &str, then: AiThen, text: &str) {
        let Some(provider) = self.context.config.ai.get(name) else { return };
        let text = text.trim();
        if text.is_empty() {
            return self.set_error("No key typed: nothing was saved");
        }
        let dir = self.context.paths.config_dir.clone();
        match ai::save_key(&provider, &dir, text) {
            Ok(file) => {
                self.set_status(format!("Key saved in {} (only you can read it)", file.display()));
                match then {
                    AiThen::Nothing => {}
                    AiThen::Script => self.ask_ai_script(),
                    AiThen::Rename(targets) => self.ask_ai_rename(targets),
                }
            }
            Err(e) => self.set_error(format!("Cannot save the key: {e}")),
        }
    }
}

/// What to tell someone who has no key yet.
fn key_guidance(provider: &AiProvider, file: &std::path::Path) -> Vec<String> {
    let mut lines = Vec::new();
    match (provider.kind, provider.name.as_str()) {
        (AiKind::Anthropic, _) => {
            lines.push("How to get a key:".into());
            lines.push("  1. Sign in at https://console.anthropic.com".into());
            lines.push("  2. Open \"API keys\" and create a key (it starts with sk-ant-).".into());
            lines.push("  3. Paste it above. Requests are billed to your account.".into());
        }
        (_, "openrouter") => {
            lines.push("How to get a key: sign in at https://openrouter.ai, open".into());
            lines.push("\"Keys\", create one and paste it above.".into());
        }
        (_, "openai") => {
            lines.push("How to get a key: sign in at https://platform.openai.com,".into());
            lines.push("open \"API keys\", create one and paste it above.".into());
        }
        _ => lines.push("Paste the key your service gave you.".into()),
    }
    lines.push(String::new());
    lines.push("Other ways to give the key:".into());
    if !provider.key_env.is_empty() {
        lines.push(format!("  - set {} before starting Pairnox", provider.key_env));
    }
    lines.push(format!(
        "  - pnxr ai key --provider {}   (reads it from standard input)",
        provider.name
    ));
    lines.push("  - Settings, AI providers (also another service or a local model)".into());
    lines.push(format!("It is saved in {} for you only.", file.display()));
    lines
}
