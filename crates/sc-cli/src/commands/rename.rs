//! Multi-rename.

use std::process::ExitCode;

use sc_app::AppContext;
use sc_ops::{CaseChange, RenameRule, apply_renames, preview_renames};

use super::CmdResult;
use crate::cli::{CaseArg, RenameArgs};
use crate::location;

pub async fn rename(context: &AppContext, args: RenameArgs) -> CmdResult {
    let paths = location::parse_all(&args.paths)?;
    let rule = RenameRule {
        template: args.template,
        find: args.find,
        replace: args.replace,
        regex: args.regex,
        case: match args.case {
            CaseArg::Keep => CaseChange::Unchanged,
            CaseArg::Lower => CaseChange::Lower,
            CaseArg::Upper => CaseChange::Upper,
            CaseArg::Title => CaseChange::Title,
        },
        counter_start: args.start,
        counter_step: args.step,
        counter_width: args.digits,
    };
    let names = preview_renames(&paths, &rule)?;
    for (path, name) in paths.iter().zip(&names) {
        let old = path.file_name().unwrap_or_default();
        if old != name {
            crate::out!("{old} -> {name}");
        }
    }
    if !args.dry_run {
        apply_renames(&context.registry, &paths, &names).await?;
    }
    Ok(ExitCode::SUCCESS)
}
