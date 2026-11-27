//! A small file that remembers, for each server, what worked: the number of
//! files at once a copy ended with and the limit learned from errors, so a
//! known server is used well from the first file (ADR 0044).
//!
//! It stays small and tidy on its own:
//! - entries of a saved connection are dropped when the connection is deleted,
//!   renamed or changed to another address, and when a new connection takes
//!   its name (the old numbers belonged to something else);
//! - entries not used for a long time are dropped (shorter for servers that
//!   were never saved as a connection), and there are never more than
//!   [`MAX_ENTRIES`];
//! - the user can turn it off (the file is deleted) or reset it in Settings.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The most servers remembered.
pub const MAX_ENTRIES: usize = 200;
/// Seconds after which an unused entry of a saved connection is dropped.
pub const KEEP_SAVED: u64 = 180 * 86_400;
/// Seconds after which an unused entry of any other server is dropped.
pub const KEEP_OTHER: u64 = 30 * 86_400;

/// The saved connection an entry belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    /// Its name in the drive list.
    pub name: String,
    /// Its address.
    pub uri: String,
}

/// What is known about one server.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ServerStats {
    /// The saved connection it was recorded for, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    /// The number of files at once a copy ended with at its best.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub good_level: Option<usize>,
    /// The most files at once it took, learned from errors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Copies that contributed.
    #[serde(default)]
    pub copies: u32,
    /// When it was last used (seconds since 1970).
    #[serde(default)]
    pub updated: u64,
}

/// The contents of the file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StatsData {
    /// Format version.
    #[serde(default)]
    pub version: u32,
    /// By server (`scheme://authority`).
    #[serde(default)]
    pub servers: BTreeMap<String, ServerStats>,
}

/// The file and what is in memory.
#[derive(Debug)]
pub struct StatsStore {
    path: Option<PathBuf>,
    enabled: bool,
    /// What is remembered.
    pub data: StatsData,
}

impl StatsStore {
    /// A store that remembers nothing and writes nothing.
    pub fn disabled() -> Self {
        StatsStore { path: None, enabled: false, data: StatsData::default() }
    }

    /// Reads the file (a missing or broken one counts as empty) and drops
    /// what is old.
    pub fn load(path: PathBuf, enabled: bool, now: u64) -> Self {
        let data = if enabled {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default()
        } else {
            StatsData::default()
        };
        let mut store = StatsStore { path: Some(path), enabled, data };
        if enabled && store.prune(now) {
            store.save();
        }
        store
    }

    /// Whether anything is remembered and written.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Turns it on, or off: off forgets everything and deletes the file.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.data = StatsData::default();
            if let Some(path) = &self.path {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Forgets everything (the file is rewritten empty, that is, removed).
    pub fn reset(&mut self) {
        self.data = StatsData::default();
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Records a result for `server`, which belongs to `owner` when it is a
    /// saved connection. `good_level` is what the last copy ended with.
    pub fn record(
        &mut self,
        server: &str,
        owner: Option<Owner>,
        good_level: Option<usize>,
        limit: Option<usize>,
        now: u64,
    ) {
        if !self.enabled {
            return;
        }
        let entry = self.data.servers.entry(server.to_owned()).or_default();
        if entry.owner != owner {
            *entry = ServerStats { owner, ..Default::default() };
        }
        if good_level.is_some() {
            entry.good_level = good_level;
            entry.copies = entry.copies.saturating_add(1);
        }
        if limit.is_some() {
            entry.limit = limit;
        }
        entry.updated = now;
        self.prune(now);
        self.save();
    }

    /// What is known about `server`.
    pub fn get(&self, server: &str) -> Option<&ServerStats> {
        self.enabled.then(|| self.data.servers.get(server)).flatten()
    }

    /// Drops what no longer belongs: entries of connections that are not in
    /// `known` (name and address) any more. Returns whether something went.
    pub fn reconcile(&mut self, known: &[Owner]) -> bool {
        let before = self.data.servers.len();
        self.data.servers.retain(|_, s| s.owner.as_ref().is_none_or(|o| known.contains(o)));
        let changed = self.data.servers.len() != before;
        if changed {
            self.save();
        }
        changed
    }

    /// Forgets what was recorded for the connection called `name`: a new
    /// connection with that name must not inherit the old one's numbers.
    pub fn forget_name(&mut self, name: &str) {
        let before = self.data.servers.len();
        self.data.servers.retain(|_, s| s.owner.as_ref().is_none_or(|o| o.name != name));
        if self.data.servers.len() != before {
            self.save();
        }
    }

    /// Drops old entries and keeps at most [`MAX_ENTRIES`]. Returns whether
    /// something went.
    pub fn prune(&mut self, now: u64) -> bool {
        let before = self.data.servers.len();
        self.data.servers.retain(|_, s| {
            let keep = if s.owner.is_some() { KEEP_SAVED } else { KEEP_OTHER };
            now.saturating_sub(s.updated) <= keep
        });
        if self.data.servers.len() > MAX_ENTRIES {
            let mut by_age: Vec<(u64, String)> =
                self.data.servers.iter().map(|(k, s)| (s.updated, k.clone())).collect();
            by_age.sort();
            let extra = self.data.servers.len() - MAX_ENTRIES;
            for (_, key) in by_age.into_iter().take(extra) {
                self.data.servers.remove(&key);
            }
        }
        self.data.servers.len() != before
    }

    /// Writes the file (through a temporary file, so it is never half
    /// written); an empty store has no file.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        if !self.enabled || self.data.servers.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        let mut data = self.data.clone();
        data.version = 1;
        let Ok(text) = serde_json::to_string_pretty(&data) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, text).is_ok() && std::fs::rename(&temp, path).is_err() {
            let _ = std::fs::remove_file(&temp);
        }
    }
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(name: &str, uri: &str) -> Option<Owner> {
        Some(Owner { name: name.into(), uri: uri.into() })
    }

