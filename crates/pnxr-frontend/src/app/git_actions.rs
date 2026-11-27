//! Basic Git actions (stage, unstage, diff, commit, discard, switch branch,
//! log, pull, push) on the repository of the active folder, with the
//! system's `git`. Nothing is linked in; without git or outside a
//! repository the command says so.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{App, AppEvent, done};
use crate::dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, InputDialog, InputPurpose,
    MessageDialog, Overlay,
};

/// The rows of the list, in order: the text and what each one does.
pub const ROWS: [(&str, &str); 9] = [
    ("Stage the selected files", "git add"),
    ("Unstage the selected files", "git restore --staged"),
    ("Show the changes (diff) of the selected files", "git diff"),
    ("Commit what is staged…", "git commit"),
    ("Discard the changes of the selected files…", "git restore"),
    ("Switch branch…", "git switch"),
    ("Show the last commits (log)", "git log"),
    ("Pull (fetch and merge the upstream)", "git pull --ff-only"),
    ("Push the current branch", "git push"),
];

/// Runs `git` in `dir`; the text it printed, or why it failed.
fn run(exe: &str, dir: &Path, args: &[&str], files: &[String]) -> Result<String, String> {
    let mut command = Command::new(exe);
    command.arg("-C").arg(dir).args(args);
    if !files.is_empty() {
        command.arg("--").args(files);
    }
    let output =
        command.env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null()).output().map_err(|e| {
            format!("Cannot run {exe}: {e} (install git, or set its path in Settings > Git)")
        })?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr);
    if !err.trim().is_empty() {
        text.push_str(&err);
    }
    if output.status.success() { Ok(text) } else { Err(text.trim().to_owned()) }
}

fn lines_of(text: &str, limit: usize) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().take(limit).map(str::to_owned).collect();
    if text.lines().count() > limit {
        lines.push(format!("… {} more lines", text.lines().count() - limit));
    }
    if lines.is_empty() {
        lines.push("(nothing to show)".to_owned());
    }
    lines
}

impl App {
    /// The Git actions list for the active folder.
    pub(super) fn ask_git(&mut self) {
        let Some(dir) = self.workspace.active().location().to_local_path() else {
            return self.set_error("Git actions work on local folders");
        };
        if crate::git::read_with(self.context.config.git.program(), &dir).is_none() {
            return self.set_error(
                "This folder is not in a Git repository (or git is not installed: Settings > Git)",
            );
        }
        let files: Vec<String> =
            self.targets().iter().filter_map(|p| p.file_name().map(str::to_owned)).collect();
        let branch = self.git[super::side_index(self.workspace.active_side())]
            .as_ref()
            .map(|(_, status)| status.summary())
            .unwrap_or_default();
        let title = if files.is_empty() {
            format!("Git ({branch})")
        } else {
            format!("Git ({branch}), {} selected", files.len())
        };
        let rows = ROWS.iter().map(|(text, cmd)| format!("{text:<50} {cmd}")).collect();
        self.overlay = Some(Overlay::Choose(ChooseDialog::new(
            &title,
            rows,
            ChoosePurpose::Git { dir, files },
        )));
    }

