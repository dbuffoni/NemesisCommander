//! The `pnxr` command on this computer: "Install the pnxr command…" in the
//! Pairnox menu puts a small launcher named `pnxr` in a folder on the PATH,
//! so that the program (terminal interface and commands) can be started from
//! any terminal, also when Pairnox itself came as an AppImage or an app
//! bundle. The same dialog can add the `pnxr shell-init` line to the shell's
//! startup file, which lets the terminal interface leave the shell in the
//! folder it ended in.
//!
//! Only files this module wrote (they carry a marker) are replaced or
//! removed.

use std::path::{Path, PathBuf};

/// Marks a launcher as written here (a comment of the script).
pub const MARKER: &str = "Pairnox launcher for the pnxr command";

/// Marks the lines added to a shell startup file.
const PROFILE_BEGIN: &str = "# >>> pnxr shell-init (Pairnox) >>>";
const PROFILE_END: &str = "# <<< pnxr shell-init (Pairnox) <<<";

/// What the launcher starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// An AppImage, which runs `pnxr` when its first argument is `pnxr`.
    AppImage(PathBuf),
    /// The `pnxr` program itself.
    Binary(PathBuf),
}

impl Program {
    /// The file the launcher starts.
    pub fn path(&self) -> &Path {
        match self {
            Program::AppImage(path) | Program::Binary(path) => path,
        }
    }
}

/// What is found at the place of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing there.
    NotInstalled,
    /// A launcher written here; `runs` is what it starts, `current` whether
    /// that is what would be written now.
    Installed {
        /// The launcher.
        file: PathBuf,
        /// The program it starts.
        runs: String,
        /// It starts the program that is running now.
        current: bool,
    },
    /// A file of that name that was not written here (left alone).
    Foreign(PathBuf),
}

/// The name of the launcher.
pub fn file_name() -> &'static str {
    if cfg!(windows) { "pnxr.cmd" } else { "pnxr" }
}

/// The folder offered first: `~/.local/bin` (Linux, macOS), and
/// `%LOCALAPPDATA%\Pairnox\bin` on Windows.
pub fn default_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(PathBuf::from);
        return base.map(|b| b.join("Pairnox").join("bin"));
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join(".local").join("bin"))
}

/// The program the launcher should start: the AppImage when running from
/// one, else the `pnxr` program (this one, or next to the window program).
pub fn program() -> Result<Program, String> {
    if let Some(image) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Ok(Program::AppImage(PathBuf::from(image)));
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = if cfg!(windows) { "pnxr.exe" } else { "pnxr" };
    let cli = me.with_file_name(exe);
    if cli.exists() {
        return Ok(Program::Binary(cli));
    }
    Err("cannot find the pnxr program (it is next to the window program, or in the AppImage)"
        .into())
}

/// Quotes `text` for a POSIX shell.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The text of the launcher for `program`.
pub fn script(program: &Program) -> String {
    let path = program.path().display().to_string();
    if cfg!(windows) {
        return match program {
            Program::AppImage(_) | Program::Binary(_) => {
                format!("@echo off\r\nREM {MARKER}\r\n\"{path}\" %*\r\n")
            }
        };
    }
    let run = match program {
        Program::AppImage(_) => format!("exec {} pnxr \"$@\"", shell_quote(&path)),
        Program::Binary(_) => format!("exec {} \"$@\"", shell_quote(&path)),
    };
    format!("#!/bin/sh\n# {MARKER}\n{run}\n")
}

/// What is at `dir/pnxr` now; `now` is the program that would be written.
pub fn state(dir: &Path, now: Option<&Program>) -> State {
    let file = dir.join(file_name());
    let Ok(text) = std::fs::read_to_string(&file) else {
        // A link or a binary is not ours either.
        return if file.symlink_metadata().is_ok() {
            State::Foreign(file)
        } else {
            State::NotInstalled
        };
    };
    if !text.contains(MARKER) || file.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        return State::Foreign(file);
    }
    let runs = text
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("exec ")
                .map(|rest| {
                    let rest = rest.trim_end_matches(" \"$@\"").trim_end_matches(" pnxr");
                    rest.trim_matches('\'').replace("'\\''", "'")
                })
                .or_else(|| line.strip_suffix(" %*").map(|rest| rest.trim_matches('"').to_owned()))
        })
        .unwrap_or_default();
    let current = now.is_some_and(|p| text == script(p));
    State::Installed { file, runs, current }
}

