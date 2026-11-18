//! The [`TransferManager`]: queue, scheduler and job runner.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nmc_ops::{
    ConflictPolicy, ConflictResolver, ExecuteOptions, Executor, OpEvent, StepOutcome, StepTracker,
    Throttle, plan_copy, plan_delete, plan_move,
};
use nmc_vfs::{ProviderRegistry, VfsError};
use tokio::sync::{Notify, broadcast};
use tokio_util::sync::CancellationToken;

use crate::job::{JobEvent, JobId, JobInfo, JobKind, JobSpec, JobState};
use crate::journal::{Journal, JournalError};
use crate::throttle::TokenBucket;
use crate::tuning::{MAX_PARALLELISM, MIN_PARALLELISM, Tuner};

/// Global settings of a [`TransferManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagerOptions {
    /// How many jobs run at the same time.
    pub max_concurrent_jobs: usize,
    /// Bandwidth limit shared by all jobs, in bytes/s (0 = unlimited).
    pub global_bandwidth_limit: u64,
}

impl Default for ManagerOptions {
    fn default() -> Self {
        ManagerOptions { max_concurrent_jobs: 2, global_bandwidth_limit: 0 }
    }
}

/// How often the automatic tuning looks at a job's throughput.
const TUNING_INTERVAL: Duration = Duration::from_secs(3);

/// The parallelism of a running job, shared with its executor and tuner.
struct ParallelControl {
    level: Arc<AtomicUsize>,
    auto: AtomicBool,
}

/// Runs jobs in the background and keeps the journal up to date.
///
/// Cloning is cheap and gives another handle to the same manager.
#[derive(Clone)]
pub struct TransferManager {
    inner: Arc<Inner>,
}

struct Inner {
    registry: Arc<ProviderRegistry>,
    journal: Journal,
    /// Jobs allowed to run at once; changeable while running.
    max_jobs: std::sync::atomic::AtomicUsize,
    resolver: Option<Arc<dyn ConflictResolver>>,
    global_throttle: Arc<TokenBucket>,
    running: Mutex<HashMap<JobId, CancellationToken>>,
    /// Live parallelism of the jobs that are copying now.
    controls: Mutex<HashMap<JobId, Arc<ParallelControl>>>,
    events: broadcast::Sender<JobEvent>,
    wake: Notify,
}

impl TransferManager {
    /// Creates a manager on top of `journal`. Jobs that were running when the
    /// previous process stopped are queued again.
    pub fn new(
        registry: Arc<ProviderRegistry>,
        journal: Journal,
        options: ManagerOptions,
    ) -> Result<Self, JournalError> {
        journal.recover_interrupted()?;
        let (events, _) = broadcast::channel(1024);
        Ok(TransferManager {
            inner: Arc::new(Inner {
                registry,
                journal,
                max_jobs: std::sync::atomic::AtomicUsize::new(options.max_concurrent_jobs),
                resolver: None,
                global_throttle: Arc::new(TokenBucket::new(options.global_bandwidth_limit)),
                running: Mutex::new(HashMap::new()),
                controls: Mutex::new(HashMap::new()),
                events,
                wake: Notify::new(),
            }),
        })
    }

    /// Sets the resolver used by jobs whose conflict policy is
    /// [`ConflictPolicy::Ask`]. Must be called before the manager is shared.
    /// Without one, `Ask` behaves like [`ConflictPolicy::Skip`].
    pub fn with_resolver(mut self, resolver: Arc<dyn ConflictResolver>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("with_resolver must be called before cloning")
            .resolver = Some(resolver);
        self
    }

