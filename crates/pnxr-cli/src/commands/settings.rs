//! config and keys.

use std::path::Path;
use std::process::ExitCode;

use pnxr_app::AppContext;
use pnxr_app::sync::{self, SyncMode};
use pnxr_config::{ExportBundle, ImportMode};

use super::CmdResult;
use crate::cli::{ConfigAction, ConfigArgs};

pub async fn config(mut context: AppContext, args: ConfigArgs) -> CmdResult {
    match args.action {
        ConfigAction::Path => {
            crate::out!("settings: {}", context.paths.config_file().display());
            crate::out!("journal:  {}", context.paths.journal_file().display());
        }
        ConfigAction::Show => crate::out!("{}", context.config.to_toml().trim_end()),
        ConfigAction::Export { file } => {
            ExportBundle::new(&context.config).write(Path::new(&file))?;
            crate::out!("settings exported to {file}");
        }
        ConfigAction::Import { file, merge } => {
            let bundle = ExportBundle::read(Path::new(&file))?;
            let mode = if merge { ImportMode::Merge } else { ImportMode::Replace };
            context.config = bundle.apply(&context.config, mode);
            context.save_config()?;
            crate::out!("settings imported from {file}");
        }
        ConfigAction::Push { locations, merge } => {
            let mode = if merge { SyncMode::Merge } else { SyncMode::Replace };
            let mut failed = false;
            for text in &locations {
                let result = async {
                    let location = crate::location::parse(text).map_err(|e| e.to_string())?;
                    sync::push(&context.registry, &context.config, &location, mode).await
                }
                .await;
                match result {
                    Ok(file) => crate::out!("settings sent to {file}"),
                    Err(e) => {
                        eprintln!("pnxr: {text}: {e}");
                        failed = true;
                    }
                }
            }
            if failed {
                return Ok(ExitCode::FAILURE);
            }
        }
        ConfigAction::Pull { location, merge } => {
            let location = crate::location::parse(&location)?;
            let (theirs, file) = sync::pull(&context.registry, &location).await?;
            let mode = if merge { ImportMode::Merge } else { ImportMode::Replace };
            context.config = ExportBundle::new(&theirs).apply(&context.config, mode);
            context.save_config()?;
            crate::out!("settings fetched from {file}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn keys(context: &AppContext) -> CmdResult {
    for (category, rows) in context.keymap.help() {
        crate::out!("{}", category.title());
        for (keys, label) in rows {
            crate::out!("  {keys:<22} {label}");
        }
    }
    Ok(ExitCode::SUCCESS)
}
