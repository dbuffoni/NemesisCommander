//! "Install the pnxr command…" in the Pairnox menu: a launcher named `pnxr`
//! in a folder of the PATH, and the `pnxr shell-init` line in the shell's
//! startup file (see [`pnxr_app::command_install`]).

use std::path::PathBuf;

use pnxr_app::command_install as cmd;
use pnxr_app::command_install::{ProfileShell, State};

use super::*;

/// One row of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallRow {
    /// Only says where the command is now (Enter shows it on the status line).
    Status,
    /// Write the launcher into this folder.
    Install(PathBuf),
    /// Write it into a folder the user types.
    InstallElsewhere,
    /// Remove the launcher from this folder.
    Uninstall(PathBuf),
    /// Add the `shell-init` line to the startup file of this shell.
    AddProfile(ProfileShell, PathBuf),
    /// Remove it again.
    RemoveProfile(ProfileShell, PathBuf),
}

/// The rows and their texts for the folder `dir` (the default one, unless a
/// launcher was found elsewhere on the PATH).
fn rows(dir: &std::path::Path) -> Vec<(String, InstallRow)> {
    let now = cmd::program().ok();
    let mut rows = Vec::new();
    let state = cmd::state(dir, now.as_ref());
    let status = match &state {
        State::NotInstalled => match cmd::found_on_path() {
            Some(other) => {
                format!("Status: not installed in {}; found {}", dir.display(), other.display())
            }
            None => format!("Status: not installed (would go to {})", dir.display()),
        },
        State::Installed { file, runs, current } => format!(
            "Status: installed, {} starts {runs}{}",
            file.display(),
            if *current { "" } else { " (not this program: install again to update)" }
        ),
        State::Foreign(file) => {
            format!("Status: {} exists and is not Pairnox's, so it is left alone", file.display())
        }
    };
    rows.push((status, InstallRow::Status));
    match state {
        State::Foreign(_) => {}
        State::Installed { current: true, .. } => {}
        State::Installed { .. } => {
            rows.push((
                format!("Update the command in {}", dir.display()),
                InstallRow::Install(dir.into()),
            ));
        }
        State::NotInstalled => {
            rows.push((format!("Install in {}", dir.display()), InstallRow::Install(dir.into())));
        }
    }
    rows.push(("Install in another folder…".into(), InstallRow::InstallElsewhere));
    if matches!(cmd::state(dir, None), State::Installed { .. }) {
        rows.push((format!("Uninstall from {}", dir.display()), InstallRow::Uninstall(dir.into())));
    }
    if let Some((shell, home)) = cmd::user_shell() {
        let file = shell.file(&home);
        if cmd::profile_has_line(shell, &home) {
            rows.push((
                format!("Remove the shell-init line from {}", file.display()),
                InstallRow::RemoveProfile(shell, home),
            ));
        } else {
            rows.push((
                format!(
                    "Add the shell-init line to {} (the shell follows the folder you leave in)",
                    file.display()
                ),
                InstallRow::AddProfile(shell, home),
            ));
        }
    }
    rows
}

impl App {
    /// The command of the menu.
    pub(super) fn open_install_command(&mut self) {
        let Some(dir) = cmd::default_dir() else {
            return self.set_error("Cannot find your home folder");
        };
        let rows = rows(&dir);
        let items = rows.iter().map(|(text, _)| text.clone()).collect();
        let purpose = ChoosePurpose::InstallCommand(rows.into_iter().map(|(_, r)| r).collect());
        let mut dialog = ChooseDialog::new("The pnxr command", items, purpose);
        dialog.footer = crate::hints::list("do it", &[], "close");
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// Enter on a row.
    pub(super) fn install_command_chosen(&mut self, rows: &[InstallRow], cursor: usize) {
        let Some(row) = rows.get(cursor).cloned() else { return };
        match row {
            InstallRow::Status => {
                let text = match (cmd::default_dir(), cmd::found_on_path()) {
                    (_, Some(found)) => format!("pnxr is found at {}", found.display()),
                    (Some(dir), None) => format!(
                        "No pnxr command on the PATH; {} is where it would go",
                        dir.display()
                    ),
                    (None, None) => "No pnxr command on the PATH".into(),
                };
                self.set_status(text);
                self.open_install_command();
            }
            InstallRow::Install(dir) => self.install_command_in(&dir),
            InstallRow::InstallElsewhere => {
                let start = cmd::default_dir().map(|d| d.display().to_string()).unwrap_or_default();
                self.ask(
                    "Install the pnxr command",
                    "Folder (it should be on your PATH):",
                    &start,
                    InputPurpose::InstallCommandFolder,
                );
            }
            InstallRow::Uninstall(dir) => {
                match cmd::uninstall(&dir) {
                    Ok(file) => self.set_status(format!("Removed {}", file.display())),
                    Err(e) => self.set_error(e),
                }
                self.open_install_command();
            }
            InstallRow::AddProfile(shell, home) => {
                match cmd::add_profile_line(shell, &home) {
                    Ok(file) => self.set_status(format!(
                        "Added the {} shell-init line to {}; it works in new shells",
                        shell.name(),
                        file.display()
                    )),
                    Err(e) => self.set_error(e),
                }
                self.open_install_command();
            }
            InstallRow::RemoveProfile(shell, home) => {
                match cmd::remove_profile_line(shell, &home) {
                    Ok(true) => self.set_status(format!(
                        "Removed the shell-init line from {}",
                        shell.file(&home).display()
                    )),
                    Ok(false) => self.set_status("There was no shell-init line to remove"),
                    Err(e) => self.set_error(e),
                }
                self.open_install_command();
            }
        }
    }

    /// Writes the launcher into `dir` and says where it is and whether the
    /// folder is on the PATH.
    pub(super) fn install_command_in(&mut self, dir: &std::path::Path) {
        let result = cmd::program().and_then(|program| cmd::install(dir, &program));
        match result {
            Ok(file) if cmd::on_path(dir) => {
                self.set_status(format!("Installed {}: start it with pnxr", file.display()))
            }
            Ok(file) => {
                self.set_status(format!("Installed {}. {}", file.display(), cmd::path_hint(dir)))
            }
            Err(e) => self.set_error(e),
        }
        self.open_install_command();
    }

    /// The folder typed for "Install in another folder…".
    pub(super) fn install_command_folder(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return self.set_error("Type a folder");
        }
        self.install_command_in(&expand_home(text));
    }
}