    /// Subscribes to state changes and live progress.
    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.inner.events.subscribe()
    }

    /// Adds a job to the queue. Planning happens when the job starts, so this
    /// returns immediately even for huge trees.
    pub fn submit(&self, spec: JobSpec) -> Result<JobId, JournalError> {
        let id = self.inner.journal.create_job(&spec)?;
        self.inner.emit(JobEvent::StateChanged { id, state: JobState::Queued });
        self.inner.wake.notify_one();
        Ok(id)
    }

    /// Snapshots of all jobs.
    pub fn jobs(&self) -> Result<Vec<JobInfo>, JournalError> {
        self.inner.journal.jobs()
    }

    /// Snapshot of one job.
    pub fn job(&self, id: JobId) -> Result<JobInfo, JournalError> {
        self.inner.journal.job(id)
    }

    /// Pauses a queued or running job. A running job stops at the next chunk
    /// boundary; its part files are kept for resuming.
    pub fn pause(&self, id: JobId) -> Result<(), JournalError> {
        self.stop(id, JobState::Paused)
    }

    /// Cancels a job for good.
    pub fn cancel(&self, id: JobId) -> Result<(), JournalError> {
        self.stop(id, JobState::Cancelled)
    }

    fn stop(&self, id: JobId, state: JobState) -> Result<(), JournalError> {
        if self.inner.journal.job(id)?.state.is_final() {
            return Ok(());
        }
        // Journal first, then signal: the runner checks the journal before
        // writing a final state, so the pause/cancel is never lost.
        self.inner.journal.set_state(id, state)?;
        if let Some(token) = self.inner.running.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
            token.cancel();
        }
        self.inner.emit(JobEvent::StateChanged { id, state });
        Ok(())
    }

    /// Puts a paused (or failed) job back in the queue.
    pub fn resume(&self, id: JobId) -> Result<(), JournalError> {
        let state = self.inner.journal.job(id)?.state;
        if matches!(state, JobState::Paused | JobState::Failed) {
            if state == JobState::Failed {
                // Failed steps get another chance.
                self.inner.journal.reset_failed_steps(id)?;
            }
            self.inner.journal.set_state(id, JobState::Queued)?;
            self.inner.emit(JobEvent::StateChanged { id, state: JobState::Queued });
            self.inner.wake.notify_one();
        }
        Ok(())
    }

    /// Changes a job's priority (higher runs first).
    pub fn set_priority(&self, id: JobId, priority: i32) -> Result<(), JournalError> {
        self.inner.journal.set_priority(id, priority)
    }

    /// Sets how many files a job copies at the same time. A running job
    /// applies it to the files it has not started yet. Choosing a level by
    /// hand turns the automatic tuning off for this job (see
    /// [`set_auto_parallelism`](Self::set_auto_parallelism)).
    pub fn set_parallelism(&self, id: JobId, parallelism: usize) -> Result<(), JournalError> {
        let parallelism = parallelism.clamp(MIN_PARALLELISM, MAX_PARALLELISM);
        self.inner.journal.set_parallelism(id, parallelism, false)?;
        if let Some(control) = self.control(id) {
            control.level.store(parallelism, Ordering::Relaxed);
            control.auto.store(false, Ordering::Relaxed);
        }
        self.inner.emit(JobEvent::ParallelismChanged { id, parallelism, auto: false });
        Ok(())
    }

    /// Turns the automatic tuning of a job's parallelism on or off. Off keeps
    /// the current level.
    pub fn set_auto_parallelism(&self, id: JobId, auto: bool) -> Result<(), JournalError> {
        let current = self.job_parallelism(id)?;
        self.inner.journal.set_parallelism(id, current, auto)?;
        if let Some(control) = self.control(id) {
            control.auto.store(auto, Ordering::Relaxed);
        }
        self.inner.emit(JobEvent::ParallelismChanged { id, parallelism: current, auto });
        Ok(())
    }

    /// The number of files a job copies at the same time now: the live value
    /// of a running job, else the one stored with it.
    pub fn job_parallelism(&self, id: JobId) -> Result<usize, JournalError> {
        if let Some(control) = self.control(id) {
            return Ok(control.level.load(Ordering::Relaxed));
        }
        Ok(self.inner.journal.job(id)?.spec.options.parallelism)
    }

    fn control(&self, id: JobId) -> Option<Arc<ParallelControl>> {
        self.inner.controls.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned()
    }

    /// Changes the global bandwidth limit (bytes/s, 0 = unlimited).
    pub fn set_global_bandwidth_limit(&self, bytes_per_second: u64) {
        self.inner.global_throttle.set_rate(bytes_per_second);
    }

    /// Changes how many jobs run at the same time (1 runs them one after
    /// the other, in priority order). Running jobs are not stopped.
    pub fn set_max_concurrent_jobs(&self, jobs: usize) {
        self.inner.max_jobs.store(jobs.max(1), std::sync::atomic::Ordering::Relaxed);
        self.inner.wake.notify_one();
    }

    /// Removes finished jobs from the journal.
    pub fn purge_finished(&self) -> Result<usize, JournalError> {
        self.inner.journal.purge_finished()
    }

    /// Starts jobs while there are free slots. Returns how many started.
    fn fill_slots(&self) -> usize {
        let mut started = 0;
        loop {
            let running: HashSet<JobId> = self
                .inner
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .copied()
                .collect();
            if running.len()
                >= self.inner.max_jobs.load(std::sync::atomic::Ordering::Relaxed).max(1)
            {
                return started;
            }
            let Ok(Some(id)) = self.inner.journal.next_queued(&running) else {
                return started;
            };
            let token = CancellationToken::new();
            self.inner.running.lock().unwrap_or_else(|e| e.into_inner()).insert(id, token.clone());
            let inner = self.inner.clone();
            tokio::spawn(async move {
                inner.run_job(id, token).await;
                inner.running.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                inner.wake.notify_one();
            });
            started += 1;
        }
    }

    fn running_count(&self) -> usize {
        self.inner.running.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Processes the queue until nothing is queued or running, then returns.
    /// This is what the CLI uses; a GUI calls [`spawn`](Self::spawn) instead.
    pub async fn run_until_idle(&self) {
        loop {
            let notified = self.inner.wake.notified();
            self.fill_slots();
            if self.running_count() == 0 {
                return;
            }
            notified.await;
        }
    }

    /// Runs the scheduler forever on the tokio runtime.
    pub fn spawn(&self) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        tokio::spawn(async move {
            loop {
                let notified = manager.inner.wake.notified();
                manager.fill_slots();
                notified.await;
            }
        })
    }
}

