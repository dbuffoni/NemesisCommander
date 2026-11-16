//! # sc-app
//!
//! The composition root of SuperCommander. Front ends (CLI, TUI, GUI, agent)
//! call [`AppContext::load`] and get the settings, the effective key map and
//! a [`ProviderRegistry`] with every provider this build supports, instead of
//! assembling those pieces themselves.

pub mod sync;

use std::sync::Arc;

use sc_config::{AppPaths, Config, ConfigError, Keymap};
use sc_ops::{ConflictPolicy, ConflictResolver, PlanOptions, RetryPolicy};
use sc_transfer::{JobOptions, Journal, JournalError, ManagerOptions, TransferManager};
use sc_vfs::ProviderRegistry;

/// Everything a front end needs to start.
pub struct AppContext {
    /// Where files live.
    pub paths: AppPaths,
    /// The loaded settings.
    pub config: Config,
    /// Key bindings (defaults + overrides from the settings).
    pub keymap: Keymap,
    /// Problems found while loading settings (shown, never fatal).
    pub warnings: Vec<String>,
    /// All providers of this build.
    pub registry: Arc<ProviderRegistry>,
}

impl AppContext {
    /// Loads settings from the default locations.
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(AppPaths::detect())
    }

    /// Loads settings from `paths`. A missing settings file means defaults.
    pub fn load_from(paths: AppPaths) -> Result<Self, ConfigError> {
        let config = Config::load(&paths.config_file())?;
        let (keymap, warnings) = Keymap::with_overrides(&config.keys);
        let registry = Arc::new(build_registry_for(&paths));
        Ok(AppContext { paths, config, keymap, warnings, registry })
    }

    /// Saves the settings back to the settings file.
    pub fn save_config(&self) -> Result<(), ConfigError> {
        self.config.save(&self.paths.config_file())
    }

    /// Opens the persistent transfer journal and creates the manager.
    pub fn transfer_manager(&self) -> Result<TransferManager, JournalError> {
        self.transfer_manager_with(None)
    }

    /// Like [`transfer_manager`](Self::transfer_manager), with a resolver
    /// that answers conflicts of jobs using [`ConflictPolicy::Ask`].
    pub fn transfer_manager_with(
        &self,
        resolver: Option<Arc<dyn ConflictResolver>>,
    ) -> Result<TransferManager, JournalError> {
        if let Some(dir) = self.paths.journal_file().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let journal = Journal::open(self.paths.journal_file())?;
        let options = ManagerOptions {
            max_concurrent_jobs: self.config.transfers.parallel_jobs,
            global_bandwidth_limit: self.config.transfers.bandwidth_limit_kib * 1024,
        };
        let manager = TransferManager::new(self.registry.clone(), journal, options)?;
        Ok(match resolver {
            Some(resolver) => manager.with_resolver(resolver),
            None => manager,
        })
    }

    /// Job options from the transfer settings. `conflict` overrides the
    /// configured conflict policy.
    pub fn job_options(&self, conflict: Option<ConflictPolicy>) -> JobOptions {
        let settings = &self.config.transfers;
        JobOptions {
            conflict: conflict.unwrap_or(settings.conflict),
            verify: settings.verify,
            plan: PlanOptions { preserve_mtime: true, preserve_permissions: true },
            parallelism: settings.parallel_files.max(1),
            retry: RetryPolicy { attempts: settings.retries, ..Default::default() },
            bandwidth_limit: settings.bandwidth_limit_kib * 1024,
        }
    }
}

/// A registry with every provider compiled into this build, using the
/// default locations for certificates.
pub fn build_registry() -> ProviderRegistry {
    build_registry_for(&AppPaths::detect())
}

/// A registry with every provider compiled into this build; `scs://` uses
/// the certificates of `paths`.
pub fn build_registry_for(paths: &AppPaths) -> ProviderRegistry {
    let mut registry = sc_providers::default_registry();
    sc_archive::register_archive_providers(&mut registry);
    registry.register(sc_agent::AgentFactory);
    registry.register(sc_agent::ServeFactory::new(paths.tls_dir()));
    #[cfg(feature = "remote")]
    {
        registry.register(sc_sftp::SftpFactory);
        registry.register(sc_webdav::WebDavFactory::https());
        registry.register(sc_webdav::WebDavFactory::http());
        registry.register(sc_s3::S3Factory);
        registry.register(sc_smb::SmbFactory);
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_defaults_and_reports_key_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::in_dir(dir.path());
        let mut context = AppContext::load_from(paths.clone()).unwrap();
        assert!(context.warnings.is_empty());
        assert!(context.registry.schemes().contains(&"zip"));
        assert!(context.registry.schemes().contains(&"agent"));
        assert!(context.registry.schemes().contains(&"scs"));
        #[cfg(feature = "remote")]
        for scheme in ["sftp", "dav", "davs", "s3", "smb"] {
            assert!(context.registry.schemes().contains(&scheme), "{scheme}");
        }

        context.config.keys.insert("nope".into(), vec!["F12".into()]);
        context.save_config().unwrap();
        let reloaded = AppContext::load_from(paths).unwrap();
        assert_eq!(reloaded.warnings.len(), 1);
    }

    #[tokio::test]
    async fn transfer_manager_uses_the_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let context = AppContext::load_from(AppPaths::in_dir(dir.path())).unwrap();
        let manager = context.transfer_manager().unwrap();
        assert!(manager.jobs().unwrap().is_empty());
        assert!(context.paths.journal_file().exists());
    }
}