/// Writes the launcher into `dir` (made if needed). A file of that name that
/// was not written here is not replaced.
pub fn install(dir: &Path, program: &Program) -> Result<PathBuf, String> {
    if let State::Foreign(file) = state(dir, None) {
        return Err(format!(
            "{} exists and was not written by Pairnox, so it is left alone. Remove it, or choose another folder",
            file.display()
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(file_name());
    std::fs::write(&file, script(program)).map_err(|e| format!("{}: {e}", file.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(file)
}

/// Removes the launcher written by [`install`].
pub fn uninstall(dir: &Path) -> Result<PathBuf, String> {
    match state(dir, None) {
        State::Installed { file, .. } => {
            std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok(file)
        }
        State::Foreign(file) => {
            Err(format!("{} was not written by Pairnox, so it is left alone", file.display()))
        }
        State::NotInstalled => Err(format!("No pnxr command in {}", dir.display())),
    }
}

/// Whether `dir` is one of the folders of the PATH.
pub fn on_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|p| p == dir))
}

/// How to put `dir` on the PATH, for a message.
pub fn path_hint(dir: &Path) -> String {
    if cfg!(windows) {
        format!(
            "{} is not on your PATH: add it in Settings > System > About > Advanced system settings > Environment Variables",
            dir.display()
        )
    } else {
        format!(
            "{} is not on your PATH: add the line  export PATH=\"{}:$PATH\"  to your shell's startup file",
            dir.display(),
            dir.display()
        )
    }
}

/// Where the command is found now: the first `pnxr` on the PATH.
pub fn found_on_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|p| p.join(file_name())).find(|p| p.is_file())
}

// ------------------------------------------------------------ shell startup

/// The shells whose startup file can get the `shell-init` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileShell {
    /// bash: `~/.bashrc`.
    Bash,
    /// zsh: `~/.zshrc`.
    Zsh,
    /// fish: `~/.config/fish/config.fish`.
    Fish,
}

impl ProfileShell {
    /// The shell named by `$SHELL` (a path), if it is one of these.
    pub fn from_env(shell: &str) -> Option<Self> {
        match Path::new(shell).file_name()?.to_str()? {
            "bash" => Some(ProfileShell::Bash),
            "zsh" => Some(ProfileShell::Zsh),
            "fish" => Some(ProfileShell::Fish),
            _ => None,
        }
    }

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            ProfileShell::Bash => "bash",
            ProfileShell::Zsh => "zsh",
            ProfileShell::Fish => "fish",
        }
    }

    /// The startup file, under `home`.
    pub fn file(self, home: &Path) -> PathBuf {
        match self {
            ProfileShell::Bash => home.join(".bashrc"),
            ProfileShell::Zsh => home.join(".zshrc"),
            ProfileShell::Fish => home.join(".config").join("fish").join("config.fish"),
        }
    }

    /// The line that sets the function up.
    fn line(self) -> String {
        match self {
            ProfileShell::Bash => "eval \"$(pnxr shell-init bash)\"".into(),
            ProfileShell::Zsh => "eval \"$(pnxr shell-init zsh)\"".into(),
            ProfileShell::Fish => "pnxr shell-init fish | source".into(),
        }
    }
}

/// The shell of the user (`$SHELL`) and its home folder.
pub fn user_shell() -> Option<(ProfileShell, PathBuf)> {
    let shell = ProfileShell::from_env(&std::env::var("SHELL").ok()?)?;
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some((shell, PathBuf::from(home)))
}

/// Whether the startup file has the lines added here.
pub fn profile_has_line(shell: ProfileShell, home: &Path) -> bool {
    std::fs::read_to_string(shell.file(home)).is_ok_and(|t| t.contains(PROFILE_BEGIN))
}

