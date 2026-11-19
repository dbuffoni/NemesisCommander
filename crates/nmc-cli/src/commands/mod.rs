//! Implementations of the subcommands.

mod ai;
mod archive;
mod files;
mod find;
mod pick;
mod rename;
mod script;
mod script_test;
mod serve;
mod settings;
mod transfer;

use std::process::ExitCode;

use nmc_app::AppContext;

use crate::cli::{Cli, Command};

/// Errors are reported as text; every command shares this alias.
pub type CmdResult = Result<ExitCode, Box<dyn std::error::Error>>;

/// Dispatches a parsed command line.
pub async fn run(cli: Cli) -> CmdResult {
    let context = AppContext::load()?;
    for warning in &context.warnings {
        eprintln!("nmc: settings: {warning}");
    }
    let Some(command) = cli.command else {
        return start_interface(context, &cli.locations).await;
    };
    match command {
        Command::Ls(args) => files::ls(&context, args).await,
        Command::Cat(args) => files::cat(&context, args).await,
        Command::Hash(args) => files::hash(&context, args).await,
        Command::Mkdir(args) => files::mkdir(&context, args).await,
        Command::Rm(args) => files::rm(&context, args).await,
        Command::Cp(args) => transfer::copy_or_move(&context, args, false).await,
        Command::Mv(args) => transfer::copy_or_move(&context, args, true).await,
        Command::Jobs(args) => transfer::jobs(&context, args).await,
        Command::Find(args) => find::find(&context, args).await,
        Command::Pack(args) => archive::pack(&context, args).await,
        Command::Unpack(args) => archive::unpack(&context, args).await,
        Command::Rename(args) => rename::rename(&context, args).await,
        Command::Config(args) => settings::config(context, args).await,
        Command::Keys => settings::keys(&context),
        Command::Agent(args) => agent(context, args).await,
        Command::Serve(args) => serve::serve(&context, args).await,
        Command::Tls(args) => serve::tls(&context, args),
        Command::Run(args) => script::run(&context, args).await,
        Command::Plugin(args) => script::plugin(&context, args).await,
        Command::Test(args) => script_test::test(&context, args).await,
        Command::Ai(args) => ai::ai(&context, args).await,
        Command::Pick(args) => pick::pick(context, args).await,
    }
}

#[cfg(feature = "tui")]
async fn start_interface(context: AppContext, locations: &[String]) -> CmdResult {
    let mut panels = crate::location::parse_all(locations)?.into_iter();
    nmc_tui::run(context, panels.next(), panels.next()).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "tui"))]
async fn start_interface(_context: AppContext, _locations: &[String]) -> CmdResult {
    Err("the terminal UI is not part of this build; see `nmc --help` for commands".into())
}

/// This machine's settings folder, which the agent tells controllers so
/// they can send their settings here (`nmc config push`).
#[cfg(target_os = "linux")]
pub(crate) fn settings_dir() -> Option<std::path::PathBuf> {
    nmc_config::AppPaths::detect().config_file().parent().map(std::path::Path::to_path_buf)
}

/// `nmc agent --stdio`: serve this machine to a controller over SSH.
#[cfg(target_os = "linux")]
async fn agent(_context: AppContext, args: crate::cli::AgentArgs) -> CmdResult {
    let limits = nmc_agent::Limits {
        roots: args.roots.iter().map(std::path::PathBuf::from).collect(),
        read_only: args.read_only,
        settings_dir: settings_dir(),
    };
    nmc_agent::serve_stdio(limits).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(target_os = "linux"))]
async fn agent(_context: AppContext, _args: crate::cli::AgentArgs) -> CmdResult {
    Err("the agent runs on Linux only; on this system use nmc as a controller".into())
}
