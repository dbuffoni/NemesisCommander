//! Running a [`Plan`].

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use nmc_vfs::{
    Capabilities, EntryKind, Provider, ProviderRegistry, Result, SetAttributes, VfsError, VfsPath,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::conflict::{Conflict, ConflictAction, ConflictPolicy, ConflictResolver};
use crate::copy::{CopyFileOptions, Throttle, VerifyMode, copy_file, verify_copy};
use crate::plan::{Plan, PlanOptions, Step, plan_copy, plan_delete};

/// Progress notifications emitted while a plan runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpEvent {
    /// A step started.
    StepStarted {
        /// Index into [`Plan::steps`].
        index: usize,
    },
    /// Bytes were written (any step).
    Bytes(u64),
    /// A step failed with a transient error and will be tried again after a
    /// pause. For lost connections the providers involved are reopened.
    Retrying {
        /// Index into [`Plan::steps`].
        index: usize,
        /// The attempt about to start (the first retry is 1).
        attempt: u32,
        /// The most retries allowed.
        attempts: u32,
        /// Why the step failed.
        message: String,
    },
    /// A step ended.
    StepFinished {
        /// Index into [`Plan::steps`].
        index: usize,
        /// How it ended.
        outcome: StepOutcome,
    },
}

/// How a single step ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOutcome {
    /// The step did its job.
    Done,
    /// The step was skipped because of a conflict decision.
    Skipped,
    /// The step failed; the message is meant for humans.
    Failed(String),
}

/// Receives [`OpEvent`]s. Implemented for any `Fn(OpEvent)`.
pub trait Observer: Send + Sync {
    /// Called for each event, possibly from several tasks at once.
    fn on_event(&self, event: OpEvent);
}

impl<F: Fn(OpEvent) + Send + Sync> Observer for F {
    fn on_event(&self, event: OpEvent) {
        self(event)
    }
}

/// Settings for an [`Executor`].
#[derive(Clone)]
pub struct ExecuteOptions {
    /// How to handle existing destinations.
    pub conflict: ConflictPolicy,
    /// How to check copied files.
    pub verify: VerifyMode,
    /// How many files may be copied at the same time.
    pub parallelism: usize,
    /// When set, the number of files copied at the same time is read from
    /// here before each file starts instead of from `parallelism`, so it can
    /// be changed while the plan runs (it applies to files not yet started).
    pub live_parallelism: Option<Arc<AtomicUsize>>,
    /// Continue interrupted copies from their part files.
    pub resume: bool,
    /// Keep going after a failed step (the failure is reported) instead of
    /// stopping the whole plan.
    pub continue_on_error: bool,
    /// Optional bandwidth limiter shared by all copies.
    pub throttle: Option<Arc<dyn Throttle>>,
    /// Cancels the plan.
    pub cancel: CancellationToken,
    /// Retries for transient failures (network hiccups, verification
    /// mismatches). Permanent errors such as "permission denied" are never
    /// retried.
    pub retry: RetryPolicy,
}

/// How often and how patiently a failed step is retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Extra attempts after the first one (0 = no retry).
    pub attempts: u32,
    /// Delay before the first retry; doubled after each failure.
    pub initial_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy { attempts: 0, initial_delay: Duration::from_millis(500) }
    }
}

/// Lets a caller persist step results and skip steps that already ran, so a
/// plan interrupted by a crash continues where it stopped. `nmc-transfer`
/// implements this on top of its journal.
pub trait StepTracker: Send + Sync {
    /// `true` if the step finished in an earlier run and must not run again.
    fn is_finished(&self, index: usize) -> bool;
    /// Called once per step with its final outcome (after retries).
    fn record(&self, index: usize, outcome: &StepOutcome);
}

/// `true` for errors that might go away if the step is tried again.
fn is_transient(error: &VfsError) -> bool {
    matches!(error, VfsError::Io { .. } | VfsError::Other(_))
}