impl Inner {
    fn emit(&self, event: JobEvent) {
        // No subscribers is fine.
        let _ = self.events.send(event);
    }

    async fn run_job(self: &Arc<Self>, id: JobId, cancel: CancellationToken) {
        if !matches!(self.journal.transition(id, JobState::Queued, JobState::Running), Ok(true)) {
            return; // paused or cancelled before it could start
        }
        self.emit(JobEvent::StateChanged { id, state: JobState::Running });

        let state = match self.execute(id, cancel).await {
            Ok(true) => JobState::Completed,
            Ok(false) => JobState::Failed,
            Err(JobError::Stopped) => return, // pause/cancel already journaled
            Err(JobError::Failed(message)) => {
                let _ = self.journal.set_error(id, &message);
                JobState::Failed
            }
        };
        if let Ok(true) = self.journal.transition(id, JobState::Running, state) {
            self.emit(JobEvent::StateChanged { id, state });
        }
    }

    /// Plans (if needed) and runs the job. `Ok(true)` if no step failed.
    async fn execute(
        self: &Arc<Self>,
        id: JobId,
        cancel: CancellationToken,
    ) -> Result<bool, JobError> {
        let spec = self.journal.job(id)?.spec;
        let plan = match self.journal.load_plan(id)? {
            Some(plan) => plan,
            None => {
                let plan = match spec.kind {
                    JobKind::Copy => {
                        plan_copy(
                            &self.registry,
                            &spec.sources,
                            destination(&spec)?,
                            spec.options.plan,
                        )
                        .await
                    }
                    JobKind::Move => {
                        plan_move(
                            &self.registry,
                            &spec.sources,
                            destination(&spec)?,
                            spec.options.plan,
                        )
                        .await
                    }
                    JobKind::Delete => plan_delete(&self.registry, &spec.sources).await,
                }
                .map_err(|e| JobError::Failed(e.to_string()))?;
                self.journal.store_plan(id, &plan)?;
                plan
            }
        };

        let mut throttle: Arc<dyn Throttle> = self.global_throttle.clone();
        if spec.options.bandwidth_limit > 0 {
            throttle =
                Arc::new(Both(throttle, Arc::new(TokenBucket::new(spec.options.bandwidth_limit))));
        }
        let has_resolver = self.resolver.is_some();
        let conflict = match spec.options.conflict {
            ConflictPolicy::Ask if !has_resolver => ConflictPolicy::Skip,
            other => other,
        };
        let level = Arc::new(AtomicUsize::new(spec.options.parallelism.max(1)));
        let control = Arc::new(ParallelControl {
            level: level.clone(),
            auto: AtomicBool::new(spec.options.auto_parallelism),
        });
        self.controls.lock().unwrap_or_else(|e| e.into_inner()).insert(id, control.clone());
        let options = ExecuteOptions {
            conflict,
            verify: spec.options.verify,
            parallelism: spec.options.parallelism,
            live_parallelism: Some(level),
            resume: true,
            continue_on_error: true,
            throttle: Some(throttle),
            cancel,
            retry: spec.options.retry,
        };
        let events = self.events.clone();
        let written = Arc::new(AtomicU64::new(0));
        let retries = Arc::new(AtomicU32::new(0));
        let observer = {
            let (written, retries) = (written.clone(), retries.clone());
            move |e| match e {
                OpEvent::Bytes(bytes) => {
                    written.fetch_add(bytes, Ordering::Relaxed);
                    let _ = events.send(JobEvent::Progress { id, bytes });
                }
                OpEvent::Retrying { attempt, attempts, message, .. } => {
                    retries.fetch_add(1, Ordering::Relaxed);
                    let _ = events.send(JobEvent::Retrying { id, attempt, attempts, message });
                }
                _ => {}
            }
        };
        let mut executor =
            Executor::new(self.registry.clone(), options).with_observer(Arc::new(observer));
        if let Some(resolver) = &self.resolver {
            executor = executor.with_resolver(resolver.clone());
        }
        let tracker = JournalTracker {
            journal: &self.journal,
            id,
            finished: self.journal.finished_steps(id)?,
        };
        // Only jobs with several files to copy can gain from more parallelism.
        let copies = plan.steps.iter().filter(|s| s.is_parallel()).count();
        let tuning = async {
            if copies < 2 {
                return std::future::pending::<()>().await;
            }
            let mut tuner = Tuner::new(control.level.load(Ordering::Relaxed));
            let mut last_bytes = 0;
            let mut shown = control.level.load(Ordering::Relaxed);
            loop {
                tokio::time::sleep(TUNING_INTERVAL).await;
                let bytes = written.load(Ordering::Relaxed);
                let speed = (bytes - last_bytes) as f64 / TUNING_INTERVAL.as_secs_f64();
                last_bytes = bytes;
                let failures = retries.swap(0, Ordering::Relaxed);
                let current = control.level.load(Ordering::Relaxed);
                if !control.auto.load(Ordering::Relaxed) {
                    tuner.reset(current);
                    continue;
                }
                if current != shown {
                    tuner.reset(current); // changed from outside (settings, by hand)
                }
                let next = tuner.next(speed, failures);
                control.level.store(next, Ordering::Relaxed);
                shown = next;
                if next != current {
                    self.emit(JobEvent::ParallelismChanged { id, parallelism: next, auto: true });
                }
            }
        };
        let result = tokio::select! {
            result = executor.run_tracked(&plan, Some(&tracker)) => result,
            _ = tuning => unreachable!("the tuning never ends"),
        };
        // Keep the level the job ended with (hand or automatic) for a resume.
        let _ = self.journal.set_parallelism(
            id,
            control.level.load(Ordering::Relaxed),
            control.auto.load(Ordering::Relaxed),
        );
        self.controls.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        match result {
            Ok(_) => Ok(self.journal.job(id)?.errors.is_empty()),
            Err(VfsError::Cancelled) => Err(JobError::Stopped),
            Err(e) => Err(JobError::Failed(e.to_string())),
        }
    }
}

