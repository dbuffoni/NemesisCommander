//! SQLite journal of jobs and their steps.
//!
//! The database runs in WAL mode with `synchronous = NORMAL`: every step
//! outcome is a tiny transaction, durable across application crashes, and
//! cheap enough to write once per copied file.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use pnxr_ops::{Plan, Step, StepOutcome};
use rusqlite::{Connection, OptionalExtension, params};

use crate::job::{JobId, JobInfo, JobSpec, JobState};

/// Errors from the journal.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// SQLite reported an error.
    #[error("journal database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Stored data could not be decoded (corrupt or from a newer version).
    #[error("journal data: {0}")]
    Decode(#[from] serde_json::Error),
    /// No job with this id.
    #[error("no job {0}")]
    NoSuchJob(JobId),
}

type Result<T> = std::result::Result<T, JournalError>;

/// Current schema version, stored in SQLite's `user_version`.
const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS jobs (
    id          INTEGER PRIMARY KEY,
    spec        TEXT    NOT NULL,          -- JobSpec as JSON
    state       TEXT    NOT NULL,
    priority    INTEGER NOT NULL,
    created     INTEGER NOT NULL,          -- unix seconds
    planned     INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER NOT NULL DEFAULT 0,
    error       TEXT                       -- job-level failure, if any
);
CREATE TABLE IF NOT EXISTS steps (
    job_id  INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    idx     INTEGER NOT NULL,
    step    TEXT    NOT NULL,              -- pnxr_ops::Step as JSON
    size    INTEGER NOT NULL DEFAULT 0,
    outcome TEXT,                          -- NULL until the step finished
    message TEXT,
    PRIMARY KEY (job_id, idx)
);
";

/// Persistent store of jobs. Safe to share between threads.
pub struct Journal {
    conn: Mutex<Connection>,
}

