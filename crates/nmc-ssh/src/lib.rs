//! # nmc-ssh
//!
//! NemesisCommander reaches servers through the OpenSSH client (`ssh`) that
//! is already installed and configured on the user's machine, instead of an
//! SSH library. That way `~/.ssh/config`, keys, the SSH agent, known hosts,
//! jump hosts and every other option behave exactly as on the command line.
//! See ADR 0009.
//!
//! This crate builds those command lines and starts them with their
//! standard streams connected, ready to carry a protocol (SFTP or the agent
//! protocol).
//!
//! There is no key setting of its own: `ssh` finds the keys the same way it
//! does for the user in a terminal (`~/.ssh/config`, the default key files,
//! the SSH agent). See ADR 0033.

use std::process::Stdio;

use tokio::process::{Child, Command};

/// `[user@]host[:port]` from a URI authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Login name, if given.
    pub user: Option<String>,
    /// Host name, address or `~/.ssh/config` alias.
    pub host: String,
    /// SSH port, if not the default.
    pub port: Option<u16>,
}

impl Target {
    /// Parses an authority. IPv6 addresses go in brackets: `[::1]:2222`.
    pub fn parse(authority: &str) -> Result<Target, String> {
        let (user, rest) = match authority.rsplit_once('@') {
            Some((user, rest)) => (Some(user.to_owned()), rest),
            None => (None, authority),
        };
        let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
            let (host, after) = inner.split_once(']').ok_or("missing ] in the address")?;
            let port = after.strip_prefix(':').map(str::parse::<u16>).transpose();
            (host.to_owned(), port.map_err(|_| "invalid port")?)
        } else {
            match rest.rsplit_once(':') {
                Some((host, port)) => {
                    (host.to_owned(), Some(port.parse::<u16>().map_err(|_| "invalid port")?))
                }
                None => (rest.to_owned(), None),
            }
        };
        if host.is_empty() {
            return Err("a host name is needed".into());
        }
        Ok(Target { user, host, port })
    }

    /// Whether the target is this computer itself (`localhost`, a loopback
    /// address), where commands run directly and no SSH server is needed.
    /// A login name other than the current user's is a different account
    /// and still needs SSH.
    pub fn is_this_computer(&self) -> bool {
        let loopback = matches!(self.host.as_str(), "localhost" | "127.0.0.1" | "::1")
            || self.host.starts_with("127.");
        let mine = match &self.user {
            None => true,
            Some(user) => std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .is_ok_and(|me| &me == user),
        };
        loopback && mine && self.port.is_none()
    }

    /// The `ssh` destination (`user@host`).
    pub fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }
}

/// What to start on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remote<'a> {
    /// A POSIX shell script (run with `sh -c`; it must not contain `'`).
    Script(&'a str),
    /// An SSH subsystem such as `sftp`.
    Subsystem(&'a str),
}

/// Options for every `ssh` connection: batch mode, a limit for connecting,
/// and a keepalive probe every 15 s so that a dead link ends the session
/// after about 45 s (the program then fails with a connection error)
/// instead of hanging.
const KEEPALIVE_OPTIONS: &[&str] =
    &["BatchMode=yes", "ConnectTimeout=30", "ServerAliveInterval=15", "ServerAliveCountMax=3"];

/// The `ssh` arguments that start `remote` on `target`.
///
/// Batch mode: keys or an SSH agent are required, because a password prompt
/// would land in the middle of the protocol. `-T`: no terminal, so the
/// streams carry bytes unchanged.
///
/// When a password is known for the target (typed for this session, in the
/// keyring or in `~/.netrc`), batch mode is off so that `ssh` may ask for it, through the
/// helper of [`password_env`], and asks once.
pub fn ssh_args(target: &Target, remote: Remote<'_>) -> Result<Vec<String>, String> {
    let mut args = vec!["-T".to_owned()];
    let with_password = password_of(target).is_some();
    for option in KEEPALIVE_OPTIONS {
        if with_password && option.starts_with("BatchMode") {
            args.push("-o".to_owned());
            args.push("NumberOfPasswordPrompts=1".to_owned());
            continue;
        }
        args.push("-o".to_owned());
        args.push((*option).to_owned());
    }
    if let Some(port) = target.port {
        args.push("-p".into());
        args.push(port.to_string());
    }
    match remote {
        Remote::Subsystem(name) => {
            args.push("-s".into());
            args.push(target.destination());
            args.push(name.into());
        }
        Remote::Script(script) => {
            debug_assert!(!script.contains('\''), "scripts are wrapped in single quotes");
            args.push(target.destination());
            args.push("--".into());
            // The remote login shell may not be POSIX (fish, ...).
            args.push(format!("sh -c '{script}'"));
        }
    }
    Ok(args)
}

/// The password for `target`: one typed for this session or saved in
/// the keyring or `~/.netrc` for the same host and user.
fn password_of(target: &Target) -> Option<String> {
    let user = target.user.as_deref()?;
    nmc_vfs::netrc::lookup(&target.host, Some(user)).map(|login| login.password)
}

/// The environment that lets `ssh` read the password of `authority` from
/// the helper script (`SSH_ASKPASS`), without a terminal; empty when no
/// password is known. Pass it to [`spawn_env`].
pub fn password_env(authority: &str) -> Vec<(String, String)> {
    Target::parse(authority).map(|t| target_env(&t)).unwrap_or_default()
}

/// [`password_env`] for a parsed target.
pub fn target_env(target: &Target) -> Vec<(String, String)> {
    let Some(password) = password_of(target) else { return Vec::new() };
    let Some(helper) = askpass_helper() else { return Vec::new() };
    vec![
        ("SSH_ASKPASS".into(), helper.display().to_string()),
        ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
        ("NMC_SSH_PASSWORD".into(), password),
    ]
}

/// A script that prints `$NMC_SSH_PASSWORD`, which `ssh` runs to ask for
/// the password. It holds no secret: the password travels in the
/// environment of the `ssh` process only.
fn askpass_helper() -> Option<std::path::PathBuf> {
    static HELPER: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    HELPER
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("nmc-askpass-{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok()?;
            let (name, text) = if cfg!(windows) {
                ("askpass.cmd", "@echo off\r\necho %NMC_SSH_PASSWORD%\r\n")
            } else {
                ("askpass.sh", "#!/bin/sh\nprintf '%s\\n' \"$NMC_SSH_PASSWORD\"\n")
            };
            let file = dir.join(name);
            std::fs::write(&file, text).ok()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).ok()?;
            }
            Some(file)
        })
        .clone()
}

