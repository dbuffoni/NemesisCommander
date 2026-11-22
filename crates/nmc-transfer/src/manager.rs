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
use nmc_vfs::{ProviderRegistry, VfsError, VfsPath};
use tokio::sync::{Notify, broadcast};
use tokio_util::sync::CancellationToken;

use crate::job::{JobEvent, JobId, JobInfo, JobKind, JobSpec, JobState};
use crate::journal::{Journal, JournalError};
use crate::stats::{self, Owner, StatsStore};
use crate::throttle::TokenBucket;
use crate::tuning::{MAX_PARALLELISM, MIN_PARALLELISM, Profile, Tuner};

/// Global settings of a [`TransferManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagerOptions {
    /// How many jobs run at the same time.
    pub max_concurrent_jobs: usize,
    /// Bandwidth limit shared by all jobs, in bytes/s (0 = unlimited).
    pub global_bandwidth_limit: u64,
    /// How boldly the automatic parallelism raises the number of files.
    pub profile: Profile,
}

impl Default for ManagerOptions {
    fn default() -> Self {
        ManagerOptions {
            max_concurrent_jobs: 2,
            global_bandwidth_limit: 0,
            profile: Profile::Balanced,
        }
    }
}

/// Copies between two `nmc serve` servers without passing the data through
/// this machine. `nmc-app` implements it with `nmc-agent`'s routes (direct,
/// then reverse); `nmc-transfer` does not depend on the agent.
#[async_trait]
pub trait ServerCopier: Send + Sync {
    /// Tries to copy `sources` to `destination` between servers. `None`:
    /// not applicable here (not two servers, no certificates, no server can
    /// reach the other): the job then copies through this machine. `progress`
    /// takes the bytes done and the total.
    async fn copy(
        &self,
        sources: &[VfsPath],
        destination: &VfsPath,
        conflict: ConflictPolicy,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Option<Result<ServerCopyReport, String>>;

    /// Whether `copy` would try to copy these between servers (so a
    /// question about existing files must be asked before, once, since
    /// nobody can answer while the servers copy). Default: no.
    fn between_servers(&self, _sources: &[VfsPath], _destination: &VfsPath) -> bool {
        false
    }
}

/// The outcome of a copy between servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCopyReport {
    /// How the data travelled: `direct` or `reverse`.
    pub route: String,
    /// Bytes copied.
    pub bytes: u64,
    /// Files that failed, with the reason.
    pub failures: Vec<String>,
}

fn profile_code(profile: Profile) -> u8 {
    match profile {
        Profile::Balanced => 0,
        Profile::MaxSpeed => 1,
        Profile::Discreet => 2,
    }
}

fn profile_of(code: u8) -> Profile {
    match code {
        1 => Profile::MaxSpeed,
        2 => Profile::Discreet,
        _ => Profile::Balanced,
    }
}

/// How often the automatic tuning looks at a job's throughput.
const TUNING_INTERVAL: Duration = Duration::from_secs(3);

/// The parallelism of a running job, shared with its executor and tuner.
struct ParallelControl {
    level: Arc<AtomicUsize>,
    auto: AtomicBool,
    /// The servers the job talks to (see [`endpoint_key`]).
    endpoints: Vec<String>,
    /// Why the automatic mode last changed the level, in words.
    reason: Mutex<String>,
}

/// What is known about how many files a server takes at once.
#[derive(Debug, Clone, Copy, Default)]
struct EndpointInfo {
    /// Set on the connection by the user (or saved from a suggestion).
    configured: Option<usize>,
    /// Learned from repeated errors in this run.
    learned: Option<usize>,
    /// The level a job ended with at its best, to start from next time.
    good: Option<usize>,
}

impl EndpointInfo {
    fn limit(&self) -> Option<usize> {
        self.configured.or(self.learned)
    }
}