fn destination(spec: &JobSpec) -> Result<&nmc_vfs::VfsPath, JobError> {
    spec.destination.as_ref().ok_or_else(|| JobError::Failed("job has no destination".into()))
}

enum JobError {
    /// Paused or cancelled by the user.
    Stopped,
    /// A job-level failure (planning, journal...).
    Failed(String),
}

impl From<JournalError> for JobError {
    fn from(e: JournalError) -> Self {
        JobError::Failed(e.to_string())
    }
}

/// Connects the executor to the journal.
struct JournalTracker<'a> {
    journal: &'a Journal,
    id: JobId,
    finished: HashSet<usize>,
}

impl StepTracker for JournalTracker<'_> {
    fn is_finished(&self, index: usize) -> bool {
        self.finished.contains(&index)
    }

    fn record(&self, index: usize, outcome: &StepOutcome) {
        // If the journal cannot be written the step simply runs again next
        // time, which is safe because steps are idempotent.
        let _ = self.journal.record_step(self.id, index, outcome);
    }
}

/// Applies two throttles (global limit and per-job limit).
struct Both(Arc<dyn Throttle>, Arc<dyn Throttle>);

#[async_trait]
impl Throttle for Both {
    async fn acquire(&self, bytes: usize) {
        self.0.acquire(bytes).await;
        self.1.acquire(bytes).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobOptions;
    use nmc_providers::memory::MemoryProvider;
    use nmc_vfs::{Provider, VfsPath};
    use std::time::Duration;

    fn p(host: &str, path: &str) -> VfsPath {
        VfsPath::parse(&format!("mem://{host}{path}")).unwrap()
    }

    fn setup() -> (Arc<ProviderRegistry>, MemoryProvider, MemoryProvider) {
        let registry = ProviderRegistry::new();
        let (a, b) = (MemoryProvider::new(), MemoryProvider::new());
        registry.insert(&p("a", "/"), Arc::new(a.clone()));
        registry.insert(&p("b", "/"), Arc::new(b.clone()));
        (Arc::new(registry), a, b)
    }

    fn copy_spec(priority: i32) -> JobSpec {
        JobSpec {
            kind: JobKind::Copy,
            sources: vec![p("a", "/src")],
            destination: Some(p("b", "/")),
            options: JobOptions::default(),
            priority,
        }
    }

    #[tokio::test]
    async fn runs_jobs_to_completion() {
        let (registry, a, b) = setup();
        a.put_file("/src/x", "1");
        a.put_file("/src/y/z", "22");
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap();
        let id = manager.submit(copy_spec(0)).unwrap();
        manager.run_until_idle().await;
        let info = manager.job(id).unwrap();
        assert_eq!(info.state, JobState::Completed, "{:?}", info.errors);
        assert_eq!(info.done_bytes, 3);
        assert_eq!(b.get_file("/src/y/z").unwrap(), b"22");
    }

    #[tokio::test]
    async fn delete_and_move_jobs() {
        let (registry, a, b) = setup();
        a.put_file("/src/x", "1");
        a.put_file("/junk/y", "1");
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap();
        let mv = JobSpec { kind: JobKind::Move, ..copy_spec(0) };
        let del = JobSpec {
            kind: JobKind::Delete,
            sources: vec![p("a", "/junk")],
            destination: None,
            ..copy_spec(0)
        };
        manager.submit(mv).unwrap();
        manager.submit(del).unwrap();
        manager.run_until_idle().await;
        assert_eq!(a.paths(), ["/"]);
        assert_eq!(b.get_file("/src/x").unwrap(), b"1");
    }

    #[tokio::test]
    async fn planning_failure_fails_the_job() {
        let (registry, _, _) = setup();
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap();
        let id = manager.submit(copy_spec(0)).unwrap(); // /src does not exist
        manager.run_until_idle().await;
        let info = manager.job(id).unwrap();
        assert_eq!(info.state, JobState::Failed);
        assert!(info.errors[0].contains("not found"), "{:?}", info.errors);
    }

    #[tokio::test]
    async fn priority_decides_the_order() {
        let (registry, a, _) = setup();
        a.put_file("/src/x", "1");
        let options = ManagerOptions { max_concurrent_jobs: 1, ..Default::default() };
        let manager =
            TransferManager::new(registry, Journal::open_in_memory().unwrap(), options).unwrap();
        let mut events = manager.subscribe();
        let low = manager.submit(copy_spec(0)).unwrap();
        let high =
            manager.submit(JobSpec { destination: Some(p("b", "/hi")), ..copy_spec(10) }).unwrap();
        manager.run_until_idle().await;
        let mut started = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let JobEvent::StateChanged { id, state: JobState::Running } = event {
                started.push(id);
            }
        }
        assert_eq!(started, [high, low]);
    }

