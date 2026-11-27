//! "Open terminal here": the command lines that open a terminal
//! window in a folder, on this computer or in an SSH session on a server.
//!
//! Everything here is a pure function of its inputs, so each platform can
//! be tested on any machine; [`launch`] starts the first command that
//! exists.

use std::path::Path;

/// Where the terminal is to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place<'a> {
    /// A folder on this computer.
    Local(&'a Path),
    /// A folder on a server reached with SSH: `ssh_args` (destination and
    /// options, without the command) and the folder there.
    Remote {
        /// The destination and options of `ssh`, without the command.
        ssh_args: Vec<String>,
        /// The folder on the server.
        dir: &'a str,
    },
}

/// The operating system, as `std::env::consts::OS` names it.
pub type Os<'a> = &'a str;

/// `'quoted'` for a POSIX shell.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The ssh command that opens a login shell in `dir` on the server.
pub fn ssh_command(ssh_args: &[String], dir: &str) -> Vec<String> {
    let mut command = vec!["ssh".to_owned(), "-t".to_owned()];
    command.extend(ssh_args.iter().cloned());
    command.push(format!("cd {} && exec \"$SHELL\" -l", quote(dir)));
    command
}

/// The terminals of Linux and BSD: the program, what goes in front of the
/// program to run for a server (`-e`, `--`), and how it is told its starting
/// folder (`{dir}` is the folder; empty: it follows the folder of the
/// process). `xdg-terminal-exec` is the freedesktop way to start the
/// terminal the desktop chose (Fedora Atomic, Bazzite...).
const LINUX_TERMINALS: &[(&str, &[&str], &[&str])] = &[
    ("xdg-terminal-exec", &[], &[]),
    ("x-terminal-emulator", &["-e"], &[]),
    // Ptyxis opens the home folder with `-d` alone when it keeps no
    // directory; with a program after `--` it opens in the folder.
    ("ptyxis", &["--"], &["--new-window", "-d", "{dir}", "--", "{shell}"]),
    ("gnome-terminal", &["--"], &["--working-directory={dir}"]),
    ("kgx", &["-e"], &["--working-directory={dir}"]),
    ("konsole", &["-e"], &["--workdir", "{dir}"]),
    ("xfce4-terminal", &["-x"], &["--working-directory={dir}"]),
    ("kitty", &[], &["--directory", "{dir}"]),
    ("alacritty", &["-e"], &["--working-directory", "{dir}"]),
    ("wezterm", &["start", "--"], &[]),
    ("foot", &[], &["--working-directory={dir}"]),
    ("xterm", &["-e"], &[]),
];

