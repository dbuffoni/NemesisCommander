//! Printing results as text or JSON.

use std::io::{IsTerminal, Write};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use pnxr_panel::format::{human_size, local_time, permissions};
use pnxr_vfs::{Entry, EntryKind};
use serde::Serialize;

/// JSON form of an entry (stable for scripts).
#[derive(Serialize)]
pub struct JsonEntry<'a> {
    pub name: &'a str,
    pub kind: &'static str,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<u64>,
    pub permissions: Option<String>,
    pub target: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub fn kind_name(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "file",
        EntryKind::Directory => "dir",
        EntryKind::Symlink => "link",
        EntryKind::Other => "other",
    }
}

pub fn unix_seconds(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs())
}

impl<'a> JsonEntry<'a> {
    pub fn new(entry: &'a Entry, path: Option<String>) -> Self {
        JsonEntry {
            name: &entry.name,
            kind: kind_name(entry.kind),
            size: entry.size,
            modified: unix_seconds(entry.modified),
            permissions: entry.permissions.map(|m| format!("{m:o}")),
            target: entry.symlink_target.as_deref(),
            path,
        }
    }
}

/// Writes one line to standard output.
///
/// Like `println!`, but a closed pipe (`pnxr ls | head`) ends the program
/// quietly instead of panicking, as command-line tools are expected to.
#[macro_export]
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::output::write_line(format_args!($($arg)*))
    };
}

/// Implementation of [`out!`].
pub fn write_line(args: std::fmt::Arguments<'_>) {
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = writeln!(stdout, "{args}") {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("pnxr: cannot write output: {error}");
        std::process::exit(1);
    }
}

/// Prints one JSON value per line.
pub fn json_line(value: &impl Serialize) {
    write_line(format_args!("{}", serde_json::to_string(value).expect("serializable")));
}

/// `ls -l` style line.
pub fn long_line(entry: &Entry, human: bool) -> String {
    let kind = match entry.kind {
        EntryKind::Directory => 'd',
        EntryKind::Symlink => 'l',
        EntryKind::Other => '?',
        EntryKind::File => '-',
    };
    let mode = entry.permissions.map(permissions).unwrap_or_else(|| "---------".into());
    let size = if entry.is_dir() {
        "-".to_owned()
    } else if human {
        human_size(entry.size)
    } else {
        entry.size.to_string()
    };
    let when = entry.modified.map(local_time).unwrap_or_else(|| " ".repeat(16));
    let mut line = format!("{kind}{mode} {size:>10} {when} {}", entry.name);
    if let Some(target) = &entry.symlink_target {
        line.push_str(" -> ");
        line.push_str(target);
    }
    line
}

/// A one-line progress display on stderr (only when stderr is a terminal).
pub struct Progress {
    enabled: bool,
    total: u64,
    done: u64,
    started: Instant,
    last_draw: Instant,
}

impl Progress {
    pub fn new(total: u64) -> Self {
        let now = Instant::now();
        Progress {
            enabled: std::io::stderr().is_terminal(),
            total,
            done: 0,
            started: now,
            last_draw: now,
        }
    }

    pub fn add(&mut self, bytes: u64) {
        self.done += bytes;
        if self.enabled && self.last_draw.elapsed().as_millis() >= 100 {
            self.last_draw = Instant::now();
            self.draw();
        }
    }

    fn draw(&self) {
        let percent = if self.total == 0 { 100 } else { self.done * 100 / self.total.max(1) };
        let seconds = self.started.elapsed().as_secs_f64().max(0.001);
        let rate = human_size((self.done as f64 / seconds) as u64);
        eprint!(
            "\r{percent:>3}%  {} / {}  {rate}/s   ",
            human_size(self.done),
            human_size(self.total)
        );
        let _ = std::io::stderr().flush();
    }

    pub fn finish(&self) {
        if self.enabled {
            self.draw();
            eprintln!();
        }
    }
}
