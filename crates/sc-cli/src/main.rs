//! `sc`: SuperCommander's single binary.
//!
//! * `sc` with no arguments starts the terminal UI.
//! * `sc <command>` runs one operation (`sc cp`, `sc ls`, ...), with text or
//!   `--json` output, for scripts and quick work over SSH.
//! * `sc agent --stdio` serves a remote controller over SSH (Linux only).

mod cli;
mod commands;
mod location;
mod output;
mod prompt;

use std::process::ExitCode;

use clap::Parser;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    match commands::run(cli).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("sc: {error}");
            ExitCode::from(1)
        }
    }
}
