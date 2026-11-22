//! find.

use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use nmc_app::AppContext;
use nmc_index::{ContentQuery, Mask, SearchCriteria, search_in_archives};
use tokio_util::sync::CancellationToken;

use super::CmdResult;
use crate::cli::FindArgs;
use crate::location;
use crate::output::{JsonEntry, json_line};

const DAY: Duration = Duration::from_secs(86_400);

pub async fn find(context: &AppContext, args: FindArgs) -> CmdResult {
    let root = location::parse(args.root.as_deref().unwrap_or("."))?;
    let size = |text: &Option<String>| -> Result<Option<u64>, String> {
        text.as_ref()
            .map(|t| nmc_panel::format::parse_size(t).ok_or_else(|| format!("invalid size {t:?}")))
            .transpose()
    };
    let content = match &args.content {
        None => None,
        Some(text) if args.regex => Some(ContentQuery::regex(text, !args.ignore_case)?),
        Some(text) => Some(ContentQuery::text(text, !args.ignore_case)),
    };
    let now = SystemTime::now();
    let criteria = SearchCriteria {
        name: Mask::parse(args.name.as_deref().unwrap_or(""))?,
        content,
        min_size: size(&args.min_size)?,
        max_size: size(&args.max_size)?,
        modified_after: args.newer_than_days.map(|d| now - DAY * d as u32),
        modified_before: args.older_than_days.map(|d| now - DAY * d as u32),
        include_directories: args.dirs,
        max_depth: args.max_depth,
        archives: args.archives,
    };
    let provider = context.registry.provider_for(&root).await?;
    let opener = std::sync::Arc::new(nmc_app::RegistryArchives(context.registry.clone()));
    let mut results =
        search_in_archives(provider, root.clone(), criteria, CancellationToken::new(), opener);
    let mut found = 0usize;
    while let Some(result) = results.recv().await {
        match result {
            Ok(hit) => {
                found += 1;
                let shown = hit
                    .path
                    .relative_to(&root)
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| hit.path.to_string());
                if args.json {
                    json_line(&JsonEntry::new(&hit.entry, Some(hit.path.to_string())));
                } else if let Some((line, text)) = &hit.first_match {
                    crate::out!("{shown}:{line}: {text}");
                } else {
                    crate::out!("{shown}");
                }
            }
            Err(e) => eprintln!("nmc: {e}"),
        }
    }
    Ok(if found > 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}
