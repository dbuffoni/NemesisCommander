//! Lets a search look inside archives.

use std::sync::Arc;

use futures::future::BoxFuture;
use pnxr_index::ArchiveOpener;
use pnxr_vfs::{Provider, ProviderRegistry, Result, VfsPath};

/// Opens the archives a search meets with the providers of the application:
/// a file whose name says it is an archive is read by the provider of its
/// format (`zip://`, `tar://`, ...).
pub struct RegistryArchives(pub Arc<ProviderRegistry>);

impl ArchiveOpener for RegistryArchives {
    fn root_of(&self, file: &VfsPath) -> Option<VfsPath> {
        pnxr_archive::archive_root(file)
    }

    fn provider_for<'a>(&'a self, root: &'a VfsPath) -> BoxFuture<'a, Result<Arc<dyn Provider>>> {
        Box::pin(self.0.provider_for(root))
    }
}
