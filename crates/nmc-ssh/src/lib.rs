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
pub fn ssh_args(target: &Target, remote: Remote<'_>) -> Result<Vec<String>, String> {
    let mut args = vec!["-T".to_owned()];
    for option in KEEPALIVE_OPTIONS {
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
    Command::new(program)
        .args(args)
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
