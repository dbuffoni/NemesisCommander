//! Writing, checking and testing the location of a connection (the drive
//! list's "New connection…" and "Edit connection").
//!
//! [`check`] says what is wrong with an address and how to write it,
//! while it is being typed; [`run_test`] opens the connection and reports
//! each step, so a failure says where it stopped.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use nmc_vfs::{ARCHIVE_SCHEMES, ProviderRegistry, VfsPath};
use regex::Regex;

/// How long each step of a test may take.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);
/// The test reads at most this many entries of the folder.
const LIST_LIMIT: usize = 1000;

/// One kind of remote location.
struct Kind {
    scheme: &'static str,
    /// What it is, in words.
    name: &'static str,
    /// An address with user and port, as it is written.
    sample: &'static str,
    /// Where the password comes from (it is never part of the address).
    password: &'static str,
}

const KINDS: &[Kind] = &[
    Kind {
        scheme: "sftp",
        name: "SFTP over SSH",
        sample: "sftp://user@host:22/path",
        password: "SSH keys, ssh-agent or ~/.ssh/config",
    },
    Kind {
        scheme: "agent",
        name: "agent over SSH",
        sample: "agent://user@host:22/path",
        password: "SSH keys, ssh-agent or ~/.ssh/config",
    },
    Kind {
        scheme: "scs",
        name: "nmc serve, TLS",
        sample: "scs://host:7443/path",
        password: "the certificate (nmc serve)",
    },
    Kind {
        scheme: "davs",
        name: "WebDAV over HTTPS",
        sample: "davs://user@host:443/path",
        password: "~/.netrc: machine host login user password …",
    },
    Kind {
        scheme: "dav",
        name: "WebDAV",
        sample: "dav://user@host:80/path",
        password: "~/.netrc: machine host login user password …",
    },
    Kind {
        scheme: "smb",
        name: "SMB",
        sample: "smb://user@host:445/share/path",
        password: "~/.netrc: machine host login user password …",
    },
    Kind {
        scheme: "s3",
        name: "S3",
        sample: "s3://profile@bucket/path",
        password: "~/.aws/credentials or the AWS_* variables",
    },
];

fn kind_of(scheme: &str) -> Option<&'static Kind> {
    let scheme = match scheme {
        "webdav" => "dav",
        "webdavs" => "davs",
        other => other,
    };
    KINDS.iter().find(|k| k.scheme == scheme)
}

/// The text above the address field: samples with user and port, and where
/// passwords come from.
pub fn prompt() -> String {
    let samples: Vec<&str> = KINDS.iter().map(|k| k.sample).collect();
    format!(
        "Location, e.g. {}. Passwords are not written in the address (SSH keys, ~/.netrc, ~/.aws). Ctrl+T tests it.",
        samples.join("   ")
    )
}

/// An address that passed [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The location.
    pub path: VfsPath,
    /// What it is, for the line under the field.
    pub summary: String,
    /// Where its password comes from, if it needs one.
    pub password: Option<&'static str>,
}

fn address() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<scheme>[A-Za-z][A-Za-z0-9+.-]*)://(?P<authority>[^/]*)(?P<path>/.*)?$")
            .expect("valid pattern")
    })
}

fn host() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?P<host>[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?|\[[0-9A-Fa-f:.]+\])(?::(?P<port>[0-9]+))?$",
        )
        .expect("valid pattern")
    })
}

fn bucket() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$").expect("valid pattern"))
}

