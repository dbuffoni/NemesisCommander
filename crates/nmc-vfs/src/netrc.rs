//! Credentials from a `.netrc` file, the format curl, ftp, git and many
//! other tools read:
//!
//! ```text
//! machine files.example.com login alice password s3cret
//! default login anonymous password guest
//! ```
//!
//! NemesisCommander never stores passwords itself; users who want saved
//! logins (WebDAV, SMB) keep them where their other tools already look.

use std::path::PathBuf;

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

/// Finds the login for `host` in the user's netrc file. When `user` is
/// given only an entry for that user matches.
pub fn lookup(host: &str, user: Option<&str>) -> Option<Login> {
    let text = std::fs::read_to_string(netrc_path()?).ok()?;
    find(&text, host, user)
}

fn netrc_path() -> Option<PathBuf> {
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
