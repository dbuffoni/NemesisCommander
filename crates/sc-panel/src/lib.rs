//! # sc-panel
//!
//! The state behind a dual-pane file manager, shared by the TUI and the GUI
//! so both behave identically:
//!
//! * [`Panel`]: one directory view: location, sorted and filtered entries,
//!   cursor, selection, quick filter and history.
//! * [`Side`]: the tabs of one half of the window.
//! * [`Workspace`]: both sides and which one is active.
//!
//! The model never does I/O itself. Navigating returns a [`LoadRequest`];
//! the front end lists the directory (asynchronously, with [`load`]) and
//! hands the result back with [`Panel::apply`]. Results of requests that
//! were superseded in the meantime are ignored, so a slow remote listing can
//! never overwrite a newer one.

pub mod format;
mod panel;
mod workspace;

pub use panel::{Listing, LoadRequest, Panel, SelectionSummary, load};
pub use workspace::{Side, SideId, Workspace};
