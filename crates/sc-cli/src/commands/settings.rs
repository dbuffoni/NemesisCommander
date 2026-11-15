//! config and keys.

use std::path::Path;
use std::process::ExitCode;

use sc_app::AppContext;
use sc_config::{ExportBundle, ImportMode};

use super::CmdResult;
use crate::cli::{ConfigAction, ConfigArgs};

pub fn config(mut context: AppContext, args: ConfigArgs) -> CmdResult {
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
