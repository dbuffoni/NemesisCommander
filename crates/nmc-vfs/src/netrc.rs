//! Credentials from a `.netrc` file, the format curl, ftp, git and many
//! other tools read:
//!
//! ```text
//! machine files.example.com login alice password s3cret
//! default login anonymous password guest
//! ```
//!
//! NemesisCommander keeps no password database of its own: saved logins
//! (WebDAV, SMB) go to this file, where the user's other tools already look
//! ([`save`], readable by the owner only), and a login typed for one session
//! is only kept in memory ([`set_session`]).

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

/// Writes the login for `host` into the netrc file (created when missing,
/// readable by its owner only), replacing the entry of the same user.
/// Returns the file written.
pub fn save(host: &str, user: &str, password: &str) -> std::io::Result<PathBuf> {
    let bad = |text: &str| text.is_empty() || text.contains(char::is_whitespace);
    if bad(host) || bad(user) || bad(password) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the netrc format cannot hold an empty value or one with spaces",
        ));
    }
    let path = match netrc_path() {
        Some(path) => path,
        None => {
            let home = directories::BaseDirs::new()
                .ok_or_else(|| std::io::Error::other("no home folder"))?
                .home_dir()
                .to_owned();
            home.join(if cfg!(windows) { "_netrc" } else { ".netrc" })
        }
    };
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut text = replace_entry(&old, host, user, password);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(text.as_bytes())?;
        // A file that existed keeps its old mode: only the owner may read it.
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, text)?;
    Ok(path)
}

/// `text` with the one-line entry for `host` and `user` replaced (or a new
/// one added at the end).
fn replace_entry(text: &str, host: &str, user: &str, password: &str) -> String {
    let entry = format!("machine {host} login {user} password {password}");
    let mut replaced = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let same = words.first() == Some(&"machine")
                && words.get(1) == Some(&host)
                && words.windows(2).any(|w| w[0] == "login" && w[1] == user);
            if same && !replaced {
                replaced = true;
                entry.clone()
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !replaced {
        lines.push(entry);
    }
    lines.join("\n")
}

/// Finds the login for `host` in the user's netrc file. When `user` is
/// given only an entry for that user matches. A login given for this
/// session comes first.
pub fn lookup(host: &str, user: Option<&str>) -> Option<Login> {
    {
        let session = session();
        let host = host.to_lowercase();
        let found = session.iter().find(|((h, u), _)| *h == host && user.is_none_or(|x| x == u));
        if let Some(((_, user), password)) = found {
            return Some(Login { user: user.clone(), password: password.clone() });
        }
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
    fn entries_are_replaced_or_added() {
        let text = "machine a login me password old\nmachine b login me password x";
        let out = replace_entry(text, "a", "me", "new");
        assert_eq!(out, "machine a login me password new\nmachine b login me password x");
        let out = replace_entry(text, "a", "you", "pw");
        assert!(out.ends_with("machine a login you password pw"), "{out}");
        assert_eq!(out.lines().count(), 3);
        assert_eq!(replace_entry("", "h", "u", "p"), "machine h login u password p");
    }

    #[test]
    fn a_session_login_comes_first() {
        set_session("Files.Example.ORG", "zoe", "typed");
        assert!(has_session("files.example.org", "zoe"));
        let login = lookup("files.example.org", Some("zoe")).unwrap();
        assert_eq!((login.user.as_str(), login.password.as_str()), ("zoe", "typed"));
    }

    #[test]
    fn values_with_spaces_cannot_be_saved() {
        assert!(save("h", "u", "two words").is_err());
        assert!(save("h", "", "p").is_err());
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