/// Checks the address of a connection. Local paths, `file://` and archive
/// locations are accepted as they are; for a server, `Err` says what is
/// wrong and how to write it.
pub fn check(text: &str) -> Result<Checked, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("Type a location, for example {}", KINDS[0].sample));
    }
    let Some(parts) = address().captures(text) else {
        // Not scheme://…: a local path, or a mistake.
        return match text.split_once(':') {
            Some((scheme, rest)) if kind_of(scheme).is_some() && !rest.starts_with("//") => {
                Err(format!("After {scheme}: write //, e.g. {}", kind_of(scheme).unwrap().sample))
            }
            _ => VfsPath::parse(text)
                .map(|path| Checked {
                    path,
                    summary: "A folder of this computer".into(),
                    password: None,
                })
                .map_err(|e| e.to_string()),
        };
    };
    let scheme = parts["scheme"].to_ascii_lowercase();
    if scheme == "file" || ARCHIVE_SCHEMES.contains(&scheme.as_str()) {
        return VfsPath::parse(text)
            .map(|path| Checked { path, summary: "A local location".into(), password: None })
            .map_err(|e| e.to_string());
    }
    let Some(kind) = kind_of(&scheme) else {
        let known: Vec<String> = KINDS.iter().map(|k| format!("{}://", k.scheme)).collect();
        return Err(format!("{scheme}:// is not supported. Use {}", known.join(", ")));
    };
    let authority = &parts["authority"];
    if authority.contains(char::is_whitespace) {
        return Err("The address has a space before the first /".into());
    }
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((user, hostport)) => (Some(user), hostport),
        None => (None, authority),
    };
    if let Some(user) = user {
        if user.contains(':') && kind.scheme != "s3" {
            return Err(format!(
                "No password in the address: it would be saved as plain text. Use {}",
                kind.password
            ));
        }
        if user.is_empty() {
            return Err(format!("A user name is missing before @, e.g. {}", kind.sample));
        }
    }
    let path = parts.name("path").map_or("", |m| m.as_str());
    let mut details = vec![kind.name.to_owned()];

    if kind.scheme == "s3" {
        if !bucket().is_match(hostport) {
            return Err(format!(
                "A bucket name is expected (3-63 lowercase letters, digits, . or -), e.g. {}",
                kind.sample
            ));
        }
        details.push(format!("bucket {hostport}"));
        if let Some(profile) = user {
            details.push(format!("profile {profile}"));
        }
    } else {
        let Some(h) = host().captures(hostport) else {
            return Err(if hostport.is_empty() {
                format!("The host is missing, e.g. {}", kind.sample)
            } else {
                format!("{hostport:?} is not a host name or address (host or host:port)")
            });
        };
        details.push(format!("host {}", &h["host"]));
        if let Some(port) = h.name("port") {
            match port.as_str().parse::<u16>() {
                Ok(n) if n > 0 => details.push(format!("port {n}")),
                _ => return Err(format!("Port {} is not between 1 and 65535", port.as_str())),
            }
        }
        if let Some(user) = user {
            details.push(format!("user {user}"));
        }
        if kind.scheme == "smb" && path.trim_matches('/').is_empty() {
            return Err(format!("An SMB location needs the share: {}", kind.sample));
        }
    }
    let path = VfsPath::parse(text).map_err(|e| e.to_string())?;
    Ok(Checked { path, summary: details.join(" · "), password: Some(kind.password) })
}

/// The lines under the address field while it is typed.
pub fn hints(text: &str) -> Vec<String> {
    if text.trim().is_empty() || text.trim().ends_with("://") {
        return Vec::new();
    }
    match check(text) {
        Ok(ok) => {
            let mut lines = vec![format!("✓ {}", ok.summary)];
            if let Some(password) = ok.password {
                lines.push(format!("  Password: {password}"));
            }
            lines
        }
        Err(e) => vec![format!("✗ {e}")],
    }
}

/// Opens the connection and reads its folder, telling each step. The last
/// line is the verdict.
pub async fn run_test(registry: Arc<ProviderRegistry>, location: String) -> Vec<String> {
    let started = Instant::now();
    let mut log = Vec::new();
    let say = |log: &mut Vec<String>, text: String| {
        log.push(format!("[{:>5.2} s] {text}", started.elapsed().as_secs_f64()));
    };
    let checked = match check(&location) {
        Ok(checked) => checked,
        Err(e) => {
            say(&mut log, format!("The address is wrong: {e}"));
            log.push("Result: not tested".into());
            return log;
        }
    };
    let path = checked.path;
    say(&mut log, format!("Address: {path}"));
    say(&mut log, format!("It is {}", checked.summary));

    // A cached connection would hide a broken one.
    registry.forget(&path);
    say(&mut log, format!("Opening the {}:// connection…", path.scheme()));
    let provider = match tokio::time::timeout(STEP_TIMEOUT, registry.provider_for(&path)).await {
        Ok(Ok(provider)) => provider,
        Ok(Err(e)) => return failed(log, started, "cannot connect", &e.to_string()),
        Err(_) => return failed(log, started, "cannot connect", "no answer in 30 seconds"),
    };
    say(&mut log, "Connected".into());

    say(&mut log, format!("Reading {}…", path.path()));
    let read = async {
        let mut stream = provider.list(&path).await?;
        let (mut files, mut folders) = (0usize, 0usize);
        while let Some(page) = stream.next().await {
            for entry in page? {
                if entry.kind == nmc_vfs::EntryKind::Directory {
                    folders += 1;
                } else {
                    files += 1;
                }
            }
            if files + folders >= LIST_LIMIT {
                break;
            }
        }
        Ok::<_, nmc_vfs::VfsError>((files, folders))
    };
    match tokio::time::timeout(STEP_TIMEOUT, read).await {
        Ok(Ok((files, folders))) => {
            let more = if files + folders >= LIST_LIMIT { " or more" } else { "" };
            say(&mut log, format!("The folder holds {folders} folders and {files} files{more}"));
        }
        Ok(Err(e)) => return failed(log, started, "cannot read the folder", &e.to_string()),
        Err(_) => return failed(log, started, "cannot read the folder", "no answer in 30 seconds"),
    }
    log.push(format!("Result: it works ({:.2} s)", started.elapsed().as_secs_f64()));
    log
}

