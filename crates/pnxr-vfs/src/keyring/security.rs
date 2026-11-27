//! The macOS Keychain, through the `security` command. A login is a generic
//! password with service `pairnox`, account `user@host` and the host
//! as its comment (to find "any login of this host").
//!
//! The password must not appear in the arguments of a process (`ps` shows
//! them), and `security add-generic-password -w` without a value asks on the
//! terminal. So the command runs in `security -i`, which reads its commands
//! from the standard input.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::PathBuf;

use super::{SERVICE, find_command, run_command};
use crate::netrc::Login;

pub(super) const STORE_NAME: &str = "the macOS Keychain";
pub(super) const MISSING: &str = "needs the security command: not found";

/// The `security` to run (macOS ships it as /usr/bin/security).
fn command() -> Option<PathBuf> {
    find_command("security", Some("/usr/bin/security"))
}

pub(super) fn available() -> bool {
    command().is_some()
}

fn run(args: &[&str], input: Option<&str>) -> std::io::Result<std::process::Output> {
    let tool = command().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "the security command was not found")
    })?;
    run_command(tool, args, input)
}

/// One argument of a `security -i` line: double-quoted, `\` and `"` escaped.
fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        if c == '\\' || c == '"' {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

fn account(host: &str, user: &str) -> String {
    format!("{user}@{host}")
}

pub(super) fn store(host: &str, user: &str, password: &str) -> std::io::Result<()> {
    if password.contains(['\n', '\r']) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the Keychain cannot keep a password with a line break",
        ));
    }
    let account = account(host, user);
    // -U updates the item when there is one.
    let line = format!(
        "add-generic-password -U -s {} -a {} -l {} -j {} -w {}\n",
        quote(SERVICE),
        quote(&account),
        quote(&format!("Pairnox: {account}")),
        quote(host),
        quote(password),
    );
    let out = run(&["-i"], Some(&line))?;
    // `security -i` reports a failed command on its output, not always in its
    // exit status.
    let text =
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() && !text.contains("security: ") {
        return Ok(());
    }
    let why = text.trim();
    Err(std::io::Error::other(if why.is_empty() {
        "the Keychain refused it (is it locked?)".to_owned()
    } else {
        why.to_owned()
    }))
}

pub(super) fn clear(host: &str, user: &str) -> std::io::Result<()> {
    let account = account(host, user);
    run(&["delete-generic-password", "-s", SERVICE, "-a", &account], None).map(|_| ())
}

pub(super) fn lookup(host: &str, user: Option<&str>) -> Option<Login> {
    command()?;
    let user = match user {
        Some(user) => user.to_owned(),
        None => first_user(host)?,
    };
    let account = account(host, &user);
    let out = run(&["find-generic-password", "-s", SERVICE, "-a", &account, "-w"], None).ok()?;
    if !out.status.success() {
        return None;
    }
    let mut password = String::from_utf8(out.stdout).ok()?;
    if password.ends_with('\n') {
        password.pop();
    }
    // `-w` prints a password with non-ASCII or control characters as hex,
    // which cannot be told from a password that is hex: ask `-g` which it is.
    if looks_like_hex(&password)
        && let Some(real) = decoded_by_g(&account)
    {
        password = real;
    }
    (!password.is_empty()).then_some(Login { user, password })
}

