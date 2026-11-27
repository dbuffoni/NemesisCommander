//! Git status of a folder: a mark for each file (modified, untracked,
//! ignored) and the branch with its distance from the upstream. It asks the
//! system's `git`, so nothing is linked in and nothing happens without a
//! repository.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

/// What git says about the files of one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    /// Current branch (`HEAD` when detached).
    pub branch: String,
    /// Commits not yet pushed.
    pub ahead: u32,
    /// Commits not yet pulled.
    pub behind: u32,
    /// A mark for each entry of the folder: `M` changed, `?` untracked,
    /// `!` ignored. A folder carries the strongest mark of what is inside.
    pub marks: HashMap<String, char>,
}

impl GitStatus {
    /// `main ↑1 ↓2`, for the title of the panel.
    pub fn summary(&self) -> String {
        let mut text = self.branch.clone();
        if self.ahead > 0 {
            text.push_str(&format!(" ↑{}", self.ahead));
        }
        if self.behind > 0 {
            text.push_str(&format!(" ↓{}", self.behind));
        }
        text
    }
}

/// Reads the status of `dir` with the system's `git`.
pub fn read(dir: &Path) -> Option<GitStatus> {
    read_with("git", dir)
}

/// Reads the status of `dir` with the program `exe`; `None` outside a
/// repository or without git.
pub fn read_with(exe: &str, dir: &Path) -> Option<GitStatus> {
    let git = |args: &[&str]| -> Option<Vec<u8>> {
        let output = Command::new(exe)
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    };
    let prefix = git(&["rev-parse", "--show-prefix"])?;
    let prefix = String::from_utf8_lossy(&prefix).trim().to_owned();
    let out = git(&["status", "--porcelain=v1", "-z", "--branch", "--ignored=matching"])?;
    Some(parse(&out, &prefix))
}

/// Reads `git status --porcelain=v1 -z --branch` output for the folder
/// `prefix` (the folder's path inside the repository, `sub/dir/` or empty).
pub fn parse(output: &[u8], prefix: &str) -> GitStatus {
    let mut status = GitStatus::default();
    let text = String::from_utf8_lossy(output);
    let mut records = text.split('\0').filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("## ") {
            read_branch(header, &mut status);
            continue;
        }
        if record.len() < 4 {
            continue;
        }
        let (code, path) = record.split_at(3);
        // A rename or copy carries the old path in the next record.
        if code.starts_with(['R', 'C']) || code[1..2].contains(['R', 'C']) {
            records.next();
        }
        let mark = match &code[..2] {
            "??" => '?',
            "!!" => '!',
            _ => 'M',
        };
        let Some(inside) = path.strip_prefix(prefix) else { continue };
        let name = inside.split('/').next().unwrap_or(inside);
        if name.is_empty() {
            continue;
        }
        let entry = status.marks.entry(name.to_owned()).or_insert(mark);
        if rank(mark) > rank(*entry) {
            *entry = mark;
        }
    }
    status
}

/// What a mark means, in words (the legend of the git marks).
pub fn legend(mark: char) -> &'static str {
    match mark {
        'M' => "M: changed since the last commit (a folder: something inside changed)",
        '?' => "?: not tracked by git yet",
        '!' => "!: ignored by git (.gitignore)",
        _ => "",
    }
}

fn rank(mark: char) -> u8 {
    match mark {
        'M' => 3,
        '?' => 2,
        _ => 1,
    }
}

fn read_branch(header: &str, status: &mut GitStatus) {
    let (names, distance) = match header.split_once(" [") {
        Some((names, rest)) => (names, rest.trim_end_matches(']')),
        None => (header, ""),
    };
    let branch = names.split("...").next().unwrap_or(names);
    status.branch = match branch.strip_prefix("No commits yet on ") {
        Some(name) => name.to_owned(),
        None if branch.starts_with("HEAD (no branch)") => "HEAD".to_owned(),
        None => branch.to_owned(),
    };
    for part in distance.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            status.ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            status.behind = n.parse().unwrap_or(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_and_branch_are_read_from_the_porcelain_output() {
        let out = b"## main...origin/main [ahead 2, behind 1]\0 M src/lib.rs\0?? notes.txt\0!! target/\0R  new.rs\0old.rs\0A  src/deep/x.rs\0";
        let root = parse(out, "");
        assert_eq!((root.branch.as_str(), root.ahead, root.behind), ("main", 2, 1));
        assert_eq!(root.summary(), "main ↑2 ↓1");
        assert_eq!(root.marks["src"], 'M', "a folder shows what is inside");
        assert_eq!(root.marks["notes.txt"], '?');
        assert_eq!(root.marks["target"], '!');
        assert_eq!(root.marks["new.rs"], 'M');
        assert!(!root.marks.contains_key("old.rs"), "the old name of a rename is not a file");
        let sub = parse(out, "src/");
        assert_eq!(sub.marks["lib.rs"], 'M');
        assert_eq!(sub.marks["deep"], 'M');
        assert!(!sub.marks.contains_key("notes.txt"));
    }

    #[test]
    fn a_new_repository_and_a_detached_head_have_names() {
        assert_eq!(parse(b"## No commits yet on main\0", "").branch, "main");
        assert_eq!(parse(b"## HEAD (no branch)\0", "").branch, "HEAD");
        assert_eq!(parse(b"## dev\0", "").summary(), "dev");
    }

    #[test]
    fn a_real_repository_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .map(|o| o.status.success())
        };
        if !matches!(run(&["init", "-q", "-b", "main"]), Ok(true)) {
            return; // no git on this machine
        }
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        let _ = run(&["add", "a.txt"]);
        let status = read(dir.path()).unwrap();
        assert_eq!(status.marks["a.txt"], 'M');
        assert_eq!(status.marks["b.txt"], '?');
        assert_eq!(status.branch, "main");
        assert!(read(std::env::temp_dir().join("pnxr-no-such-folder").as_path()).is_none());
    }
}
