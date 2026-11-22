//! pack and unpack.

use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use nmc_app::AppContext;
use nmc_archive::{ArchiveFormat, PackOptions, archive_root};
use nmc_ops::{ConflictPolicy, ExecuteOptions, Executor, OpEvent, PlanOptions, plan_copy};
use nmc_vfs::collect_listing;

use super::CmdResult;
use super::transfer::conflict_policy;
use crate::cli::{PackArgs, UnpackArgs};
use crate::output::Progress;
use crate::{location, prompt};

/// The environment variable that holds the password of a zip or 7z archive.
const PASSWORD_VARIABLE: &str = "NMC_ARCHIVE_PASSWORD";

pub async fn pack(context: &AppContext, args: PackArgs) -> CmdResult {
    let sources = location::parse_all(&args.sources)?;
    let output = location::parse(&args.output)?;
    let format = match &args.format {
        Some(name) => {
            ArchiveFormat::parse(name).ok_or_else(|| format!("unknown archive format {name:?}"))?
        }
        None => output
            .file_name()
            .and_then(ArchiveFormat::from_file_name)
            .ok_or("cannot tell the archive format from the output name; use --format")?,
    };
    let progress = Arc::new(Mutex::new(Progress::new(0)));
    let tick = progress.clone();
    let options = PackOptions {
        format,
        level: args.level,
        // Never on the command line (it would show in `ps`).
        password: std::env::var(PASSWORD_VARIABLE).ok().filter(|p| !p.is_empty()),
        progress: Some(Arc::new(move |n| tick.lock().unwrap_or_else(|e| e.into_inner()).add(n))),
        cancel: Default::default(),
    };
    let count = nmc_archive::pack(&context.registry, &sources, &output, options).await;
    progress.lock().unwrap_or_else(|e| e.into_inner()).finish();
    crate::out!("{} entries written to {}", count?, args.output);
    Ok(ExitCode::SUCCESS)
}

pub async fn unpack(context: &AppContext, args: UnpackArgs) -> CmdResult {
    let archive = location::parse(&args.archive)?;
    let root = archive_root(&archive).ok_or("not a recognized archive name")?;
    if let Some(password) = std::env::var(PASSWORD_VARIABLE).ok().filter(|p| !p.is_empty()) {
        nmc_archive::set_password(&archive, &password);
    }
    let destination = location::parse(args.destination.as_deref().unwrap_or("."))?;
    let provider = context.registry.provider_for(&root).await?;
    let members: Vec<_> = collect_listing(provider.as_ref(), &root)
        .await?
        .into_iter()
        .map(|e| root.join(&e.name))
        .collect();
    if members.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    let destination_provider = context.registry.provider_for(&destination).await?;
    if destination_provider.stat(&destination).await.is_err() {
        destination_provider.create_dir(&destination).await?;
    }
    let plan = plan_copy(&context.registry, &members, &destination, PlanOptions::default()).await?;
    let default = if prompt::interactive() { ConflictPolicy::Ask } else { ConflictPolicy::Fail };
    let progress = Arc::new(Mutex::new(Progress::new(plan.total_bytes)));
    let tick = progress.clone();
    let executor = Executor::new(
        context.registry.clone(),
        ExecuteOptions { conflict: conflict_policy(args.conflict, default), ..Default::default() },
    )
    .with_resolver(Arc::new(prompt::TerminalResolver))
    .with_observer(Arc::new(move |e| {
        if let OpEvent::Bytes(n) = e {
            tick.lock().unwrap_or_else(|e| e.into_inner()).add(n);
        }
    }));
    let report = executor.run(&plan).await;
    progress.lock().unwrap_or_else(|e| e.into_inner()).finish();
    let report = report?;
    for (_, message) in &report.failed {
        eprintln!("nmc: {message}");
    }
    Ok(if report.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}
