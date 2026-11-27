//! AI providers (ADR 0063): the services that write scripts and rename rules.
//! Anthropic is built in; OpenRouter, OpenAI and any server that speaks the
//! OpenAI chat API (also local ones: Ollama, LM Studio) are added in Settings
//! with a name, an address, a model and a key.
//!
//! Keys are never in the settings file (which is exported and shared): each
//! is read from its environment variable, else from a private file in the
//! settings folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The name of the built-in provider.
pub const ANTHROPIC: &str = "anthropic";

/// The file `pnxr ai key` always wrote for Anthropic.
const ANTHROPIC_KEY_FILE: &str = "anthropic-api-key";

/// How a provider is spoken to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiKind {
    /// The Anthropic Messages API.
    #[default]
    Anthropic,
    /// The OpenAI chat completions API (OpenAI, OpenRouter, local servers).
    OpenAi,
}

impl AiKind {
    /// The name shown in Settings.
    pub fn label(self) -> &'static str {
        match self {
            AiKind::Anthropic => "Anthropic",
            AiKind::OpenAi => "OpenAI-compatible",
        }
    }

    /// The other kind.
    pub fn other(self) -> Self {
        match self {
            AiKind::Anthropic => AiKind::OpenAi,
            AiKind::OpenAi => AiKind::Anthropic,
        }
    }
}

/// One AI service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiProvider {
    /// What it is called (unique; also names its key file).
    pub name: String,
    /// How it is spoken to.
    #[serde(default)]
    pub kind: AiKind,
    /// The address of the API, without the path (`https://api.openai.com/v1`).
    #[serde(default)]
    pub base_url: String,
    /// The model to ask.
    #[serde(default)]
    pub model: String,
    /// The environment variable that may hold the key (empty: none).
    #[serde(default)]
    pub key_env: String,
}

impl AiProvider {
    /// Anthropic, as it always worked.
    pub fn anthropic() -> Self {
        AiProvider {
            name: ANTHROPIC.into(),
            kind: AiKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            model: "claude-opus-5-5".into(),
            key_env: "ANTHROPIC_API_KEY".into(),
        }
    }

    /// The ready-made providers offered when one is added: name, kind,
    /// address, model, key variable.
    pub fn presets() -> Vec<AiProvider> {
        let make = |name: &str, kind, url: &str, model: &str, env: &str| AiProvider {
            name: name.into(),
            kind,
            base_url: url.into(),
            model: model.into(),
            key_env: env.into(),
        };
        vec![
            AiProvider::anthropic(),
            make(
                "openrouter",
                AiKind::OpenAi,
                "https://openrouter.ai/api/v1",
                "openrouter/auto",
                "OPENROUTER_API_KEY",
            ),
            make(
                "openai",
                AiKind::OpenAi,
                "https://api.openai.com/v1",
                "gpt-4.1",
                "OPENAI_API_KEY",
            ),
            make("local", AiKind::OpenAi, "http://localhost:11434/v1", "", ""),
            make("custom", AiKind::OpenAi, "", "", ""),
        ]
    }

    /// Whether the provider needs a key (a local server usually does not:
    /// no variable is named and the address is this computer).
    pub fn needs_key(&self) -> bool {
        match self.kind {
            AiKind::Anthropic => true,
            AiKind::OpenAi => {
                !self.key_env.is_empty()
                    || !(self.base_url.contains("://localhost")
                        || self.base_url.contains("://127.0.0.1")
                        || self.base_url.contains("://[::1]"))
            }
        }
    }

    /// The file that holds this provider's key.
    pub fn key_file(&self, config_dir: &Path) -> PathBuf {
        if self.name == ANTHROPIC {
            return config_dir.join(ANTHROPIC_KEY_FILE);
        }
        let safe: String = self
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        config_dir.join(format!("ai-key-{safe}"))
    }
}

/// Where a key was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// An environment variable.
    Environment(String),
    /// The provider's key file.
    File(PathBuf),
}

impl KeySource {
    /// For a message.
    pub fn describe(&self) -> String {
        match self {
            KeySource::Environment(name) => format!("the {name} variable"),
            KeySource::File(path) => path.display().to_string(),
        }
    }
}

/// The AI section of the settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AiSettings {
    /// The provider used when none is named (empty: Anthropic).
    pub default: String,
    /// The providers added in Settings. Anthropic is always there; an entry
    /// named `anthropic` changes its model or address.
    pub providers: Vec<AiProvider>,
}

impl AiSettings {
    /// Nothing was changed: the file has no AI section.
    pub fn is_unchanged(&self) -> bool {
        *self == AiSettings::default()
    }

    /// Every provider: Anthropic first (as changed by the user), then the others.
    pub fn all(&self) -> Vec<AiProvider> {
        let mut list = Vec::new();
        match self.providers.iter().find(|p| p.name == ANTHROPIC) {
            Some(own) => list.push(own.clone()),
            None => list.push(AiProvider::anthropic()),
        }
        list.extend(self.providers.iter().filter(|p| p.name != ANTHROPIC).cloned());
        list
    }

