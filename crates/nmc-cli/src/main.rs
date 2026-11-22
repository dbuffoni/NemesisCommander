//! `nmc`: NemesisCommander's single binary.
//!
//! * `nmc` with no arguments starts the terminal UI.
//! * `nmc <command>` runs one operation (`nmc cp`, `nmc ls`, ...), with text or
//!   `--json` output, for scripts and quick work over SSH.
//! * `nmc agent --stdio` serves a remote controller over SSH (Linux only).

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
            eprintln!("nmc: {error}");
            ExitCode::from(1)
        }
    }
}