/// The server a path lives on, as `scheme://authority` (`None` for local
/// paths). Archives count as the place that holds them.
pub fn endpoint_key(path: &nmc_vfs::VfsPath) -> Option<String> {
    let mut path = path;
    while let Some(container) = path.container() {
        path = container;
    }
    if path.is_local() { None } else { Some(format!("{}://{}", path.scheme(), path.authority())) }
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
    server_copier: Option<Arc<dyn ServerCopier>>,
    global_throttle: Arc<TokenBucket>,
    running: Mutex<HashMap<JobId, CancellationToken>>,
    /// Live parallelism of the jobs that are copying now.
    controls: Mutex<HashMap<JobId, Arc<ParallelControl>>>,
    profile: std::sync::atomic::AtomicU8,
    endpoints: Mutex<HashMap<String, EndpointInfo>>,
    /// What worked on each server, kept in a small file.
    stats: Mutex<StatsStore>,
    /// The saved connection of each server.
    owners: Mutex<HashMap<String, Owner>>,
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
                server_copier: None,
                global_throttle: Arc::new(TokenBucket::new(options.global_bandwidth_limit)),
                running: Mutex::new(HashMap::new()),
                controls: Mutex::new(HashMap::new()),
                profile: std::sync::atomic::AtomicU8::new(profile_code(options.profile)),
                endpoints: Mutex::new(HashMap::new()),
                stats: Mutex::new(StatsStore::disabled()),
                owners: Mutex::new(HashMap::new()),
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

    /// Sets how copies between two `nmc serve` servers are done without
    /// passing through this machine. Must be called before the manager is
    /// shared.
    pub fn with_server_copier(mut self, copier: Arc<dyn ServerCopier>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("with_server_copier must be called before cloning")
            .server_copier = Some(copier);
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
        let current = self.job_parallelism(id)?.max(1);
        self.inner.journal.set_parallelism(id, current, auto)?;
        if let Some(control) = self.control(id) {
            control.auto.store(auto, Ordering::Relaxed);
        }
        self.inner.emit(JobEvent::ParallelismChanged { id, parallelism: current, auto });
        Ok(())
    }

    /// Sets how boldly new and running jobs raise the number of files.
    pub fn set_profile(&self, profile: Profile) {
        self.inner.profile.store(profile_code(profile), Ordering::Relaxed);
    }

    /// Sets the most files at once for a server (from its connection), or
    /// removes the limit with `None`.
    pub fn set_endpoint_limit(&self, endpoint: &str, limit: Option<usize>) {
        let mut map = self.inner.endpoints.lock().unwrap_or_else(|e| e.into_inner());
        let info = map.entry(endpoint.to_owned()).or_default();
        info.configured = limit.map(|l| l.max(1));
        if limit.is_some() {
            info.learned = None;
        }
    }

    /// Keeps what works on each server in `file` (when `enabled`), and starts
    /// from what it holds.
    pub fn use_stats_file(&self, file: std::path::PathBuf, enabled: bool) {
        *self.inner.stats.lock().unwrap_or_else(|e| e.into_inner()) =
            StatsStore::load(file, enabled, stats::now());
        self.inner.seed_from_stats();
    }

    /// Turns the memory of servers on or off. Off forgets it and deletes the file.
    pub fn set_stats_enabled(&self, enabled: bool) {
        self.inner.stats.lock().unwrap_or_else(|e| e.into_inner()).set_enabled(enabled);
        if !enabled {
            self.inner.forget_learned();
        }
    }

    /// Forgets everything remembered about servers.
    pub fn reset_stats(&self) {
        self.inner.stats.lock().unwrap_or_else(|e| e.into_inner()).reset();
        self.inner.forget_learned();
    }

    /// Tells the manager which connections are saved (name, address): what was
    /// remembered for connections that are gone, renamed or moved is dropped.
    pub fn sync_connections(&self, connections: &[(String, String)]) {
        let mut owners = HashMap::new();
        for (name, uri) in connections {
            if let Ok(path) = nmc_vfs::VfsPath::parse(uri)
                && let Some(key) = endpoint_key(&path)
            {
                owners.insert(key, Owner { name: name.clone(), uri: uri.clone() });
            }
        }
        let known: Vec<Owner> =
            connections.iter().map(|(n, u)| Owner { name: n.clone(), uri: u.clone() }).collect();
        *self.inner.owners.lock().unwrap_or_else(|e| e.into_inner()) = owners;
        let changed = self.inner.stats.lock().unwrap_or_else(|e| e.into_inner()).reconcile(&known);
        if changed {
            self.inner.forget_learned();
            self.inner.seed_from_stats();
        }
    }

    /// A new connection is called `name`: what was remembered for an older
    /// connection of that name does not apply to it.
    pub fn forget_connection_stats(&self, name: &str) {
        self.inner.stats.lock().unwrap_or_else(|e| e.into_inner()).forget_name(name);
        self.inner.forget_learned();
        self.inner.seed_from_stats();
    }

    /// What is remembered about servers, for the settings screen: how many.
    pub fn stats_count(&self) -> usize {
        self.inner.stats.lock().unwrap_or_else(|e| e.into_inner()).data.servers.len()
    }

    /// Why the automatic mode last changed the level of a running job.
    pub fn job_reason(&self, id: JobId) -> Option<String> {
        let control = self.control(id)?;
        let reason = control.reason.lock().unwrap_or_else(|e| e.into_inner()).clone();
        (!reason.is_empty()).then_some(reason)
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
    /// Puts what the stats file holds into the live limits.
    fn seed_from_stats(&self) {
        let stats = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        if !stats.enabled() {
            return;
        }
        let mut map = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
        for (key, server) in &stats.data.servers {
            let info = map.entry(key.clone()).or_default();
            info.learned = info.learned.or(server.limit);
            info.good = info.good.or(server.good_level);
        }
    }

    /// Drops the learned limits and good levels (the configured limits stay).
    fn forget_learned(&self) {
        for info in self.endpoints.lock().unwrap_or_else(|e| e.into_inner()).values_mut() {
            info.learned = None;
            info.good = None;
        }
    }

    fn record_stats(&self, endpoint: &str, good: Option<usize>, limit: Option<usize>) {
        let owner = self.owners.lock().unwrap_or_else(|e| e.into_inner()).get(endpoint).cloned();
        self.stats.lock().unwrap_or_else(|e| e.into_inner()).record(
            endpoint,
            owner,
            good,
            limit,
            stats::now(),
        );
    }

    /// The most files at once the servers of a job allow it: the limit of
    /// each server shared among the running jobs that use it.
    fn endpoint_cap(&self, control: &ParallelControl) -> usize {
        let map = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
        let controls = self.controls.lock().unwrap_or_else(|e| e.into_inner());
        let mut cap = usize::MAX;
        for endpoint in &control.endpoints {
            let Some(limit) = map.get(endpoint).and_then(EndpointInfo::limit) else { continue };
            let users = controls.values().filter(|c| c.endpoints.contains(endpoint)).count().max(1);
            cap = cap.min(limit.div_ceil(users).max(1));
        }
        cap
    }

    /// Remembers a limit learned from errors and tells the interfaces, so
    /// that they can save it on the connection.
    fn learn_limit(&self, endpoints: &[String], limit: usize) {
        for endpoint in endpoints {
            let mut map = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
            let info = map.entry(endpoint.clone()).or_default();
            if info.configured.is_some() || info.learned.is_some_and(|l| l <= limit) {
                continue;
            }
            info.learned = Some(limit);
            drop(map);
            self.record_stats(endpoint, None, Some(limit));
            self.emit(JobEvent::EndpointLimit { endpoint: endpoint.clone(), limit });
        }
    }

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

    /// Servers copy by themselves and cannot ask anything while they do:
    /// when something is already there, the user is asked once, here, and
    /// the answer is used for the whole copy. Without a way to ask, or
    /// when nothing is in the way, existing files are kept (skipped).
    async fn ask_before_server_copy(
        &self,
        copier: &dyn ServerCopier,
        sources: &[VfsPath],
        destination: &VfsPath,
    ) -> Result<ConflictPolicy, JobError> {
        use nmc_ops::{Conflict, ConflictAction};
        let Some(resolver) = &self.resolver else { return Ok(ConflictPolicy::Skip) };
        if !copier.between_servers(sources, destination) {
            return Ok(ConflictPolicy::Skip);
        }
        let (Ok(to), Some(_)) = (self.registry.provider_for(destination).await, sources.first())
        else {
            return Ok(ConflictPolicy::Skip);
        };
        for source in sources {
            let Some(name) = source.file_name() else { continue };
            let target = destination.join(name);
            let Ok(existing) = to.stat(&target).await else { continue };
            let Ok(from) = self.registry.provider_for(source).await else { continue };
            let Ok(incoming) = from.stat(source).await else { continue };
            let conflict = Conflict {
                source: source.clone(),
                source_entry: incoming,
                destination: target,
                destination_entry: existing,
            };
            let answer = resolver.resolve(&conflict).await;
            return match answer.action {
                ConflictAction::Overwrite => Ok(ConflictPolicy::Overwrite),
                ConflictAction::Skip => Ok(ConflictPolicy::Skip),
                ConflictAction::RenameTo(_) => Ok(ConflictPolicy::KeepBoth),
                ConflictAction::Abort => Err(JobError::Failed(format!(
                    "{} already exists and the copy was stopped",
                    conflict.destination
                ))),
            };
        }
        Ok(ConflictPolicy::Skip)
    }

    /// Plans (if needed) and runs the job. `Ok(true)` if no step failed.
    async fn execute(
        self: &Arc<Self>,
        id: JobId,
        cancel: CancellationToken,
    ) -> Result<bool, JobError> {
        let spec = self.journal.job(id)?.spec;
        if spec.kind == JobKind::Copy
            && let (Some(copier), Some(destination), None) =
                (&self.server_copier, &spec.destination, self.journal.load_plan(id)?)
        {
            let conflict = match spec.options.conflict {
                ConflictPolicy::Ask => {
                    self.ask_before_server_copy(copier.as_ref(), &spec.sources, destination).await?
                }
                other => other,
            };
            let shown = AtomicU64::new(0);
            let progress = |done: u64, _total: u64| {
                let before = shown.fetch_max(done, Ordering::Relaxed);
                if done > before {
                    self.emit(JobEvent::Progress { id, bytes: done - before });
                }
            };
            let attempt = copier.copy(&spec.sources, destination, conflict, &progress);
            let outcome = tokio::select! {
                outcome = attempt => outcome,
                _ = cancel.cancelled() => return Err(JobError::Stopped),
            };
            match outcome {
                None => {} // not between servers, or they cannot reach each other
                Some(Err(message)) => return Err(JobError::Failed(message)),
                Some(Ok(report)) => {
                    self.emit(JobEvent::Route { id, route: report.route.clone() });
                    self.journal.store_plan(
                        id,
                        &nmc_ops::Plan {
                            steps: Vec::new(),
                            total_bytes: report.bytes,
                            total_files: 0,
                            warnings: Vec::new(),
                        },
                    )?;
                    if !report.failures.is_empty() {
                        self.journal.set_error(id, &report.failures.join("; "))?;
                    }
                    return Ok(report.failures.is_empty());
                }
            }
        }
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
        let mut endpoints: Vec<String> =
            spec.sources.iter().chain(spec.destination.iter()).filter_map(endpoint_key).collect();
        endpoints.sort();
        endpoints.dedup();
        let auto_start = spec.options.auto_parallelism;
        let profile = profile_of(self.profile.load(Ordering::Relaxed));
        let start = if auto_start {
            // Automatic: start low, or where the last job on these servers did well.
            let remembered = {
                let map = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
                endpoints.iter().filter_map(|e| map.get(e).and_then(|i| i.good)).min()
            };
            remembered.unwrap_or(profile.start_level()).clamp(1, profile.max_level())
        } else {
            spec.options.parallelism.max(1)
        };
        let level = Arc::new(AtomicUsize::new(start));
        let control = Arc::new(ParallelControl {
            level: level.clone(),
            auto: AtomicBool::new(auto_start),
            endpoints: endpoints.clone(),
            reason: Mutex::new(String::new()),
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
            let mut tuner = Tuner::new(control.level.load(Ordering::Relaxed), profile);
            let mut last_bytes = 0;
            let mut shown = control.level.load(Ordering::Relaxed);
            let mut was_auto = control.auto.load(Ordering::Relaxed);
            loop {
                tokio::time::sleep(TUNING_INTERVAL).await;
                let bytes = written.load(Ordering::Relaxed);
                let speed = (bytes - last_bytes) as f64 / TUNING_INTERVAL.as_secs_f64();
                last_bytes = bytes;
                let failures = retries.swap(0, Ordering::Relaxed);
                let current = control.level.load(Ordering::Relaxed);
                let auto = control.auto.load(Ordering::Relaxed);
                let profile = profile_of(self.profile.load(Ordering::Relaxed));
                if !auto {
                    tuner = Tuner::new(current, profile);
                    shown = current;
                    was_auto = false;
                    continue;
                }
                if current != shown || !was_auto || tuner.profile() != profile {
                    tuner = Tuner::new(current, profile); // changed from outside (settings, by hand)
                }
                was_auto = true;
                let limit = self.endpoint_cap(&control);
                let decision = tuner.next(speed, failures, limit);
                control.level.store(decision.level, Ordering::Relaxed);
                shown = decision.level;
                if let Some(reason) = decision.reason {
                    *control.reason.lock().unwrap_or_else(|e| e.into_inner()) = reason;
                }
                if let Some(learned) = decision.suggest_limit {
                    self.learn_limit(&control.endpoints, learned);
                }
                if decision.level != current {
                    self.emit(JobEvent::ParallelismChanged {
                        id,
                        parallelism: decision.level,
                        auto: true,
                    });
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
            control.level.load(Ordering::Relaxed).max(1),
            control.auto.load(Ordering::Relaxed),
        );
        if control.auto.load(Ordering::Relaxed) && copies >= 2 {
            let level = control.level.load(Ordering::Relaxed);
            if level > 0 {
                let mut map = self.endpoints.lock().unwrap_or_else(|e| e.into_inner());
                for endpoint in &control.endpoints {
                    map.entry(endpoint.clone()).or_default().good = Some(level);
                }
                drop(map);
                for endpoint in &control.endpoints {
                    self.record_stats(endpoint, Some(level), None);
                }
            }
        }
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

    /// A copier that answers with a fixed result.
    struct FixedCopier(Option<Result<ServerCopyReport, String>>);

    #[async_trait]
    impl ServerCopier for FixedCopier {
        async fn copy(
            &self,
            _: &[VfsPath],
            _: &VfsPath,
            _: ConflictPolicy,
            progress: &(dyn Fn(u64, u64) + Send + Sync),
        ) -> Option<Result<ServerCopyReport, String>> {
            progress(5, 10);
            progress(10, 10);
            self.0.clone()
        }
    }

    async fn run_with_copier(copier: FixedCopier) -> (JobInfo, MemoryProvider, Vec<JobEvent>) {
        let (registry, a, b) = setup();
        a.put_file("/src/x", "1");
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap()
        .with_server_copier(Arc::new(copier));
        let mut events = manager.subscribe();
        let id = manager.submit(copy_spec(0)).unwrap();
        manager.run_until_idle().await;
        let mut seen = Vec::new();
        while let Ok(event) = events.try_recv() {
            seen.push(event);
        }
        (manager.job(id).unwrap(), b, seen)
    }

    #[tokio::test]
    async fn a_copy_between_servers_skips_the_local_copy() {
        let report = ServerCopyReport { route: "direct".into(), bytes: 10, failures: vec![] };
        let (info, b, events) = run_with_copier(FixedCopier(Some(Ok(report)))).await;
        assert_eq!(info.state, JobState::Completed);
        assert!(b.get_file("/src/x").is_none(), "nothing was copied through here");
        assert!(events.contains(&JobEvent::Route { id: info.id, route: "direct".into() }));
        let bytes: u64 = events
            .iter()
            .filter_map(|e| match e {
                JobEvent::Progress { bytes, .. } => Some(*bytes),
                _ => None,
            })
            .sum();
        assert_eq!(bytes, 10);
    }

    #[tokio::test]
    async fn failures_between_servers_fail_the_job() {
        let report = ServerCopyReport {
            route: "reverse".into(),
            bytes: 4,
            failures: vec!["a.txt: permission denied".into()],
        };
        let (info, _, _) = run_with_copier(FixedCopier(Some(Ok(report)))).await;
        assert_eq!(info.state, JobState::Failed);
        assert!(info.errors.iter().any(|e| e.contains("permission denied")), "{:?}", info.errors);
        let (info, _, _) = run_with_copier(FixedCopier(Some(Err("no route".into())))).await;
        assert_eq!(info.state, JobState::Failed);
    }

    #[tokio::test]
    async fn without_a_route_the_job_copies_through_here() {
        let (info, b, _) = run_with_copier(FixedCopier(None)).await;
        assert_eq!(info.state, JobState::Completed);
        assert_eq!(b.get_file("/src/x").unwrap(), b"1");
    }

    #[tokio::test]
    async fn server_memory_follows_the_saved_connections() {
        let (registry, _, _) = setup();
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("connection-stats.json");
        manager.use_stats_file(file.clone(), true);
        let nas = vec![("nas".to_owned(), "sftp://me@nas/srv".to_owned())];
        manager.sync_connections(&nas);
        manager.inner.record_stats("sftp://me@nas", Some(5), Some(3));
        assert!(file.exists());
        assert_eq!(manager.stats_count(), 1);
        // the connection is deleted: its numbers go
        manager.sync_connections(&[]);
        assert_eq!(manager.stats_count(), 0);
        assert!(!file.exists());
        // a new connection called nas does not inherit anything
        manager.sync_connections(&nas);
        manager.inner.record_stats("sftp://me@nas", Some(5), Some(3));
        manager.forget_connection_stats("nas");
        assert_eq!(manager.stats_count(), 0);
        // off: nothing is written
        manager.set_stats_enabled(false);
        manager.inner.record_stats("sftp://me@nas", Some(5), None);
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn parallelism_is_stored_with_the_job() {
        let (registry, _, _) = setup();
        let manager = TransferManager::new(
            registry,
            Journal::open_in_memory().unwrap(),
            ManagerOptions::default(),
        )
        .unwrap();
        let id = manager.submit(copy_spec(0)).unwrap();
        assert_eq!(manager.job_parallelism(id).unwrap(), 4);
        manager.set_parallelism(id, 7).unwrap();
        assert_eq!(manager.job_parallelism(id).unwrap(), 7);
        let options = manager.job(id).unwrap().spec.options;
        assert_eq!(options.parallelism, 7);
        assert!(!options.auto_parallelism, "a level chosen by hand turns the tuning off");
        manager.set_auto_parallelism(id, true).unwrap();
        assert!(manager.job(id).unwrap().spec.options.auto_parallelism);
        manager.set_parallelism(id, 1000).unwrap();
        assert_eq!(manager.job_parallelism(id).unwrap(), crate::MAX_PARALLELISM);
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
        let options = ManagerOptions {
            max_concurrent_jobs: 1,
            global_bandwidth_limit: 1_000_000,
            ..Default::default()
        };
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
