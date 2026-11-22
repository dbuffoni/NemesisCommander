//! `nmc pick`: the terminal UI as a file chooser.
//!
//! The interface is drawn on standard error; the chosen paths are printed
//! on standard output, one per line (or NUL-terminated with `-0`), after the
//! terminal is back to normal. Exit status 0 when something was chosen, 1
//! when the user cancelled.

use std::io::Write;
use std::process::ExitCode;

use nmc_app::AppContext;
use nmc_vfs::VfsPath;

use super::CmdResult;
use crate::cli::PickArgs;

#[cfg(feature = "tui")]
pub async fn pick(context: AppContext, args: PickArgs) -> CmdResult {
    let start = args.location.as_deref().map(crate::location::parse).transpose()?;
    let mode = nmc_tui::PickMode { folder: args.dir, multiple: args.multiple };
    let Some(chosen) = nmc_tui::pick(context, start, mode).await? else {
        return Ok(ExitCode::from(1));
    };
    let end = if args.null { '\0' } else { '\n' };
    let mut out = std::io::stdout().lock();
    for path in &chosen {
        write!(out, "{}{end}", printable(path))?;
    }
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "tui"))]
pub async fn pick(_context: AppContext, _args: PickArgs) -> CmdResult {
    Err("the terminal UI is not part of this build; see `nmc --help` for commands".into())
}

/// A local path as the system writes it, any other location as its URI.
fn printable(path: &VfsPath) -> String {
    match path.to_local_path() {
        Some(local) => local.display().to_string(),
        None => path.to_string(),
    }
}
