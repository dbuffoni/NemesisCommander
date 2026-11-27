//! ls, cat, hash, mkdir, rm.

use std::process::ExitCode;
use std::sync::Arc;

use pnxr_app::AppContext;
use pnxr_index::{SortKey, SortSpec, sort_entries};
use pnxr_ops::{DeleteOptions, delete_paths};
use pnxr_vfs::{VfsError, collect_listing};
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
    let algorithm = pnxr_hash::parse_algorithm(&args.algorithm)
        .ok_or_else(|| format!("unknown algorithm {:?} (blake3, xxh3, sha256)", args.algorithm))?;
    let mut status = ExitCode::SUCCESS;
    if let Some(file) = &args.check {
        use pnxr_ops::checksums::{Verdict, check_checksums};
        let results = check_checksums(&context.registry, &location::parse(file)?).await?;
        let mut bad = 0;
        for (name, verdict) in &results {
            match verdict {
                Verdict::Ok => crate::out!("{name}: OK"),
                Verdict::Failed => {
                    bad += 1;
                    crate::out!("{name}: FAILED");
                }
                Verdict::Missing(why) => {
                    bad += 1;
                    crate::out!("{name}: FAILED open or read ({why})");
                }
            }
        }
        if bad > 0 {
            eprintln!("pnxr: WARNING: {bad} of {} listed files did not match", results.len());
            status = ExitCode::from(1);
        }
        return Ok(status);
    }
    if args.write.is_some() || args.per_file {
        use pnxr_ops::checksums::{Layout, write_checksums};
        let sources =
            args.paths.iter().map(|p| location::parse(p)).collect::<Result<Vec<_>, _>>()?;
        let (layout, target) = match &args.write {
            Some(file) => (Layout::Combined, location::parse(file)?),
            None => (Layout::PerFile, sources[0].parent().unwrap_or_else(|| sources[0].clone())),
        };
        // A combined file takes the name given; the folder is its own.
        let (folder, base) = match layout {
            Layout::Combined => {
                let folder = target.parent().unwrap_or_else(|| target.clone());
                (folder.clone(), folder)
            }
            Layout::PerFile => (target.clone(), target),
        };
        let written = write_checksums(
            &context.registry,
            &base,
            &sources,
            algorithm,
            layout,
            &folder,
            args.force,
        )
        .await?;
        // `--write FILE` names the file: move the standard name to it.
        if let (Layout::Combined, Some(file)) = (layout, &args.write) {
            let wanted = location::parse(file)?;
            if let Some(made) = written.files.first()
                && *made != wanted
            {
                let provider = context.registry.provider_for(made).await?;
                if args.force {
                    let _ = provider.remove_file(&wanted).await;
                }
                provider.rename(made, &wanted).await?;
            }
        }
        eprintln!(
            "pnxr: wrote {} checksum file(s) for {} file(s)",
            written.files.len(),
            written.hashed
        );
        return Ok(status);
    }
    for text in &args.paths {
        let path = location::parse(text)?;
        let provider = context.registry.provider_for(&path).await?;
        match pnxr_ops::copy::checksum(provider.as_ref(), &path, algorithm).await {
            Ok(digest) => crate::out!("{digest}  {text}"),
            Err(e) => {
                eprintln!("pnxr: {e}");
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
        eprintln!("pnxr: {message}");
    }
    Ok(if report.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}
