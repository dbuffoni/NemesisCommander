//! Text editors for the Editor settings: a list of open source editors with
//! the command line that makes each one work (waiting for the file to be
//! closed where that matters), which of them are installed, and how an editor
//! setting becomes the command that is run.
//!
//! Everything but [`installed`] is a pure function of its inputs.

use std::path::{Path, PathBuf};

/// Whether an editor draws its own window or runs inside a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Opens a window of its own.
    Window,
    /// Runs in a terminal and takes it over.
    Terminal,
}

/// An editor the settings suggest.
#[derive(Debug, Clone, Copy)]
pub struct Editor {
    /// The name shown.
    pub name: &'static str,
    /// The program looked for (a name on `PATH`).
    pub program: &'static str,
    /// The command line to store: the program and the options that make it
    /// wait until the file is closed. `{file}` and `{line}` may be used;
    /// without `{file}` the file is added at the end.
    pub command: &'static str,
    /// The licence, as listed in the third-party notices.
    pub licence: &'static str,
    /// Window or terminal.
    pub kind: Kind,
    /// How to install it, shown for editors that are not found.
    pub install: &'static str,
}

/// Open source editors only (proprietary ones are left out).
pub const KNOWN: &[Editor] = &[
    Editor {
        name: "VSCodium",
        program: "codium",
        command: "codium --wait",
        licence: "MIT",
        kind: Kind::Window,
        install: "flatpak install com.vscodium.codium, or brew install --cask vscodium, or winget install VSCodium.VSCodium",
    },
    Editor {
        name: "Kate",
        program: "kate",
        command: "kate --block",
        licence: "LGPL/GPL",
        kind: Kind::Window,
        install: "apt install kate, or flatpak install org.kde.kate",
    },
    Editor {
        name: "GNOME Text Editor",
        program: "gnome-text-editor",
        command: "gnome-text-editor --standalone",
        licence: "GPL-3.0",
        kind: Kind::Window,
        install: "apt install gnome-text-editor, or flatpak install org.gnome.TextEditor",
    },
    Editor {
        name: "gedit",
        program: "gedit",
        command: "gedit --wait",
        licence: "GPL-2.0",
        kind: Kind::Window,
        install: "apt install gedit, or flatpak install org.gnome.gedit",
    },
    Editor {
        name: "Mousepad",
        program: "mousepad",
        command: "mousepad --disable-server",
        licence: "GPL-2.0",
        kind: Kind::Window,
        install: "apt install mousepad",
    },
    Editor {
        name: "Geany",
        program: "geany",
        command: "geany --no-ctags",
        licence: "GPL-2.0",
        kind: Kind::Window,
        install: "apt install geany, or brew install --cask geany",
    },
    Editor {
        name: "Zed",
        program: "zed",
        command: "zed --wait",
        licence: "GPL-3.0",
        kind: Kind::Window,
        install: "brew install --cask zed, or https://zed.dev",
    },
    Editor {
        name: "Pulsar",
        program: "pulsar",
        command: "pulsar --wait",
        licence: "MIT",
        kind: Kind::Window,
        install: "flatpak install dev.pulsar_edit.Pulsar",
    },
    Editor {
        name: "Lite XL",
        program: "lite-xl",
        command: "lite-xl",
        licence: "MIT",
        kind: Kind::Window,
        install: "flatpak install com.lite_xl.LiteXL, or brew install --cask lite-xl",
    },
    Editor {
        name: "Notepad++",
        program: "notepad++",
        command: "notepad++ -multiInst -nosession",
        licence: "GPL-3.0",
        kind: Kind::Window,
        install: "winget install Notepad++.Notepad++",
    },
    Editor {
        name: "Emacs",
        program: "emacs",
        command: "emacs",
        licence: "GPL-3.0",
        kind: Kind::Window,
        install: "apt install emacs, or brew install --cask emacs",
    },
    Editor {
        name: "Neovim",
        program: "nvim",
        command: "nvim",
        licence: "Apache-2.0 / Vim",
        kind: Kind::Terminal,
        install: "apt install neovim, or brew install neovim, or winget install Neovim.Neovim",
    },
    Editor {
        name: "Vim",
        program: "vim",
        command: "vim",
        licence: "Vim",
        kind: Kind::Terminal,
        install: "apt install vim, or brew install vim",
    },
    Editor {
        name: "Helix",
        program: "hx",
        command: "hx",
        licence: "MPL-2.0",
        kind: Kind::Terminal,
        install: "apt install helix, or brew install helix",
    },
    Editor {
        name: "Micro",
        program: "micro",
        command: "micro",
        licence: "MIT",
        kind: Kind::Terminal,
        install: "apt install micro, or brew install micro",
    },
    Editor {
        name: "GNU nano",
        program: "nano",
        command: "nano",
        licence: "GPL-3.0",
        kind: Kind::Terminal,
        install: "apt install nano, or brew install nano",
    },
    Editor {
        name: "Emacs",
        program: "emacs",
        command: "emacs -nw",
        licence: "GPL-3.0",
        kind: Kind::Terminal,
        install: "apt install emacs-nox, or brew install emacs",
    },
    Editor {
        name: "Kakoune",
        program: "kak",
        command: "kak",
        licence: "Unlicense",
        kind: Kind::Terminal,
        install: "apt install kakoune, or brew install kakoune",
    },
];

