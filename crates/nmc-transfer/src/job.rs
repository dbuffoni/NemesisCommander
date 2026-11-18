//! Job descriptions and states.

use std::fmt;

use nmc_ops::{ConflictPolicy, PlanOptions, RetryPolicy, VerifyMode};
use nmc_vfs::VfsPath;
use serde::{Deserialize, Serialize};

/// Identifier of a job in the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct JobId(pub i64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobKind {
    /// Copy sources to a destination.
    Copy,
    /// Move sources to a destination.
    Move,
    /// Delete sources.
    Delete,
}

fn default_auto() -> bool {
    true
}

/// Tunables of a job, stored with it in the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobOptions {
    /// Conflict handling. Background jobs cannot ask, so `Ask` is treated
    /// as `Skip` unless the manager has a resolver.
    pub conflict: ConflictPolicy,
    /// Verification after each copied file.
    pub verify: VerifyMode,
    /// What to preserve.
    pub plan: PlanOptions,
    /// Files copied in parallel within the job (the starting level when
    /// `auto_parallelism` is on).
    pub parallelism: usize,
    /// Let the manager adjust `parallelism` while the job runs, from the
    /// measured throughput. Turned off by choosing a level by hand.
    #[serde(default = "default_auto")]
    pub auto_parallelism: bool,
    /// Retries for transient errors.
    pub retry: RetryPolicy,
    /// Per-job bandwidth limit in bytes/s (0 = only the global limit).
    pub bandwidth_limit: u64,
}

impl Default for JobOptions {
    fn default() -> Self {
        JobOptions {
            conflict: ConflictPolicy::Skip,
            verify: VerifyMode::Size,
            plan: PlanOptions::default(),
            parallelism: 4,
            auto_parallelism: true,
            retry: RetryPolicy { attempts: 3, ..Default::default() },
            bandwidth_limit: 0,
        }
    }
}

/// A request for a new job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSpec {
    /// The kind of work.
    pub kind: JobKind,
    /// Items to work on.
    pub sources: Vec<VfsPath>,
    /// Destination (ignored for deletes).
    pub destination: Option<VfsPath>,
    /// Options.
    pub options: JobOptions,
    /// Higher runs first. Jobs with equal priority run in submission order.
    pub priority: i32,
}

impl JobSpec {
    /// A short human-readable description ("Copy 3 items to /backup").
    pub fn title(&self) -> String {
        let what = match self.sources.as_slice() {
            [one] => one.file_name().unwrap_or("/").to_owned(),
            many => format!("{} items", many.len()),
        };
        match (self.kind, &self.destination) {
            (JobKind::Copy, Some(d)) => format!("Copy {what} to {d}"),
            (JobKind::Move, Some(d)) => format!("Move {what} to {d}"),
            (JobKind::Delete, _) | (_, None) => format!("Delete {what}"),
        }
    }
}

/// Lifecycle of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobState {
    /// Waiting for a free slot.
    Queued,
    /// Running now.
    Running,
    /// Stopped by the user; can be resumed.
    Paused,
    /// Finished, every step succeeded or was skipped.
    Completed,
    /// Finished with at least one failed step.
    Failed,
    /// Stopped by the user for good.
    Cancelled,
}

impl JobState {
    /// `true` once the job will never run again.
    pub fn is_final(self) -> bool {
        matches!(self, JobState::Completed | JobState::Failed | JobState::Cancelled)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Paused => "paused",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => JobState::Queued,
            "running" => JobState::Running,
            "paused" => JobState::Paused,
            "completed" => JobState::Completed,
            "failed" => JobState::Failed,
            "cancelled" => JobState::Cancelled,
            _ => return None,
        })
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A snapshot of a job for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInfo {
    /// Identifier.
    pub id: JobId,
    /// What was requested.
    pub spec: JobSpec,
    /// Current state.
    pub state: JobState,
    /// Total steps in the plan.
    pub total_steps: usize,
    /// Steps finished (done, skipped or failed).
    pub finished_steps: usize,
    /// Total bytes to copy.
    pub total_bytes: u64,
    /// Bytes copied so far (in finished steps).
    pub done_bytes: u64,
    /// Messages of failed steps.
    pub errors: Vec<String>,
}

/// Notifications from the manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEvent {
    /// A job changed state.
    StateChanged {
        /// Which job.
        id: JobId,
        /// New state.
        state: JobState,
    },
    /// A step failed with a transient error and is about to be tried again.
    Retrying {
        /// Which job.
        id: JobId,
        /// The attempt about to start (the first retry is 1).
        attempt: u32,
        /// The most retries allowed.
        attempts: u32,
        /// Why the step failed.
        message: String,
    },
    /// The number of files a running job copies at once changed (by hand or
    /// by the automatic tuning).
    ParallelismChanged {
        /// Which job.
        id: JobId,
        /// The new level.
        parallelism: usize,
        /// Whether the automatic tuning is on.
        auto: bool,
    },
    /// Bytes were written by a job (live progress, not yet journaled).
    Progress {
        /// Which job.
        id: JobId,
        /// Bytes written since the last event.
        bytes: u64,
    },
}
