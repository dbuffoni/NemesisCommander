//! Which folders were opened, how often and how recently ("frecency"), for
//! the jump command. Locations are kept as VFS URIs, so remote folders
//! count too.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Folders kept; the least interesting ones are dropped beyond this.
const MAX_ENTRIES: usize = 500;

/// One folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    /// The location as a VFS URI.
    pub path: String,
    /// How many times it was opened.
    pub visits: u32,
    /// When it was last opened (seconds since 1970).
    pub last: u64,
}

impl Visit {
    /// How interesting the folder is `now`: visits, weighted by how long ago
    /// the last one was.
    pub fn score(&self, now: u64) -> f64 {
        let age = now.saturating_sub(self.last);
        let weight = match age {
            0..=3_599 => 4.0,
            3_600..=86_399 => 2.0,
            86_400..=604_799 => 0.5,
            _ => 0.25,
        };
        f64::from(self.visits) * weight
    }
}

/// The remembered folders.
#[derive(Debug, Default)]
pub struct Frecency {
    visits: Vec<Visit>,
    file: Option<PathBuf>,
}

impl Frecency {
    /// Loads `file` (empty when it is missing or unreadable); changes are
    /// saved there.
    pub fn load(file: &Path) -> Self {
        let visits = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Frecency { visits, file: Some(file.to_owned()) }
    }

    /// An empty list that is never saved (tests).
    pub fn in_memory() -> Self {
        Frecency::default()
    }

    /// Notes that `location` was opened at `now`.
    pub fn record(&mut self, location: &str, now: u64) {
        match self.visits.iter_mut().find(|v| v.path == location) {
            Some(visit) => {
                visit.visits += 1;
                visit.last = now;
            }
            None => self.visits.push(Visit { path: location.to_owned(), visits: 1, last: now }),
        }
        if self.visits.len() > MAX_ENTRIES {
            self.visits.sort_by(|a, b| b.score(now).total_cmp(&a.score(now)));
            self.visits.truncate(MAX_ENTRIES);
        }
        self.save();
    }

    /// Forgets a folder (it was deleted).
    pub fn forget(&mut self, location: &str) {
        self.visits.retain(|v| v.path != location);
        self.save();
    }

    /// The most recently opened folders whose location holds `query`
    /// (case does not matter), newest first.
    pub fn recent(&self, query: &str, limit: usize) -> Vec<&Visit> {
        let needle = query.to_lowercase();
        let mut found: Vec<&Visit> =
            self.visits.iter().filter(|v| v.path.to_lowercase().contains(&needle)).collect();
        found.sort_by(|a, b| b.last.cmp(&a.last).then(b.visits.cmp(&a.visits)));
        found.truncate(limit);
        found
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string(&self.visits) {
            let _ = std::fs::write(file, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_folders_match_the_text_and_come_newest_first() {
        let mut f = Frecency::in_memory();
        f.record("file:///home/ada/projects/App", 10);
        f.record("file:///home/ada/projects/app-docs", 30);
        f.record("file:///srv/app", 20);
        f.record("file:///srv/other", 40);
        let found: Vec<&str> = f.recent("APP", 2).iter().map(|v| v.path.as_str()).collect();
        assert_eq!(found, ["file:///home/ada/projects/app-docs", "file:///srv/app"]);
        assert_eq!(f.recent("", 10).len(), 4, "no text: everything");
        assert!(f.recent("nothing", 10).is_empty());
    }

    #[test]
    fn the_list_is_saved_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/frecency.json");
        let mut f = Frecency::load(&file);
        f.record("file:///a", 5);
        f.record("file:///a", 6);
        let again = Frecency::load(&file);
        assert_eq!(again.recent("a", 5)[0].visits, 2);
        let mut f = Frecency::in_memory();
        for i in 0..MAX_ENTRIES + 20 {
            f.record(&format!("file:///d{i}"), 5);
        }
        assert_eq!(f.visits.len(), MAX_ENTRIES);
        f.forget("file:///d0");
        assert!(f.recent("d0", 5).iter().all(|v| v.path != "file:///d0"));
    }
}
