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
//! A saved connection may name a private key file (a `.pem`, an OpenSSH
//! key, ...). Front ends hand those to [`set_identity_files`]; every `ssh`
//! command line for that login then passes the key with `-i`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::RwLock;

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

    /// Whether both name the same login (no port = port 22).
    fn same_login(&self, other: &Target) -> bool {
        self.user == other.user
            && self.host.eq_ignore_ascii_case(&other.host)
            && self.port.unwrap_or(22) == other.port.unwrap_or(22)
    }

    /// The `ssh` destination (`user@host`).
    pub fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }
}

/// Private key files of saved connections, by login.
static IDENTITY_FILES: RwLock<Vec<(Target, String)>> = RwLock::new(Vec::new());

/// Replaces the private key files used for logins: pairs of a URI
/// authority (`user@host:port`) and a key file (`~/` allowed). Authorities
/// that do not parse are ignored; the first key of a login wins.
pub fn set_identity_files<I, A, F>(list: I)
where
    I: IntoIterator<Item = (A, F)>,
    A: AsRef<str>,
    F: Into<String>,
{
    let parsed = list
        .into_iter()
        .filter_map(|(authority, file)| {
            Some((Target::parse(authority.as_ref()).ok()?, file.into()))
        })
        .collect();
    *IDENTITY_FILES.write().unwrap_or_else(|e| e.into_inner()) = parsed;
}

/// The private key file configured for `target`, if any.
pub fn identity_file(target: &Target) -> Option<String> {
    let list = IDENTITY_FILES.read().unwrap_or_else(|e| e.into_inner());
    list.iter().find(|(t, _)| t.same_login(target)).map(|(_, file)| file.clone())
}

/// `path` with a leading `~/` (or `~` alone) replaced by the home folder.
pub fn expand_home(path: &str) -> PathBuf {
    let home = || std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if path == "~" {
        return home().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(path));
    }
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => match home() {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

/// Checks that `file` (already expanded) can serve as a private key: it
/// exists, is a file and, on Unix, only its owner can read it (OpenSSH
/// ignores keys that others can read).
pub fn check_identity_file(file: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(file).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("private key {} not found", file.display()),
        _ => format!("private key {}: {e}", file.display()),
    })?;
    if !metadata.is_file() {
        return Err(format!("private key {} is not a file", file.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(format!(
                "private key {} can be read by other users ({mode:o}) and ssh refuses it: \
                 run chmod 600 {}",
                file.display(),
                file.display()
            ));
        }
    }
    Ok(())
}

/// What to start on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remote<'a> {
    /// A POSIX shell script (run with `sh -c`; it must not contain `'`).
    Script(&'a str),
    /// An SSH subsystem such as `sftp`.
    Subsystem(&'a str),
}

/// The `ssh` arguments that start `remote` on `target`.
///
/// Batch mode: keys or an SSH agent are required, because a password prompt
/// would land in the middle of the protocol. `-T`: no terminal, so the
/// streams carry bytes unchanged. When a private key file is set for the
/// login ([`set_identity_files`]), ssh uses that key only; an error explains
/// a missing key file or one that others can read.
pub fn ssh_args(target: &Target, remote: Remote<'_>) -> Result<Vec<String>, String> {
    let mut args = vec!["-T".to_owned(), "-o".to_owned(), "BatchMode=yes".to_owned()];
    if let Some(file) = identity_file(target) {
        let path = expand_home(&file);
        check_identity_file(&path)?;
        args.push("-i".into());
        args.push(path.display().to_string());
        args.push("-o".into());
        args.push("IdentitiesOnly=yes".into());
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
            ["-T", "-o", "BatchMode=yes", "-p", "2200", "me@h", "--", "sh -c 'echo hi'"]
        );
        assert_eq!(
            ssh_args(&Target::parse("h").unwrap(), Remote::Subsystem("sftp")).unwrap(),
            ["-T", "-o", "BatchMode=yes", "-s", "h", "sftp"]
        );
    }

    /// One test for the global key list, so tests never race on it.
    #[test]
    fn private_keys_of_saved_connections() {
        let dir = std::env::temp_dir().join(format!("nmc-ssh-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("server.pem");
        std::fs::write(&key, "key").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let key_text = key.display().to_string();
        set_identity_files([
            ("me@keyhost:22", key_text.clone()),
            ("me@missing", dir.join("nope.pem").display().to_string()),
            ("not a host:x", "ignored".to_owned()),
        ]);

        // Same login, port 22 written or not, host in any case.
        let args = ssh_args(&Target::parse("me@KeyHost").unwrap(), Remote::Subsystem("sftp"));
        assert_eq!(
            args.unwrap(),
            ["-T", "-o", "BatchMode=yes", "-i", &key_text, "-o", "IdentitiesOnly=yes"]
                .into_iter()
                .chain(["-s", "me@KeyHost", "sftp"])
                .collect::<Vec<_>>()
        );
        // Another user or port: the user's usual setup.
        for other in ["you@keyhost", "me@keyhost:2222", "keyhost"] {
            let args = ssh_args(&Target::parse(other).unwrap(), Remote::Subsystem("sftp"));
            assert!(!args.unwrap().contains(&"-i".to_owned()), "{other}");
        }
        // A key file that is not there.
        let error =
            ssh_args(&Target::parse("me@missing").unwrap(), Remote::Subsystem("sftp")).unwrap_err();
        assert!(error.contains("nope.pem not found"), "{error}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
            let error = ssh_args(&Target::parse("me@keyhost").unwrap(), Remote::Script("true"))
                .unwrap_err();
            assert!(error.contains("chmod 600"), "{error}");
        }
        assert!(check_identity_file(&dir).unwrap_err().contains("not a file"));

        set_identity_files(Vec::<(String, String)>::new());
        assert_eq!(identity_file(&Target::parse("me@keyhost").unwrap()), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn home_is_expanded() {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        if let Some(home) = home {
            assert_eq!(expand_home("~/.ssh/a.pem"), PathBuf::from(&home).join(".ssh/a.pem"));
            assert_eq!(expand_home("~"), PathBuf::from(&home));
        }
        assert_eq!(expand_home("/k/a.pem"), PathBuf::from("/k/a.pem"));
        assert_eq!(expand_home("~other/a.pem"), PathBuf::from("~other/a.pem"));
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
