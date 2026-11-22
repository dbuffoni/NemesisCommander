//! ls, cat, hash, mkdir, rm.

use std::process::ExitCode;
use std::sync::Arc;

use nmc_app::AppContext;
use nmc_index::{SortKey, SortSpec, sort_entries};
use nmc_ops::{DeleteOptions, delete_paths};
use nmc_vfs::{VfsError, collect_listing};
use tokio::io::AsyncWriteExt;

use super::CmdResult;
use crate::cli::{CatArgs, HashArgs, LsArgs, MkdirArgs, RmArgs, SortArg};
use crate::output::{JsonEntry, json_line, long_line};
use crate::{location, prompt};

pub async fn ls(context: &AppContext, args: LsArgs) -> CmdResult {
    let path = location::parse(args.path.as_deref().unwrap_or("."))?;
    let provider = context.registry.provider_for(&path).await?;
    let mut entries = collect_listing(provider.as_ref(), &path).await?;
    entries.retain(|e| args.all || !e.is_hidden());
    let key = match args.sort {
        SortArg::Name => SortKey::Name,
        SortArg::Ext => SortKey::Extension,
        SortArg::Size => SortKey::Size,
        SortArg::Date => SortKey::Modified,
    };
    sort_entries(&mut entries, SortSpec { key, descending: args.reverse });
    for entry in &entries {
        if args.json {
            json_line(&JsonEntry::new(entry, None));
        } else if args.long {
            crate::out!("{}", long_line(entry, !args.bytes));
        } else {
            crate::out!("{}{}", entry.name, if entry.is_dir() { "/" } else { "" });
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub async fn cat(context: &AppContext, args: CatArgs) -> CmdResult {
    let mut stdout = tokio::io::stdout();
    for text in &args.paths {
        let path = location::parse(text)?;
        let provider = context.registry.provider_for(&path).await?;
        let mut reader = provider.read(&path, 0).await?;
        tokio::io::copy(&mut reader, &mut stdout).await.map_err(|e| VfsError::from_io(&path, e))?;
    }
    stdout.flush().await?;
    Ok(ExitCode::SUCCESS)
}

pub async fn hash(context: &AppContext, args: HashArgs) -> CmdResult {
    let algorithm = nmc_hash::parse_algorithm(&args.algorithm)
        .ok_or_else(|| format!("unknown algorithm {:?} (blake3, xxh3, sha256)", args.algorithm))?;
    let mut status = ExitCode::SUCCESS;
    for text in &args.paths {
        let path = location::parse(text)?;
        let provider = context.registry.provider_for(&path).await?;
        match nmc_ops::copy::checksum(provider.as_ref(), &path, algorithm).await {
            Ok(digest) => crate::out!("{digest}  {text}"),
            Err(e) => {
                eprintln!("nmc: {e}");
                status = ExitCode::from(1);
            }
        }
    }
    Ok(status)
}

pub async fn mkdir(context: &AppContext, args: MkdirArgs) -> CmdResult {
    for text in &args.paths {
        let path = location::parse(text)?;
        let provider = context.registry.provider_for(&path).await?;
        if !args.parents {
            provider.create_dir(&path).await?;
            continue;
        }
        // Create each missing ancestor, top-down.
        let mut missing = Vec::new();
        let mut cursor = Some(path.clone());
        while let Some(dir) = cursor {
            match provider.stat(&dir).await {
                Ok(entry) if entry.is_dir() => break,
                Ok(_) => return Err(VfsError::NotADirectory(dir).into()),
                Err(e) if e.is_not_found() => {
                    cursor = dir.parent();
                    missing.push(dir);
                }
                Err(e) => return Err(e.into()),
            }
        }
        for dir in missing.iter().rev() {
            match provider.create_dir(dir).await {
                Ok(()) | Err(VfsError::AlreadyExists(_)) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub async fn rm(context: &AppContext, args: RmArgs) -> CmdResult {
    let paths = location::parse_all(&args.paths)?;
    // Deleting is the one operation that cannot be undone, so scripts must
    // say so explicitly with --yes instead of silently skipping the question.
    if !args.yes && !prompt::interactive() {
        return Err("refusing to delete without a terminal to confirm; pass --yes".into());
    }
    if !args.yes && context.config.general.confirm_delete {
        let what =
            if paths.len() == 1 { paths[0].to_string() } else { format!("{} items", paths.len()) };
        let verb = if args.trash { "Move to trash" } else { "Permanently delete" };
        if !prompt::confirm(&format!("{verb} {what}?")) {
            return Ok(ExitCode::from(1));
        }
    }
    let options = DeleteOptions { use_trash: args.trash, observer: None };
    let report = delete_paths(&Arc::clone(&context.registry), &paths, options).await?;
    for (_, message) in &report.failed {
        eprintln!("nmc: {message}");
    }
    Ok(if report.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}