/// The command lines to try, in order, to open `place` with the setting
/// `custom` (`general.terminal`, empty for the default of the platform) and
/// `env_terminal` (`$TERMINAL`).
///
/// In `custom`, `{dir}` is the folder and `{cmd}` the program to run for a
/// server (`ssh ...`); a setting without `{dir}` starts in the folder
/// ([`current_dir`]) instead.
pub fn commands(
    custom: &str,
    env_terminal: Option<&str>,
    place: &Place<'_>,
    os: Os<'_>,
) -> Vec<Vec<String>> {
    let remote_command = match place {
        Place::Remote { ssh_args, dir } => Some(ssh_command(ssh_args, dir)),
        Place::Local(_) => None,
    };
    let dir_text = match place {
        Place::Local(dir) => dir.display().to_string(),
        Place::Remote { .. } => String::new(),
    };
    if !custom.trim().is_empty() {
        return vec![custom_command(custom.trim(), &dir_text, remote_command.as_deref())];
    }
    // A local folder is given with the option of the terminal (all of them
    // also start in the folder of the process, see [`current_dir`]); a server
    // gets the ssh command after the separator the terminal needs.
    let plain = |program: &str, before: &[&str], folder: &[&str]| -> Vec<String> {
        let mut command = vec![program.to_owned()];
        match &remote_command {
            Some(remote) => {
                command.extend(before.iter().map(|s| (*s).to_owned()));
                command.extend(remote.iter().cloned());
            }
            None => command.extend(
                folder.iter().map(|s| s.replace("{dir}", &dir_text).replace("{shell}", &shell())),
            ),
        }
        command
    };
    match os {
        "macos" => match &remote_command {
            // Terminal.app runs a command through a script; open it in a
            // new window the simple way: a .command-less `open -a` cannot.
            Some(remote) => {
                let line = remote.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ");
                vec![vec![
                    "osascript".into(),
                    "-e".into(),
                    format!(
                        "tell application \"Terminal\" to do script \"{}\"",
                        line.replace('\\', "\\\\").replace('"', "\\\"")
                    ),
                ]]
            }
            None => vec![vec!["open".into(), "-a".into(), "Terminal".into(), dir_text]],
        },
        "windows" => match &remote_command {
            Some(remote) => vec![
                [vec!["wt.exe".to_owned()], remote.clone()].concat(),
                [vec!["cmd".into(), "/C".into(), "start".into(), String::new()], remote.clone()]
                    .concat(),
            ],
            None => vec![
                vec!["wt.exe".into(), "-d".into(), dir_text.clone()],
                vec![
                    "cmd".into(),
                    "/C".into(),
                    "start".into(),
                    String::new(),
                    "cmd".into(),
                    "/K".into(),
                    format!("cd /d \"{dir_text}\""),
                ],
            ],
        },
        _ => {
            let mut list = Vec::new();
            if let Some(term) = env_terminal.map(str::trim).filter(|t| !t.is_empty()) {
                let mut words: Vec<String> = term.split_whitespace().map(str::to_owned).collect();
                if let Some(remote) = &remote_command {
                    words.push("-e".into());
                    words.extend(remote.iter().cloned());
                }
                list.push(words);
            }
            for (program, before, folder) in LINUX_TERMINALS {
                list.push(plain(program, before, folder));
            }
            list
        }
    }
}

/// The shell of the user, for terminals that need a program to start.
fn shell() -> String {
    std::env::var("SHELL").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "/bin/sh".into())
}

/// A custom terminal setting as a command line.
fn custom_command(custom: &str, dir: &str, remote: Option<&[String]>) -> Vec<String> {
    let mut command = Vec::new();
    let words = split_words(custom);
    // Warp is not given a folder as an argument (it takes it as a URL and
    // fails); it opens in the folder the process starts in.
    let is_warp = words.first().is_some_and(|w| {
        Path::new(w)
            .file_name()
            .is_some_and(|n| n.to_string_lossy().to_lowercase().contains("warp"))
    });
    for word in words {
        if is_warp && word == "{dir}" {
            continue;
        }
        if word == "{cmd}" {
            if let Some(remote) = remote {
                command.extend(remote.iter().cloned());
            }
        } else {
            command.push(word.replace("{dir}", dir).replace("{dir_url}", &percent_encode(dir)));
        }
    }
    // A server with no {cmd}: the terminal is given the command at the end.
    if let Some(remote) = remote
        && !custom.contains("{cmd}")
    {
        command.push("-e".into());
        command.extend(remote.iter().cloned());
    }
    command
}

