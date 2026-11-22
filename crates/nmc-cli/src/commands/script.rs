//! `nmc run`: TypeScript scripts with the `nmc` API (ADR 0015).

#[cfg(feature = "scripting")]
use std::process::ExitCode;

use nmc_app::AppContext;

use super::CmdResult;
use crate::cli::{PluginArgs, RunArgs};

/// Runs a script with the permissions given on the command line.
#[cfg(feature = "scripting")]
pub async fn run(context: &AppContext, args: RunArgs) -> CmdResult {
    use std::sync::Arc;

    use nmc_script::{Permissions, ScriptOptions};

    if args.types {
        print!("{}", nmc_script::TYPES);
        return Ok(ExitCode::SUCCESS);
    }
    if args.guide {
        print!("{}", nmc_script::GUIDE);
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
        Some(panel) => Some(nmc_script::Invocation::new(
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
    nmc_script::run(&name, &source, options).await?;
    Ok(ExitCode::SUCCESS)
}

/// `nmc plugin ...`.
#[cfg(feature = "scripting")]
pub async fn plugin(context: &AppContext, args: PluginArgs) -> CmdResult {
    use std::sync::Arc;

    use nmc_script::plugin::{self, Context, Grants};

    use crate::cli::PluginAction;

    let plugins_dir = context.paths.plugins_dir();
    let mut grants = Grants::load(&context.paths.plugin_grants_file())?;
    let mut lock = nmc_script::source::Lock::load(&context.paths.plugin_lock_file())?;
    let installed = |name: &str| -> Result<plugin::Plugin, String> {
        plugin::Plugin::load(&plugins_dir.join(name))
            .map_err(|_| format!("no plugin {name:?} is installed (see `nmc plugin list`)"))
    };
    match args.action {
        PluginAction::List => {
            let (plugins, problems) = plugin::discover(&plugins_dir);
            for problem in problems {
                eprintln!("nmc: {problem}");
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
                if let Some(source) = lock.get(&manifest.name) {
                    let reference =
                        source.reference.as_deref().map(|r| format!("#{r}")).unwrap_or_default();
                    crate::out!(
                        "    from {}{reference} at {}",
                        source.url,
                        &source.commit[..source.commit.len().min(10)]
                    );
                }
                for command in &manifest.commands {
                    crate::out!("    {}  {}", command.id, command.title);
                }
                for line in manifest.permissions.describe() {
                    crate::out!("    may {line}");
                }
            }
        }
        PluginAction::Install { source, subdir } => {
            let plugin = match nmc_script::source::parse_repository(&source) {
                Some(repository) => {
                    eprintln!("nmc: fetching {}…", repository.url);
                    nmc_script::source::install(
                        &repository,
                        subdir.as_deref(),
                        &plugins_dir,
                        &mut lock,
                    )?
                }
                None => {
                    if subdir.is_some() {
                        return Err("--subdir applies to a Git repository only".into());
                    }
                    let plugin = plugin::install(std::path::Path::new(&source), &plugins_dir)?;
                    // A copy from a folder no longer follows its repository.
                    lock.forget(plugin.name())?;
                    plugin
                }
            };
            crate::out!("installed {} {}", plugin.name(), plugin.manifest.version);
            if !grants.is_granted(&plugin) {
                eprintln!(
                    "nmc: it asks to {}; allow it with `nmc plugin allow {}` or at first use",
                    plugin.manifest.permissions.describe().join(", "),
                    plugin.name()
                );
            }
        }
        PluginAction::Update { names } => {
            let names: Vec<String> =
                if names.is_empty() { lock.names().map(str::to_owned).collect() } else { names };
            if names.is_empty() {
                crate::out!("no plugin was installed from Git");
            }
            let mut failed = false;
            for name in names {
                match nmc_script::source::update(&name, &plugins_dir, &mut lock) {
                    Ok(nmc_script::source::Updated::Current { version }) => {
                        crate::out!("{name} {version} is up to date");
                    }
                    Ok(nmc_script::source::Updated::Newer { plugin, from }) => {
                        crate::out!(
                            "updated {name} {} -> {} ({} -> {})",
                            from.0,
                            plugin.manifest.version,
                            &from.1[..from.1.len().min(10)],
                            lock.get(&name).map_or("", |s| &s.commit[..s.commit.len().min(10)]),
                        );
                        if !grants.is_granted(&plugin) {
                            eprintln!(
                                "nmc: {name} now asks to {}; allow it with `nmc plugin allow {name}` or at first use",
                                plugin.manifest.permissions.describe().join(", ")
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("nmc: {name}: {e}");
                        failed = true;
                    }
                }
            }
            if failed {
                return Ok(ExitCode::from(1));
            }
        }
        PluginAction::Remove { name } => {
            let plugin = installed(&name)?;
            std::fs::remove_dir_all(&plugin.dir)?;
            grants.revoke(&name)?;
            lock.forget(&name)?;
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
                        "plugin {name} asks to {request}: allow it with `nmc plugin allow {name}`"
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
                None => nmc_vfs::VfsPath::from_local(std::env::current_dir()?)?,
            };
            let invocation = Context {
                panel: Some(panel),
                other: other.as_deref().map(crate::location::parse).transpose()?,
                selection: crate::location::parse_all(&selection)?,
                home: home_dir(),
            };
            let output: nmc_script::Output = Arc::new(|line| crate::out!("{line}"));
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
