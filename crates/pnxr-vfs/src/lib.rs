//! # pnxr-vfs
//!
//! The virtual file system layer shared by every part of Pairnox.
//!
//! Everything the application can browse (the local disk, an SFTP server,
//! a zip archive, a remote `pnxr` agent, ...) is exposed through the same small
//! set of types:
//!
//! * [`VfsPath`]: a location, written as a URI such as `file:///home/me`,
//!   `sftp://me@host/srv` or `zip:///tmp/a.zip!/docs`.
//! * [`Entry`]: the metadata of one item in a directory.
//! * [`Capabilities`]: what a provider can do natively (seek, atomic rename,
//!   server-side hashing, ...). Higher layers pick a strategy from these flags
//!   instead of assuming local-disk semantics.
//! * [`Provider`]: the async trait every backend implements.
//! * [`ProviderRegistry`]: maps a [`VfsPath`] to the provider that serves it.
//!
//! This crate has no knowledge of any concrete backend and no UI dependency.

mod capabilities;
mod entry;
mod error;
pub mod names;
pub mod proc;
mod path;
mod provider;
mod registry;
mod upload;
mod walk;
pub mod winpath;

pub use capabilities::Capabilities;
pub use entry::{Entry, EntryKind, os_flags};
pub use error::{Result, VfsError};
pub use path::{ARCHIVE_SCHEMES, VfsPath};
pub use provider::{
    HashAlgorithm, ListStream, Provider, ReadStream, SetAttributes, WriteMode, WriteOptions,
    WriteSink, collect_listing,
};
pub use registry::{ProviderFactory, ProviderRegistry};
pub use upload::{UPLOAD_CHUNK, UploadBody, upload_sink};
pub use walk::{WalkItem, walk};