/// `true` if `message` looks like a lost or stalled connection, so that
/// retrying only helps after the provider has been opened again.
pub fn is_connection_error(message: &str) -> bool {
    let message = message.to_lowercase();
    [
        "connection lost",
        "connection closed",
        "connection reset",
        "connection aborted",
        "broken pipe",
        "unexpected eof",
        "timed out",
        "timeout",
        "unavailable",
        "channel closed",
        "was closed",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// How long to wait before retrying a lost connection at least.
const CONNECTION_RETRY_DELAY: Duration = Duration::from_secs(2);
/// The longest pause between two retries.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

impl Default for ExecuteOptions {
    fn default() -> Self {
        ExecuteOptions {
            conflict: ConflictPolicy::Ask,
            verify: VerifyMode::Size,
            parallelism: 4,
            live_parallelism: None,
            resume: true,
            continue_on_error: true,
            throttle: None,
            cancel: CancellationToken::new(),
            retry: RetryPolicy::default(),
        }
    }
}

/// Summary of a finished run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Steps that completed.
    pub done: usize,
    /// Steps skipped by conflict decisions.
    pub skipped: usize,
    /// Failed steps: index and message.
    pub failed: Vec<(usize, String)>,
    /// Bytes written.
    pub bytes: u64,
}

/// Runs plans against a [`ProviderRegistry`].
///
/// Independent file copies run concurrently (up to
/// [`ExecuteOptions::parallelism`]); everything else runs in plan order.
pub struct Executor {
    registry: Arc<ProviderRegistry>,
    options: ExecuteOptions,
    resolver: Option<Arc<dyn ConflictResolver>>,
    observer: Option<Arc<dyn Observer>>,
    /// The conflict policy can change mid-run ("overwrite all").
    policy: Mutex<ConflictPolicy>,
    /// Serializes questions to the resolver so dialogs do not overlap.
    asking: tokio::sync::Mutex<()>,
}

impl Executor {
    /// Creates an executor.
    pub fn new(registry: Arc<ProviderRegistry>, options: ExecuteOptions) -> Self {
        let policy = Mutex::new(options.conflict);
        Executor {
            registry,
            options,
            resolver: None,
            observer: None,
            policy,
            asking: tokio::sync::Mutex::new(()),
        }
    }

    /// Sets who answers conflict questions under [`ConflictPolicy::Ask`].
    /// Without a resolver, `Ask` behaves like [`ConflictPolicy::Fail`].
    pub fn with_resolver(mut self, resolver: Arc<dyn ConflictResolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Sets the progress observer.
    pub fn with_observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = Some(observer);
        self
    }

    fn emit(&self, event: OpEvent) {
        if let Some(observer) = &self.observer {
            observer.on_event(event);
        }
    }

    /// Runs every step of `plan`.
    ///
    /// Returns `Err` only if the run was cancelled or aborted (by a conflict
    /// decision, or by a failure when `continue_on_error` is off); individual
    /// failures are otherwise listed in the [`Report`].
    pub async fn run(&self, plan: &Plan) -> Result<Report> {
        self.run_tracked(plan, None).await
    }

    /// Like [`run`](Self::run), but skips steps the tracker reports as
    /// finished and records every outcome in it.
    pub async fn run_tracked(
        &self,
        plan: &Plan,
        tracker: Option<&dyn StepTracker>,
    ) -> Result<Report> {
        let mut report = Report::default();
        let mut index = 0;
        while index < plan.steps.len() {
            if self.options.cancel.is_cancelled() {
                return Err(VfsError::Cancelled);
            }
            // Group consecutive parallel steps into one concurrent batch.
            let end = if plan.steps[index].is_parallel() {
                index + plan.steps[index..].iter().take_while(|s| s.is_parallel()).count()
            } else {
                index + 1
            };
            let mut pending =
                (index..end).filter(|&i| !tracker.is_some_and(|t| t.is_finished(i))).peekable();
            let mut running = FuturesUnordered::new();
            let mut batch = Vec::new();
            loop {
                while running.len() < self.parallelism_now() {
                    match pending.next() {
                        Some(i) => running.push(self.run_indexed(plan, i)),
                        None => break,
                    }
                }
                if running.is_empty() {
                    if pending.peek().is_some() && self.options.live_parallelism.is_some() {
                        // The level is 0: waiting for a server. Look again soon.
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                            _ = self.options.cancel.cancelled() => return Err(VfsError::Cancelled),
                        }
                        continue;
                    }
                    break;
                }
                if self.options.live_parallelism.is_some() {
                    // Look at the limit again now and then: it may have been raised.
                    tokio::select! {
                        done = running.next() => batch.extend(done),
                        _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                    }
                } else {
                    batch.extend(running.next().await);
                }
            }
            for (i, result) in batch {
                let outcome = match result {
                    Ok(outcome) => outcome,
                    Err(VfsError::Cancelled) => return Err(VfsError::Cancelled),
                    Err(e) => StepOutcome::Failed(e.to_string()),
                };
                if let Some(tracker) = tracker {
                    tracker.record(i, &outcome);
                }
                match &outcome {
                    StepOutcome::Done => report.done += 1,
                    StepOutcome::Skipped => report.skipped += 1,
                    StepOutcome::Failed(message) => {
                        report.failed.push((i, message.clone()));
                        if !self.options.continue_on_error {
                            return Err(VfsError::Other(message.clone()));
                        }
                    }
                }
                if let Step::CopyFile { size, .. } = &plan.steps[i]
                    && outcome == StepOutcome::Done
                {
                    report.bytes += size;
                }
            }
            index = end;
        }
        Ok(report)
    }

    /// How many parallel steps may run now.
    fn parallelism_now(&self) -> usize {
        match &self.options.live_parallelism {
            Some(live) => live.load(Ordering::Relaxed),
            None => self.options.parallelism,
        }
        .max(1)
    }

    async fn run_indexed(&self, plan: &Plan, index: usize) -> (usize, Result<StepOutcome>) {
        (index, self.run_step_with_retry(index, &plan.steps[index]).await)
    }

    /// Runs a step, retrying transient failures per [`ExecuteOptions::retry`].
    ///
    /// After a lost connection the providers of the step are forgotten, so
    /// the next attempt opens them again (a new agent, a new SFTP session),
    /// and the pause is at least [`CONNECTION_RETRY_DELAY`].
    async fn run_step_with_retry(&self, index: usize, step: &Step) -> Result<StepOutcome> {
        let mut delay = self.options.retry.initial_delay;
        let mut attempt = 0;
        loop {
            let result = self.run_step(index, step).await;
            let (retryable, message) = match &result {
                Ok(StepOutcome::Failed(message)) => (true, message.clone()),
                Err(e) if is_transient(e) => (true, e.to_string()),
                _ => (false, String::new()),
            };
            if !retryable
                || attempt >= self.options.retry.attempts
                || self.options.cancel.is_cancelled()
            {
                return result;
            }
            attempt += 1;
            if is_connection_error(&message) {
                for path in step.paths() {
                    self.registry.forget(path);
                }
                delay = delay.max(CONNECTION_RETRY_DELAY);
            }
            self.emit(OpEvent::Retrying {
                index,
                attempt,
                attempts: self.options.retry.attempts,
                message,
            });
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = self.options.cancel.cancelled() => return Err(VfsError::Cancelled),
            }
            delay = (delay * 2).min(MAX_RETRY_DELAY);
        }
    }

    /// Runs a single step once and reports it to the observer.
    pub async fn run_step(&self, index: usize, step: &Step) -> Result<StepOutcome> {
        self.emit(OpEvent::StepStarted { index });
        let result = match step {
            Step::CreateDir { path } => self.create_dir(path).await,
            Step::CopyFile { from, to, modified, permissions, .. } => {
                let attributes = SetAttributes { modified: *modified, permissions: *permissions };
                self.copy(from, to, &attributes).await
            }
            Step::CopySymlink { from, to, target } => self.copy_symlink(from, to, target).await,
            Step::SetAttributes { path, attributes } => {
                let provider = self.registry.provider_for(path).await?;
                apply_attributes(provider.as_ref(), path, attributes)
                    .await
                    .map(|_| StepOutcome::Done)
            }
            Step::Move { from, to } => self.move_item(from, to).await,
            Step::Remove { path, is_dir } => {
                let provider = self.registry.provider_for(path).await?;
                let removed = if *is_dir {
                    provider.remove_dir(path).await
                } else {
                    provider.remove_file(path).await
                };
                match removed {
                    Ok(()) => Ok(StepOutcome::Done),
                    Err(e) if e.is_not_found() => Ok(StepOutcome::Done), // idempotent
                    Err(e) => Err(e),
                }
            }
        };
        let outcome = match &result {
            Ok(outcome) => outcome.clone(),
            Err(e) => StepOutcome::Failed(e.to_string()),
        };
        self.emit(OpEvent::StepFinished { index, outcome });
        result
    }

    async fn create_dir(&self, path: &VfsPath) -> Result<StepOutcome> {
        let provider = self.registry.provider_for(path).await?;
        match provider.create_dir(path).await {
            Ok(()) => Ok(StepOutcome::Done),
            Err(VfsError::AlreadyExists(_)) => match provider.stat(path).await {
                Ok(entry) if entry.is_dir() => Ok(StepOutcome::Done), // merge into it
                _ => Err(VfsError::AlreadyExists(path.clone())),
            },
            Err(e) => Err(e),
        }
    }

    /// Looks at an existing destination and decides what to do.
    async fn resolve_conflict(
        &self,
        src: &dyn Provider,
        from: &VfsPath,
        dst: &dyn Provider,
        to: &VfsPath,
    ) -> Result<Option<ConflictAction>> {
        let existing = match dst.stat(to).await {
            Ok(entry) => entry,
            Err(e) if e.is_not_found() => return Ok(None),
            Err(e) => return Err(e),
        };
        let conflict = Conflict {
            source: from.clone(),
            source_entry: src.stat(from).await?,
            destination: to.clone(),
            destination_entry: existing,
        };
        let parent = to.parent().unwrap_or_else(|| to.clone());
        let names: Vec<String> =
            nmc_vfs::collect_listing(dst, &parent).await?.into_iter().map(|e| e.name).collect();
        let taken = |name: &str| names.iter().any(|n| n == name);

        let policy = *self.policy.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(action) = policy.decide(&conflict, taken) {
            return Ok(Some(action));
        }
        let Some(resolver) = &self.resolver else {
            return Ok(Some(ConflictAction::Abort));
        };
        let _one_question_at_a_time = self.asking.lock().await;
        // The policy may have become "all" while we waited for the lock.
        let policy = *self.policy.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(action) = policy.decide(&conflict, taken) {
            return Ok(Some(action));
        }
        let mut resolution = resolver.resolve(&conflict).await;
        // A resolver may propose a name that is itself taken; pick the next
        // free one rather than failing.
        if let ConflictAction::RenameTo(name) = &resolution.action
            && taken(name)
        {
            let original = conflict.destination.file_name().unwrap_or("file");
            resolution.action =
                ConflictAction::RenameTo(crate::conflict::unique_name(original, taken));
        }
        if resolution.apply_to_all {
            let standing = match &resolution.action {
                ConflictAction::Overwrite => ConflictPolicy::Overwrite,
                ConflictAction::Skip => ConflictPolicy::Skip,
                ConflictAction::RenameTo(_) => ConflictPolicy::KeepBoth,
                ConflictAction::Abort => ConflictPolicy::Fail,
            };
            *self.policy.lock().unwrap_or_else(|e| e.into_inner()) = standing;
        }
        Ok(Some(resolution.action))
    }

    async fn copy(
        &self,
        from: &VfsPath,
        to: &VfsPath,
        attributes: &SetAttributes,
    ) -> Result<StepOutcome> {
        let src = self.registry.provider_for(from).await?;
        let dst = self.registry.provider_for(to).await?;
        let mut target = to.clone();
        let mut overwrite = false;
        match self.resolve_conflict(src.as_ref(), from, dst.as_ref(), to).await? {
            None => {}
            Some(ConflictAction::Overwrite) => overwrite = true,
            Some(ConflictAction::Skip) => return Ok(StepOutcome::Skipped),
            Some(ConflictAction::RenameTo(name)) => {
                target = to.parent().unwrap_or_else(|| to.clone()).join(&name);
            }
            Some(ConflictAction::Abort) => return Err(VfsError::AlreadyExists(to.clone())),
        }

        let observer = self.observer.clone();
        let options = CopyFileOptions {
            overwrite,
            resume: self.options.resume,
            hash: self.options.verify.algorithm(),
            force_streaming: false,
            progress: observer.map(|o| Arc::new(move |n| o.on_event(OpEvent::Bytes(n))) as _),
            throttle: self.options.throttle.clone(),
            cancel: self.options.cancel.clone(),
        };
        let outcome = copy_file(src.as_ref(), from, dst.as_ref(), &target, &options).await?;
        let verified = verify_copy(
            src.as_ref(),
            from,
            dst.as_ref(),
            &target,
            self.options.verify,
            outcome.source_hash.as_deref(),
        )
        .await?;
        if !verified {
            return Ok(StepOutcome::Failed(format!("{target}: verification failed")));
        }
        apply_attributes(dst.as_ref(), &target, attributes).await?;
        Ok(StepOutcome::Done)
    }

    async fn copy_symlink(
        &self,
        from: &VfsPath,
        to: &VfsPath,
        target: &str,
    ) -> Result<StepOutcome> {
        let src = self.registry.provider_for(from).await?;
        let dst = self.registry.provider_for(to).await?;
        if dst.capabilities().contains(Capabilities::SYMLINKS) {
            return match dst.create_symlink(to, target).await {
                Ok(()) => Ok(StepOutcome::Done),
                Err(VfsError::AlreadyExists(_)) => {
                    match self.resolve_conflict(src.as_ref(), from, dst.as_ref(), to).await? {
                        Some(ConflictAction::Overwrite) => {
                            dst.remove_file(to).await?;
                            dst.create_symlink(to, target).await?;
                            Ok(StepOutcome::Done)
                        }
                        Some(ConflictAction::RenameTo(name)) => {
                            let renamed = to.parent().unwrap_or_else(|| to.clone()).join(&name);
                            dst.create_symlink(&renamed, target).await?;
                            Ok(StepOutcome::Done)
                        }
                        Some(ConflictAction::Abort) => Err(VfsError::AlreadyExists(to.clone())),
                        _ => Ok(StepOutcome::Skipped),
                    }
                }
                Err(e) => Err(e),
            };
        }
        // The destination cannot store links: copy what the link points to.
        // `read` follows the link on providers that have links.
        match src.read(from, 0).await {
            Ok(_) => self.copy(from, to, &SetAttributes::default()).await,
            Err(_) => Ok(StepOutcome::Failed(format!(
                "{from}: link cannot be recreated on {} and does not point to a file",
                dst.scheme()
            ))),
        }
    }

    async fn move_item(&self, from: &VfsPath, to: &VfsPath) -> Result<StepOutcome> {
        let provider = self.registry.provider_for(from).await?;
        let source_entry = provider.stat(from).await?;
        let mut target = to.clone();
        match self.resolve_conflict(provider.as_ref(), from, provider.as_ref(), to).await? {
            None => {}
            Some(ConflictAction::Skip) => return Ok(StepOutcome::Skipped),
            Some(ConflictAction::Abort) => return Err(VfsError::AlreadyExists(to.clone())),
            Some(ConflictAction::RenameTo(name)) => {
                target = to.parent().unwrap_or_else(|| to.clone()).join(&name);
            }
            Some(ConflictAction::Overwrite) => {
                let existing = provider.stat(to).await?;
                if source_entry.kind == EntryKind::Directory
                    || existing.kind == EntryKind::Directory
                {
                    // Merging directories cannot be a single rename.
                    return self.move_by_copy(from, to, ConflictPolicy::Overwrite).await;
                }
                if !provider.capabilities().contains(Capabilities::ATOMIC_RENAME) {
                    provider.remove_file(to).await?;
                }
            }
        }
        match provider.rename(from, &target).await {
            Ok(()) => Ok(StepOutcome::Done),
            Err(e @ (VfsError::NotFound(_) | VfsError::PermissionDenied(_))) => Err(e),
            // Typically "crosses devices" or "unsupported": copy, then delete.
            Err(_) => self.move_by_copy(from, &target, ConflictPolicy::Overwrite).await,
        }
    }

    /// Moves by copying everything and deleting the source afterwards. The
    /// source is only deleted if every copy succeeded.
    async fn move_by_copy(
        &self,
        from: &VfsPath,
        to: &VfsPath,
        policy: ConflictPolicy,
    ) -> Result<StepOutcome> {
        let mut copy =
            plan_copy(&self.registry, std::slice::from_ref(from), to, PlanOptions::default())
                .await?;
        // `plan_copy` copies *into* an existing directory; for a move onto an
        // existing directory we want to merge, so rewrite targets if needed.
        let to_provider = self.registry.provider_for(to).await?;
        if to_provider.stat(to).await.map(|e| e.is_dir()).unwrap_or(false) {
            copy = merge_plan(&self.registry, from, to).await?;
        }
        let inner = Executor::new(
            self.registry.clone(),
            ExecuteOptions { conflict: policy, continue_on_error: false, ..self.options.clone() },
        );
        let inner = match &self.observer {
            Some(o) => inner.with_observer(Arc::new(BytesOnly(o.clone()))),
            None => inner,
        };
        let report = inner.run(&copy).await?;
        if !report.failed.is_empty() {
            return Ok(StepOutcome::Failed(format!("{from}: move incomplete, source kept")));
        }
        let removal = plan_delete(&self.registry, std::slice::from_ref(from)).await?;
        inner.run(&removal).await?;
        Ok(StepOutcome::Done)
    }
}

