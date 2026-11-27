//! `pnxr`: Pairnox's single binary.
//!
//! * `pnxr` with no arguments starts the terminal UI.
//! * `pnxr <command>` runs one operation (`pnxr cp`, `pnxr ls`, ...), with text or
//!   `--json` output, for scripts and quick work over SSH.
//! * `pnxr agent --stdio` serves a remote controller over SSH (Linux only).

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
            eprintln!("pnxr: {error}");
            ExitCode::from(1)
        }
    }
}
