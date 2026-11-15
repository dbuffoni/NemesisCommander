//! # sc-providers
//!
//! Concrete implementations of [`sc_vfs::Provider`].
//!
//! * [`local`]: the local file system (`file://`).
//! * [`memory`]: an in-memory file system (`mem://`), used by tests and as
//!   a reference implementation of the trait's semantics.
//!
//! Network providers (SFTP, the `sc` agent) live in their own crates so that
//! builds which do not need them stay small.

pub mod local;
pub mod memory;

use sc_vfs::ProviderRegistry;

/// A registry with every provider of this crate registered.
pub fn default_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(local::LocalFactory);
    registry.register(memory::MemoryFactory);
    registry
}
