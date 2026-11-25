//! The Secret Service of the desktop (GNOME Keyring, KWallet, KeePassXC),
//! through `secret-tool`, the command that ships with libsecret. A login has
//! the attributes `service nemesiscommander`, `host` and `user`.

#![cfg_attr(target_os = "macos", allow(dead_code))]

use super::{SERVICE, find_command, run_command};
use crate::netrc::Login;
use std::path::PathBuf;

pub(super) const STORE_NAME: &str = "the system keyring (GNOME Keyring, KWallet)";
pub(super) const MISSING: &str = "needs secret-tool: not found";

/// The `secret-tool` to run, if there is one.
fn command() -> Option<PathBuf> {
    find_command("secret-tool", None)
}

pub(super) fn available() -> bool {
    command().is_some()
}

fn run(args: &[&str], input: Option<&str>) -> std::io::Result<std::process::Output> {
    let tool = command().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "secret-tool is not installed (package libsecret-tools or libsecret)",
        )
    })?;
    run_command(tool, args, input)
}

/// Saves the password of `user` on `host`, replacing the one stored before.
pub(super) fn store(host: &str, user: &str, password: &str) -> std::io::Result<()> {
    let label = format!("NemesisCommander: {user}@{host}");
    let out = run(
        &["store", "--label", &label, "service", SERVICE, "host", host, "user", user],
        Some(password),
    )?;
    if out.status.success() {
        return Ok(());
    }
    let why = String::from_utf8_lossy(&out.stderr);
    let why = why.trim();
    Err(std::io::Error::other(if why.is_empty() {
        "the keyring refused it (is a Secret Service such as GNOME Keyring running?)".to_owned()
    } else {
        why.to_owned()
    }))
}

/// Removes the stored login of `user` on `host` (nothing happens when there
/// is none).
pub(super) fn clear(host: &str, user: &str) -> std::io::Result<()> {
    run(&["clear", "service", SERVICE, "host", host, "user", user], None).map(|_| ())
}

/// The stored login for `host`. With `user` only that user's password is
/// looked for; without, the first login saved for the host.
pub(super) fn lookup(host: &str, user: Option<&str>) -> Option<Login> {
    command()?;
    if let Some(user) = user {
        let out = run(&["lookup", "service", SERVICE, "host", host, "user", user], None).ok()?;
        let password = String::from_utf8(out.stdout).ok()?;
        return (out.status.success() && !password.is_empty())
            .then(|| Login { user: user.to_owned(), password });
    }
    let out = run(&["search", "--all", "service", SERVICE, "host", host], None).ok()?;
    parse_search(&String::from_utf8_lossy(&out.stdout))
}

/// The first login in the output of `secret-tool search`: blocks of
/// `secret = …` and `attribute.user = …` lines.
fn parse_search(text: &str) -> Option<Login> {
    let (mut user, mut password) = (None, None);
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("secret = ") {
            password = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("attribute.user = ") {
            user = Some(value.to_owned());
        }
        if let (Some(user), Some(password)) = (&user, &password) {
            return Some(Login { user: user.clone(), password: password.clone() });
        }
    }
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::{ONE_AT_A_TIME, set_command};
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A fake `secret-tool` that keeps secrets in files of `dir`, named by
    /// the host and user attributes.
    fn fake(dir: &std::path::Path) -> PathBuf {
        let script = dir.join("secret-tool");
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
cmd=$1; shift
[ "$cmd" = store ] && {{ shift 2; }}   # --label TEXT
[ "$cmd" = search ] && shift           # --all
while [ $# -gt 0 ]; do
  case $1 in host) h=$2;; user) u=$2;; esac
  shift 2
done
f={dir}/$h.${{u:-any}}
case $cmd in
  store) cat > "$f" ;;
  lookup) [ -f "$f" ] && cat "$f" || exit 1 ;;
  clear) rm -f "$f" ;;
  search) g=$(ls {dir}/$h.* 2>/dev/null | head -1); [ -n "$g" ] || exit 1
          echo "[/1]"; echo "secret = $(cat $g)"; echo "attribute.user = ${{g##*.}}" ;;
esac
"#,
                dir = dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[test]
    fn a_login_is_stored_found_and_cleared() {
        let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile_dir("keyring-roundtrip");
        set_command(Some(fake(&dir)));
        assert!(available());
        store("files.example.org", "anna", "s3 cret").unwrap();
        let login = lookup("files.example.org", Some("anna")).unwrap();
        assert_eq!((login.user.as_str(), login.password.as_str()), ("anna", "s3 cret"));
        assert_eq!(lookup("files.example.org", Some("bob")), None);
        let any = lookup("files.example.org", None).unwrap();
        assert_eq!((any.user.as_str(), any.password.as_str()), ("anna", "s3 cret"));
        clear("files.example.org", "anna").unwrap();
        assert_eq!(lookup("files.example.org", Some("anna")), None);
        set_command(None);
    }

    #[test]
    fn without_secret_tool_nothing_is_found_and_saving_says_why() {
        let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        set_command(Some(PathBuf::from("/nonexistent/secret-tool")));
        assert!(!available());
        assert_eq!(lookup("h", Some("u")), None);
        let err = store("h", "u", "p").unwrap_err();
        assert!(err.to_string().contains("secret-tool"), "{err}");
        set_command(None);
    }

    #[test]
    fn search_output_is_parsed() {
        let text = "[/org/x/1]\nlabel = a\nsecret = pw\nattribute.user = zoe\nattribute.host = h\n";
        assert_eq!(parse_search(text), Some(Login { user: "zoe".into(), password: "pw".into() }));
        assert_eq!(parse_search(""), None);
    }

    fn tempfile_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nmc-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
