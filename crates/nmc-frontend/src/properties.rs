//! The Properties dialog: one dialog that shows what is known about the
//! selected items (read-only) and, on a key, lets the user change the dates
//! and permissions with assisted controls (a check box for each permission
//! with the octal value kept in step, a validated date, "recurse into
//! folders"). It replaces the old pair "Properties" and "Change dates and
//! permissions".
//!
//! This module only holds the state and builds the rows; the keys and the
//! work are in `app/properties.rs`.

use nmc_panel::format;
use nmc_vfs::{Entry, EntryKind, VfsPath};

/// What a row of the dialog is, in edit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropRow {
    /// A line of information.
    Info,
    /// The date of the last change.
    Modified,
    /// One permission bit (the mask).
    Bit(u32),
    /// The permissions as an octal number.
    Octal,
    /// Also change what is inside folders.
    Recursive,
    /// Do the change.
    Apply,
}

/// The state of the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropsState {
    /// The items the dialog is about.
    pub targets: Vec<VfsPath>,
    /// Their entries, in the same order.
    pub entries: Vec<Entry>,
    /// The location shown for them.
    pub location: String,
    /// Editing is on.
    pub edit: bool,
    /// The permissions as they are now (the first item's, when they differ).
    pub original_mode: Option<u32>,
    /// The permissions to set (`None`: left alone).
    pub mode: Option<u32>,
    /// The items do not all have the same permissions.
    pub modes_vary: bool,
    /// The date to set, as typed (`None`: left alone).
    pub modified: Option<String>,
    /// Also change the content of folders.
    pub recursive: bool,
    /// The rows of the last [`Self::items`], to know what Enter means.
    pub rows: Vec<PropRow>,
}

/// The permission bits as rows: who, what, mask.
const BITS: [(&str, &str, u32); 12] = [
    ("Owner", "read", 0o400),
    ("Owner", "write", 0o200),
    ("Owner", "execute", 0o100),
    ("Group", "read", 0o040),
    ("Group", "write", 0o020),
    ("Group", "execute", 0o010),
    ("Others", "read", 0o004),
    ("Others", "write", 0o002),
    ("Others", "execute", 0o001),
    ("Special", "run as the owner (setuid)", 0o4000),
    ("Special", "run as the group (setgid)", 0o2000),
    ("Special", "only the owner can delete here (sticky)", 0o1000),
];

impl PropsState {
    /// The dialog for `entries` (found at `targets`, in `location`).
    pub fn new(targets: Vec<VfsPath>, entries: Vec<Entry>, location: String) -> Self {
        let modes: Vec<u32> = entries.iter().filter_map(|e| e.permissions).collect();
        let original_mode = modes.first().map(|m| m & 0o7777);
        let modes_vary = modes.iter().any(|m| Some(m & 0o7777) != original_mode);
        PropsState {
            targets,
            entries,
            location,
            edit: false,
            original_mode,
            mode: None,
            modes_vary,
            modified: None,
            recursive: false,
            rows: Vec::new(),
        }
    }

    /// Whether any item is a folder.
    pub fn has_folders(&self) -> bool {
        self.entries.iter().any(Entry::is_dir)
    }

    /// The permissions now shown: the new ones, else the old ones.
    pub fn shown_mode(&self) -> Option<u32> {
        self.mode.or(self.original_mode)
    }

    /// Switches one permission bit.
    pub fn toggle_bit(&mut self, mask: u32) {
        let Some(mode) = self.shown_mode() else { return };
        self.mode = Some(mode ^ mask);
    }

    /// Sets the permissions from an octal number (`644`, `0755`, `4755`).
    pub fn set_octal(&mut self, text: &str) -> Result<(), String> {
        let mode = u32::from_str_radix(text.trim(), 8)
            .ok()
            .filter(|m| *m <= 0o7777)
            .ok_or_else(|| format!("Not an octal mode: {}", text.trim()))?;
        self.mode = Some(mode);
        Ok(())
    }

