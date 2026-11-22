//! # nmc-app
//!
//! The composition root of NemesisCommander. Front ends (CLI, TUI, GUI, agent)
//! call [`AppContext::load`] and get the settings, the effective key map and
//! a [`ProviderRegistry`] with every provider this build supports, instead of
//! assembling those pieces themselves.

pub mod desktop;
pub mod sync;

mod archive_search;

pub use archive_search::RegistryArchives;

use std::sync::Arc;

use nmc_config::{AppPaths, Config, ConfigError, Keymap};
use nmc_ops::{ConflictPolicy, ConflictResolver, PlanOptions, RetryPolicy};
use nmc_transfer::{JobOptions, Journal, JournalError, ManagerOptions, TransferManager};
use nmc_vfs::ProviderRegistry;

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
            profile: to_profile(self.config.transfers.profile),
        };
        let manager = TransferManager::new(self.registry.clone(), journal, options)?
            .with_server_copier(Arc::new(ServerCopies { tls_dir: self.paths.tls_dir() }));
        manager.use_stats_file(self.paths.stats_file(), self.config.transfers.keep_stats);
        apply_endpoint_limits(&manager, &self.config);
        Ok(match resolver {
            Some(resolver) => manager.with_resolver(resolver),
            None => manager,
        })
    }

    /// Job options from the transfer settings. `conflict` overrides the
    /// configured conflict policy.
    pub fn job_options(&self, conflict: Option<ConflictPolicy>) -> JobOptions {
        self.job_options_to(conflict, None)
    }

    /// Like [`job_options`](Self::job_options) for a job that copies to
    /// `destination`, which decides the default check (see
    /// [`effective_verify`]).
    pub fn job_options_to(
        &self,
        conflict: Option<ConflictPolicy>,
        destination: Option<&nmc_vfs::VfsPath>,
    ) -> JobOptions {
        let settings = &self.config.transfers;
        JobOptions {
            conflict: conflict.unwrap_or(settings.conflict),
            verify: match destination {
                Some(destination) => effective_verify(settings, destination),
                None => settings.verify,
            },
            plan: PlanOptions { preserve_mtime: true, preserve_permissions: true },
            parallelism: settings.parallel_files.max(1),
            auto_parallelism: settings.auto_parallelism,
            retry: RetryPolicy { attempts: settings.retries, ..Default::default() },
            bandwidth_limit: settings.bandwidth_limit_kib * 1024,
        }
    }
}

/// The profile of the settings as the transfer queue knows it.
pub fn to_profile(profile: nmc_config::TransferProfile) -> nmc_transfer::Profile {
    use nmc_config::TransferProfile as P;
    match profile {
        P::Balanced => nmc_transfer::Profile::Balanced,
        P::MaxSpeed => nmc_transfer::Profile::MaxSpeed,
        P::Discreet => nmc_transfer::Profile::Discreet,
    }
}

/// Gives the transfer queue the limits of simultaneous files that the saved
/// connections carry (option `max_connections`).
pub fn apply_endpoint_limits(manager: &TransferManager, config: &nmc_config::Config) {
    let connections: Vec<(String, String)> =
        config.connections.iter().map(|c| (c.name.clone(), c.uri.clone())).collect();
    manager.sync_connections(&connections);
    for connection in &config.connections {
        let Ok(path) = nmc_vfs::VfsPath::parse(&connection.uri) else { continue };
        if let Some(key) = nmc_transfer::endpoint_key(&path) {
            manager.set_endpoint_limit(&key, connection.max_connections());
        }
    }
}

/// Copies between two `nmc serve` servers directly (or in reverse), through
/// `nmc_agent::route`; anything else is left to the normal copy.
struct ServerCopies {
    tls_dir: std::path::PathBuf,
}

#[async_trait::async_trait]
impl nmc_transfer::ServerCopier for ServerCopies {
    fn between_servers(
        &self,
        sources: &[nmc_vfs::VfsPath],
        destination: &nmc_vfs::VfsPath,
    ) -> bool {
        let copy =
            nmc_agent::route::ServerCopy { sources, destination, conflict: ConflictPolicy::Skip };
        copy.is_between_servers()
            && nmc_agent::ServeFactory::new(self.tls_dir.clone()).identity().is_ok()
    }

    async fn copy(
        &self,
        sources: &[nmc_vfs::VfsPath],
        destination: &nmc_vfs::VfsPath,
        conflict: ConflictPolicy,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Option<Result<nmc_transfer::ServerCopyReport, String>> {
        use nmc_agent::route::{Outcome, Route, ServerCopy, copy_between_servers};
        let copy = ServerCopy { sources, destination, conflict };
        if !copy.is_between_servers() {
            return None;
        }
        let identity = nmc_agent::ServeFactory::new(self.tls_dir.clone()).identity().ok()?;
        let outcome =
            copy_between_servers(&identity, &copy, &[Route::Direct, Route::Reverse], |m| {
                progress(m.bytes_done, m.bytes_total)
            })
            .await;
        match outcome {
            Ok(Outcome::Copied(route, last)) => Some(Ok(nmc_transfer::ServerCopyReport {
                route: route.to_string(),
                bytes: last.bytes_done,
                failures: last.failures,
            })),
            Ok(Outcome::UseController(_)) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// The check after copying to `destination`: the one in the settings, except
/// that "same size" becomes a XXH3 content check on an agent or `nmc serve`
/// server (which hashes on the server, so it costs no traffic) unless
/// `verify_remote_fast` is off.
pub fn effective_verify(
    settings: &nmc_config::TransferSettings,
    destination: &nmc_vfs::VfsPath,
) -> nmc_ops::VerifyMode {
    use nmc_ops::VerifyMode;
    let on_server = matches!(destination.scheme(), "agent" | "scs");
    if settings.verify == VerifyMode::Size && settings.verify_remote_fast && on_server {
        VerifyMode::Fast
    } else {
        settings.verify
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
    let mut registry = nmc_providers::default_registry();
    nmc_archive::register_archive_providers(&mut registry);
    registry.register(nmc_agent::AgentFactory);
    registry.register(nmc_agent::ServeFactory::new(paths.tls_dir()));
    #[cfg(feature = "remote")]
    {
        registry.register(nmc_sftp::SftpFactory);
        registry.register(nmc_webdav::WebDavFactory::https());
        registry.register(nmc_webdav::WebDavFactory::http());
        registry.register(nmc_s3::S3Factory);
        registry.register(nmc_smb::SmbFactory);
    }
    registry
}

#[cfg(test)]
mod tests {
    #[test]
    fn size_checks_become_content_checks_on_servers() {
        use nmc_ops::VerifyMode;
        let mut settings = nmc_config::TransferSettings::default();
        let path = |u: &str| nmc_vfs::VfsPath::parse(u).unwrap();
        assert_eq!(super::effective_verify(&settings, &path("agent://h/x")), VerifyMode::Fast);
        assert_eq!(super::effective_verify(&settings, &path("scs://h/x")), VerifyMode::Fast);
        assert_eq!(super::effective_verify(&settings, &path("sftp://h/x")), VerifyMode::Size);
        settings.verify_remote_fast = false;
        assert_eq!(super::effective_verify(&settings, &path("agent://h/x")), VerifyMode::Size);
        settings.verify = VerifyMode::Full;
        settings.verify_remote_fast = true;
        assert_eq!(super::effective_verify(&settings, &path("agent://h/x")), VerifyMode::Full);
    }

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
