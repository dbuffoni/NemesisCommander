use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// The type of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory (or an archive root shown as a directory).
    Directory,
    /// A symbolic link. See [`Entry::symlink_target`] and [`Entry::link_is_dir`].
    Symlink,
    /// Devices, sockets, FIFOs and anything else.
    Other,
}

/// Metadata about a single item inside a directory.
///
/// Fields that a provider cannot know are `None` rather than guessed, so UIs
/// can show "unknown" instead of a misleading value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The file name, without any directory component.
    pub name: String,
    /// What kind of item this is.
    pub kind: EntryKind,
    /// Size in bytes (0 for directories when unknown).
    pub size: u64,
    /// Last modification time, if known.
    pub modified: Option<SystemTime>,
    /// POSIX permission bits (`0o755` style), if the provider has them.
    pub permissions: Option<u32>,
    /// Owner user id, if known.
    pub uid: Option<u32>,
    /// Owner group id, if known.
    pub gid: Option<u32>,
    /// Target of a symbolic link, if this entry is one.
    pub symlink_target: Option<String>,
    /// For symbolic links: whether the link resolves to a directory.
    pub link_is_dir: bool,
    /// Windows file attributes (`os_flags` constants), when the provider is
    /// the local file system on Windows.
    #[serde(default)]
    pub os_flags: Option<u32>,
}

/// Bits of [`Entry::os_flags`] (the same values as the Windows
/// `FILE_ATTRIBUTE_*` constants they come from).
pub mod os_flags {
    /// Read-only.
    pub const READONLY: u32 = 0x1;
    /// Hidden.
    pub const HIDDEN: u32 = 0x2;
    /// System file.
    pub const SYSTEM: u32 = 0x4;
    /// All the bits this program reads and sets.
    pub const ALL: u32 = READONLY | HIDDEN | SYSTEM;

    /// The names of the bits set in `flags`, as shown to the user.
    pub fn describe(flags: u32) -> Vec<&'static str> {
        [(READONLY, "read-only"), (HIDDEN, "hidden"), (SYSTEM, "system")]
            .into_iter()
            .filter(|(bit, _)| flags & bit != 0)
            .map(|(_, name)| name)
            .collect()
    }
}

impl Entry {
    /// Creates an entry with only a name and a kind; every other field is
    /// empty. Handy for tests and for providers that learn metadata lazily.
    pub fn new(name: impl Into<String>, kind: EntryKind) -> Self {
        Entry {
            name: name.into(),
            kind,
            size: 0,
            modified: None,
            permissions: None,
            uid: None,
            gid: None,
            symlink_target: None,
            link_is_dir: false,
            os_flags: None,
        }
    }

    /// Builder-style helper that sets [`Entry::size`].
    pub fn with_size(mut self, size: u64) -> Self {
        self.size = size;
        self
    }

    /// Builder-style helper that sets [`Entry::modified`].
    pub fn with_modified(mut self, modified: SystemTime) -> Self {
        self.modified = Some(modified);
        self
    }

    /// `true` for directories and for symlinks pointing at directories:
    /// anything a user can "enter".
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory || (self.kind == EntryKind::Symlink && self.link_is_dir)
    }

    /// `true` for regular files.
    pub fn is_file(&self) -> bool {
        self.kind == EntryKind::File
    }

    /// Unix convention: names starting with a dot are hidden.
    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.') && self.name != ".."
    }

    /// The extension after the last dot, lowercased, without the dot.
    /// Hidden files like `.bashrc` have no extension.
    pub fn extension(&self) -> Option<String> {
        self.extension_str().map(str::to_ascii_lowercase)
    }

    /// The extension as written (no allocation, for sorting).
    pub fn extension_str(&self) -> Option<&str> {
        let stem_start = usize::from(self.name.starts_with('.'));
        let name = &self.name[stem_start..];
        name.rfind('.').filter(|&i| i + 1 < name.len()).map(|i| &name[i + 1..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_rules() {
        assert_eq!(Entry::new("a.TXT", EntryKind::File).extension().as_deref(), Some("txt"));
        assert_eq!(Entry::new("a.tar.gz", EntryKind::File).extension().as_deref(), Some("gz"));
        assert_eq!(Entry::new(".bashrc", EntryKind::File).extension(), None);
        assert_eq!(
            Entry::new(".config.toml", EntryKind::File).extension().as_deref(),
            Some("toml")
        );
        assert_eq!(Entry::new("README", EntryKind::File).extension(), None);
        assert_eq!(Entry::new("trailing.", EntryKind::File).extension(), None);
    }

    #[test]
    fn symlink_to_dir_counts_as_dir() {
        let mut e = Entry::new("link", EntryKind::Symlink);
        assert!(!e.is_dir());
        e.link_is_dir = true;
        assert!(e.is_dir());
    }

    #[test]
    fn hidden_files() {
        assert!(Entry::new(".git", EntryKind::Directory).is_hidden());
        assert!(!Entry::new("..", EntryKind::Directory).is_hidden());
        assert!(!Entry::new("src", EntryKind::Directory).is_hidden());
    }
}