    /// Sets the date to give: `now`, `2026-11-17`, `10:30`, `2026-11-17 10:30`.
    pub fn set_modified(&mut self, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            self.modified = None;
            return Ok(());
        }
        nmc_ops::parse_change(text, std::time::SystemTime::now())?;
        self.modified = Some(text.to_owned());
        Ok(())
    }

    /// The text for [`nmc_ops::parse_change`] that makes the change, or
    /// `None` when nothing was changed.
    pub fn change_text(&self) -> Option<String> {
        let mut words = Vec::new();
        if let Some(modified) = &self.modified {
            words.push(modified.clone());
        }
        if let Some(mode) = self.mode.filter(|m| Some(*m) != self.original_mode) {
            words.push(format!("mode={mode:o}"));
        }
        if words.is_empty() {
            return None;
        }
        if self.recursive {
            words.push("-r".to_owned());
        }
        Some(words.join(" "))
    }

    /// The rows to show, and what each one is.
    pub fn items(&mut self) -> Vec<String> {
        let mut rows = Vec::new();
        let mut items = Vec::new();
        let mut add = |row: PropRow, text: String| {
            rows.push(row);
            items.push(text);
        };
        let one = self.entries.len() == 1;
        if one {
            let e = &self.entries[0];
            let kind = match e.kind {
                EntryKind::File => "file",
                EntryKind::Directory => "folder",
                EntryKind::Symlink => "symbolic link",
                EntryKind::Other => "special file",
            };
            add(PropRow::Info, format!("Name:         {}", e.name));
            add(PropRow::Info, format!("Location:     {}", self.location));
            add(PropRow::Info, format!("Type:         {kind}"));
            if !e.is_dir() {
                add(
                    PropRow::Info,
                    format!("Size:         {} ({} bytes)", format::human_size(e.size), e.size),
                );
            }
            if let Some(target) = &e.symlink_target {
                add(PropRow::Info, format!("Points to:    {target}"));
            }
        } else {
            let bytes: u64 = self.entries.iter().filter(|e| !e.is_dir()).map(|e| e.size).sum();
            let folders = self.entries.iter().filter(|e| e.is_dir()).count();
            add(
                PropRow::Info,
                format!(
                    "Items:        {} ({} folders), {} in files",
                    self.entries.len(),
                    folders,
                    format::human_size(bytes)
                ),
            );
            add(PropRow::Info, format!("Location:     {}", self.location));
        }
        // The date.
        let now_text = {
            let times: Vec<_> = self.entries.iter().filter_map(|e| e.modified).collect();
            match times.first() {
                None => "unknown".to_owned(),
                Some(first) => {
                    let text = format::local_time(*first);
                    if times.iter().any(|t| format::local_time(*t) != text) {
                        "(varies)".to_owned()
                    } else {
                        text
                    }
                }
            }
        };
        match (&self.modified, self.edit) {
            (Some(new), true) => {
                add(PropRow::Modified, format!("Modified:     {now_text}  ->  {new}"));
            }
            (None, true) => add(
                PropRow::Modified,
                format!("Modified:     {now_text}   (Enter: change: now, 2026-11-17, 10:30)"),
            ),
            (_, false) => add(PropRow::Info, format!("Modified:     {now_text}")),
        }
        // The permissions.
        if let Some(mode) = self.shown_mode() {
            let changed = if self.mode.is_some() && self.mode != self.original_mode {
                "  (changed)"
            } else {
                ""
            };
            let varies = if self.modes_vary && self.mode.is_none() {
                "  (differs between items; the shown ones are the first's)"
            } else {
                ""
            };
            add(
                if self.edit { PropRow::Octal } else { PropRow::Info },
                format!(
                    "Permissions:  {} ({mode:04o}){changed}{varies}",
                    format::permissions(mode)
                ),
            );
            if self.edit {
                for (who, what, mask) in BITS {
                    let on = if mode & mask != 0 { "[x]" } else { "[ ]" };
                    add(PropRow::Bit(mask), format!("    {on} {who}: {what}"));
                }
            }
        }
        // Owner and group, when known.
        if one {
            let e = &self.entries[0];
            match (e.uid, e.gid) {
                (Some(uid), Some(gid)) => {
                    add(
                        PropRow::Info,
                        format!("Owner:        user {uid}, group {gid} (not changed here)"),
                    );
                }
                _ => {}
            }
            if let Some(flags) = e.os_flags {
                let names = nmc_vfs::os_flags::describe(flags);
                let shown = if names.is_empty() { "none".to_owned() } else { names.join(", ") };
                add(PropRow::Info, format!("Attributes:   {shown}"));
            }
        }
        if self.edit {
            if self.has_folders() {
                let on = if self.recursive { "[x]" } else { "[ ]" };
                add(PropRow::Recursive, format!("{on} Also change everything inside the folders"));
            }
            let text = match self.change_text() {
                Some(change) => format!(">> Apply: {change}"),
                None => ">> Apply (nothing is changed yet)".to_owned(),
            };
            add(PropRow::Apply, text);
        }
        self.rows = rows;
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, mode: u32) -> Entry {
        Entry {
            name: name.into(),
            kind: EntryKind::File,
            size: 10,
            modified: None,
            permissions: Some(mode),
            uid: Some(1000),
            gid: Some(1000),
            symlink_target: None,
            link_is_dir: false,
            os_flags: None,
        }
    }

    fn state(modes: &[u32]) -> PropsState {
        let entries: Vec<Entry> =
            modes.iter().enumerate().map(|(i, m)| entry(&format!("f{i}"), *m)).collect();
        let targets =
            entries.iter().map(|e| VfsPath::parse(&format!("/tmp/{}", e.name)).unwrap()).collect();
        PropsState::new(targets, entries, "/tmp".into())
    }

    #[test]
    fn it_opens_read_only_and_shows_no_edit_rows() {
        let mut s = state(&[0o100644]);
        let items = s.items();
        assert!(items.iter().any(|i| i.starts_with("Permissions:") && i.contains("rw-r--r--")));
        assert!(s.rows.iter().all(|r| *r == PropRow::Info));
        assert_eq!(s.change_text(), None);
    }

    #[test]
    fn check_boxes_and_octal_stay_in_step() {
        let mut s = state(&[0o100644]);
        s.edit = true;
        s.toggle_bit(0o100);
        assert_eq!(s.shown_mode(), Some(0o744));
        assert_eq!(s.change_text().as_deref(), Some("mode=744"));
        let items = s.items();
        assert!(items.iter().any(|i| i.contains("[x] Owner: execute")));
        s.set_octal("0755").unwrap();
        assert_eq!(s.shown_mode(), Some(0o755));
        assert!(s.set_octal("999").is_err());
        assert!(s.set_octal("17777").is_err());
        // Back to the original: nothing to change.
        s.set_octal("644").unwrap();
        assert_eq!(s.change_text(), None);
    }

    #[test]
    fn dates_are_checked_and_folders_can_be_recursed() {
        let mut s = state(&[0o100644]);
        s.edit = true;
        assert!(s.set_modified("not a date").is_err());
        s.set_modified("2026-11-17 10:30").unwrap();
        s.recursive = true;
        assert_eq!(s.change_text().as_deref(), Some("2026-11-17 10:30 -r"));
        s.set_modified("").unwrap();
        assert_eq!(s.change_text(), None);
    }

    #[test]
    fn several_items_say_when_their_permissions_differ() {
        let mut s = state(&[0o100644, 0o100600]);
        assert!(s.modes_vary);
        let items = s.items();
        assert!(items[0].starts_with("Items:"));
        assert!(items.iter().any(|i| i.contains("differs")));
    }
}
