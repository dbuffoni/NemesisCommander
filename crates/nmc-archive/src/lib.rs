//! # nmc-archive
//!
//! Native archive support. Archives are first-class citizens of the VFS:
//!
//! * **Browsing**: [`ArchiveProvider`] serves `zip://`, `tar://`, `7z://`,
//!   `rar://` (read-only, optional feature) and single-file compressors
//!   (`gz://`, `xz://`, `bz2://`, `zst://`) so an archive opens like a folder.
//!   Extracting is therefore just a copy from the archive to a folder, with
//!   all the conflict handling and progress of `nmc-ops`.
//! * **Packing**: [`pack`] writes zip, tar (plain, gz, xz, bz2, zstd), 7z
//!   and single compressed files from any provider's files.
//!
//! The format libraries are synchronous; they run on blocking threads and
//! talk to the async world through bounded channels, so memory stays
//! constant regardless of entry size.

mod compression;
mod format;
mod index;
mod pack;
mod password;
mod pipe;
mod provider;
mod readers;
mod time;

pub use compression::Compression;
pub use format::{ArchiveFormat, archive_root, detect_format};
pub use pack::{PackOptions, pack};
pub use password::{forget as forget_password, needs_password, set_password, was_refused};
pub use provider::{ArchiveFactory, ArchiveProvider, register_archive_providers};
