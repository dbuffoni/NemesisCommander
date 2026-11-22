//! "Open terminal here" (SC-36): the command lines that open a terminal
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

/// What the terminals of Linux and BSD need in front of the program to run
/// (`None`: none runs a program, it is a plain terminal in a folder).
const LINUX_TERMINALS: &[(&str, &[&str])] = &[
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("xterm", &["-e"]),
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
    let plain = |program: &str, before: &[&str]| -> Vec<String> {
        let mut command = vec![program.to_owned()];
        command.extend(before.iter().map(|s| (*s).to_owned()));
        if let Some(remote) = &remote_command {
            command.extend(remote.iter().cloned());
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
            for (program, before) in LINUX_TERMINALS {
                list.push(plain(program, before));
            }
            list
        }
    }
}

/// A custom terminal setting as a command line.
fn custom_command(custom: &str, dir: &str, remote: Option<&[String]>) -> Vec<String> {
    let mut command = Vec::new();
    for word in custom.split_whitespace() {
        if word == "{cmd}" {
            if let Some(remote) = remote {
                command.extend(remote.iter().cloned());
            }
        } else {
            command.push(word.replace("{dir}", dir));
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

/// The folder the terminal process starts in: that of a local place,
/// whatever the command line says (a setting without `{dir}` relies on it).
pub fn current_dir<'a>(place: &'a Place<'_>) -> Option<&'a Path> {
    match place {
        Place::Local(dir) => Some(dir),
        Place::Remote { .. } => None,
    }
}

/// Starts the first of `candidates` that can be started, detached from this
/// program (it never waits for the terminal to be closed). Returns the
/// command that started, or what went wrong with the last one.
pub fn launch(candidates: &[Vec<String>], dir: Option<&Path>) -> Result<Vec<String>, String> {
    let mut last = "no terminal program is known".to_owned();
    for command in candidates {
        let Some((program, args)) = command.split_first() else { continue };
        let mut process = std::process::Command::new(program);
        process
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(dir) = dir {
            process.current_dir(dir);
        }
        match process.spawn() {
            Ok(mut child) => {
                // Reaped in the background so that no zombie stays.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(command.clone());
            }
            Err(e) => last = format!("{program}: {e}"),
        }
    }
    Err(format!(
        "could not open a terminal ({last}); set the command in Settings > Terminal \
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
        assert_eq!(list[0], ["x-terminal-emulator", "-e"]);
        assert!(list.iter().any(|c| c[0] == "gnome-terminal"));
        assert!(list.iter().any(|c| c[0] == "kitty"));
        assert_eq!(current_dir(&local()), Some(Path::new("/home/me/My Files")));
    }

    #[test]
    fn the_terminal_variable_comes_first() {
        let list = commands("", Some("foot --title nmc"), &local(), "linux");
        assert_eq!(list[0], ["foot", "--title", "nmc"]);
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
    fn launching_reports_a_missing_terminal() {
        let error = launch(&[vec!["/nonexistent/terminal".into()]], None).unwrap_err();
        assert!(error.contains("Settings"), "{error}");
        let started = launch(&[vec!["/nonexistent/x".into()], vec!["true".into()]], None);
        assert_eq!(started, Ok(vec!["true".to_owned()]));
    }
}
