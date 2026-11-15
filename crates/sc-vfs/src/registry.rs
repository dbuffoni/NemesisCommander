//! [`ProviderRegistry`]: finds (and caches) the provider serving a path.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::{Provider, Result, VfsError, VfsPath};

/// Creates provider instances for one URI scheme.
///
/// A factory receives the registry so that wrapping providers (archives,
/// for instance) can obtain the provider of their container.
#[async_trait]
pub trait ProviderFactory: Send + Sync + 'static {
    /// The scheme this factory handles.
    fn scheme(&self) -> &str;

    /// Creates (connects, opens...) the provider for `path`'s provider key.
    async fn create(
        &self,
        path: &VfsPath,
        registry: &ProviderRegistry,
    ) -> Result<Arc<dyn Provider>>;
}

/// Maps paths to providers.
///
/// Providers are created lazily on first use and cached by
/// [`VfsPath::provider_key`], so all paths of one SFTP host share one
/// connection and all paths inside one archive share one opened index.
#[derive(Default)]
pub struct ProviderRegistry {
    factories: HashMap<String, Arc<dyn ProviderFactory>>,
    instances: Mutex<HashMap<String, Arc<dyn Provider>>>,
}

impl ProviderRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a factory, replacing any previous one for the same scheme.
    pub fn register(&mut self, factory: impl ProviderFactory) {
        self.factories.insert(factory.scheme().to_owned(), Arc::new(factory));
    }

    /// The schemes that have a factory.
    pub fn schemes(&self) -> Vec<&str> {
        let mut schemes: Vec<&str> = self.factories.keys().map(String::as_str).collect();
        schemes.sort_unstable();
        schemes
    }

    /// Returns the provider serving `path`, creating it if needed.
    pub async fn provider_for(&self, path: &VfsPath) -> Result<Arc<dyn Provider>> {
        let key = path.provider_key();
        if let Some(found) = self.lock().get(&key) {
            return Ok(found.clone());
        }
        let factory = self
            .factories
            .get(path.scheme())
            .ok_or_else(|| VfsError::UnknownScheme(path.scheme().to_owned()))?
            .clone();
        let created = factory.create(path, self).await?;
        // Another task may have raced us; keep whichever got there first so
        // that everyone shares one instance.
        Ok(self.lock().entry(key).or_insert(created).clone())
    }

    /// Installs an already-created provider for `path`'s provider key.
    /// Useful for tests and for providers that need interactive setup
    /// (asking for a password, for example) before they can be used.
    pub fn insert(&self, path: &VfsPath, provider: Arc<dyn Provider>) {
        self.lock().insert(path.provider_key(), provider);
    }

    /// Drops a cached provider (for example after a connection error), so the
    /// next call to [`provider_for`](Self::provider_for) creates a fresh one.
    pub fn forget(&self, path: &VfsPath) {
        self.lock().remove(&path.provider_key());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<dyn Provider>>> {
        // A poisoned lock only means another thread panicked while holding
        // it; the map itself is still consistent.
        self.instances.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capabilities, Entry, ListStream, ReadStream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Dummy;

    #[async_trait]
    impl Provider for Dummy {
        fn scheme(&self) -> &str {
            "mem"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::empty()
        }
        async fn stat(&self, path: &VfsPath) -> Result<Entry> {
            Err(VfsError::NotFound(path.clone()))
        }
        async fn list(&self, path: &VfsPath) -> Result<ListStream> {
            Err(VfsError::NotFound(path.clone()))
        }
        async fn read(&self, path: &VfsPath, _offset: u64) -> Result<ReadStream> {
            Err(VfsError::NotFound(path.clone()))
        }
    }

    struct CountingFactory(Arc<AtomicUsize>);

    #[async_trait]
    impl ProviderFactory for CountingFactory {
        fn scheme(&self) -> &str {
            "mem"
        }
        async fn create(&self, _: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(Dummy))
        }
    }

    #[tokio::test]
    async fn providers_are_cached_per_key() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut registry = ProviderRegistry::new();
        registry.register(CountingFactory(count.clone()));

        let a = VfsPath::parse("mem://one/x").unwrap();
        let b = VfsPath::parse("mem://one/y/z").unwrap();
        let c = VfsPath::parse("mem://two/x").unwrap();
        registry.provider_for(&a).await.unwrap();
        registry.provider_for(&b).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1, "same authority shares a provider");
        registry.provider_for(&c).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);

        registry.forget(&a);
        registry.provider_for(&a).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn unknown_scheme_is_an_error() {
        let registry = ProviderRegistry::new();
        let err = registry.provider_for(&VfsPath::parse("nope://x/").unwrap()).await.err().unwrap();
        assert!(matches!(err, VfsError::UnknownScheme(s) if s == "nope"));
    }

    #[tokio::test]
    async fn default_methods_report_unsupported() {
        let p = VfsPath::parse("mem://x/a").unwrap();
        let err = Dummy.rename(&p, &p).await.unwrap_err();
        assert!(matches!(err, VfsError::Unsupported { operation: "rename", .. }));
    }
}
