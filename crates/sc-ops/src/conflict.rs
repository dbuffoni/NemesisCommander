//! What to do when the destination of a copy or move already exists.

use async_trait::async_trait;
use sc_vfs::{Entry, VfsPath};
use serde::{Deserialize, Serialize};

/// The user's standing choice for name collisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ConflictPolicy {
    /// Ask the [`ConflictResolver`] for every conflict (the default for UIs).
    #[default]
    Ask,
    /// Replace the existing file.
    Overwrite,
    /// Replace only if the source is newer than the destination.
    OverwriteIfNewer,
    /// Replace only if sizes differ (a cheap "is it the same file?" check).
    OverwriteIfDifferentSize,
    /// Leave the existing file alone.
    Skip,
    /// Keep both: the new file gets a unique name like `report (1).pdf`.
    KeepBoth,
    /// Stop with an error.
    Fail,
}

/// A single resolved decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictAction {
    /// Replace the destination.
    Overwrite,
    /// Do not copy this item.
    Skip,
    /// Write the item under this other file name (same directory).
    RenameTo(String),
    /// Abort the whole operation.
    Abort,
}

/// Information shown to the user about a collision.
#[derive(Debug, Clone)]
pub struct Conflict {
    /// The item being copied.
    pub source: VfsPath,
    /// Its metadata.
    pub source_entry: Entry,
    /// The existing destination.
    pub destination: VfsPath,
    /// The existing destination's metadata.
    pub destination_entry: Entry,
}

/// The answer of a [`ConflictResolver`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// What to do with this conflict.
    pub action: ConflictAction,
    /// If set, apply the same choice to every later conflict of the
    /// operation without asking again ("Overwrite all", "Skip all").
    pub apply_to_all: bool,
}

/// Asks someone (usually the user, through a dialog) how to resolve a conflict.
#[async_trait]
pub trait ConflictResolver: Send + Sync {
    /// Called for each conflict while the policy is [`ConflictPolicy::Ask`].
    async fn resolve(&self, conflict: &Conflict) -> Resolution;
}

/// A resolver that always gives the same answer; handy for non-interactive
/// front ends and tests.
pub struct FixedResolver(pub ConflictAction);

#[async_trait]
impl ConflictResolver for FixedResolver {
    async fn resolve(&self, _: &Conflict) -> Resolution {
        Resolution { action: self.0.clone(), apply_to_all: false }
    }
}

impl ConflictPolicy {
    /// Decides a conflict without asking, or returns `None` if the policy is
    /// [`ConflictPolicy::Ask`]. `taken` reports whether a name already exists
    /// in the destination directory (used by [`ConflictPolicy::KeepBoth`]).
    pub fn decide(
        self,
        conflict: &Conflict,
        taken: impl Fn(&str) -> bool,
    ) -> Option<ConflictAction> {
        let src = &conflict.source_entry;
        let dst = &conflict.destination_entry;
        Some(match self {
            ConflictPolicy::Ask => return None,
            ConflictPolicy::Overwrite => ConflictAction::Overwrite,
            ConflictPolicy::Skip => ConflictAction::Skip,
            ConflictPolicy::Fail => ConflictAction::Abort,
            ConflictPolicy::OverwriteIfNewer => match (src.modified, dst.modified) {
                (Some(s), Some(d)) if s > d => ConflictAction::Overwrite,
                (Some(_), None) => ConflictAction::Overwrite,
                _ => ConflictAction::Skip,
            },
            ConflictPolicy::OverwriteIfDifferentSize => {
                if src.size != dst.size {
                    ConflictAction::Overwrite
                } else {
                    ConflictAction::Skip
                }
            }
            ConflictPolicy::KeepBoth => {
                let name = conflict.destination.file_name().unwrap_or("file");
                ConflictAction::RenameTo(unique_name(name, taken))
            }
        })
    }
}

/// Returns `name` with a ` (n)` counter before the extension, choosing the
/// smallest `n >= 1` for which `taken` is false.
///
/// ```
/// let taken = |n: &str| n == "a (1).txt";
/// assert_eq!(sc_ops::unique_name("a.txt", taken), "a (2).txt");
/// assert_eq!(sc_ops::unique_name(".bashrc", |_| false), ".bashrc (1)");
/// ```
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    // Split before the last dot, but treat a leading dot as part of the stem.
    let split = name.rfind('.').filter(|&i| i > 0);
    let (stem, ext) = match split {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    (1u64..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !taken(candidate))
        .expect("an unused name always exists")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_vfs::EntryKind;
    use std::time::{Duration, UNIX_EPOCH};

    fn conflict(src_age: Option<u64>, dst_age: Option<u64>, sizes: (u64, u64)) -> Conflict {
        let entry = |age: Option<u64>, size| {
            let mut e = Entry::new("f.txt", EntryKind::File).with_size(size);
            e.modified = age.map(|s| UNIX_EPOCH + Duration::from_secs(s));
            e
        };
        Conflict {
            source: VfsPath::parse("/a/f.txt").unwrap(),
            source_entry: entry(src_age, sizes.0),
            destination: VfsPath::parse("/b/f.txt").unwrap(),
            destination_entry: entry(dst_age, sizes.1),
        }
    }

    #[test]
    fn newer_policy_compares_times() {
        let decide = |c: &Conflict| ConflictPolicy::OverwriteIfNewer.decide(c, |_| false).unwrap();
        assert_eq!(decide(&conflict(Some(10), Some(5), (1, 1))), ConflictAction::Overwrite);
        assert_eq!(decide(&conflict(Some(5), Some(10), (1, 1))), ConflictAction::Skip);
        assert_eq!(decide(&conflict(Some(5), Some(5), (1, 1))), ConflictAction::Skip);
        assert_eq!(decide(&conflict(None, Some(5), (1, 1))), ConflictAction::Skip);
    }

    #[test]
    fn size_policy() {
        let decide =
            |c: &Conflict| ConflictPolicy::OverwriteIfDifferentSize.decide(c, |_| false).unwrap();
        assert_eq!(decide(&conflict(None, None, (1, 2))), ConflictAction::Overwrite);
        assert_eq!(decide(&conflict(None, None, (2, 2))), ConflictAction::Skip);
    }

    #[test]
    fn keep_both_finds_a_free_name() {
        let c = conflict(None, None, (1, 1));
        let action = ConflictPolicy::KeepBoth.decide(&c, |n| n == "f (1).txt").unwrap();
        assert_eq!(action, ConflictAction::RenameTo("f (2).txt".into()));
    }

    #[test]
    fn ask_defers_to_resolver() {
        assert_eq!(ConflictPolicy::Ask.decide(&conflict(None, None, (1, 1)), |_| false), None);
    }

    #[test]
    fn unique_name_shapes() {
        assert_eq!(unique_name("archive.tar.gz", |_| false), "archive.tar (1).gz");
        assert_eq!(unique_name("README", |_| false), "README (1)");
    }
}
