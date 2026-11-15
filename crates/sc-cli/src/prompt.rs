//! Interactive questions on the terminal.

use std::io::{BufRead, IsTerminal, Write};

use async_trait::async_trait;
use sc_ops::{Conflict, ConflictAction, ConflictResolver, Resolution, unique_name};

/// `true` when both stdin and stderr are terminals, so questions can be asked.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Asks a yes/no question on stderr; default is "no".
pub fn confirm(question: &str) -> bool {
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok()
        && matches!(answer.trim(), "y" | "Y" | "yes")
}

/// Resolves copy conflicts by asking on the terminal.
pub struct TerminalResolver;

#[async_trait]
impl ConflictResolver for TerminalResolver {
    async fn resolve(&self, conflict: &Conflict) -> Resolution {
        let question = format!(
            "{} exists ({} bytes, source {} bytes).\n  [o]verwrite [s]kip [r]ename [a]bort (capital letter = for all): ",
            conflict.destination, conflict.destination_entry.size, conflict.source_entry.size
        );
        let name = conflict.destination.file_name().unwrap_or("file").to_owned();
        tokio::task::spawn_blocking(move || {
            loop {
                eprint!("{question}");
                let _ = std::io::stderr().flush();
                let mut answer = String::new();
                if std::io::stdin().lock().read_line(&mut answer).unwrap_or(0) == 0 {
                    return Resolution { action: ConflictAction::Abort, apply_to_all: false };
                }
                let answer = answer.trim();
                let all = answer.chars().next().is_some_and(char::is_uppercase);
                let action = match answer.to_lowercase().as_str() {
                    "o" => ConflictAction::Overwrite,
                    "s" => ConflictAction::Skip,
                    // The executor recomputes a free name for "all"; for a single
                    // rename we pick one here (the name "(1)" is checked again).
                    "r" => ConflictAction::RenameTo(unique_name(&name, |_| false)),
                    "a" => ConflictAction::Abort,
                    _ => continue,
                };
                return Resolution { action, apply_to_all: all };
            }
        })
        .await
        .unwrap_or(Resolution { action: ConflictAction::Abort, apply_to_all: false })
    }
}
