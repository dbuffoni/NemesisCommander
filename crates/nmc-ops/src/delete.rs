//! Deleting files and directory trees, optionally through the recycle bin.

use std::sync::Arc;

use nmc_vfs::{Capabilities, ProviderRegistry, Result, VfsPath};

use crate::execute::{ExecuteOptions, Executor, Observer, Report};
use crate::plan::plan_delete;

/// Options for [`delete_paths`].
#[derive(Clone, Default)]
pub struct DeleteOptions {
    /// Move items to the recycle bin when the provider supports it, instead
    /// of deleting them permanently. Providers without a bin delete.
    pub use_trash: bool,
    /// Progress observer.
    pub observer: Option<Arc<dyn Observer>>,
}

/// Deletes `paths` (recursively for directories).
pub async fn delete_paths(
    registry: &Arc<ProviderRegistry>,
    paths: &[VfsPath],
    options: DeleteOptions,
) -> Result<Report> {
    let mut report = Report::default();
    let mut permanent = Vec::new();
    for path in paths {
        let provider = registry.provider_for(path).await?;
        if options.use_trash && provider.capabilities().contains(Capabilities::TRASH) {
            match provider.trash(path).await {
                Ok(()) => report.done += 1,
                Err(e) => report.failed.push((0, e.to_string())),
            }
        } else {
            permanent.push(path.clone());
        }
    }
    if !permanent.is_empty() {
        let plan = plan_delete(registry, &permanent).await?;
        let executor = Executor::new(registry.clone(), ExecuteOptions::default());
        let executor = match options.observer {
            Some(observer) => executor.with_observer(observer),
            None => executor,
        };
        let result = executor.run(&plan).await?;
        report.done += result.done;
        report.failed.extend(result.failed);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmc_providers::memory::MemoryProvider;

    #[tokio::test]
    async fn deletes_trees() {
        let registry = ProviderRegistry::new();
        let mem = MemoryProvider::new();
        mem.put_file("/a/b/c", "1");
        mem.put_file("/keep", "1");
        let root = VfsPath::parse("mem://m/").unwrap();
        registry.insert(&root, Arc::new(mem.clone()));
        let registry = Arc::new(registry);
        // memory has no trash: falls back to permanent deletion
        let options = DeleteOptions { use_trash: true, observer: None };
        let report = delete_paths(&registry, &[root.join("a")], options).await.unwrap();
        assert_eq!(report.done, 3);
        assert_eq!(mem.paths(), ["/", "/keep"]);
    }
}