/// A command line from the environment variable `variable`, if set, with
/// `{host}` replaced by `authority`. Lets tests and unusual setups (for
/// example `docker exec`) replace `ssh`.
pub fn command_override(variable: &str, authority: &str) -> Option<(String, Vec<String>)> {
    expand_template(&std::env::var(variable).ok()?, authority)
}

/// Splits a command template into program and arguments, replacing
/// `{host}` with `authority`. `None` for an empty template.
pub fn expand_template(template: &str, authority: &str) -> Option<(String, Vec<String>)> {
    let mut words = template.split_whitespace().map(|w| w.replace("{host}", authority));
    let program = words.next()?;
    Some((program, words.collect()))
}

/// Starts `program` with piped standard streams. The process is killed when
/// the returned [`Child`] is dropped.
pub fn spawn(program: &str, args: &[String]) -> std::io::Result<Child> {
    spawn_env(program, args, &[])
}

/// [`spawn`] with extra environment variables.
pub fn spawn_env(
    program: &str,
    args: &[String],
    env: &[(String, String)],
) -> std::io::Result<Child> {
    Command::new(program)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        let t = Target::parse("me@example.org:2222").unwrap();
        assert_eq!(
            (t.user.as_deref(), t.host.as_str(), t.port),
            (Some("me"), "example.org", Some(2222))
        );
        assert_eq!(t.destination(), "me@example.org");
        let t = Target::parse("[::1]:22").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("::1", Some(22)));
        assert_eq!(Target::parse("host").unwrap().port, None);
        assert!(Target::parse("host:abc").is_err());
        assert!(Target::parse("me@").is_err());
    }

    #[test]
    fn a_known_password_turns_batch_mode_into_one_askpass_try() {
        let target = Target::parse("tester@askpass-host.invalid").unwrap();
        let batch = ssh_args(&target, Remote::Subsystem("sftp")).unwrap();
        assert!(batch.contains(&"BatchMode=yes".to_owned()));
        assert!(target_env(&target).is_empty());

        nmc_vfs::netrc::set_session("askpass-host.invalid", "tester", "pa ss");
        let args = ssh_args(&target, Remote::Subsystem("sftp")).unwrap();
        assert!(!args.contains(&"BatchMode=yes".to_owned()), "{args:?}");
        assert!(args.contains(&"NumberOfPasswordPrompts=1".to_owned()));
        let env = target_env(&target);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("NMC_SSH_PASSWORD"), "pa ss");
        assert_eq!(get("SSH_ASKPASS_REQUIRE"), "force");
        #[cfg(unix)]
        {
            let out = std::process::Command::new(get("SSH_ASKPASS"))
                .env("NMC_SSH_PASSWORD", "pa ss")
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "pa ss\n");
        }
        // A user the login is not for gets nothing.
        let other = Target::parse("someone@askpass-host.invalid").unwrap();
        assert!(target_env(&other).is_empty());
    }

    #[test]
    fn command_lines() {
        let target = Target::parse("me@h:2200").unwrap();
        assert_eq!(
            ssh_args(&target, Remote::Script("echo hi")).unwrap(),
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=30",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-p",
                "2200",
                "me@h",
                "--",
                "sh -c 'echo hi'"
            ]
        );
        assert_eq!(
            ssh_args(&Target::parse("h").unwrap(), Remote::Subsystem("sftp")).unwrap(),
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=30",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-s",
                "h",
                "sftp"
            ]
        );
    }

    #[test]
    fn templates_replace_the_host() {
        let (program, args) = expand_template("docker exec -i {host} nmc agent", "box").unwrap();
        assert_eq!(program, "docker");
        assert_eq!(args, ["exec", "-i", "box", "nmc", "agent"]);
        assert!(expand_template("   ", "box").is_none());
        assert!(command_override("NMC_SSH_TEST_UNSET_VARIABLE", "h").is_none());
    }
}