/// The command lines that run `program` in a terminal window opened in
/// `dir` (for an editor that needs a terminal).
pub fn run_in(
    custom: &str,
    env_terminal: Option<&str>,
    dir: &Path,
    program: &[String],
    os: Os<'_>,
) -> Vec<Vec<String>> {
    let dir_text = dir.display().to_string();
    let with_program = |mut command: Vec<String>, separator: &[&str]| {
        command.extend(separator.iter().map(|s| (*s).to_owned()));
        command.extend(program.iter().cloned());
        command
    };
    if !custom.trim().is_empty() {
        let words = split_words(custom.trim());
        let mut command = Vec::new();
        let mut placed = false;
        for word in words {
            if word == "{cmd}" {
                command.extend(program.iter().cloned());
                placed = true;
            } else {
                command.push(
                    word.replace("{dir}", &dir_text)
                        .replace("{dir_url}", &percent_encode(&dir_text)),
                );
            }
        }
        return vec![if placed { command } else { with_program(command, &["-e"]) }];
    }
    match os {
        "macos" => {
            let line = program.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ");
            let script = format!("cd {} && {line}", quote(&dir_text));
            vec![vec![
                "osascript".into(),
                "-e".into(),
                format!(
                    "tell application \"Terminal\" to do script \"{}\"",
                    script.replace('\\', "\\\\").replace('"', "\\\"")
                ),
            ]]
        }
        "windows" => vec![
            with_program(vec!["wt.exe".into(), "-d".into(), dir_text.clone()], &[]),
            with_program(vec!["cmd".into(), "/C".into(), "start".into(), String::new()], &[]),
        ],
        _ => {
            let mut list = Vec::new();
            if let Some(term) = env_terminal.map(str::trim).filter(|t| !t.is_empty()) {
                list.push(with_program(split_words(term), &["-e"]));
            }
            for (name, before, folder) in LINUX_TERMINALS {
                let mut command = vec![(*name).to_owned()];
                // ptyxis runs the program after `--` in place of the shell.
                if *name == "ptyxis" {
                    command.extend(["--new-window".into(), "-d".into(), dir_text.clone()]);
                } else {
                    command.extend(
                        folder.iter().map(|s| s.replace("{dir}", &dir_text).replace("{shell}", "")),
                    );
                }
                list.push(with_program(command, before));
            }
            list
        }
    }
}

/// The words of a command line: spaces separate them, `'...'` and `"..."`
/// keep spaces inside one word, a backslash keeps the next character (a
/// program in a folder with spaces: `"/opt/My Apps/term" {dir}`). No shell
/// is involved, so nothing is expanded except a leading `~/`.
pub fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                word.push(chars.next().unwrap_or('\\'));
            }
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => word.push(c),
        }
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    if let Some(first) = words.first_mut()
        && let Some(rest) = first.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        *first = Path::new(&home).join(rest).display().to_string();
    }
    words
}

/// `text` for a URL: everything but letters, digits and `-._~/` as `%XX`.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The folder the terminal process starts in: that of a local place,
/// whatever the command line says (a setting without `{dir}` relies on it).
pub fn current_dir<'a>(place: &'a Place<'_>) -> Option<&'a Path> {
    match place {
        Place::Local(dir) => Some(dir),
        Place::Remote { .. } => None,
    }
}

/// Variables that only the bundled program (AppImage) needs; the terminal
/// and what it starts would load our Qt and libraries with them.
const BUNDLE_VARS: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "APPIMAGE_EXTRACT_AND_RUN",
    "QT_PLUGIN_PATH",
    "QT_QPA_PLATFORM_PLUGIN_PATH",
    "QML2_IMPORT_PATH",
    "QML_IMPORT_PATH",
    "QT_QPA_FONTDIR",
    "GDK_PIXBUF_MODULE_FILE",
    "GIO_EXTRA_MODULES",
    "GSETTINGS_SCHEMA_DIR",
    "PERLLIB",
    "PYTHONHOME",
    "PYTHONPATH",
];

/// Lists of folders from which the entries inside the bundle are dropped.
const BUNDLE_PATH_VARS: &[&str] = &["LD_LIBRARY_PATH", "PATH", "XDG_DATA_DIRS", "XDG_CONFIG_DIRS"];

/// Removes from `process` the environment that belongs to our own bundle
/// (`appdir` is where it is mounted), so that another program, even another
/// AppImage, starts as if it was started from the desktop.
fn clean_environment(process: &mut std::process::Command, appdir: Option<&std::ffi::OsStr>) {
    let Some(appdir) = appdir.filter(|d| !d.is_empty()) else { return };
    for var in BUNDLE_VARS {
        process.env_remove(var);
    }
    for var in BUNDLE_PATH_VARS {
        let Some(value) = std::env::var_os(var) else { continue };
        let kept: Vec<_> =
            std::env::split_paths(&value).filter(|p| !p.starts_with(appdir)).collect();
        match std::env::join_paths(kept) {
            Ok(joined) if !joined.is_empty() => process.env(var, joined),
            _ => process.env_remove(var),
        };
    }
}

/// How long a terminal that fails right away is waited for. A terminal that
/// runs longer started (or handed its window to a running instance).
const STARTUP_WAIT: std::time::Duration = std::time::Duration::from_millis(400);

