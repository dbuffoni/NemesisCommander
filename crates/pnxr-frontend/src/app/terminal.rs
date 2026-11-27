//! "Open terminal here" and the folder the shell is left in when
//! the terminal interface ends.

use std::path::PathBuf;

use super::{App, External};
use crate::terminal::{self, Place};

impl App {
    /// The file the terminal interface writes the folder of the active
    /// panel to when it ends, for the shell function of `pnxr shell-init`
    /// (`--cwd-file`). Marks the front end as one that cannot open a
    /// terminal window of its own.
    pub fn set_cwd_file(&mut self, file: Option<PathBuf>) {
        self.cwd_file = file;
    }

    /// The local folder of the active panel, when it has one.
    pub fn active_local_dir(&self) -> Option<PathBuf> {
        let location = self.workspace.active().location();
        if location.scheme() == "file" { location.to_local_path() } else { None }
    }

    /// What goes to `--cwd-file` when the interface ends: the active
    /// folder if it is on this computer, else nothing (the shell stays
    /// where it was).
    pub fn cwd_to_write(&self) -> Option<(PathBuf, String)> {
        let file = self.cwd_file.clone()?;
        let dir = self.active_local_dir()?;
        Some((file, dir.display().to_string()))
    }

    /// The command "Open terminal here".
    pub(super) fn open_terminal(&mut self) {
        let location = self.workspace.active().location().clone();
        // A server reached with SSH gets an SSH session in the folder.
        if matches!(location.scheme(), "sftp" | "agent") {
            return self.open_remote_terminal(&location);
        }
        let Some(dir) = self.active_local_dir() else {
            return self.set_error(
                "A terminal can open in a folder of this computer or of a server reached \
                 with SSH (sftp, agent); this location has no system folder",
            );
        };
        if !self.graphical {
            // The terminal interface has no window to open: it ends and the
            // shell function moves the shell to the folder.
            if self.cwd_file.is_none() {
                return self.set_error(
                    "To leave the shell in this folder, start pnxr with the shell function: \
                     eval \"$(pnxr shell-init bash)\" (also zsh, fish, pwsh)",
                );
            }
            self.running = false;
            return;
        }
        let place = Place::Local(&dir);
        let candidates = terminal::commands(
            &self.context.config.general.terminal,
            std::env::var("TERMINAL").ok().as_deref(),
            &place,
            std::env::consts::OS,
        );
        match self.host.launch_terminal(&candidates, terminal::current_dir(&place)) {
            Ok(_) => self.set_status(format!("Terminal opened in {}", dir.display())),
            Err(e) => self.set_error(e),
        }
    }

    fn open_remote_terminal(&mut self, location: &pnxr_vfs::VfsPath) {
        let target = match self.context.remotes.ssh_target(location.authority()) {
            Ok(target) => target,
            Err(e) => return self.set_error(e),
        };
        let mut ssh_args = Vec::new();
        if let Some(port) = target.port {
            ssh_args.push("-p".to_owned());
            ssh_args.push(port.to_string());
        }
        ssh_args.push(target.destination());
        let place = Place::Remote { ssh_args, dir: location.path() };
        if !self.graphical {
            // The terminal is handed to ssh until the session ends.
            let Place::Remote { ssh_args, dir } = &place else { return };
            self.external = Some(External {
                command: terminal::ssh_command(ssh_args, dir),
                reload_settings: false,
                rename: None,
            });
            return;
        }
        let candidates = terminal::commands(
            &self.context.config.general.terminal,
            std::env::var("TERMINAL").ok().as_deref(),
            &place,
            std::env::consts::OS,
        );
        match self.host.launch_terminal(&candidates, None) {
            Ok(_) => self.set_status(format!("SSH session opened on {}", target.host)),
            Err(e) => self.set_error(e),
        }
    }
}