fn looks_like_hex(text: &str) -> bool {
    !text.is_empty() && text.len().is_multiple_of(2) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The password as `find-generic-password -g` shows it on its error output:
/// `password: "text"`, or `password: 0xHEX  "text"` when it is not plain.
/// `None` when it is plain (the `-w` text stands) or cannot be read.
fn decoded_by_g(account: &str) -> Option<String> {
    let out = run(&["find-generic-password", "-s", SERVICE, "-a", account, "-g"], None).ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text.lines().find_map(|l| l.strip_prefix("password: "))?;
    let hex = line.strip_prefix("0x")?;
    let digits: String = hex.chars().take_while(char::is_ascii_hexdigit).collect();
    let bytes = (0..digits.len() / 2)
        .map(|i| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

/// The user of the first login saved for `host`.
fn first_user(host: &str) -> Option<String> {
    let out = run(&["find-generic-password", "-s", SERVICE, "-j", host], None).ok()?;
    if !out.status.success() {
        return None;
    }
    parse_account(&String::from_utf8_lossy(&out.stdout), host)
}

/// The user in the `"acct"<blob>="user@host"` line of `find-generic-password`.
fn parse_account(text: &str, host: &str) -> Option<String> {
    let line = text.lines().find(|l| l.trim_start().starts_with("\"acct\"<blob>="))?;
    let value = line.split_once('=')?.1.trim().strip_prefix('"')?.strip_suffix('"')?;
    value.strip_suffix(&format!("@{host}")).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a \"b\" \\c"), "\"a \\\"b\\\" \\\\c\"");
    }

    #[test]
    fn the_account_is_read_from_the_attributes() {
        let text = "keychain: \"/Users/a/Library/Keychains/login.keychain-db\"\n\
                    class: \"genp\"\nattributes:\n    \"acct\"<blob>=\"zoe@files.example.org\"\n";
        assert_eq!(parse_account(text, "files.example.org").as_deref(), Some("zoe"));
        assert_eq!(parse_account(text, "other.org"), None);
        assert_eq!(parse_account("", "h"), None);
    }

    /// A fake `security` that keeps passwords in files of `dir` and reads
    /// `-i` commands like the real one: one line of quoted arguments.
    #[cfg(unix)]
    fn fake(dir: &std::path::Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("security");
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
d={dir}
if [ "$1" = -i ]; then
  read -r line
  eval "set -- $line"
fi
cmd=$1; shift
while [ $# -gt 0 ]; do
  case $1 in
    -a) a=$2; shift 2;;
    -j) j=$2; shift 2;;
    -w) if [ -n "$2" ] && [ "${{2#-}}" = "$2" ] && [ "$cmd" = add-generic-password ]; then w=$2; shift 2; else shift; fi;;
    -s|-l) shift 2;;
    -U) shift;;
    *) shift;;
  esac
done
f="$d/$a"
case $cmd in
  add-generic-password) printf '%s' "$w" > "$f"; echo "$j" > "$f.host" ;;
  find-generic-password)
    if [ -n "$a" ]; then [ -f "$f" ] && cat "$f" && echo || exit 44
    else g=$(grep -l "^$j$" "$d"/*.host 2>/dev/null | head -1); [ -n "$g" ] || exit 44
         echo "attributes:"; echo "    \"acct\"<blob>=\"$(basename "${{g%.host}}")\"" ; fi ;;
  delete-generic-password) rm -f "$f" "$f.host" ;;
esac
"#,
                dir = dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[cfg(unix)]
    #[test]
    fn a_login_is_stored_found_and_cleared_without_the_password_in_arguments() {
        use crate::keyring::{ONE_AT_A_TIME, set_command};
        let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("pnxr-security-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        set_command(Some(fake(&dir)));
        assert!(available());
        store("files.example.org", "anna", "s3 \"cret\\ x").unwrap();
        // The fake kept exactly what the quoted line said.
        let login = lookup("files.example.org", Some("anna")).unwrap();
        assert_eq!((login.user.as_str(), login.password.as_str()), ("anna", "s3 \"cret\\ x"));
        assert_eq!(lookup("files.example.org", Some("bob")), None);
        let any = lookup("files.example.org", None).unwrap();
        assert_eq!(any.user, "anna");
        clear("files.example.org", "anna").unwrap();
        assert_eq!(lookup("files.example.org", Some("anna")), None);
        set_command(None);
    }

    /// The real Keychain (the macOS runner of the nightly CI, which makes a
    /// temporary default keychain first): a throwaway host name, removed at
    /// the end.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_real_keychain_stores_finds_and_clears_a_login() {
        // The tests with a fake `security` set a process-wide command: wait
        // for them, and use the real one.
        let _turn = crate::keyring::ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        crate::keyring::set_command(None);
        let host = format!("pnxr-test-{}.invalid", std::process::id());
        store(&host, "anna", "s3 \"cret\\ é").unwrap();
        let login = lookup(&host, Some("anna")).unwrap();
        assert_eq!((login.user.as_str(), login.password.as_str()), ("anna", "s3 \"cret\\ é"));
        assert_eq!(lookup(&host, Some("bob")), None);
        assert_eq!(lookup(&host, None).unwrap().user, "anna");
        store(&host, "anna", "second").unwrap();
        assert_eq!(lookup(&host, Some("anna")).unwrap().password, "second");
        clear(&host, "anna").unwrap();
        assert_eq!(lookup(&host, Some("anna")), None);
    }
}
