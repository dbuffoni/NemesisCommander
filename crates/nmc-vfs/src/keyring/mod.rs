//! Saved logins in the system's own secret store, one backend per system:
//!
//! - Linux and others: the Secret Service of the desktop (GNOME Keyring,
//!   KWallet, KeePassXC) through `secret-tool`, the command of libsecret;
//! - macOS: the Keychain through the `security` command;
//! - Windows: the Credential Manager through the Win32 credential API.
//!
//! A login is stored under the service `nemesiscommander`, the host and the
//! user, so the system's viewer (Seahorse, Keychain Access, Credential
//! Manager) shows and removes it. The password never travels on a command
//! line: `secret-tool` and `security -i` read it from their standard input,
//! the Windows API takes it in memory. See ADR 0051.
//!
//! Without the store (no `secret-tool`, no Secret Service on the bus, a
//! refused keychain) nothing is found and nothing can be saved; callers fall
//! back to asking.

#[cfg(unix)]
mod secret_tool;
#[cfg(unix)]
mod security;
#[cfg(windows)]
mod wincred;

#[cfg(unix)]
use std::io::Write;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::netrc::Login;

/// Environment variable naming the command to use instead of the one found
/// on `PATH` (`secret-tool`, or `security` on macOS).
pub const COMMAND_VAR: &str = "NMC_SECRET_TOOL";

/// The command is process-wide: tests that set it run one at a time.
#[cfg(all(test, unix))]
pub(crate) static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The value of the `service` attribute of every stored login.
pub(crate) const SERVICE: &str = "nemesiscommander";

fn command_override() -> MutexGuard<'static, Option<PathBuf>> {
    static COMMAND: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    COMMAND.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Uses `path` as the command of this process's backend (before
/// `NMC_SECRET_TOOL` and `PATH`); `None` goes back to them. For tests and
/// embedding.
pub fn set_command(path: Option<PathBuf>) {
    *command_override() = path;
}

/// The command to run: the override, `NMC_SECRET_TOOL`, then `name` on `PATH`
/// (or `fallback` when `PATH` has none).
#[cfg(unix)]
pub(crate) fn find_command(name: &str, fallback: Option<&str>) -> Option<PathBuf> {
    let explicit =
        command_override().clone().or_else(|| std::env::var_os(COMMAND_VAR).map(Into::into));
    if let Some(path) = explicit {
        return path.is_file().then_some(path);
    }
    if let Some(path) = std::env::var_os("PATH")
        && let Some(found) =
            std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.is_file())
    {
        return Some(found);
    }
    fallback.map(PathBuf::from).filter(|p| p.is_file())
}

/// Runs `tool` with `args`, writing `input` to its standard input.
#[cfg(unix)]
pub(crate) fn run_command(
    tool: PathBuf,
    args: &[&str],
    input: Option<&str>,
) -> std::io::Result<std::process::Output> {
    let mut child = Command::new(tool)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(text) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait_with_output()
}

fn missing_input() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "a host, a user and a password are needed",
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
use secret_tool as backend;
#[cfg(target_os = "macos")]
use security as backend;
#[cfg(windows)]
use wincred as backend;

/// What the system's store is called, for the login question.
pub fn store_name() -> &'static str {
    backend::STORE_NAME
}

/// What is missing when [`available`] is false, for the login question.
pub fn missing() -> &'static str {
    backend::MISSING
}

/// Whether logins can be saved here (the command is installed, or the store
/// is part of the system). It does not know whether the store is unlocked or
/// reachable: [`store`] reports that.
pub fn available() -> bool {
    backend::available()
}

/// Saves the password of `user` on `host`, replacing the one stored before.
pub fn store(host: &str, user: &str, password: &str) -> std::io::Result<()> {
    if host.is_empty() || user.is_empty() || password.is_empty() {
        return Err(missing_input());
    }
    backend::store(&host.to_lowercase(), user, password)
}

/// Removes the stored login of `user` on `host` (nothing happens when there
/// is none).
pub fn clear(host: &str, user: &str) -> std::io::Result<()> {
    backend::clear(&host.to_lowercase(), user)
}

/// The stored login for `host`. With `user` only that user's password is
/// looked for; without, the first login saved for the host.
pub fn lookup(host: &str, user: Option<&str>) -> Option<Login> {
    backend::lookup(&host.to_lowercase(), user)
}