    fn store(dir: &tempfile::TempDir) -> StatsStore {
        StatsStore::load(dir.path().join("connection-stats.json"), true, 1000)
    }

    #[test]
    fn remembers_across_runs() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        s.record("sftp://nas", owner("nas", "sftp://nas/"), Some(5), Some(3), 1000);
        let again = store(&dir);
        let nas = again.get("sftp://nas").unwrap();
        assert_eq!((nas.good_level, nas.limit, nas.copies), (Some(5), Some(3), 1));
    }

    #[test]
    fn deleted_renamed_or_moved_connections_are_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        s.record("sftp://nas", owner("nas", "sftp://nas/"), Some(5), None, 1000);
        s.record("sftp://box", owner("box", "sftp://box/"), Some(2), None, 1000);
        s.record("sftp://typed", None, Some(4), None, 1000);
        // "box" is gone, "nas" is still there
        assert!(s.reconcile(&[Owner { name: "nas".into(), uri: "sftp://nas/".into() }]));
        assert!(s.get("sftp://nas").is_some());
        assert!(s.get("sftp://box").is_none());
        assert!(
            s.get("sftp://typed").is_some(),
            "servers typed by hand are not tied to a connection"
        );
        // "nas" now points elsewhere
        assert!(s.reconcile(&[Owner { name: "nas".into(), uri: "sftp://other/".into() }]));
        assert!(s.get("sftp://nas").is_none());
    }

    #[test]
    fn a_new_connection_with_an_old_name_starts_clean() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        s.record("sftp://nas", owner("nas", "sftp://nas/"), Some(5), Some(3), 1000);
        s.forget_name("nas");
        assert!(s.get("sftp://nas").is_none());
        assert!(!dir.path().join("connection-stats.json").exists(), "an empty store has no file");
    }

    #[test]
    fn the_same_server_under_a_new_owner_does_not_keep_old_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        s.record("sftp://nas", owner("old", "sftp://nas/"), Some(5), Some(3), 1000);
        s.record("sftp://nas", owner("new", "sftp://nas/"), Some(2), None, 1001);
        let nas = s.get("sftp://nas").unwrap();
        assert_eq!((nas.good_level, nas.limit), (Some(2), None));
    }

    #[test]
    fn old_entries_go_by_themselves() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        s.record("sftp://a", owner("a", "sftp://a/"), Some(2), None, 1000);
        s.record("sftp://b", None, Some(2), None, 1000);
        assert!(!s.prune(1000 + KEEP_OTHER - 1));
        assert!(s.prune(1000 + KEEP_OTHER + 1));
        assert!(s.get("sftp://a").is_some() && s.get("sftp://b").is_none());
        assert!(s.prune(1000 + KEEP_SAVED + 1));
        assert!(s.get("sftp://a").is_none());
    }

    #[test]
    fn never_more_than_the_maximum() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        for i in 0..MAX_ENTRIES + 20 {
            s.record(&format!("sftp://h{i}"), None, Some(2), None, 1000 + i as u64);
        }
        assert_eq!(s.data.servers.len(), MAX_ENTRIES);
        assert!(
            s.get("sftp://h0").is_none()
                && s.get(&format!("sftp://h{}", MAX_ENTRIES + 19)).is_some()
        );
    }

    #[test]
    fn off_deletes_the_file_and_remembers_nothing_and_reset_empties_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("connection-stats.json");
        let mut s = store(&dir);
        s.record("sftp://a", None, Some(2), None, 1000);
        assert!(file.exists());
        s.set_enabled(false);
        assert!(!file.exists());
        s.record("sftp://a", None, Some(2), None, 1000);
        assert!(!file.exists() && s.get("sftp://a").is_none());
        s.set_enabled(true);
        s.record("sftp://a", None, Some(2), None, 1000);
        assert!(file.exists());
        s.reset();
        assert!(!file.exists() && s.get("sftp://a").is_none());
    }

    #[test]
    fn a_broken_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("connection-stats.json"), "not json").unwrap();
        assert!(store(&dir).data.servers.is_empty());
    }
}
