//! Implementations of the subcommands.

mod ai;
mod archive;
mod files;
mod find;
#[cfg(feature = "mcp")]
mod mcp;
mod pick;
mod rename;
mod script;
mod script_test;
mod serve;
mod settings;
mod shell;
mod transfer;

use std::process::ExitCode;

use pnxr_app::AppContext;

use crate::cli::{Cli, Command};

/// Errors are reported as text; every command shares this alias.
pub type CmdResult = Result<ExitCode, Box<dyn std::error::Error>>;

/// Dispatches a parsed command line.
pub async fn run(cli: Cli) -> CmdResult {
    let context = AppContext::load()?;
    for warning in &context.warnings {
        eprintln!("pnxr: settings: {warning}");
    }
    let Some(command) = cli.command else {
        return start_interface(context, &cli.locations, cli.cwd_file).await;
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
        Command::ShellInit { shell } => shell::shell_init(shell),
        #[cfg(feature = "mcp")]
        Command::Mcp(args) => mcp::mcp(&context, args).await,
        #[cfg(not(feature = "mcp"))]
        Command::Mcp(_) => Err("this build has no MCP server (build with --features mcp)".into()),
        Command::Agent(args) => agent(context, args).await,
        Command::Serve(args) => serve::serve(&context, args).await,
        Command::Tls(args) => serve::tls(&context, args),
        Command::Run(args) => script::run(&context, args).await,
        Command::Plugin(args) => script::plugin(&context, args).await,
        Command::Desktop(args) => desktop(args),
        Command::Test(args) => script_test::test(&context, args).await,
        Command::Ai(args) => ai::ai(&context, args).await,
        Command::Pick(args) => pick::pick(context, args).await,
    }
}

#[cfg(feature = "tui")]
async fn start_interface(
    context: AppContext,
    locations: &[String],
    cwd_file: Option<std::path::PathBuf>,
) -> CmdResult {
    let mut panels = crate::location::parse_all(locations)?.into_iter();
    pnxr_tui::run(context, panels.next(), panels.next(), cwd_file).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "tui"))]
async fn start_interface(
    _context: AppContext,
    _locations: &[String],
    _cwd_file: Option<std::path::PathBuf>,
) -> CmdResult {
    Err("the terminal UI is not part of this build; see `pnxr --help` for commands".into())
}

/// This machine's settings folder, which the agent tells controllers so
/// they can send their settings here (`pnxr config push`).
#[cfg(target_os = "linux")]
pub(crate) fn settings_dir() -> Option<std::path::PathBuf> {
    pnxr_config::AppPaths::detect().config_file().parent().map(std::path::Path::to_path_buf)
}

/// `pnxr agent --stdio`: serve this machine to a controller over SSH.
#[cfg(target_os = "linux")]
async fn agent(_context: AppContext, args: crate::cli::AgentArgs) -> CmdResult {
    let limits = pnxr_agent::Limits {
        roots: args.roots.iter().map(std::path::PathBuf::from).collect(),
        read_only: args.read_only,
        settings_dir: settings_dir(),
    };
    pnxr_agent::serve_stdio(limits).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(target_os = "linux"))]
async fn agent(_context: AppContext, _args: crate::cli::AgentArgs) -> CmdResult {
    Err("the agent runs on Linux only; on this system use pnxr as a controller".into())
}

/// `pnxr desktop install|remove`.
fn desktop(args: crate::cli::DesktopArgs) -> CmdResult {
    use crate::cli::DesktopAction;
    let lines = match args.action {
        DesktopAction::Install { force } => pnxr_app::desktop::install(force),
        DesktopAction::Remove => pnxr_app::desktop::remove(),
    }?;
    for line in lines {
        crate::out!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}