    /// The provider called `name`.
    pub fn get(&self, name: &str) -> Option<AiProvider> {
        self.all().into_iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// The default provider (Anthropic when none is chosen or it is gone).
    pub fn default_provider(&self) -> AiProvider {
        self.get(&self.default).unwrap_or_else(AiProvider::anthropic)
    }

    /// Whether `name` is the default one.
    pub fn is_default(&self, name: &str) -> bool {
        self.default_provider().name.eq_ignore_ascii_case(name)
    }

    /// Adds or replaces the provider with this name.
    pub fn set(&mut self, provider: AiProvider) {
        match self.providers.iter_mut().find(|p| p.name == provider.name) {
            Some(slot) => *slot = provider,
            None => self.providers.push(provider),
        }
    }

    /// Replaces the provider called `old` (possibly renaming it). `false`
    /// when another provider already has the new name.
    pub fn replace(&mut self, old: &str, provider: AiProvider) -> bool {
        if provider.name != old && self.get(&provider.name).is_some() {
            return false;
        }
        if self.default == old {
            self.default = provider.name.clone();
        }
        self.providers.retain(|p| p.name != old);
        self.providers.push(provider);
        true
    }

    /// Removes a provider; Anthropic cannot be removed (its own entry is only
    /// reset).
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.providers.len();
        self.providers.retain(|p| p.name != name);
        if self.default == name {
            self.default.clear();
        }
        self.providers.len() != before
    }
}

/// The key of `provider`: its variable, else its key file.
pub fn find_key(provider: &AiProvider, config_dir: &Path) -> Option<(String, KeySource)> {
    if !provider.key_env.is_empty()
        && let Ok(key) = std::env::var(&provider.key_env)
        && !key.trim().is_empty()
    {
        return Some((key.trim().to_owned(), KeySource::Environment(provider.key_env.clone())));
    }
    let file = provider.key_file(config_dir);
    let key = std::fs::read_to_string(&file).ok()?.trim().to_owned();
    (!key.is_empty()).then_some((key, KeySource::File(file)))
}

/// Saves `key` for `provider`, readable only by the user; returns the file.
pub fn save_key(provider: &AiProvider, config_dir: &Path, key: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(config_dir)?;
    let file = provider.key_file(config_dir);
    let key = key.trim();
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&file)?;
        out.write_all(key.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&file, key)?;
    Ok(file)
}

/// Forgets the saved key of `provider`; `true` when there was one.
pub fn forget_key(provider: &AiProvider, config_dir: &Path) -> bool {
    std::fs::remove_file(provider.key_file(config_dir)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_is_always_there_and_the_default() {
        let settings = AiSettings::default();
        assert_eq!(settings.all().len(), 1);
        assert_eq!(settings.default_provider().name, ANTHROPIC);
    }

    #[test]
    fn providers_are_added_replaced_renamed_and_removed() {
        let mut settings = AiSettings::default();
        let mut local = AiProvider::presets().into_iter().find(|p| p.name == "local").unwrap();
        settings.set(local.clone());
        settings.default = "local".into();
        assert_eq!(settings.default_provider().name, "local");
        assert!(!settings.needs_key_for_local());
        local.name = "ollama".into();
        assert!(settings.replace("local", local));
        assert_eq!(settings.default, "ollama");
        // Another provider's name is refused.
        let mut again = AiProvider::anthropic();
        again.name = "ollama".into();
        assert!(!settings.replace(ANTHROPIC, again));
        assert!(settings.remove("ollama"));
        assert_eq!(settings.default_provider().name, ANTHROPIC);
    }

    impl AiSettings {
        fn needs_key_for_local(&self) -> bool {
            self.get("local").unwrap().needs_key()
        }
    }

    #[test]
    fn keys_come_from_the_variable_first_then_the_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut provider = AiProvider::presets().into_iter().find(|p| p.name == "openai").unwrap();
        provider.key_env = "PNXR_TEST_AI_KEY_UNSET".into();
        assert!(find_key(&provider, dir.path()).is_none());
        let file = save_key(&provider, dir.path(), "  sk-test \n").unwrap();
        assert!(file.ends_with("ai-key-openai"));
        let (key, source) = find_key(&provider, dir.path()).unwrap();
        assert_eq!(key, "sk-test");
        assert_eq!(source, KeySource::File(file));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                std::fs::metadata(provider.key_file(dir.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "the key file is private");
        }
        assert!(forget_key(&provider, dir.path()));
        assert!(find_key(&provider, dir.path()).is_none());
    }

    #[test]
    fn anthropic_keeps_its_old_key_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(AiProvider::anthropic().key_file(dir.path()).ends_with("anthropic-api-key"));
    }

    #[test]
    fn a_local_server_needs_no_key_unless_a_variable_is_named() {
        let local = AiProvider::presets().into_iter().find(|p| p.name == "local").unwrap();
        assert!(!local.needs_key());
        let remote = AiProvider::presets().into_iter().find(|p| p.name == "openrouter").unwrap();
        assert!(remote.needs_key());
    }
}