/// The tail of what the program wrote on standard error.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(3);
    lines[start..].join(" | ")
}

/// Starts the first of `candidates` that can be started, detached from this
/// program (it never waits for the terminal to be closed). A terminal that
/// ends with an error right after it started (a program that is not what it
/// should be, an option it does not know) counts as failed, with the tail of
/// its error output, and the next one is tried. Returns the command that
/// started, or what went wrong with each one.
pub fn launch(candidates: &[Vec<String>], dir: Option<&Path>) -> Result<Vec<String>, String> {
    let appdir = std::env::var_os("APPDIR");
    let sandboxed = std::env::var_os("FLATPAK_ID").is_some_and(|v| !v.is_empty());
    let mut problems: Vec<String> = Vec::new();
    for command in candidates {
        let Some((program, args)) = command.split_first() else { continue };
        let mut process = if sandboxed {
            // In a Flatpak sandbox the terminals are those of the computer.
            let mut p = std::process::Command::new("flatpak-spawn");
            p.arg("--host");
            if let Some(dir) = dir {
                p.arg(format!("--directory={}", dir.display()));
            }
            p.arg(program);
            p
        } else {
            std::process::Command::new(program)
        };
        process.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null());
        clean_environment(&mut process, appdir.as_deref());
        if let Some(dir) = dir {
            process.current_dir(dir);
        }
        // Standard error goes to a file that is read if the program fails.
        let log = tempfile::tempfile().ok();
        match log.as_ref().and_then(|f| f.try_clone().ok()) {
            Some(file) => process.stderr(file),
            None => process.stderr(std::process::Stdio::null()),
        };
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(e) => {
                problems.push(format!("{program}: {e}"));
                continue;
            }
        };
        let started = std::time::Instant::now();
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if started.elapsed() >= STARTUP_WAIT => break None,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => break None,
            }
        };
        match outcome {
            Some(status) if !status.success() => {
                let mut text = String::new();
                if let Some(mut file) = log {
                    use std::io::{Read, Seek};
                    let _ = file.rewind();
                    let _ = file.read_to_string(&mut text);
                }
                let why = tail(&text);
                let code = status.code().map_or("a signal".to_owned(), |c| format!("code {c}"));
                problems.push(if why.is_empty() {
                    format!("{program} ended with {code}")
                } else {
                    format!("{program} ended with {code}: {why}")
                });
            }
            outcome => {
                if outcome.is_none() {
                    // Still running: reaped in the background so that no
                    // zombie stays.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                return Ok(command.clone());
            }
        }
    }
    let tried = if problems.is_empty() {
        "no terminal program is known".to_owned()
    } else {
        problems.join("; ")
    };
    Err(format!(
        "could not open a terminal ({tried}); set the command in Settings > Terminal \
         (for example `kitty --directory {{dir}}`)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> Place<'static> {
        Place::Local(Path::new("/home/me/My Files"))
    }

    fn remote() -> Place<'static> {
        Place::Remote {
            ssh_args: vec!["-p".into(), "2222".into(), "me@nas".into()],
            dir: "/srv/it's",
        }
    }

    #[test]
    fn linux_tries_the_known_terminals_in_the_folder() {
        let list = commands("", None, &local(), "linux");
        // The terminal the desktop chose first, with no stray `-e`.
        assert_eq!(list[0], ["xdg-terminal-exec"]);
        assert_eq!(list[1], ["x-terminal-emulator"]);
        let ptyxis = list.iter().find(|c| c[0] == "ptyxis").unwrap();
        assert_eq!(ptyxis[1..5], ["--new-window", "-d", "/home/me/My Files", "--"]);
        let kitty = list.iter().find(|c| c[0] == "kitty").unwrap();
        assert_eq!(kitty[1..], ["--directory", "/home/me/My Files"]);
        assert!(list.iter().any(|c| c[0] == "gnome-terminal"));
        assert_eq!(current_dir(&local()), Some(Path::new("/home/me/My Files")));
    }

    #[test]
    fn the_terminal_variable_comes_first() {
        let list = commands("", Some("foot --title nmc"), &local(), "linux");
        assert_eq!(list[0], ["foot", "--title", "nmc"]);
    }

    #[test]
    fn words_keep_quotes_and_the_url_form_of_the_folder() {
        assert_eq!(
            split_words(r#""/opt/My Apps/term" --dir {dir} 'a b' c\ d"#),
            ["/opt/My Apps/term", "--dir", "{dir}", "a b", "c d"]
        );
        let warp = commands(
            r#"xdg-open "warp://action/new_window?path={dir_url}""#,
            None,
            &local(),
            "linux",
        );
        assert_eq!(warp, [["xdg-open", "warp://action/new_window?path=/home/me/My%20Files"]]);
        // A path with spaces stays one word.
        let appimage =
            commands("/var/home/me/AppImages/warp.appimage {dir}", None, &local(), "linux");
        assert_eq!(appimage[0], ["/var/home/me/AppImages/warp.appimage"], "Warp follows the cwd");
    }

    #[test]
    fn macos_and_windows_have_their_own_commands() {
        assert_eq!(
            commands("", None, &local(), "macos"),
            [["open", "-a", "Terminal", "/home/me/My Files"]]
        );
        let windows = commands("", None, &local(), "windows");
        assert_eq!(windows[0], ["wt.exe", "-d", "/home/me/My Files"]);
        assert_eq!(windows[1][..5], ["cmd", "/C", "start", "", "cmd"]);
    }

    #[test]
    fn a_custom_command_gets_the_folder() {
        let list = commands("kitty --directory {dir}", None, &local(), "linux");
        assert_eq!(list, [["kitty", "--directory", "/home/me/My Files"]]);
        // Without {dir} the process starts in the folder instead.
        let list = commands("alacritty", None, &local(), "linux");
        assert_eq!(list, [["alacritty"]]);
    }

    #[test]
    fn a_server_gets_an_ssh_session_in_its_folder() {
        let ssh = ssh_command(&["me@nas".into()], "/srv/it's");
        assert_eq!(ssh[..3], ["ssh", "-t", "me@nas"]);
        assert_eq!(ssh[3], "cd '/srv/it'\\''s' && exec \"$SHELL\" -l");
        let list = commands("", None, &remote(), "linux");
        let gnome = list.iter().find(|c| c[0] == "gnome-terminal").unwrap();
        assert_eq!(gnome[1], "--");
        assert_eq!(gnome[2], "ssh");
        let custom = commands("myterm --run {cmd}", None, &remote(), "linux");
        assert_eq!(custom[0][..3], ["myterm", "--run", "ssh"]);
        assert_eq!(current_dir(&remote()), None);
    }

    #[test]
    fn a_terminal_that_fails_is_reported_and_the_next_one_is_tried() {
        let failing =
            vec!["sh".to_owned(), "-c".into(), "echo 'unknown option --nope' >&2; exit 3".into()];
        let error = launch(std::slice::from_ref(&failing), None).unwrap_err();
        assert!(error.contains("code 3") && error.contains("unknown option --nope"), "{error}");
        let started = launch(&[failing, vec!["true".into()]], None);
        assert_eq!(started, Ok(vec!["true".to_owned()]));
    }

    #[test]
    fn a_terminal_starts_in_the_folder_and_without_the_bundle_environment() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let script = format!(
            "pwd > {0}; echo \"${{QT_PLUGIN_PATH:-unset}}\" >> {0}",
            quote(&out.display().to_string())
        );
        let command = vec!["sh".to_owned(), "-c".into(), script];
        // SAFETY of the env: only read by the spawned process in this test.
        let started = launch(std::slice::from_ref(&command), Some(dir.path()));
        assert!(started.is_ok(), "{started:?}");
        let text = std::fs::read_to_string(&out).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            Path::new(lines.next().unwrap()).canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn launching_reports_a_missing_terminal() {
        let error = launch(&[vec!["/nonexistent/terminal".into()]], None).unwrap_err();
        assert!(error.contains("Settings"), "{error}");
        let started = launch(&[vec!["/nonexistent/x".into()], vec!["true".into()]], None);
        assert_eq!(started, Ok(vec!["true".to_owned()]));
    }
}
