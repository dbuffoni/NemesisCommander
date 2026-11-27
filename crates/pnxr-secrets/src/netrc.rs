//! Credentials from a `.netrc` file, the format curl, ftp, git and many
//! other tools read:
//!
//! ```text
//! machine files.example.com login alice password s3cret
//! default login anonymous password guest
//! ```
//!
//! Pairnox keeps no password database of its own (ADR 0040): saved
//! logins live in the system keyring ([`crate::keyring`]), a login typed for
//! one session only in memory ([`set_session`]), and this file is only read,
//! for logins the user already keeps there for curl, git and the like.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Environment variable naming the file to read instead of `~/.netrc`
/// (the same variable curl honours).
pub const NETRC_VAR: &str = "NETRC";

/// A login and password for a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    /// User name.
    pub user: String,
    /// Password.
    pub password: String,
}

fn session() -> MutexGuard<'static, HashMap<(String, String), String>> {
    static SESSION: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
    SESSION.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Keeps a login in memory until the program ends (a password typed for a
/// connection that is not saved anywhere).
pub fn set_session(host: &str, user: &str, password: &str) {
    session().insert((host.to_lowercase(), user.to_owned()), password.to_owned());
}

/// Whether a login for `host` and `user` was given for this session.
pub fn has_session(host: &str, user: &str) -> bool {
    session().contains_key(&(host.to_lowercase(), user.to_owned()))
}

/// Finds the login for `host`: one given for this session, else one in the
/// system keyring, else one in the user's netrc file. When `user` is given
/// only a login for that user matches.
pub fn lookup(host: &str, user: Option<&str>) -> Option<Login> {
    {
        let session = session();
        let host = host.to_lowercase();
        let found = session.iter().find(|((h, u), _)| *h == host && user.is_none_or(|x| x == u));
        if let Some(((_, user), password)) = found {
            return Some(Login { user: user.clone(), password: password.clone() });
        }
    }
    if let Some(found) = crate::keyring::lookup(host, user) {
        return Some(found);
    }
    let text = std::fs::read_to_string(netrc_path()?).ok()?;
    find(&text, host, user)
}

/// Uses `path` as the netrc file of this process (before `NETRC` and the
/// home folder); `None` goes back to them. For tests and embedding.
pub fn set_file(path: Option<PathBuf>) {
    *file_override() = path;
}

fn file_override() -> MutexGuard<'static, Option<PathBuf>> {
    static FILE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    FILE.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn netrc_path() -> Option<PathBuf> {
    if let Some(path) = file_override().clone() {
        return Some(path);
    }
    if let Some(path) = std::env::var_os(NETRC_VAR) {
        return Some(path.into());
    }
    let home = directories::BaseDirs::new()?.home_dir().to_owned();
    // Windows tools traditionally use `_netrc`.
    [".netrc", "_netrc"].into_iter().map(|name| home.join(name)).find(|p| p.exists())
}

/// Finds the login for `host` in netrc `text`. The first matching
/// `machine` entry wins; a `default` entry applies to every host.
pub fn find(text: &str, host: &str, user: Option<&str>) -> Option<Login> {
    let mut tokens = text.split_whitespace();
    let mut fallback = None;
    // The entry being read: is it for our host (or default), and its fields.
    let mut current: Option<(bool, Option<String>, Option<String>)> = None;

    let finish = |entry: Option<(bool, Option<String>, Option<String>)>,
                  fallback: &mut Option<Login>|
     -> Option<Login> {
        let (is_default, login, password) = entry?;
        let login = Login { user: login?, password: password.unwrap_or_default() };
        if user.is_some_and(|u| u != login.user) {
            return None;
        }
        if is_default {
            fallback.get_or_insert(login);
            None
        } else {
            Some(login)
        }
    };

    while let Some(token) = tokens.next() {
        match token {
            "machine" | "default" => {
                if let Some(found) = finish(current.take(), &mut fallback) {
                    return Some(found);
                }
                if token == "default" {
                    current = Some((true, None, None));
                } else if tokens.next() == Some(host) {
                    current = Some((false, None, None));
                }
            }
            "login" | "password" | "account" => {
                let value = tokens.next().map(str::to_owned);
                if let Some((_, login, password)) = current.as_mut() {
                    match token {
                        "login" => *login = value,
                        "password" => *password = value,
                        _ => {}
                    }
                }
            }
            // A macro definition runs to the next blank line; it never
            // holds credentials, and ignoring its words is harmless.
            _ => {}
        }
    }
    finish(current, &mut fallback).or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_login_comes_first() {
        set_session("Files.Example.ORG", "zoe", "typed");
        assert!(has_session("files.example.org", "zoe"));
        let login = lookup("files.example.org", Some("zoe")).unwrap();
        assert_eq!((login.user.as_str(), login.password.as_str()), ("zoe", "typed"));
    }

    const SAMPLE: &str = "
        machine other.example.com login bob password b0b
        machine files.example.com
            login alice
            password s3cret
        machine files.example.com login carol password c4rol
        default login anonymous password guest
    ";

    fn login(user: &str, password: &str) -> Option<Login> {
        Some(Login { user: user.into(), password: password.into() })
    }

    #[test]
    fn finds_the_first_entry_for_the_host() {
        assert_eq!(find(SAMPLE, "files.example.com", None), login("alice", "s3cret"));
        assert_eq!(find(SAMPLE, "other.example.com", None), login("bob", "b0b"));
    }

    #[test]
    fn a_user_in_the_location_picks_the_matching_entry() {
        assert_eq!(find(SAMPLE, "files.example.com", Some("carol")), login("carol", "c4rol"));
        assert_eq!(find(SAMPLE, "files.example.com", Some("dave")), None);
    }

    #[test]
    fn default_applies_to_other_hosts() {
        assert_eq!(find(SAMPLE, "unknown.example.com", None), login("anonymous", "guest"));
        assert_eq!(find("machine a login x", "b", None), None);
    }
}
