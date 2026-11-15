//! cp, mv and the job queue.

use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use sc_app::AppContext;
use sc_ops::{
    ConflictPolicy, ExecuteOptions, Executor, OpEvent, PlanOptions, RetryPolicy, VerifyMode,
    plan_copy, plan_move,
};
use sc_panel::format::human_size;
use sc_transfer::{JobId, JobKind, JobOptions, JobSpec, JobState, TokenBucket};
use serde::Serialize;

use super::CmdResult;
use crate::cli::{ConflictArg, JobsAction, JobsArgs, TransferArgs, VerifyArg};
use crate::output::{Progress, json_line};
use crate::{location, prompt};

pub fn conflict_policy(arg: Option<ConflictArg>, default: ConflictPolicy) -> ConflictPolicy {
    match arg {
        None => default,
        Some(ConflictArg::Ask) => ConflictPolicy::Ask,
        Some(ConflictArg::Overwrite) => ConflictPolicy::Overwrite,
        Some(ConflictArg::Newer) => ConflictPolicy::OverwriteIfNewer,
        Some(ConflictArg::Skip) => ConflictPolicy::Skip,
        Some(ConflictArg::Rename) => ConflictPolicy::KeepBoth,
        Some(ConflictArg::Fail) => ConflictPolicy::Fail,
    }
}

fn verify_mode(arg: Option<VerifyArg>, default: VerifyMode) -> VerifyMode {
    match arg {
        None => default,
        Some(VerifyArg::None) => VerifyMode::None,
        Some(VerifyArg::Size) => VerifyMode::Size,
        Some(VerifyArg::Fast) => VerifyMode::Fast,
        Some(VerifyArg::Full) => VerifyMode::Full,
    }
}

pub async fn copy_or_move(context: &AppContext, args: TransferArgs, is_move: bool) -> CmdResult {
    let mut paths = location::parse_all(&args.paths)?;
    let destination = paths.pop().expect("clap requires two paths");
    let settings = &context.config.transfers;
    // "Ask" only makes sense with a terminal; otherwise fail on conflicts.
    let default_conflict =
        if prompt::interactive() { settings.conflict } else { ConflictPolicy::Fail };
    let conflict = conflict_policy(args.conflict, default_conflict);
    let verify = verify_mode(args.verify, settings.verify);
    let parallelism = args.parallel.unwrap_or(settings.parallel_files).max(1);
    let preserve = !args.no_preserve;
    let plan_options = PlanOptions { preserve_mtime: preserve, preserve_permissions: preserve };
    let limit = match &args.limit {
        Some(text) => {
            sc_panel::format::parse_size(text).ok_or_else(|| format!("invalid --limit {text:?}"))?
        }
        None => settings.bandwidth_limit_kib * 1024,
    };

    if args.queue {
        let manager = context.transfer_manager()?;
        let spec = JobSpec {
            kind: if is_move { JobKind::Move } else { JobKind::Copy },
            sources: paths,
            destination: Some(destination),
            options: JobOptions {
                conflict: if conflict == ConflictPolicy::Ask {
                    ConflictPolicy::Skip
                } else {
                    conflict
                },
                verify,
                plan: plan_options,
                parallelism,
                retry: RetryPolicy { attempts: settings.retries, ..Default::default() },
                bandwidth_limit: limit,
            },
            priority: 0,
        };
        let title = spec.title();
        let id = manager.submit(spec)?;
        crate::out!("queued {id}: {title}");
        return Ok(ExitCode::SUCCESS);
    }

    let plan = if is_move {
        plan_move(&context.registry, &paths, &destination, plan_options).await?
    } else {
        plan_copy(&context.registry, &paths, &destination, plan_options).await?
    };
    for warning in &plan.warnings {
        eprintln!("sc: warning: {warning}");
    }

    let progress = Arc::new(Mutex::new(Progress::new(plan.total_bytes)));
    let observer = {
        let progress = progress.clone();
        move |event: OpEvent| {
            if let OpEvent::Bytes(n) = event {
                progress.lock().unwrap_or_else(|e| e.into_inner()).add(n);
            }
        }
    };
    let options = ExecuteOptions {
        conflict,
        verify,
        parallelism,
        resume: true,
        continue_on_error: true,
        throttle: (limit > 0).then(|| Arc::new(TokenBucket::new(limit)) as _),
        cancel: Default::default(),
        retry: RetryPolicy { attempts: settings.retries, ..Default::default() },
    };
    let executor = Executor::new(context.registry.clone(), options)
        .with_observer(Arc::new(observer))
        .with_resolver(Arc::new(prompt::TerminalResolver));
    let report = executor.run(&plan).await;
    progress.lock().unwrap_or_else(|e| e.into_inner()).finish();
    let report = report?;
    for (_, message) in &report.failed {
        eprintln!("sc: {message}");
    }
    if report.skipped > 0 {
        eprintln!("sc: {} item(s) skipped", report.skipped);
    }
    Ok(if report.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

#[derive(Serialize)]
struct JsonJob<'a> {
    id: i64,
    title: String,
    state: String,
    steps: usize,
    finished_steps: usize,
    bytes: u64,
    done_bytes: u64,
    errors: &'a [String],
}

pub async fn jobs(context: &AppContext, args: JobsArgs) -> CmdResult {
    let manager = context.transfer_manager()?;
    match args.action.unwrap_or(JobsAction::List) {
        JobsAction::List => {
            for job in manager.jobs()? {
                if args.json {
                    json_line(&JsonJob {
                        id: job.id.0,
                        title: job.spec.title(),
                        state: job.state.to_string(),
                        steps: job.total_steps,
                        finished_steps: job.finished_steps,
                        bytes: job.total_bytes,
                        done_bytes: job.done_bytes,
                        errors: &job.errors,
                    });
                } else {
                    crate::out!(
                        "{:>5}  {:<10} {:>10} / {:<10} {}",
                        job.id.to_string(),
                        job.state.to_string(),
                        human_size(job.done_bytes),
                        human_size(job.total_bytes),
                        job.spec.title()
                    );
                    for error in &job.errors {
                        crate::out!("         ! {error}");
                    }
                }
            }
        }
        JobsAction::Run => {
            manager.run_until_idle().await;
            let failed = manager.jobs()?.iter().filter(|j| j.state == JobState::Failed).count();
            if failed > 0 {
                eprintln!("sc: {failed} job(s) failed; see `sc jobs`");
                return Ok(ExitCode::from(1));
            }
        }
        JobsAction::Pause { id } => manager.pause(JobId(id))?,
        JobsAction::Resume { id } => manager.resume(JobId(id))?,
        JobsAction::Cancel { id } => manager.cancel(JobId(id))?,
        JobsAction::Purge => crate::out!("removed {} finished job(s)", manager.purge_finished()?),
    }
    Ok(ExitCode::SUCCESS)
}
