//! # pnxr-index
//!
//! Everything about *finding* and *ordering* entries:
//!
//! * [`sort`]: natural, directories-first ordering by name, extension,
//!   size or date.
//! * [`mask`]: wildcard masks such as `*.rs;*.toml;!target*`, used for
//!   "select by mask", panel filters and search.
//! * [`search`]: recursive search by name, size, date and content on any
//!   provider, streaming results as they are found.

pub mod mask;
pub mod search;
pub mod sort;

pub use mask::{Mask, MaskError};
pub use search::{
    ArchiveOpener, ContentQuery, SearchCriteria, SearchHit, search, search_in_archives,
};
pub use sort::{SortKey, SortSpec, natural_cmp, sort_entries};