    #[tokio::test]
    async fn pause_and_resume_continue_where_they_stopped() {
        let (registry, a, b) = setup();
        for i in 0..50 {
            a.put_file(&format!("/src/f{i:02}"), vec![b'x'; 64 * 1024]);
        }
        // 1 MB/s global limit: the job takes ~3 seconds, plenty of time to pause.
        let options = ManagerOptions { max_concurrent_jobs: 1, global_bandwidth_limit: 1_000_000 };
        let manager =
            TransferManager::new(registry, Journal::open_in_memory().unwrap(), options).unwrap();
        let id = manager.submit(copy_spec(0)).unwrap();
        let runner = {
            let m = manager.clone();
            tokio::spawn(async move { m.run_until_idle().await })
        };
        tokio::time::sleep(Duration::from_millis(400)).await;
        manager.pause(id).unwrap();
        runner.await.unwrap();
        let paused = manager.job(id).unwrap();
        assert_eq!(paused.state, JobState::Paused);
        assert!(paused.finished_steps < paused.total_steps, "paused too late to test anything");

        manager.set_global_bandwidth_limit(0);
        manager.resume(id).unwrap();
        manager.run_until_idle().await;
        let done = manager.job(id).unwrap();
        assert_eq!(done.state, JobState::Completed, "{:?}", done.errors);
        for i in 0..50 {
            assert_eq!(b.get_file(&format!("/src/f{i:02}")).unwrap().len(), 64 * 1024);
        }
        assert!(b.paths().iter().all(|p| !p.ends_with(".nmc-part")));
    }