    pub(super) fn git_chosen(&mut self, dir: PathBuf, files: Vec<String>, row: usize) {
        let exe = self.context.config.git.program().to_owned();
        let needs_files = matches!(row, 0..=2 | 4);
        if needs_files && files.is_empty() && row != 2 {
            return self.set_error("Select the files first (or put the cursor on one)");
        }
        match row {
            3 => {
                self.overlay = Some(Overlay::Input(InputDialog::new(
                    "Git commit",
                    "Commit message (what is staged is committed):",
                    "",
                    InputPurpose::GitCommit(dir),
                )));
            }
            4 => {
                let mut lines = vec!["The changes of these files are lost for good:".to_owned()];
                lines.extend(files.iter().take(8).map(|f| format!("  {f}")));
                if files.len() > 8 {
                    lines.push(format!("  … and {} more", files.len() - 8));
                }
                self.overlay = Some(Overlay::Confirm(ConfirmDialog {
                    title: "Discard changes".into(),
                    lines,
                    purpose: ConfirmPurpose::GitDiscard(dir, files),
                }));
            }
            5 => {
                self.spawn_task(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        run(&exe, &dir, &["branch", "--format=%(refname:short)"], &[])
                            .map(|text| (dir, text))
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                    match result {
                        Ok((dir, text)) => {
                            let names: Vec<String> =
                                text.lines().map(str::to_owned).filter(|l| !l.is_empty()).collect();
                            AppEvent::Show(Box::new(Overlay::Choose(ChooseDialog::new(
                                "Switch to branch",
                                names.clone(),
                                ChoosePurpose::GitBranches { dir, names },
                            ))))
                        }
                        Err(e) => done(Err(pnxr_vfs::VfsError::Other(e)), None),
                    }
                });
            }
            _ => self.git_run(exe, dir, files, row),
        }
    }

    /// Runs the action of `row` in the background and reports.
    fn git_run(&mut self, exe: String, dir: PathBuf, files: Vec<String>, row: usize) {
        self.set_status(format!("{}…", ROWS[row].1));
        self.spawn_task(async move {
            let result = tokio::task::spawn_blocking(move || match row {
                0 => run(&exe, &dir, &["add"], &files)
                    .map(|_| format!("Staged {} item(s)", files.len())),
                1 => run(&exe, &dir, &["restore", "--staged"], &files)
                    .map(|_| format!("Unstaged {} item(s)", files.len())),
                2 => run(&exe, &dir, &["diff", "--no-color", "HEAD"], &files),
                6 => run(
                    &exe,
                    &dir,
                    &["log", "--no-color", "--oneline", "--decorate", "-n", "40"],
                    &[],
                ),
                7 => run(&exe, &dir, &["pull", "--ff-only"], &[]),
                _ => run(&exe, &dir, &["push"], &[]),
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            match (row, result) {
                (2 | 6 | 7 | 8, Ok(text)) => {
                    let title = ROWS[row].1;
                    AppEvent::Show(Box::new(Overlay::Message(MessageDialog::new(
                        title,
                        lines_of(&text, 400),
                    ))))
                }
                (_, Ok(message)) => done(Ok(message), None),
                (_, Err(e)) => AppEvent::Show(Box::new(Overlay::Message(MessageDialog::new(
                    "Git",
                    lines_of(&e, 40),
                )))),
            }
        });
    }

    pub(super) fn git_branch_chosen(&mut self, dir: PathBuf, names: &[String], row: usize) {
        let Some(name) = names.get(row).cloned() else { return };
        let exe = self.context.config.git.program().to_owned();
        self.spawn_task(async move {
            let result = tokio::task::spawn_blocking(move || {
                run(&exe, &dir, &["switch", &name], &[]).map(|_| format!("Switched to {name}"))
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            match result {
                Ok(message) => done(Ok(message), None),
                Err(e) => AppEvent::Show(Box::new(Overlay::Message(MessageDialog::new(
                    "Git switch",
                    lines_of(&e, 40),
                )))),
            }
        });
    }

    pub(super) fn git_commit(&mut self, dir: PathBuf, message: String) {
        if message.trim().is_empty() {
            return self.set_error("A commit needs a message");
        }
        let exe = self.context.config.git.program().to_owned();
        self.spawn_task(async move {
            let result = tokio::task::spawn_blocking(move || {
                run(&exe, &dir, &["commit", "-m", message.trim()], &[])
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            match result {
                Ok(text) => done(Ok(text.lines().next().unwrap_or("Committed").to_owned()), None),
                Err(e) => AppEvent::Show(Box::new(Overlay::Message(MessageDialog::new(
                    "Git commit",
                    lines_of(&e, 40),
                )))),
            }
        });
    }

    pub(super) fn git_discard(&mut self, dir: PathBuf, files: Vec<String>) {
        let exe = self.context.config.git.program().to_owned();
        self.spawn_task(async move {
            let result = tokio::task::spawn_blocking(move || {
                run(&exe, &dir, &["restore"], &files)
                    .map(|_| format!("Discarded changes of {} item(s)", files.len()))
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            match result {
                Ok(message) => done(Ok(message), None),
                Err(e) => AppEvent::Show(Box::new(Overlay::Message(MessageDialog::new(
                    "Git discard",
                    lines_of(&e, 40),
                )))),
            }
        });
    }
}
