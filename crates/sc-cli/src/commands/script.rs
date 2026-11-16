//! `sc run`: TypeScript scripts with the `sc` API (ADR 0015).

#[cfg(feature = "scripting")]
use std::process::ExitCode;

use sc_app::AppContext;

use super::CmdResult;
use crate::cli::{PluginArgs, RunArgs};

/// Runs a script with the permissions given on the command line.
#[cfg(feature = "scripting")]
pub async fn run(context: &AppContext, args: RunArgs) -> CmdResult {
    use std::sync::Arc;

    use sc_script::{Permissions, ScriptOptions};

    if args.types {
        print!("{}", sc_script::TYPES);
        return Ok(ExitCode::SUCCESS);
    }
    if args.guide {
        print!("{}", sc_script::GUIDE);
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
    let invocation = match &args.panel {
        Some(panel) => Some(sc_script::Invocation::new(
            "run",
            Some(&crate::location::parse(panel)?),
            args.other.as_deref().map(crate::location::parse).transpose()?.as_ref(),
            &crate::location::parse_all(&args.selected)?,
        )),
        None => None,
    };
    let options = ScriptOptions {
        registry: context.registry.clone(),
        base: std::env::current_dir()?,
        permissions,
        args: args.args,
        output: Arc::new(|line| crate::out!("{line}")),
        heap_limit: args.memory.max(16) * 1024 * 1024,
        time_limit,
        invocation,
    };
    sc_script::run(&name, &source, options).await?;
    Ok(ExitCode::SUCCESS)
}

/// `sc plugin ...`.
#[cfg(feature = "scripting")]
pub async fn plugin(context: &AppContext, args: PluginArgs) -> CmdResult {
    use std::sync::Arc;

    use sc_script::plugin::{self, Context, Grants};

    use crate::cli::PluginAction;

    let plugins_dir = context.paths.plugins_dir();
    let mut grants = Grants::load(&context.paths.plugin_grants_file())?;
    let installed = |name: &str| -> Result<plugin::Plugin, String> {
        plugin::Plugin::load(&plugins_dir.join(name))
            .map_err(|_| format!("no plugin {name:?} is installed (see `sc plugin list`)"))
    };
    match args.action {
        PluginAction::List => {
            let (plugins, problems) = plugin::discover(&plugins_dir);
            for problem in problems {
                eprintln!("sc: {problem}");
            }
            for plugin in &plugins {
                let manifest = &plugin.manifest;
                let state = if grants.is_granted(plugin) { "allowed" } else { "not allowed yet" };
                crate::out!(
                    "{} {} ({state})  {}",
                    manifest.name,
                    manifest.version,
                    manifest.description
                );
                for command in &manifest.commands {
                    crate::out!("    {}  {}", command.id, command.title);
                }
                for line in manifest.permissions.describe() {
                    crate::out!("    may {line}");
                }
            }
        }
        PluginAction::Install { folder } => {
            let plugin = plugin::install(std::path::Path::new(&folder), &plugins_dir)?;
            crate::out!("installed {} {}", plugin.name(), plugin.manifest.version);
            if !grants.is_granted(&plugin) {
                eprintln!(
                    "sc: it asks to {}; allow it with `sc plugin allow {}` or at first use",
                    plugin.manifest.permissions.describe().join(", "),
                    plugin.name()
                );
            }
        }
        PluginAction::Remove { name } => {
            let plugin = installed(&name)?;
            std::fs::remove_dir_all(&plugin.dir)?;
            grants.revoke(&name)?;
            crate::out!("removed {name}");
        }
        PluginAction::Allow { name } => grants.grant(&installed(&name)?)?,
        PluginAction::Deny { name } => grants.revoke(&name)?,
        PluginAction::Run { name, command, selection, panel, other } => {
            let plugin = installed(&name)?;
            if !grants.is_granted(&plugin) {
                let request = plugin.manifest.permissions.describe().join(", ");
                let question = format!("Allow plugin {name} to {request}?");
                if !crate::prompt::interactive() {
                    return Err(format!(
                        "plugin {name} asks to {request}: allow it with `sc plugin allow {name}`"
                    )
                    .into());
                }
                if !crate::prompt::confirm(&question) {
                    return Ok(ExitCode::from(1));
                }
                grants.grant(&plugin)?;
            }
            let panel = match panel {
                Some(text) => crate::location::parse(&text)?,
                None => sc_vfs::VfsPath::from_local(std::env::current_dir()?)?,
            };
            let invocation = Context {
                panel: Some(panel),
                other: other.as_deref().map(crate::location::parse).transpose()?,
                selection: crate::location::parse_all(&selection)?,
                home: home_dir(),
            };
            let output: sc_script::Output = Arc::new(|line| crate::out!("{line}"));
            plugin.run(&command, &invocation, context.registry.clone(), output).await?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The user's home folder.
#[cfg(feature = "scripting")]
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(Into::into)
}

#[cfg(not(feature = "scripting"))]
pub async fn plugin(_context: &AppContext, _args: PluginArgs) -> CmdResult {
    Err("plugins are not part of this build (server builds leave them out)".into())
}

#[cfg(not(feature = "scripting"))]
pub async fn run(_context: &AppContext, _args: RunArgs) -> CmdResult {
    Err("scripts are not part of this build (server builds leave them out)".into())
}