/// The program of an editor command, without its folder (`/usr/bin/nvim -u x`
/// is `nvim`).
fn program_name(command: &str) -> String {
    let first = crate::terminal::split_words(command).into_iter().next().unwrap_or_default();
    Path::new(&first).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// What kind of editor `command` runs, when it is a known one. Emacs is a
/// window unless it is told `-nw` or `-t`.
pub fn kind_of(command: &str) -> Option<Kind> {
    let program = program_name(command);
    if program == "emacs" {
        let words = crate::terminal::split_words(command);
        let terminal =
            words.iter().any(|w| matches!(w.as_str(), "-nw" | "--no-window-system" | "-t"));
        return Some(if terminal { Kind::Terminal } else { Kind::Window });
    }
    KNOWN
        .iter()
        .find(|e| e.program == program || program_name(e.command) == program)
        .map(|e| e.kind)
        .or(match program.as_str() {
            "vi" | "ed" | "ne" | "joe" | "jed" | "mcedit" | "pico" | "e3" => Some(Kind::Terminal),
            "code" | "subl" | "gvim" | "kwrite" | "xed" | "pluma" | "leafpad" | "notepad"
            | "xdg-open" | "open" => Some(Kind::Window),
            _ => None,
        })
}

/// Whether `command` suits a front end: a window front end wants window
/// editors, a terminal wants terminal ones. A command that is not known suits
/// both.
pub fn suits(command: &str, window: bool) -> bool {
    match kind_of(command) {
        Some(Kind::Window) => window || command_is_detached_ok(command),
        Some(Kind::Terminal) => !window,
        None => true,
    }
}

/// A window editor can also be started from a terminal front end (detached).
fn command_is_detached_ok(_command: &str) -> bool {
    true
}

/// `command` as a command line for `file`: `{file}` and `{line}` are
/// replaced inside the words (a file with spaces stays one word); without
/// `{file}` the file is added at the end.
pub fn command_line(command: &str, file: &Path, line: usize) -> Vec<String> {
    let name = file.display().to_string();
    let mut words = crate::terminal::split_words(command);
    let has_file = words.iter().any(|w| w.contains("{file}"));
    for word in &mut words {
        *word = word.replace("{file}", &name).replace("{line}", &line.to_string());
    }
    if !has_file {
        words.push(name);
    }
    words
}

/// The first of the `PATH` folders (and the usual places of programs on
/// systems that keep them elsewhere) that holds `program`.
pub fn find_program(program: &str, path: &std::ffi::OsStr, extra: &[PathBuf]) -> Option<PathBuf> {
    let folders = std::env::split_paths(path).chain(extra.iter().cloned());
    for folder in folders {
        let base = folder.join(program);
        let mut candidates = vec![base.clone()];
        if cfg!(windows) {
            candidates.extend(["exe", "cmd", "bat"].map(|ext| base.with_extension(ext)));
        }
        for candidate in candidates {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The usual places of programs that are not on `PATH` of a desktop session.
fn extra_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = [
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/snap/bin",
        "/var/lib/flatpak/exports/bin",
        "/usr/bin",
        "/bin",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        folders.push(home.join(".local/bin"));
        folders.push(home.join(".local/share/flatpak/exports/bin"));
        folders.push(home.join(".cargo/bin"));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(var) {
            let root = PathBuf::from(root);
            folders.push(root.join("Notepad++"));
            folders.push(root.join("Neovim").join("bin"));
        }
    }
    folders
}

/// The editors of [`KNOWN`] found on this computer, by their index.
pub fn installed() -> Vec<usize> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let extra = extra_folders();
    KNOWN
        .iter()
        .enumerate()
        .filter(|(_, e)| find_program(e.program, &path, &extra).is_some())
        .map(|(i, _)| i)
        .collect()
}

/// One line of the list that offers the editors for a setting: the editors
/// of the wanted kind first (installed ones before the others), then the other
/// kind.
pub struct Suggestion {
    /// The row as shown.
    pub label: String,
    /// The command to store ("" for the first row: the default).
    pub command: String,
}

/// The suggestions for a setting that wants window editors (`window`) or
/// terminal editors; `installed` holds indexes into [`KNOWN`] and `default`
/// says what the setting does when it is empty.
pub fn suggestions(window: bool, installed: &[usize], default: &str) -> Vec<Suggestion> {
    let wanted = if window { Kind::Window } else { Kind::Terminal };
    let mut order: Vec<usize> = (0..KNOWN.len()).collect();
    // Wanted kind first, installed first, otherwise the order of the table.
    order.sort_by_key(|&i| (KNOWN[i].kind != wanted, !installed.contains(&i), i));
    let mut rows =
        vec![Suggestion { label: format!("(the default: {default})"), command: String::new() }];
    for i in order {
        let e = &KNOWN[i];
        let kind = match e.kind {
            Kind::Window => "window",
            Kind::Terminal => "terminal",
        };
        let state = if installed.contains(&i) {
            "installed".to_owned()
        } else {
            format!("not found: {}", e.install)
        };
        rows.push(Suggestion {
            label: format!("{} ({kind}, {}, {state})", e.name, e.licence),
            command: e.command.to_owned(),
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_gets_the_file_with_spaces_in_one_word() {
        let file = Path::new("/home/me/My Notes/a b.txt");
        assert_eq!(
            command_line("codium --wait", file, 1),
            ["codium", "--wait", "/home/me/My Notes/a b.txt"]
        );
        assert_eq!(
            command_line("kate --line {line} {file}", file, 7),
            ["kate", "--line", "7", "/home/me/My Notes/a b.txt"]
        );
        assert_eq!(
            command_line(r#""/opt/My Apps/ed" +{line}"#, file, 3)[..2],
            ["/opt/My Apps/ed", "+3"]
        );
    }

    #[test]
    fn editors_know_whether_they_are_windows_or_terminal_programs() {
        assert_eq!(kind_of("nvim"), Some(Kind::Terminal));
        assert_eq!(kind_of("/usr/bin/nvim -u NONE"), Some(Kind::Terminal));
        assert_eq!(kind_of("codium --wait"), Some(Kind::Window));
        assert_eq!(kind_of("emacs -nw"), Some(Kind::Terminal));
        assert_eq!(kind_of("emacs"), Some(Kind::Window));
        assert_eq!(kind_of("my-private-editor"), None);
        assert!(suits("nvim", false) && !suits("nvim", true));
        assert!(suits("kate", true));
        assert!(suits("my-private-editor", true) && suits("my-private-editor", false));
    }

    #[test]
    fn suggestions_list_the_wanted_kind_and_the_installed_first() {
        let nano = KNOWN.iter().position(|e| e.program == "nano").unwrap();
        let rows = suggestions(false, &[nano], "$EDITOR");
        assert_eq!(rows[0].command, "");
        assert_eq!(rows[1].command, "nano", "installed terminal editor first");
        assert!(rows[1].label.contains("installed"));
        assert!(rows[2].label.contains("terminal"));
        assert!(rows.iter().any(|r| r.command == "codium --wait" && r.label.contains("not found")));
        let window = suggestions(true, &[], "$EDITOR");
        assert_eq!(window[1].command, "codium --wait");
    }

    #[test]
    fn a_program_is_found_on_the_given_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("fake-editor"), "").unwrap();
        let found = find_program("fake-editor", dir.path().as_os_str(), &[]);
        assert_eq!(found, Some(dir.path().join("fake-editor")));
        assert_eq!(find_program("nothing-here", dir.path().as_os_str(), &[]), None);
    }
}
