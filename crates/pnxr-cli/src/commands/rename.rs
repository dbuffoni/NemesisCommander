//! Multi-rename.

use std::process::ExitCode;

use pnxr_app::AppContext;
use pnxr_ops::{CaseChange, RenameRule, apply_renames, preview_renames, rename_conflicts, renamed};

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
    // The preview: every changed name, with the ones that would replace
    // another file marked. Nothing is renamed while any is marked.
    let conflicts = rename_conflicts(&context.registry, &paths, &names).await?;
    for (path, name) in paths.iter().zip(&names) {
        let old = path.file_name().unwrap_or_default();
        if old == name {
            continue;
        }
        let taken = conflicts.contains(&renamed(path, name));
        crate::out!("{old} -> {name}{}", if taken { "  (already exists)" } else { "" });
    }
    if !conflicts.is_empty() {
        return Err(format!(
            "{} new names belong to other files; nothing renamed",
            conflicts.len()
        )
        .into());
    }
    if !args.dry_run {
        apply_renames(&context.registry, &paths, &names).await?;
    }
    Ok(ExitCode::SUCCESS)
}
