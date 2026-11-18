//! # nmc-transfer
//!
//! The background transfer engine.
//!
//! A [`TransferManager`] accepts jobs (copy, move, delete), plans them with
//! `nmc-ops`, stores the plan in a [`Journal`] (SQLite in WAL mode) and runs
//! them according to priority. Because every step's result is written to the
//! journal, jobs survive crashes and application restarts: on the next start
//! unfinished jobs are queued again and continue with the first step that did
//! not finish, resuming partially copied files from their part files.
//!
//! Bandwidth limits are implemented by [`TokenBucket`].

mod job;
mod journal;
mod manager;
mod throttle;
mod tuning;

pub use job::{JobEvent, JobId, JobInfo, JobKind, JobOptions, JobSpec, JobState};
pub use journal::{Journal, JournalError};
pub use manager::{ManagerOptions, ServerCopier, ServerCopyReport, TransferManager};
pub use throttle::TokenBucket;
pub use tuning::{MAX_PARALLELISM, MIN_PARALLELISM, Tuner};