    #[tokio::test]
    async fn cancel_before_start() {
        let (registry, a, b) = setup();
        a.put_file("/src/x", "1");
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap();
        let id = manager.submit(copy_spec(0)).unwrap();
        manager.cancel(id).unwrap();
        manager.run_until_idle().await;
        assert_eq!(manager.job(id).unwrap().state, JobState::Cancelled);
        assert!(b.get_file("/src/x").is_none());
        // resume does nothing on a cancelled job
        manager.resume(id).unwrap();
        assert_eq!(manager.job(id).unwrap().state, JobState::Cancelled);
    }

    #[tokio::test]
    async fn restart_resumes_unfinished_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("journal.db");
        let (registry, a, b) = setup();
        a.put_file("/src/x", "1");
        a.put_file("/src/y", "2");
        let id = {
            // Simulate a crash: the job was planned and marked running, one step done.
            let journal = Journal::open(&db).unwrap();
            let id = journal.create_job(&copy_spec(0)).unwrap();
            let plan = plan_copy(&registry, &[p("a", "/src")], &p("b", "/"), Default::default())
                .await
                .unwrap();
            journal.store_plan(id, &plan).unwrap();
            journal.set_state(id, JobState::Running).unwrap();
            id
        };
        let manager =
            TransferManager::new(registry, Journal::open(&db).unwrap(), ManagerOptions::default())
                .unwrap();
        assert_eq!(manager.job(id).unwrap().state, JobState::Queued);
        manager.run_until_idle().await;
        assert_eq!(manager.job(id).unwrap().state, JobState::Completed);
        assert_eq!(b.get_file("/src/y").unwrap(), b"2");
    }

    #[tokio::test]
    async fn failed_jobs_can_be_retried() {
        let (registry, a, b) = setup();
        a.put_file("/src/x", "new");
        b.put_file("/src/x", "old");
        let mut spec = copy_spec(0);
        spec.options.conflict = ConflictPolicy::Fail;
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap();
        let id = manager.submit(spec).unwrap();
        manager.run_until_idle().await;
        assert_eq!(manager.job(id).unwrap().state, JobState::Failed);
        b.remove_file(&p("b", "/src/x")).await.unwrap();
        manager.resume(id).unwrap();
        manager.run_until_idle().await;
        assert_eq!(manager.job(id).unwrap().state, JobState::Completed);
        assert_eq!(b.get_file("/src/x").unwrap(), b"new");
    }
}
