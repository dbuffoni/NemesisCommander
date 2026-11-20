//! In-memory index of an archive's entries.

use std::collections::BTreeMap;

use nmc_vfs::{Entry, EntryKind};

/// How to find an entry's data again inside the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Locator {
    /// Position in the zip central directory.
    Zip(usize),
    /// Raw path of the member as stored (tar, 7z, rar).
    Named(String),
    /// The one entry of a single compressed file.
    Single,
    /// A directory implied by entries below it, with no data of its own.
    Synthetic,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexEntry {
    pub entry: Entry,
    pub locator: Locator,
}

/// Entries keyed by normalized absolute path (`/dir/file`).
#[derive(Debug, Default)]
pub(crate) struct ArchiveIndex {
    entries: BTreeMap<String, IndexEntry>,
    /// Some member is encrypted.
    pub locked: bool,
}

/// Turns a stored member name (`./a//b/`, `a\b`) into `/a/b`, rejecting
/// names that try to escape the archive (`../x`).
pub(crate) fn normalize_member(raw: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p => parts.push(p),
        }
    }
    if parts.is_empty() { None } else { Some(format!("/{}", parts.join("/"))) }
}

impl ArchiveIndex {
    /// Adds a member, creating synthetic parent directories as needed.
    /// Members with unsafe names are ignored. A later real directory entry
    /// replaces an earlier synthetic one.
    pub fn insert(&mut self, raw_name: &str, mut entry: Entry, locator: Locator) {
        let Some(path) = normalize_member(raw_name) else {
            return;
        };
        entry.name = path.rsplit('/').next().unwrap_or_default().to_owned();
        let mut parent = path.as_str();
        while let Some(cut) = parent.rfind('/').filter(|&i| i > 0) {
            parent = &parent[..cut];
            self.entries.entry(parent.to_owned()).or_insert_with(|| IndexEntry {
                entry: Entry::new(
                    parent.rsplit('/').next().unwrap_or_default(),
                    EntryKind::Directory,
                ),
                locator: Locator::Synthetic,
            });
        }
        match self.entries.get(&path) {
            Some(existing)
                if existing.locator != Locator::Synthetic && entry.kind == EntryKind::Directory => {
            }
            _ => {
                self.entries.insert(path, IndexEntry { entry, locator });
            }
        }
    }

    /// Looks up a path (`/` is the root and always exists).
    pub fn get(&self, path: &str) -> Option<&IndexEntry> {
        self.entries.get(path)
    }

    /// Direct children of a directory path.
    pub fn children(&self, dir: &str) -> Vec<Entry> {
        let prefix = if dir == "/" { "/".to_owned() } else { format!("{dir}/") };
        self.entries
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .filter(|(key, _)| !key[prefix.len()..].contains('/'))
            .map(|(_, item)| item.entry.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_and_safety() {
        assert_eq!(normalize_member("./a//b/").as_deref(), Some("/a/b"));
        assert_eq!(normalize_member("a\\b").as_deref(), Some("/a/b"));
        assert_eq!(normalize_member("../etc/passwd"), None);
        assert_eq!(normalize_member("a/../../x"), None);
        assert_eq!(normalize_member("./"), None);
    }

    #[test]
    fn synthesizes_parents_and_lists_children() {
        let mut index = ArchiveIndex::default();
        index.insert("x/y/z.txt", Entry::new("", EntryKind::File).with_size(3), Locator::Zip(0));
        index.insert("top.txt", Entry::new("", EntryKind::File), Locator::Zip(1));
        index.insert("x/", Entry::new("", EntryKind::Directory), Locator::Zip(2));
        assert_eq!(index.entries.len(), 4);
        assert_eq!(
            index.get("/x").unwrap().locator,
            Locator::Zip(2),
            "real dir replaces synthetic"
        );
        assert_eq!(index.get("/x/y").unwrap().locator, Locator::Synthetic);
        let names: Vec<String> = index.children("/").into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["top.txt", "x"]);
        assert_eq!(index.children("/x/y")[0].size, 3);
        assert!(index.children("/x/y/z.txt").is_empty());
    }
}