/// Adds the `shell-init` line to the startup file (once); returns the file.
pub fn add_profile_line(shell: ProfileShell, home: &Path) -> Result<PathBuf, String> {
    let file = shell.file(home);
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    if old.contains(PROFILE_BEGIN) {
        return Ok(file);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut text = old;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{PROFILE_BEGIN}\n{}\n{PROFILE_END}\n", shell.line()));
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

/// Removes the lines added by [`add_profile_line`]; returns whether there
/// were any.
pub fn remove_profile_line(shell: ProfileShell, home: &Path) -> Result<bool, String> {
    let file = shell.file(home);
    let Ok(old) = std::fs::read_to_string(&file) else { return Ok(false) };
    let Some(start) = old.find(PROFILE_BEGIN) else { return Ok(false) };
    let end =
        old[start..].find(PROFILE_END).map(|i| start + i + PROFILE_END.len()).unwrap_or(old.len());
    let end = if old[end..].starts_with('\n') { end + 1 } else { end };
    let text = format!("{}{}", &old[..start], &old[end..]);
    std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Program {
        Program::AppImage(PathBuf::from("/opt/Pairnox's.AppImage"))
    }

    #[cfg(unix)]
    #[test]
    fn the_launcher_runs_the_appimage_with_pnxr_and_quotes_the_path() {
        let text = script(&image());
        assert!(text.starts_with("#!/bin/sh\n"));
        assert!(text.contains(MARKER));
        assert!(text.contains("exec '/opt/Pairnox'\\''s.AppImage' pnxr \"$@\""), "{text}");
        let binary = script(&Program::Binary("/usr/bin/pnxr".into()));
        assert!(binary.contains("exec '/usr/bin/pnxr' \"$@\""), "{binary}");
    }

    #[cfg(unix)]
    #[test]
    fn install_status_and_uninstall_follow_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        assert_eq!(state(&bin, None), State::NotInstalled);
        let file = install(&bin, &image()).unwrap();
        match state(&bin, Some(&image())) {
            State::Installed { file: f, runs, current } => {
                assert_eq!(f, file);
                assert_eq!(runs, "/opt/Pairnox's.AppImage");
                assert!(current);
            }
            other => panic!("{other:?}"),
        }
        // Another program now: the installed one is out of date.
        let other = Program::Binary("/usr/bin/pnxr".into());
        assert!(matches!(state(&bin, Some(&other)), State::Installed { current: false, .. }));
        uninstall(&bin).unwrap();
        assert_eq!(state(&bin, None), State::NotInstalled);
        assert!(uninstall(&bin).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_is_not_ours_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pnxr"), "#!/bin/sh\necho mine\n").unwrap();
        assert!(matches!(state(dir.path(), None), State::Foreign(_)));
        assert!(install(dir.path(), &image()).is_err());
        assert!(uninstall(dir.path()).is_err());
        assert!(std::fs::read_to_string(dir.path().join("pnxr")).unwrap().contains("mine"));
    }

    #[test]
    fn the_shell_init_line_is_added_once_and_removed_cleanly() {
        let home = tempfile::tempdir().unwrap();
        let rc = home.path().join(".bashrc");
        std::fs::write(&rc, "export A=1").unwrap();
        assert!(!profile_has_line(ProfileShell::Bash, home.path()));
        add_profile_line(ProfileShell::Bash, home.path()).unwrap();
        add_profile_line(ProfileShell::Bash, home.path()).unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(text.matches("shell-init bash").count(), 1, "{text}");
        assert!(profile_has_line(ProfileShell::Bash, home.path()));
        assert!(remove_profile_line(ProfileShell::Bash, home.path()).unwrap());
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "export A=1\n");
        assert!(!remove_profile_line(ProfileShell::Bash, home.path()).unwrap());
    }

    #[test]
    fn fish_gets_its_own_file_and_syntax() {
        let home = tempfile::tempdir().unwrap();
        let file = add_profile_line(ProfileShell::Fish, home.path()).unwrap();
        assert!(file.ends_with(".config/fish/config.fish"));
        assert!(std::fs::read_to_string(file).unwrap().contains("pnxr shell-init fish | source"));
        assert_eq!(ProfileShell::from_env("/usr/bin/zsh"), Some(ProfileShell::Zsh));
        assert_eq!(ProfileShell::from_env("/bin/tcsh"), None);
    }
}
