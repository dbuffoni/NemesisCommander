//! # sc-ops
//!
//! The semantics of file operations, independent of any UI and of the
//! provider that stores the files.
//!
//! The flow for copy and move is always the same:
//!
//! 1. **Plan** ([`plan`]): walk the sources and produce a [`Plan`], a flat,
//!    serializable list of idempotent [`Step`]s. Plans can be shown to the
//!    user, stored in a journal, and resumed after a crash.
//! 2. **Execute** ([`execute`]): run the steps, resolving conflicts with a
//!    [`ConflictPolicy`] and reporting progress through an [`Observer`].
//!
//! Single-file primitives ([`copy_file`], [`verify_copy`]) are public so the
//! transfer engine (`sc-transfer`) can add queuing, retries and journaling on
//! top of exactly the same behavior.

pub mod conflict;
pub mod copy;
pub mod delete;
pub mod execute;
pub mod plan;
pub mod rename;

pub use conflict::{
    Conflict, ConflictAction, ConflictPolicy, ConflictResolver, FixedResolver, Resolution,
    unique_name,
};
pub use copy::{CopyFileOptions, CopyOutcome, Throttle, VerifyMode, copy_file, verify_copy};
pub use delete::{DeleteOptions, delete_paths};
pub use execute::{
    ExecuteOptions, Executor, Observer, OpEvent, Report, RetryPolicy, StepOutcome, StepTracker,
};
pub use plan::{Plan, PlanOptions, Step, plan_copy, plan_delete, plan_move};
pub use rename::{CaseChange, RenameError, RenameRule, apply_renames, preview_renames};