impl Journal {
    /// Opens (or creates) the journal file at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    /// A journal that lives only in memory (tests, `--no-journal`).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Journal { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stores a new, not yet planned job in state [`JobState::Queued`].
    pub fn create_job(&self, spec: &JobSpec) -> Result<JobId> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let conn = self.conn();
        conn.execute(
            "INSERT INTO jobs (spec, state, priority, created) VALUES (?1, ?2, ?3, ?4)",
            params![
                serde_json::to_string(spec)?,
                JobState::Queued.as_str(),
                spec.priority,
                now as i64
            ],
        )?;
        Ok(JobId(conn.last_insert_rowid()))
    }

    /// Stores the plan of a job, replacing any previous one.
    pub fn store_plan(&self, id: JobId, plan: &Plan) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM steps WHERE job_id = ?1", [id.0])?;
        {
            let mut insert =
                tx.prepare("INSERT INTO steps (job_id, idx, step, size) VALUES (?1, ?2, ?3, ?4)")?;
            for (idx, step) in plan.steps.iter().enumerate() {
                let size = match step {
                    Step::CopyFile { size, .. } => *size as i64,
                    _ => 0,
                };
                insert.execute(params![id.0, idx as i64, serde_json::to_string(step)?, size])?;
            }
        }
        let changed = tx.execute(
            "UPDATE jobs SET planned = 1, total_bytes = ?2 WHERE id = ?1",
            params![id.0, plan.total_bytes as i64],
        )?;
        if changed == 0 {
            return Err(JournalError::NoSuchJob(id));
        }
        tx.commit()?;
        Ok(())
    }

    /// The stored plan, or `None` if the job has not been planned yet.
    pub fn load_plan(&self, id: JobId) -> Result<Option<Plan>> {
        let conn = self.conn();
        let planned: Option<(bool, i64)> = conn
            .query_row("SELECT planned, total_bytes FROM jobs WHERE id = ?1", [id.0], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let Some((planned, total_bytes)) = planned else {
            return Err(JournalError::NoSuchJob(id));
        };
        if !planned {
            return Ok(None);
        }
        let mut query = conn.prepare("SELECT step FROM steps WHERE job_id = ?1 ORDER BY idx")?;
        let steps = query
            .query_map([id.0], |r| r.get::<_, String>(0))?
            .map(|json| Ok(serde_json::from_str::<Step>(&json?)?))
            .collect::<Result<Vec<Step>>>()?;
        let total_files = steps.iter().filter(|s| s.is_parallel()).count() as u64;
        Ok(Some(Plan { steps, total_bytes: total_bytes as u64, total_files, warnings: Vec::new() }))
    }

    /// Indices of steps that already have an outcome.
    pub fn finished_steps(&self, id: JobId) -> Result<HashSet<usize>> {
        let conn = self.conn();
        let mut query =
            conn.prepare("SELECT idx FROM steps WHERE job_id = ?1 AND outcome IS NOT NULL")?;
        let rows = query.query_map([id.0], |r| r.get::<_, i64>(0))?;
        Ok(rows.map(|r| r.map(|i| i as usize)).collect::<rusqlite::Result<_>>()?)
    }

    /// Records the final outcome of one step.
    pub fn record_step(&self, id: JobId, idx: usize, outcome: &StepOutcome) -> Result<()> {
        let (outcome, message) = match outcome {
            StepOutcome::Done => ("done", None),
            StepOutcome::Skipped => ("skipped", None),
            StepOutcome::Failed(m) => ("failed", Some(m.as_str())),
        };
        self.conn().execute(
            "UPDATE steps SET outcome = ?3, message = ?4 WHERE job_id = ?1 AND idx = ?2",
            params![id.0, idx as i64, outcome, message],
        )?;
        Ok(())
    }

    /// Clears the outcome of failed steps (and the job error) so that they
    /// run again when the job is resumed.
    pub fn reset_failed_steps(&self, id: JobId) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE steps SET outcome = NULL, message = NULL WHERE job_id = ?1 AND outcome = 'failed'",
            [id.0],
        )?;
        conn.execute("UPDATE jobs SET error = NULL WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// Sets the state unconditionally.
    pub fn set_state(&self, id: JobId, state: JobState) -> Result<()> {
        let changed = self
            .conn()
            .execute("UPDATE jobs SET state = ?2 WHERE id = ?1", params![id.0, state.as_str()])?;
        if changed == 0 { Err(JournalError::NoSuchJob(id)) } else { Ok(()) }
    }

    /// Sets the state only if the job is currently in `expected`. Returns
    /// whether it changed. Used to avoid overwriting a pause or cancel that
    /// happened while the job was finishing.
    pub fn transition(&self, id: JobId, expected: JobState, state: JobState) -> Result<bool> {
        let changed = self.conn().execute(
            "UPDATE jobs SET state = ?3 WHERE id = ?1 AND state = ?2",
            params![id.0, expected.as_str(), state.as_str()],
        )?;
        Ok(changed > 0)
    }

    /// Stores a job-level error message (planning failed, ...).
    pub fn set_error(&self, id: JobId, message: &str) -> Result<()> {
        self.conn().execute("UPDATE jobs SET error = ?2 WHERE id = ?1", params![id.0, message])?;
        Ok(())
    }

    /// Changes how many files a job copies at once and whether that is
    /// adjusted automatically.
    pub fn set_parallelism(&self, id: JobId, parallelism: usize, auto: bool) -> Result<()> {
        let conn = self.conn();
        let spec_json: String = conn
            .query_row("SELECT spec FROM jobs WHERE id = ?1", [id.0], |r| r.get(0))
            .optional()?
            .ok_or(JournalError::NoSuchJob(id))?;
        let mut spec: JobSpec = serde_json::from_str(&spec_json)?;
        spec.options.parallelism = parallelism;
        spec.options.auto_parallelism = auto;
        conn.execute(
            "UPDATE jobs SET spec = ?2 WHERE id = ?1",
            params![id.0, serde_json::to_string(&spec)?],
        )?;
        Ok(())
    }

    /// Changes a job's priority (higher runs first).
    pub fn set_priority(&self, id: JobId, priority: i32) -> Result<()> {
        let conn = self.conn();
        let spec_json: String = conn
            .query_row("SELECT spec FROM jobs WHERE id = ?1", [id.0], |r| r.get(0))
            .optional()?
            .ok_or(JournalError::NoSuchJob(id))?;
        let mut spec: JobSpec = serde_json::from_str(&spec_json)?;
        spec.priority = priority;
        conn.execute(
            "UPDATE jobs SET priority = ?2, spec = ?3 WHERE id = ?1",
            params![id.0, priority, serde_json::to_string(&spec)?],
        )?;
        Ok(())
    }

    /// The queued job that should run next: highest priority, then oldest.
    pub fn next_queued(&self, exclude: &HashSet<JobId>) -> Result<Option<JobId>> {
        let conn = self.conn();
        let mut query = conn
            .prepare("SELECT id FROM jobs WHERE state = 'queued' ORDER BY priority DESC, id ASC")?;
        let ids = query.query_map([], |r| r.get::<_, i64>(0))?;
        for id in ids {
            let id = JobId(id?);
            if !exclude.contains(&id) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Puts jobs that were running when the application stopped back in
    /// the queue. Returns how many were recovered.
    pub fn recover_interrupted(&self) -> Result<usize> {
        Ok(self.conn().execute("UPDATE jobs SET state = 'queued' WHERE state = 'running'", [])?)
    }

    /// Deletes every job in a final state.
    pub fn purge_finished(&self) -> Result<usize> {
        Ok(self
            .conn()
            .execute("DELETE FROM jobs WHERE state IN ('completed', 'failed', 'cancelled')", [])?)
    }

    /// A snapshot of one job.
    pub fn job(&self, id: JobId) -> Result<JobInfo> {
        self.jobs_where("WHERE j.id = ?1", params![id.0])?.pop().ok_or(JournalError::NoSuchJob(id))
    }

    /// Snapshots of every job, oldest first.
    pub fn jobs(&self) -> Result<Vec<JobInfo>> {
        self.jobs_where("", params![])
    }

    fn jobs_where(&self, filter: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<JobInfo>> {
        let conn = self.conn();
        let sql = format!(
            "SELECT j.id, j.spec, j.state, j.total_bytes, j.error,
                    COUNT(s.idx),
                    COUNT(s.outcome),
                    COALESCE(SUM(CASE WHEN s.outcome = 'done' THEN s.size ELSE 0 END), 0)
             FROM jobs j LEFT JOIN steps s ON s.job_id = j.id
             {filter}
             GROUP BY j.id ORDER BY j.id"
        );
        let mut query = conn.prepare(&sql)?;
        let rows = query.query_map(args, |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            let (id, spec, state, total_bytes, error, total, finished, done_bytes) = row?;
            let mut errors: Vec<String> = error.into_iter().collect();
            let mut failed = conn.prepare_cached(
                "SELECT message FROM steps WHERE job_id = ?1 AND outcome = 'failed' ORDER BY idx",
            )?;
            for message in failed.query_map([id], |r| r.get::<_, Option<String>>(0))? {
                errors.extend(message?);
            }
            jobs.push(JobInfo {
                id: JobId(id),
                spec: serde_json::from_str(&spec)?,
                state: JobState::parse(&state).unwrap_or(JobState::Failed),
                total_steps: total as usize,
                finished_steps: finished as usize,
                total_bytes: total_bytes as u64,
                done_bytes: done_bytes as u64,
                errors,
            });
        }
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobKind, JobOptions};
    use pnxr_vfs::VfsPath;

    fn spec(priority: i32) -> JobSpec {
        JobSpec {
            kind: JobKind::Copy,
            sources: vec![VfsPath::parse("/a").unwrap()],
            destination: Some(VfsPath::parse("/b").unwrap()),
            options: JobOptions::default(),
            priority,
        }
    }

    fn plan() -> Plan {
        let copy = |n: &str, size| Step::CopyFile {
            from: VfsPath::parse(&format!("/a/{n}")).unwrap(),
            to: VfsPath::parse(&format!("/b/{n}")).unwrap(),
            size,
            modified: None,
            permissions: None,
            os_flags: None,
            xattrs: Default::default(),
        };
        Plan {
            steps: vec![
                Step::CreateDir { path: VfsPath::parse("/b").unwrap() },
                copy("x", 10),
                copy("y", 20),
            ],
            total_bytes: 30,
            total_files: 2,
            warnings: vec![],
        }
    }

    #[test]
    fn plan_roundtrip_and_progress() {
        let journal = Journal::open_in_memory().unwrap();
        let id = journal.create_job(&spec(0)).unwrap();
        assert_eq!(journal.load_plan(id).unwrap(), None);
        journal.store_plan(id, &plan()).unwrap();
        let loaded = journal.load_plan(id).unwrap().unwrap();
        assert_eq!(loaded.steps, plan().steps);
        assert_eq!(loaded.total_bytes, 30);

        journal.record_step(id, 0, &StepOutcome::Done).unwrap();
        journal.record_step(id, 2, &StepOutcome::Done).unwrap();
        journal.record_step(id, 1, &StepOutcome::Failed("disk full".into())).unwrap();
        let info = journal.job(id).unwrap();
        assert_eq!(info.total_steps, 3);
        assert_eq!(info.finished_steps, 3);
        assert_eq!(info.done_bytes, 20);
        assert_eq!(info.errors, ["disk full"]);
        assert_eq!(journal.finished_steps(id).unwrap(), HashSet::from([0, 1, 2]));
    }

    #[test]
    fn scheduling_order_is_priority_then_age() {
        let journal = Journal::open_in_memory().unwrap();
        let low = journal.create_job(&spec(0)).unwrap();
        let high = journal.create_job(&spec(5)).unwrap();
        let low2 = journal.create_job(&spec(0)).unwrap();
        assert_eq!(journal.next_queued(&HashSet::new()).unwrap(), Some(high));
        assert_eq!(journal.next_queued(&HashSet::from([high])).unwrap(), Some(low));
        journal.set_priority(low2, 9).unwrap();
        assert_eq!(journal.next_queued(&HashSet::new()).unwrap(), Some(low2));
        assert_eq!(journal.job(low2).unwrap().spec.priority, 9);
    }

    #[test]
    fn transitions_are_conditional() {
        let journal = Journal::open_in_memory().unwrap();
        let id = journal.create_job(&spec(0)).unwrap();
        journal.set_state(id, JobState::Paused).unwrap();
        assert!(!journal.transition(id, JobState::Running, JobState::Completed).unwrap());
        assert_eq!(journal.job(id).unwrap().state, JobState::Paused);
    }

    #[test]
    fn survives_reopen_and_recovers_running_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.db");
        let id = {
            let journal = Journal::open(&path).unwrap();
            let id = journal.create_job(&spec(0)).unwrap();
            journal.store_plan(id, &plan()).unwrap();
            journal.record_step(id, 0, &StepOutcome::Done).unwrap();
            journal.set_state(id, JobState::Running).unwrap();
            id
        };
        let journal = Journal::open(&path).unwrap();
        assert_eq!(journal.recover_interrupted().unwrap(), 1);
        let info = journal.job(id).unwrap();
        assert_eq!(info.state, JobState::Queued);
        assert_eq!(journal.finished_steps(id).unwrap(), HashSet::from([0]));
    }

    #[test]
    fn purge_keeps_active_jobs() {
        let journal = Journal::open_in_memory().unwrap();
        let done = journal.create_job(&spec(0)).unwrap();
        let active = journal.create_job(&spec(0)).unwrap();
        journal.store_plan(done, &plan()).unwrap();
        journal.set_state(done, JobState::Completed).unwrap();
        assert_eq!(journal.purge_finished().unwrap(), 1);
        assert_eq!(journal.jobs().unwrap().iter().map(|j| j.id).collect::<Vec<_>>(), [active]);
    }

    #[test]
    fn unknown_jobs_are_errors() {
        let journal = Journal::open_in_memory().unwrap();
        assert!(matches!(journal.job(JobId(42)), Err(JournalError::NoSuchJob(_))));
        assert!(matches!(
            journal.set_state(JobId(42), JobState::Paused),
            Err(JournalError::NoSuchJob(_))
        ));
    }
}
