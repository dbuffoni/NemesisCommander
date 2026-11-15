//! Implementations of the subcommands.

mod archive;
mod files;
mod find;
mod rename;
mod settings;
mod transfer;

use std::process::ExitCode;

use sc_app::AppContext;

use crate::cli::{Cli, Command};

/// Errors are reported as text; every command shares this alias.
pub type CmdResult = Result<ExitCode, Box<dyn std::error::Error>>;

/// Dispatches a parsed command line.
pub async fn run(cli: Cli) -> CmdResult {
    let context = AppContext::load()?;
    for warning in &context.warnings {
        eprintln!("sc: settings: {warning}");
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
        Command::Config(args) => settings::config(context, args),
        Command::Keys => settings::keys(&context),
        Command::Agent(args) => agent(context, args).await,
    }
}

#[cfg(feature = "tui")]
async fn start_interface(context: AppContext, locations: &[String]) -> CmdResult {
    let mut panels = crate::location::parse_all(locations)?.into_iter();
    sc_tui::run(context, panels.next(), panels.next()).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "tui"))]
async fn start_interface(_context: AppContext, _locations: &[String]) -> CmdResult {
    Err("the terminal UI is not part of this build; see `sc --help` for commands".into())
}

async fn agent(_context: AppContext, _args: crate::cli::AgentArgs) -> CmdResult {
    Err("agent mode is not part of this build".into())
}
