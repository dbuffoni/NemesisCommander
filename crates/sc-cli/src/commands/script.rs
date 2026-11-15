//! `sc run`: TypeScript scripts with the `sc` API (ADR 0015).

use std::process::ExitCode;

use sc_app::AppContext;

use super::CmdResult;
use crate::cli::RunArgs;

/// Runs a script with the permissions given on the command line.
#[cfg(feature = "scripting")]
pub async fn run(context: &AppContext, args: RunArgs) -> CmdResult {
    use std::sync::Arc;

    use sc_script::{Permissions, ScriptOptions};

    if args.types {
        print!("{}", sc_script::TYPES);
        return Ok(ExitCode::SUCCESS);
    }
    let name = args.script.expect("clap requires a script unless --types");
    let source = std::fs::read_to_string(&name).map_err(|e| format!("{name}: {e}"))?;

    let mut permissions = if args.allow_all { Permissions::all() } else { Permissions::none() };
    for location in &args.allow_read {
        permissions.allow_read(crate::location::parse(location)?);
    }
    for location in &args.allow_write {
        permissions.allow_write(crate::location::parse(location)?);
    }
    let time_limit = args
        .timeout
        .map(|seconds| {
            std::time::Duration::try_from_secs_f64(seconds)
                .map_err(|_| format!("--timeout {seconds}: not a number of seconds"))
        })
        .transpose()?;
    let options = ScriptOptions {
        registry: context.registry.clone(),
        base: std::env::current_dir()?,
        permissions,
        args: args.args,
        output: Arc::new(|line| crate::out!("{line}")),
        heap_limit: args.memory.max(16) * 1024 * 1024,
        time_limit,
    };
    sc_script::run(&name, &source, options).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "scripting"))]
pub async fn run(_context: &AppContext, _args: RunArgs) -> CmdResult {
    Err("scripts are not part of this build (server builds leave them out)".into())
}