/// Forwards only byte counts from a nested executor, so step indices of the
/// nested plan do not confuse the outer observer.
struct BytesOnly(Arc<dyn Observer>);

impl Observer for BytesOnly {
    fn on_event(&self, event: OpEvent) {
        if let OpEvent::Bytes(_) = event {
            self.0.on_event(event);
        }
    }
}

/// A copy plan that merges the *contents* of directory `from` into the
/// existing directory `to`.
async fn merge_plan(registry: &ProviderRegistry, from: &VfsPath, to: &VfsPath) -> Result<Plan> {
    let provider = registry.provider_for(from).await?;
    let children: Vec<VfsPath> = nmc_vfs::collect_listing(provider.as_ref(), from)
        .await?
        .into_iter()
        .map(|e| from.join(&e.name))
        .collect();
    if children.is_empty() {
        return Ok(Plan::default());
    }
    plan_copy(registry, &children, to, PlanOptions::default()).await
}

/// Applies the attributes the destination supports and ignores the rest.
async fn apply_attributes(
    provider: &dyn Provider,
    path: &VfsPath,
    wanted: &SetAttributes,
) -> Result<()> {
    let caps = provider.capabilities();
    let supported = SetAttributes {
        modified: wanted.modified.filter(|_| caps.contains(Capabilities::SET_MTIME)),
        permissions: wanted.permissions.filter(|_| caps.contains(Capabilities::POSIX_PERMISSIONS)),
    };
    if supported == SetAttributes::default() {
        return Ok(());
    }
    provider.set_attributes(path, &supported).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::{FixedResolver, Resolution};
    use crate::plan::plan_move;
    use async_trait::async_trait;
    use nmc_providers::memory::MemoryProvider;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    fn executor(registry: &Arc<ProviderRegistry>, conflict: ConflictPolicy) -> Executor {
        Executor::new(registry.clone(), ExecuteOptions { conflict, ..Default::default() })
    }

    #[tokio::test]
    async fn copies_a_tree_and_reports_bytes() {
        let (registry, a, b) = setup();
        for i in 0..20 {
            a.put_file(&format!("/src/d{}/f{i}", i % 3), vec![b'x'; i]);
        }
        let plan = plan_copy(&registry, &[p("a", "/src")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();
        let bytes = Arc::new(AtomicUsize::new(0));
        let counter = bytes.clone();
        let exec = executor(&registry, ConflictPolicy::Fail).with_observer(Arc::new(move |e| {
            if let OpEvent::Bytes(n) = e {
                counter.fetch_add(n as usize, Ordering::SeqCst);
            }
        }));
        let report = exec.run(&plan).await.unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.bytes, plan.total_bytes);
        assert_eq!(bytes.load(Ordering::SeqCst) as u64, plan.total_bytes);
        for i in 0..20 {
            assert_eq!(b.get_file(&format!("/src/d{}/f{i}", i % 3)).unwrap().len(), i);
        }
    }

    #[tokio::test]
    async fn conflict_policies_are_applied() {
        let (registry, a, b) = setup();
        a.put_file("/f", "new");
        b.put_file("/f", "old");
        let plan = plan_copy(&registry, &[p("a", "/f")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();

        let report = executor(&registry, ConflictPolicy::Skip).run(&plan).await.unwrap();
        assert_eq!(report.skipped, 1);
        assert_eq!(b.get_file("/f").unwrap(), b"old");

        executor(&registry, ConflictPolicy::KeepBoth).run(&plan).await.unwrap();
        assert_eq!(b.get_file("/f (1)").unwrap(), b"new");

        executor(&registry, ConflictPolicy::Overwrite).run(&plan).await.unwrap();
        assert_eq!(b.get_file("/f").unwrap(), b"new");
    }

    #[tokio::test]
    async fn ask_without_resolver_fails_the_step() {
        let (registry, a, b) = setup();
        a.put_file("/f", "new");
        b.put_file("/f", "old");
        let plan = plan_copy(&registry, &[p("a", "/f")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();
        let report = executor(&registry, ConflictPolicy::Ask).run(&plan).await.unwrap();
        assert_eq!(report.failed.len(), 1);
        assert_eq!(b.get_file("/f").unwrap(), b"old");
    }

    struct CountingResolver(AtomicUsize);

    #[async_trait]
    impl ConflictResolver for CountingResolver {
        async fn resolve(&self, _: &Conflict) -> Resolution {
            self.0.fetch_add(1, Ordering::SeqCst);
            Resolution { action: ConflictAction::Overwrite, apply_to_all: true }
        }
    }

    #[tokio::test]
    async fn apply_to_all_asks_only_once() {
        let (registry, a, b) = setup();
        for i in 0..10 {
            a.put_file(&format!("/d/f{i}"), "new");
            b.put_file(&format!("/d/f{i}"), "old");
        }
        let plan = plan_copy(&registry, &[p("a", "/d")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();
        let resolver = Arc::new(CountingResolver(AtomicUsize::new(0)));
        let exec = executor(&registry, ConflictPolicy::Ask).with_resolver(resolver.clone());
        let report = exec.run(&plan).await.unwrap();
        assert!(report.failed.is_empty());
        assert_eq!(resolver.0.load(Ordering::SeqCst), 1);
        assert_eq!(b.get_file("/d/f7").unwrap(), b"new");
    }

    #[tokio::test]
    async fn fixed_resolver_skip() {
        let (registry, a, b) = setup();
        a.put_file("/f", "new");
        b.put_file("/f", "old");
        let plan = plan_copy(&registry, &[p("a", "/f")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();
        let exec = executor(&registry, ConflictPolicy::Ask)
            .with_resolver(Arc::new(FixedResolver(ConflictAction::Skip)));
        assert_eq!(exec.run(&plan).await.unwrap().skipped, 1);
    }

    #[tokio::test]
    async fn move_within_provider_renames() {
        let (registry, a, _) = setup();
        a.put_file("/src/x", "1");
        let plan = plan_move(&registry, &[p("a", "/src")], &p("a", "/dst"), PlanOptions::default())
            .await
            .unwrap();
        executor(&registry, ConflictPolicy::Fail).run(&plan).await.unwrap();
        assert_eq!(a.paths(), ["/", "/dst", "/dst/x"]);
    }

    #[tokio::test]
    async fn move_across_providers_copies_then_deletes() {
        let (registry, a, b) = setup();
        a.put_file("/src/x", "1");
        a.put_file("/src/sub/y", "2");
        let plan = plan_move(&registry, &[p("a", "/src")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();
        let report = executor(&registry, ConflictPolicy::Fail).run(&plan).await.unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(a.paths(), ["/"]);
        assert_eq!(b.get_file("/src/sub/y").unwrap(), b"2");
    }

    #[tokio::test]
    async fn move_onto_existing_directory_merges() {
        let (registry, a, _) = setup();
        a.put_file("/src/new", "1");
        a.put_file("/dst/old", "2");
        let step = Step::Move { from: p("a", "/src"), to: p("a", "/dst") };
        let plan = Plan { steps: vec![step], ..Default::default() };
        let report = executor(&registry, ConflictPolicy::Overwrite).run(&plan).await.unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(a.paths(), ["/", "/dst", "/dst/new", "/dst/old"]);
    }

    #[tokio::test]
    async fn overwrite_move_without_atomic_rename() {
        // Without ATOMIC_RENAME the existing target is removed first.
        let registry = ProviderRegistry::new();
        let weak = MemoryProvider::with_capabilities(Capabilities::WRITE | Capabilities::SEEK_READ);
        weak.put_file("/a", "1");
        weak.put_file("/b", "2");
        registry.insert(&p("w", "/"), Arc::new(weak.clone()));
        let registry = Arc::new(registry);
        let plan = Plan {
            steps: vec![Step::Move { from: p("w", "/a"), to: p("w", "/b") }],
            ..Default::default()
        };
        let report = executor(&registry, ConflictPolicy::Overwrite).run(&plan).await.unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(weak.get_file("/b").unwrap(), b"1");
        assert!(weak.get_file("/a").is_none());
    }

    #[tokio::test]
    async fn cancellation_stops_the_run() {
        let (registry, a, _) = setup();
        a.put_file("/f", "x");
        let plan = plan_copy(&registry, &[p("a", "/f")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();
        let options = ExecuteOptions::default();
        options.cancel.cancel();
        let err = Executor::new(registry, options).run(&plan).await.unwrap_err();
        assert!(matches!(err, VfsError::Cancelled));
    }

    #[tokio::test]
    async fn stop_on_first_error_when_asked() {
        let (registry, a, b) = setup();
        a.put_file("/f", "new");
        b.put_file("/f", "old");
        let plan = plan_copy(&registry, &[p("a", "/f")], &p("b", "/"), PlanOptions::default())
            .await
            .unwrap();
        let options = ExecuteOptions {
            conflict: ConflictPolicy::Fail,
            continue_on_error: false,
            ..Default::default()
        };
        assert!(Executor::new(registry, options).run(&plan).await.is_err());
    }

    /// Fails reads of one file a given number of times, then behaves.
    struct Flaky {
        inner: MemoryProvider,
        failures: AtomicUsize,
        message: &'static str,
    }

    #[async_trait]
    impl Provider for Flaky {
        fn scheme(&self) -> &str {
            "mem"
        }
        fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }
        async fn stat(&self, p: &VfsPath) -> Result<nmc_vfs::Entry> {
            self.inner.stat(p).await
        }
        async fn list(&self, p: &VfsPath) -> Result<nmc_vfs::ListStream> {
            self.inner.list(p).await
        }
        async fn read(&self, p: &VfsPath, offset: u64) -> Result<nmc_vfs::ReadStream> {
            if self.failures.load(Ordering::SeqCst) > 0 {
                self.failures.fetch_sub(1, Ordering::SeqCst);
                return Err(VfsError::Other(self.message.into()));
            }
            self.inner.read(p, offset).await
        }
    }

    fn flaky_setup(failures: usize) -> (Arc<ProviderRegistry>, MemoryProvider) {
        let registry = ProviderRegistry::new();
        let src = MemoryProvider::new();
        src.put_file("/f", "data");
        let dst = MemoryProvider::new();
        registry.insert(
            &p("a", "/"),
            Arc::new(Flaky {
                inner: src,
                failures: AtomicUsize::new(failures),
                message: "disk glitch",
            }),
        );
        registry.insert(&p("b", "/"), Arc::new(dst.clone()));
        (Arc::new(registry), dst)
    }

    fn copy_f_plan() -> Plan {
        let step = Step::CopyFile {
            from: p("a", "/f"),
            to: p("b", "/f"),
            size: 4,
            modified: None,
            permissions: None,
        };
        Plan { steps: vec![step], total_bytes: 4, total_files: 1, warnings: vec![] }
    }

    #[tokio::test]
    async fn transient_errors_are_retried() {
        let (registry, dst) = flaky_setup(2);
        let retry = RetryPolicy { attempts: 2, initial_delay: Duration::from_millis(1) };
        let options =
            ExecuteOptions { conflict: ConflictPolicy::Fail, retry, ..Default::default() };
        let report = Executor::new(registry, options).run(&copy_f_plan()).await.unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(dst.get_file("/f").unwrap(), b"data");
    }

    #[tokio::test]
    async fn retries_are_bounded() {
        let (registry, _) = flaky_setup(5);
        let retry = RetryPolicy { attempts: 2, initial_delay: Duration::from_millis(1) };
        let options =
            ExecuteOptions { conflict: ConflictPolicy::Fail, retry, ..Default::default() };
        let report = Executor::new(registry, options).run(&copy_f_plan()).await.unwrap();
        assert_eq!(report.failed.len(), 1);
    }

    /// Opens a provider whose first read reports a lost connection.
    struct ReopeningFactory {
        opened: Arc<AtomicUsize>,
        destination: MemoryProvider,
    }

    #[async_trait]
    impl nmc_vfs::ProviderFactory for ReopeningFactory {
        fn scheme(&self) -> &str {
            "mem"
        }
        async fn create(&self, path: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
            if path.authority() == "b" {
                return Ok(Arc::new(self.destination.clone()));
            }
            let first = self.opened.fetch_add(1, Ordering::SeqCst) == 0;
            let inner = MemoryProvider::new();
            inner.put_file("/f", "data");
            Ok(Arc::new(Flaky {
                inner,
                failures: AtomicUsize::new(if first { 1 } else { 0 }),
                message: "agent connection lost: transport error",
            }) as Arc<dyn Provider>)
        }
    }

    #[test]
    fn recognises_lost_connections() {
        assert!(is_connection_error("agent connection lost: transport error"));
        assert!(is_connection_error("sftp://h: Broken pipe"));
        assert!(is_connection_error("operation timed out"));
        assert!(!is_connection_error("permission denied"));
        assert!(!is_connection_error("disk glitch"));
    }

    #[tokio::test(start_paused = true)]
    async fn lost_connections_are_reopened_before_retrying() {
        // A failure that is not a connection error would keep the dead
        // provider; "connection lost" makes the executor forget it so the
        // factory opens a fresh one.
        let opened = Arc::new(AtomicUsize::new(0));
        let destination = MemoryProvider::new();
        let mut registry = ProviderRegistry::new();
        registry.register(ReopeningFactory {
            opened: opened.clone(),
            destination: destination.clone(),
        });
        let registry = Arc::new(registry);
        let retry = RetryPolicy { attempts: 2, initial_delay: Duration::from_millis(1) };
        let options =
            ExecuteOptions { conflict: ConflictPolicy::Fail, retry, ..Default::default() };
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let executor = Executor::new(registry, options)
            .with_observer(Arc::new(move |e| seen.lock().unwrap().push(e)));
        let report = executor.run(&copy_f_plan()).await.unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(destination.get_file("/f").unwrap(), b"data");
        assert_eq!(opened.load(Ordering::SeqCst), 2, "the provider was opened again");
        assert!(events.lock().unwrap().iter().any(|e| matches!(e, OpEvent::Retrying { .. })));
    }

    #[tokio::test]
    async fn parallelism_can_change_while_running() {
        let (registry, a, b) = setup();
        for i in 0..6 {
            a.put_file(&format!("/f{i}"), "x");
        }
        let sources: Vec<_> = (0..6).map(|i| p("a", &format!("/f{i}"))).collect();
        let plan =
            plan_copy(&registry, &sources, &p("b", "/"), PlanOptions::default()).await.unwrap();
        let live = Arc::new(AtomicUsize::new(1));
        let options = ExecuteOptions {
            conflict: ConflictPolicy::Fail,
            live_parallelism: Some(live.clone()),
            ..Default::default()
        };
        let executor = Executor::new(registry, options);
        live.store(3, Ordering::Relaxed);
        let report = executor.run(&plan).await.unwrap();
        assert_eq!(report.done, plan.steps.len());
        assert_eq!(b.get_file("/f5").unwrap(), b"x");
    }

    #[tokio::test]
    async fn level_zero_waits_and_then_continues() {
        let (registry, a, b) = setup();
        for i in 0..3 {
            a.put_file(&format!("/f{i}"), "x");
        }
        let sources: Vec<_> = (0..3).map(|i| p("a", &format!("/f{i}"))).collect();
        let plan =
            plan_copy(&registry, &sources, &p("b", "/"), PlanOptions::default()).await.unwrap();
        let live = Arc::new(AtomicUsize::new(0)); // waiting for a server
        let options = ExecuteOptions {
            conflict: ConflictPolicy::Fail,
            live_parallelism: Some(live.clone()),
            ..Default::default()
        };
        let executor = Executor::new(registry, options);
        let raise = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(600)).await;
            live.store(2, Ordering::Relaxed);
        });
        let report = executor.run(&plan).await.unwrap();
        raise.await.unwrap();
        assert_eq!(report.done, plan.steps.len());
        assert_eq!(b.get_file("/f2").unwrap(), b"x");
    }

    struct Recorder(Mutex<Vec<(usize, StepOutcome)>>, usize);

    impl StepTracker for Recorder {
        fn is_finished(&self, index: usize) -> bool {
            index < self.1
        }
        fn record(&self, index: usize, outcome: &StepOutcome) {
            self.0.lock().unwrap().push((index, outcome.clone()));
        }
    }

    #[tokio::test]
    async fn tracker_skips_finished_steps_and_records_the_rest() {
        let (registry, a, b) = setup();
        a.put_file("/x", "1");
        a.put_file("/y", "2");
        let plan = plan_copy(
            &registry,
            &[p("a", "/x"), p("a", "/y")],
            &p("b", "/"),
            PlanOptions::default(),
        )
        .await
        .unwrap();
        let tracker = Recorder(Mutex::new(Vec::new()), 1); // step 0 "already done"
        executor(&registry, ConflictPolicy::Fail).run_tracked(&plan, Some(&tracker)).await.unwrap();
        assert_eq!(*tracker.0.lock().unwrap(), [(1, StepOutcome::Done)]);
        assert!(b.get_file("/x").is_none());
        assert!(b.get_file("/y").is_some());
    }

    #[tokio::test]
    async fn local_disk_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("deep/er")).unwrap();
        std::fs::write(src.join("deep/er/file"), b"payload").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("deep/er/file", src.join("link")).unwrap();
        let registry = Arc::new(nmc_providers::default_registry());
        let from = VfsPath::from_local(&src).unwrap();
        let to = VfsPath::from_local(dir.path().join("dst")).unwrap();
        let plan = plan_copy(&registry, &[from], &to, PlanOptions::default()).await.unwrap();
        let options = ExecuteOptions {
            conflict: ConflictPolicy::Fail,
            verify: VerifyMode::Full,
            ..Default::default()
        };
        let report = Executor::new(registry, options).run(&plan).await.unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(std::fs::read(dir.path().join("dst/deep/er/file")).unwrap(), b"payload");
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(dir.path().join("dst/link")).unwrap().to_str(),
            Some("deep/er/file")
        );
        let src_mtime = std::fs::metadata(src.join("deep")).unwrap().modified().unwrap();
        let dst_mtime = std::fs::metadata(dir.path().join("dst/deep")).unwrap().modified().unwrap();
        assert_eq!(src_mtime, dst_mtime, "directory mtimes are restored after the contents");
    }
}
