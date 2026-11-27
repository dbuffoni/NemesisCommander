//! The "Install pnxr serve on a server" dialog: writing the server and its
//! options with help while typing, what the install does (F1), a test that
//! changes nothing (Ctrl+T) and the log of the real install.

use std::path::PathBuf;
use std::time::Instant;

use pnxr_app::{InstallOptions, Remotes};

use crate::dialogs::{InputDialog, InputPurpose, MessageDialog, Secret};

/// The text above the field.
pub fn prompt() -> &'static str {
    "Server over SSH, then options:"
}

/// The options that can follow the server.
const OPTIONS: &[(&str, &str)] = &[
    ("--root DIR", "serve only this folder (repeat for more)"),
    ("--read-only", "refuse every change"),
    ("--port N", "TCP port of pnxr serve (default 7443)"),
    ("--name NAME", "name in certificate and scs:// (default: the host)"),
    ("--renew", "issue a new certificate even if there is one"),
];

/// Reads what was typed: the server, then the options of `pnxr serve
/// install`.
pub fn parse(
    text: &str,
    tls_dir: PathBuf,
    remotes: &dyn Remotes,
) -> Result<InstallOptions, String> {
    let mut words = text.split_whitespace();
    let host = words.next().ok_or("type the server, for example me@nas.lan")?;
    if host.starts_with('-') {
        return Err("the server comes first, for example me@nas.lan, then the options".into());
    }
    if host.contains('/') || host.contains("://") {
        return Err("write the server as [user@]host[:ssh-port], not as an address".into());
    }
    remotes.ssh_target(host)?;
    let mut options = InstallOptions::new(host, tls_dir);
    while let Some(word) = words.next() {
        match word {
            "--read-only" => options.read_only = true,
            "--renew" => options.renew = true,
            "--root" => options.roots.push(words.next().ok_or("--root needs a folder")?.to_owned()),
            "--name" => options.name = Some(words.next().ok_or("--name needs a name")?.to_owned()),
            "--port" => {
                options.port = words
                    .next()
                    .and_then(|p| p.parse().ok())
                    .filter(|p| *p > 0)
                    .ok_or("--port needs a number from 1 to 65535")?;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(options)
}

/// Samples and options, for under the field while it is empty.
fn examples() -> Vec<String> {
    let mut lines = vec![
        "Examples:".to_owned(),
        "  me@nas.lan                        the simplest".to_owned(),
        "  me@nas.lan:2222                   SSH on another port".to_owned(),
        "  me@nas.lan --root /data --read-only".to_owned(),
        "Options after the server:".to_owned(),
    ];
    lines.extend(OPTIONS.iter().map(|(option, what)| format!("  {option:<14} {what}")));
    lines.push("F1 explains what is done, Ctrl+T tests without installing".to_owned());
    lines
}

/// The lines under the field while it is typed: samples while it is
/// empty, then what was understood or what is wrong, plus the options that
/// fit the word being typed.
pub fn hints(text: &str, remotes: &dyn Remotes) -> Vec<String> {
    if text.trim().is_empty() {
        return examples();
    }
    // An option still being typed is not an error: it is completed below.
    let last = text.split_whitespace().last().unwrap_or("");
    let partial = last.starts_with('-')
        && !text.ends_with(' ')
        && OPTIONS.iter().any(|(option, _)| option.starts_with(last))
        && !OPTIONS.iter().any(|(option, _)| option.split(' ').next() == Some(last));
    let understood = if partial {
        let cut = text.trim_end().len() - last.len();
        &text[..cut]
    } else {
        text
    };
    let mut lines = match parse(understood, PathBuf::new(), remotes) {
        Ok(options) => vec![format!("✓ {}", summary(&options))],
        Err(e) if e.contains("needs") => vec![format!("… {e}")],
        Err(e) => vec![format!("✗ {e}")],
    };
    // Complete the option being typed.
    if partial {
        let fits: Vec<String> = OPTIONS
            .iter()
            .filter(|(option, _)| option.starts_with(last))
            .map(|(option, what)| format!("  {option:<14} {what}"))
            .collect();
        lines.extend(fits);
    } else if text.ends_with(' ') && !text.trim().is_empty() {
        lines.push("  Options: --root DIR  --read-only  --port N  --name NAME  --renew".into());
    }
    lines
}

/// What the typed install would do, in one line.
pub fn summary(options: &InstallOptions) -> String {
    let mut parts = vec![format!("install on {}", options.authority)];
    parts.push(format!("port {}", options.port));
    if options.roots.is_empty() {
        parts.push("serves every folder".to_owned());
    } else {
        parts.push(format!("serves only {}", options.roots.join(", ")));
    }
    if options.read_only {
        parts.push("read only".to_owned());
    }
    if options.renew {
        parts.push("new certificate".to_owned());
    }
    parts.join(" · ")
}

/// What F1 shows: what the install does and how to use the result.
pub fn explanation() -> Vec<String> {
    // One line per paragraph or list item: each interface wraps it to its
    // width.
    [
        "Install pnxr serve on a server",
        "",
        "pnxr serve is a small server program that lets this computer (and other \
         servers) copy, list and checksum files quickly and securely, with \
         certificates on both sides. Everything is done from here over SSH:",
        "",
        "  1. creates this computer's certificate authority, if there is none;",
        "  2. checks the server: Linux, same processor, systemd;",
        "  3. copies pnxr to ~/.local/bin/pnxr on the server;",
        "  4. issues the server's certificate and copies it to \
         ~/.config/pnxr/serve-tls (private key readable by you only);",
        "  5. adds a systemd user service pnxr-serve and starts it (it keeps \
         running after you log out);",
        "  6. connects to scs://server:7443 to prove it works.",
        "",
        "Nothing needs root. Run it again to update; the certificates are kept. \
         Remove it with `pnxr serve uninstall me@host`.",
        "",
        "How to use it: type the server, for example me@nas.lan. Options:",
        "  --root DIR    serve only this folder",
        "  --read-only   no changes",
        "  --port N      another TCP port",
        "  --name NAME   name in the certificate (default: the host)",
        "  --renew       new certificate",
        "",
        "Ctrl+T tests first, without installing: it only reads from the server \
         and changes nothing. When the install ends, the location scs://… is \
         shown: open it from the drive list (it is saved there with F4).",
    ]
    .iter()
    .map(|l| (*l).to_owned())
    .collect()
}

fn stamp(started: Instant, text: &str) -> String {
    // What a command printed is shown under the step, without a time.
    if text.starts_with("    ") {
        return format!("{:>11}{}", "", text.trim_start());
    }
    format!("[{:>5.2} s] {text}", started.elapsed().as_secs_f64())
}

/// What the install will do, for the question before it starts.
pub fn plan(options: &InstallOptions) -> Vec<String> {
    let mut lines = vec![summary(options), String::new(), "It will, over SSH:".into()];
    lines.extend(
        [
            "  - check the server (Linux, same processor, systemd);",
            "  - copy pnxr to ~/.local/bin/pnxr;",
            "  - keep or create certificates in ~/.config/pnxr/serve-tls;",
            "  - add and start the user service pnxr-serve;",
            "  - connect to it, to prove that it works.",
        ]
        .map(str::to_owned),
    );
    if options.roots.is_empty() {
        lines.push("The server will let you read and change every file you can.".into());
    }
    lines.push(String::new());
    lines.push("Nothing needs root. Every step is shown while it runs.".into());
    lines
}

/// The log of a test that changes nothing; the last line is the verdict.
/// Every line is also given to `emit` as it happens.
pub async fn run_check(
    remotes: &dyn Remotes,
    options: InstallOptions,
    emit: impl Fn(&str) + Send + Sync,
) -> Vec<String> {
    let started = Instant::now();
    let first = stamp(started, &summary(&options));
    emit(&first);
    let mut log = vec![first];
    let result = remotes
        .check_serve(&options, &mut |step| {
            let line = stamp(started, step);
            emit(&line);
            log.push(line);
        })
        .await;
    match result {
        Ok(()) => log.push(format!(
            "Result: it can be installed ({:.2} s). Enter in the question installs it.",
            started.elapsed().as_secs_f64()
        )),
        Err(e) => {
            log.push(stamp(started, "Failed"));
            log.extend(e.lines().filter(|l| !l.trim().is_empty()).map(|l| format!("    {l}")));
            log.push("Result: it cannot be installed like this".to_owned());
        }
    }
    log
}

/// Installs, and returns the log (each step with its time, the commands
/// run on the server included) and the location when it worked. Every line
/// is also given to `emit` as it happens.
pub async fn run_install(
    remotes: &dyn Remotes,
    options: InstallOptions,
    emit: impl Fn(&str) + Send + Sync,
) -> (Vec<String>, Result<String, String>) {
    let started = Instant::now();
    let log = std::sync::Mutex::new(Vec::new());
    let say = |text: &str| {
        let line = stamp(started, text);
        emit(&line);
        log.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    };
    say(&summary(&options));
    let result = remotes.install_serve(&options, &mut |step| say(step)).await;
    match &result {
        Ok(location) => {
            say(&format!("pnxr serve is running at {location}"));
            let line = format!("Result: installed ({:.2} s)", started.elapsed().as_secs_f64());
            emit(&line);
            log.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
        Err(e) => {
            say("Failed");
            for line in e.lines().filter(|l| !l.trim().is_empty()) {
                say(&format!("    {line}"));
            }
            let line = "Result: not installed".to_owned();
            emit(&line);
            log.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
    }
    (log.into_inner().unwrap_or_else(|e| e.into_inner()), result)
}

/// The window that ends an install: says plainly whether it worked and, if
/// so, Enter saves the connection.
pub fn finished(
    lines: Vec<String>,
    result: Result<String, String>,
    install_text: &str,
    linger_off: &str,
) -> MessageDialog {
    match result {
        Ok(location) => {
            let mut all = vec![
                format!("✓ pnxr serve is installed and answering at {location}"),
                String::new(),
            ];
            all.extend(lines);
            let mut window = MessageDialog::new("Install pnxr serve: done", all);
            let name = location
                .trim_start_matches("scs://")
                .split(['/', ':'])
                .next()
                .unwrap_or("")
                .to_owned();
            window.then = Some(InputDialog::new(
                "Save connection",
                &format!("Name for {location} (it appears in the drive list):"),
                &name,
                InputPurpose::NameConnection { uri: location, secret: Secret::default() },
            ));
            window
        }
        Err(error) => {
            let mut all = vec![
                "✗ The install did not finish. Read the steps below.".to_owned(),
                String::new(),
            ];
            all.extend(lines);
            let mut window = MessageDialog::new("Install pnxr serve: failed", all);
            // Lingering off: the service stops with the last login. Offer to
            // turn it on with the sudo password of the server (asked, used
            // once, never kept) and to run the install again.
            if error.contains(linger_off) {
                let who = install_text.split_whitespace().next().unwrap_or("the server");
                let mut ask = InputDialog::new(
                    "Turn lingering on",
                    &format!(
                        "sudo password on {who} (used once, not saved). Lingering keeps the \
                         service running after you log out:"
                    ),
                    "",
                    InputPurpose::InstallLinger(install_text.to_owned()),
                );
                ask.secret = true;
                window.then = Some(ask);
            }
            window
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnxr_app::FakeRemotes;

    /// A dialog 80 columns wide holds 68: table rows are not wrapped between
    /// words, so they must fit.
    #[test]
    fn option_tables_fit_80_columns() {
        for line in explanation().iter().chain(examples().iter()) {
            if line.trim_start().contains("  ") {
                assert!(line.chars().count() <= 68, "{line}");
            }
        }
    }

    #[test]
    fn the_server_and_options_are_read() {
        let options = parse("me@nas.lan:2222 --root /data --root /srv --read-only --port 9000 --name nas", "/tls".into(), &FakeRemotes::new())
        .unwrap();
        assert_eq!(options.authority, "me@nas.lan:2222");
        assert_eq!(options.roots, ["/data", "/srv"]);
        assert!(options.read_only && !options.renew);
        assert_eq!((options.port, options.name.as_deref()), (9000, Some("nas")));
        assert!(parse("", "/tls".into(), &FakeRemotes::new()).is_err());
        assert!(parse("--read-only", "/tls".into(), &FakeRemotes::new()).is_err());
        assert!(parse("sftp://me@nas/x", "/tls".into(), &FakeRemotes::new()).is_err());
        assert!(parse("h --bogus", "/tls".into(), &FakeRemotes::new()).is_err());
        assert!(parse("h --port x", "/tls".into(), &FakeRemotes::new()).is_err());
        assert!(parse("h --port 0", "/tls".into(), &FakeRemotes::new()).is_err());
    }

    #[test]
    fn hints_show_samples_then_what_was_understood() {
        assert!(hints("", &FakeRemotes::new())[0].starts_with("Examples"));
        assert!(hints("me@nas.lan", &FakeRemotes::new())[0].starts_with("✓ install on me@nas.lan"));
        assert!(hints("me@nas.lan --bogus", &FakeRemotes::new())[0].starts_with('✗'));
        assert!(hints("me@nas.lan --root", &FakeRemotes::new())[0].starts_with('…'), "not typed yet is not an error");
        let completing = hints("me@nas.lan --re", &FakeRemotes::new());
        assert!(completing.iter().any(|l| l.contains("--read-only")), "{completing:?}");
        assert!(completing.iter().any(|l| l.contains("--renew")), "{completing:?}");
        assert!(hints("me@nas.lan ", &FakeRemotes::new()).iter().any(|l| l.contains("--read-only")));
    }

    #[test]
    fn the_explanation_names_what_is_done() {
        let text = explanation().join("\n");
        for word in ["systemd", "~/.local/bin/pnxr", "Ctrl+T", "--read-only"] {
            assert!(text.contains(word), "{word}");
        }
    }
}
