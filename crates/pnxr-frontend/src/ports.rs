//! The port to the computer itself: starting a terminal, asking Git about a
//! folder and listing the disks. The interface holds a `dyn HostTools`;
//! [`SystemHost`] starts the real programs (the code of `terminal`, `git` and
//! `disks`) and [`FakeHost`] starts nothing and answers from memory, so tests
//! run without spawning a process (ADR 0061).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::disks::DiskRow;
use crate::git::GitStatus;

/// What the interface asks of the computer.
pub trait HostTools: Send + Sync {
    /// Starts the first of `candidates` (a program and its arguments) that
    /// works, in `dir`; returns the command that started.
    fn launch_terminal(
        &self,
        candidates: &[Vec<String>],
        dir: Option<&Path>,
    ) -> Result<Vec<String>, String>;
    /// The Git state of `dir`, or `None` outside a repository (or without Git).
    fn git_status(&self, program: &str, dir: &Path) -> Option<GitStatus>;
    /// The disks and partitions of this computer.
    fn disks(&self) -> Result<Vec<DiskRow>, String>;
}

/// The real programs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemHost;

impl HostTools for SystemHost {
    fn launch_terminal(
        &self,
        candidates: &[Vec<String>],
        dir: Option<&Path>,
    ) -> Result<Vec<String>, String> {
        crate::terminal::launch(candidates, dir)
    }

    fn git_status(&self, program: &str, dir: &Path) -> Option<GitStatus> {
        crate::git::read_with(program, dir)
    }

    fn disks(&self) -> Result<Vec<DiskRow>, String> {
        crate::disks::list()
    }
}

/// A host that starts nothing: it remembers the terminals it was asked for
/// and answers Git and disks from what a test sets.
#[derive(Debug, Default)]
pub struct FakeHost {
    /// Each terminal launch asked for: the first candidate and the folder.
    pub launches: Mutex<Vec<(Vec<String>, Option<PathBuf>)>>,
    /// What `git_status` answers.
    pub git: Mutex<Option<GitStatus>>,
    /// What `disks` answers.
    pub disks: Mutex<Vec<DiskRow>>,
}

impl FakeHost {
    /// A fake that finds no repository and no disks.
    pub fn new() -> Self {
        Self::default()
    }
}

impl HostTools for FakeHost {
    fn launch_terminal(
        &self,
        candidates: &[Vec<String>],
        dir: Option<&Path>,
    ) -> Result<Vec<String>, String> {
        let first = candidates.first().cloned().ok_or("no terminal to start")?;
        self.launches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((first.clone(), dir.map(Path::to_path_buf)));
        Ok(first)
    }

    fn git_status(&self, _: &str, _: &Path) -> Option<GitStatus> {
        self.git.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn disks(&self) -> Result<Vec<DiskRow>, String> {
        Ok(self.disks.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }
}