fn failed(mut log: Vec<String>, started: Instant, step: &str, error: &str) -> Vec<String> {
    log.push(format!("[{:>5.2} s] Failed: {step}", started.elapsed().as_secs_f64()));
    for line in error.lines().filter(|l| !l.trim().is_empty()) {
        log.push(format!("    {line}"));
    }
    let lower = error.to_lowercase();
    let hint = if lower.contains("permission denied") || lower.contains("authentication") {
        Some("Check the user name and the key, agent or ~/.netrc entry for this host")
    } else if lower.contains("refused") {
        Some("The host answers but nothing listens on that port: check the port and the service")
    } else if lower.contains("resolve")
        || lower.contains("name or service")
        || lower.contains("lookup")
    {
        Some("The host name is not known: check the spelling and the network")
    } else if lower.contains("timed out") || lower.contains("no answer") {
        Some("The host does not answer: check the address, the port and a firewall")
    } else {
        None
    };
    if let Some(hint) = hint {
        log.push(format!("Hint: {hint}"));
    }
    log.push("Result: failed".into());
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_addresses_are_described() {
        let ok = check("sftp://me@nas:2222/srv").unwrap();
        assert_eq!(ok.summary, "SFTP over SSH · host nas · port 2222 · user me");
        assert_eq!(ok.path.to_string(), "sftp://me@nas:2222/srv");
        assert!(check("davs://cloud.example.org/remote.php/dav").is_ok());
        assert!(check("smb://nas/photos").is_ok());
        assert!(check("s3://profile@my-bucket/2026").is_ok());
        assert!(check("scs://[::1]:7443/").is_ok());
        assert!(check("agent://build01").is_ok());
        assert!(check("/tmp").is_ok(), "local paths stay valid");
        assert!(check("file:///tmp").is_ok());
    }

    #[test]
    fn mistakes_are_explained() {
        let bad = |text: &str| check(text).unwrap_err();
        assert!(bad("").contains("Type a location"));
        assert!(bad("ftp://host/x").contains("not supported"));
        assert!(bad("sftp:host/x").contains("write //"));
        assert!(bad("sftp:///x").contains("host is missing"));
        assert!(bad("sftp://me@/x").contains("host"));
        assert!(bad("sftp://@host/x").contains("user name is missing"));
        assert!(bad("sftp://host:99999/x").contains("1 and 65535"));
        assert!(bad("sftp://host:0/x").contains("1 and 65535"));
        assert!(bad("sftp://ho st/x").contains("space"));
        assert!(bad("sftp://me:secret@host/x").contains("No password"));
        assert!(bad("davs://me:secret@host/x").contains(".netrc"));
        assert!(bad("smb://host").contains("share"));
        assert!(bad("s3://Bad_Bucket/x").contains("bucket"));
        assert!(bad("sftp://bad_host!/x").contains("not a host"));
    }

    #[test]
    fn hints_follow_the_typing() {
        assert!(hints("").is_empty());
        assert!(hints("sftp://").is_empty());
        assert_eq!(hints("sftp://me@nas")[0], "✓ SFTP over SSH · host nas · user me");
        assert!(hints("sftp://me@nas")[1].contains("Password"));
        assert!(hints("ftp://x")[0].starts_with("✗ "));
    }

    #[tokio::test]
    async fn a_test_tells_each_step_and_the_verdict() {
        let registry = Arc::new(ProviderRegistry::new());
        let log = run_test(registry.clone(), "sftp://".into()).await;
        assert!(log.last().unwrap().contains("not tested"), "{log:?}");
        let log = run_test(registry, "sftp://me@nas/srv".into()).await;
        let text = log.join("\n");
        assert!(text.contains("Opening the sftp:// connection"), "{text}");
        assert!(text.contains("Failed: cannot connect"), "{text}");
        assert!(text.contains("no provider"), "{text}");
        assert_eq!(log.last().unwrap(), "Result: failed");
    }
}
