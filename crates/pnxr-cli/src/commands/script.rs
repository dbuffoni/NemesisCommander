//! `pnxr run`: TypeScript scripts with the `pnxr` API (ADR 0015).

#[cfg(feature = "scripting")]
use std::process::ExitCode;

use pnxr_app::AppContext;

use super::CmdResult;
use crate::cli::{PluginArgs, RunArgs};

/// Runs a script with the permissions given on the command line.
#[cfg(feature = "scripting")]
pub async fn run(context: &AppContext, args: RunArgs) -> CmdResult {
    use std::sync::Arc;

    use pnxr_script::{Permissions, ScriptOptions};

    if args.types {
        print!("{}", pnxr_script::TYPES);
        return Ok(ExitCode::SUCCESS);
    }
    if args.guide {
        print!("{}", pnxr_script::GUIDE);
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
        Some(panel) => Some(pnxr_script::Invocation::new(
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
    pnxr_script::run(&name, &source, options).await?;
    Ok(ExitCode::SUCCESS)
}

/// `pnxr plugin ...`.
#[cfg(feature = "scripting")]
pub async fn plugin(context: &AppContext, args: PluginArgs) -> CmdResult {
    use std::sync::Arc;

    use pnxr_script::catalog;
    use pnxr_script::plugin::{self, Context, Grants};

    use crate::cli::PluginAction;

    let plugins_dir = context.paths.plugins_dir();
    let mut grants = Grants::load(&context.paths.plugin_grants_file())?;
    let mut lock = pnxr_script::source::Lock::load(&context.paths.plugin_lock_file())?;
    let installed = |name: &str| -> Result<plugin::Plugin, String> {
        plugin::Plugin::load(&plugins_dir.join(name))
            .map_err(|_| format!("no plugin {name:?} is installed (see `pnxr plugin list`)"))
    };
    match args.action {
        PluginAction::List => {
            let (plugins, problems) = plugin::discover(&plugins_dir);
            for problem in problems {
                eprintln!("pnxr: {problem}");
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
        PluginAction::Available { search, refresh } => {
            let specs = repository_specs(&context.config);
            let (entries, problems) =
                catalog::available(&context.paths.plugin_catalog_dir(), &specs, refresh);
            for problem in problems {
                eprintln!("pnxr: {problem}");
            }
            let rows = catalog::rows(&entries, search.as_deref().unwrap_or(""), &plugins_dir);
            if rows.is_empty() {
                crate::out!("no plugin found");
            }
            for row in rows {
                let state = row.state();
                crate::out!(
                    "{} {}  [{}]{}{}",
                    row.entry.name,
                    row.entry.version,
                    row.origin(),
                    if state.is_empty() { String::new() } else { format!(" ({state})") },
                    if row.entry.description.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", row.entry.description)
                    },
                );
            }
        }
        PluginAction::Repo { action } => return repo(context, action),
        PluginAction::Install { source, subdir } => {
            let plugin = match pnxr_script::source::parse_repository(&source) {
                Some(repository) => {
                    eprintln!("pnxr: fetching {}…", repository.url);
                    pnxr_script::source::install(
                        &repository,
                        subdir.as_deref(),
                        &plugins_dir,
                        &mut lock,
                    )?
                }
                None if !named_folder(&source) => {
                    if subdir.is_some() {
                        return Err("--subdir applies to a Git repository only".into());
                    }
                    let specs = repository_specs(&context.config);
                    let (entries, problems) =
                        catalog::available(&context.paths.plugin_catalog_dir(), &specs, false);
                    for problem in problems {
                        eprintln!("pnxr: {problem}");
                    }
                    let entry = catalog::find(&entries, &source)?;
                    eprintln!("pnxr: fetching {}…", entry.repository);
                    catalog::install(entry, &plugins_dir, &mut lock)?
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
            for warning in plugin.warnings() {
                eprintln!("pnxr: {warning}");
            }
            if !grants.is_granted(&plugin) {
                eprintln!(
                    "pnxr: it asks to {}; allow it with `pnxr plugin allow {}` or at first use",
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
                match pnxr_script::source::update(&name, &plugins_dir, &mut lock) {
                    Ok(pnxr_script::source::Updated::Current { version }) => {
                        crate::out!("{name} {version} is up to date");
                    }
                    Ok(pnxr_script::source::Updated::Newer { plugin, from }) => {
                        crate::out!(
                            "updated {name} {} -> {} ({} -> {})",
                            from.0,
                            plugin.manifest.version,
                            &from.1[..from.1.len().min(10)],
                            lock.get(&name).map_or("", |s| &s.commit[..s.commit.len().min(10)]),
                        );
                        if !grants.is_granted(&plugin) {
                            eprintln!(
                                "pnxr: {name} now asks to {}; allow it with `pnxr plugin allow {name}` or at first use",
                                plugin.manifest.permissions.describe().join(", ")
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("pnxr: {name}: {e}");
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
                        "plugin {name} asks to {request}: allow it with `pnxr plugin allow {name}`"
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
                None => pnxr_vfs::VfsPath::from_local(std::env::current_dir()?)?,
            };
            let invocation = Context {
                panel: Some(panel),
                other: other.as_deref().map(crate::location::parse).transpose()?,
                selection: crate::location::parse_all(&selection)?,
                home: home_dir(),
            };
            let output: pnxr_script::Output = Arc::new(|line| crate::out!("{line}"));
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

/// The repositories in use: the official one (unless switched off) and the
/// user's.
#[cfg(feature = "scripting")]
fn repository_specs(config: &pnxr_config::Config) -> Vec<pnxr_script::catalog::RepositorySpec> {
    let extra: Vec<_> = config
        .plugins
        .repositories
        .iter()
        .map(|r| pnxr_script::catalog::RepositorySpec {
            url: r.url.clone(),
            reference: r.reference.clone(),
        })
        .collect();
    pnxr_script::catalog::repositories(config.plugins.official, &extra)
}

/// Whether `text` names something on disk (a folder to install from), as
/// opposed to a plugin's name.
#[cfg(feature = "scripting")]
fn named_folder(text: &str) -> bool {
    std::path::Path::new(text).exists() || text.contains(['/', '\\']) || text.starts_with('.')
}

/// `pnxr plugin repo ...`.
#[cfg(feature = "scripting")]
fn repo(context: &AppContext, action: crate::cli::RepoAction) -> CmdResult {
    use pnxr_config::PluginRepository;
    use pnxr_script::catalog::RepositorySpec;

    use crate::cli::RepoAction;

    let mut config = context.config.clone();
    match action {
        RepoAction::List => {
            if config.plugins.official {
                crate::out!("{}  (official)", pnxr_script::catalog::OFFICIAL_URL);
            } else {
                crate::out!("official repository is off (pnxr plugin repo official on)");
            }
            for repository in &config.plugins.repositories {
                crate::out!("{}", repository.label());
            }
            return Ok(ExitCode::SUCCESS);
        }
        RepoAction::Add { repository, yes } => {
            let spec = RepositorySpec::parse(&repository)
                .ok_or_else(|| format!("{repository:?} is not a Git repository address"))?;
            if spec.is_official() {
                return Err("the official repository is built in".into());
            }
            if !yes {
                let question = format!(
                    "Add {}? Its plugins run code on your files once you allow them",
                    spec.label()
                );
                if !crate::prompt::interactive() || !crate::prompt::confirm(&question) {
                    return Err("not added (confirm with --yes)".into());
                }
            }
            eprintln!("pnxr: checking {}…", spec.url);
            let (entries, problems) =
                pnxr_script::catalog::refresh(&context.paths.plugin_catalog_dir(), &spec)?;
            if entries.is_empty() {
                return Err(format!(
                    "{} has no plugins in a plugins/ folder{}",
                    spec.url,
                    problems.first().map(|p| format!(" ({p})")).unwrap_or_default()
                )
                .into());
            }
            if !config.plugins.add(PluginRepository { url: spec.url, reference: spec.reference }) {
                crate::out!("already in the list");
                return Ok(ExitCode::SUCCESS);
            }
            crate::out!("added: {} plugin(s) available", entries.len());
        }
        RepoAction::Remove { repository } => {
            if !config.plugins.remove(&repository) {
                return Err(
                    format!("{repository:?} is not in the list (pnxr plugin repo list)").into()
                );
            }
            crate::out!("removed (plugins installed from it stay installed)");
        }
        RepoAction::Official { state } => config.plugins.official = state == "on",
    }
    config.save(&context.paths.config_file())?;
    Ok(ExitCode::SUCCESS)
}
