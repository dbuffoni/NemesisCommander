//! Ordering of directory entries.

use std::cmp::Ordering;

use nmc_vfs::Entry;
use serde::{Deserialize, Serialize};

/// The column entries are sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortKey {
    /// File name, natural order (`file2` before `file10`).
    #[default]
    Name,
    /// Extension, then name.
    Extension,
    /// Size, then name.
    Size,
    /// Modification time, then name.
    Modified,
}

/// A sort column plus direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct SortSpec {
    /// Column.
    pub key: SortKey,
    /// Largest/latest/last first.
    pub descending: bool,
}

/// Compares two strings the way people expect file names to be ordered:
/// case-insensitively, with runs of digits compared by numeric value.
///
/// ```
/// use std::cmp::Ordering;
/// assert_eq!(nmc_index::natural_cmp("file2.txt", "File10.txt"), Ordering::Less);
/// ```
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take_number = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (na, nb) = (take_number(&mut a), take_number(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                // Longer (without leading zeros) means larger; then lexicographic.
                let by_value = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if by_value != Ordering::Equal {
                    return by_value;
                }
                // "01" vs "1": fewer leading zeros first, for a stable order.
                let by_zeros = na.len().cmp(&nb.len());
                if by_zeros != Ordering::Equal {
                    return by_zeros;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Final tie-breaker so the order is total and stable across runs.
fn by_name(a: &Entry, b: &Entry) -> Ordering {
    natural_cmp(&a.name, &b.name).then_with(|| a.name.cmp(&b.name))
}

/// Sorts entries in place. Directories (and links to directories) always
/// come first, and `..` always comes before everything else.
pub fn sort_entries(entries: &mut [Entry], spec: SortSpec) {
    entries.sort_by(|a, b| {
        let rank = |e: &Entry| (e.name != "..", !e.is_dir());
        rank(a).cmp(&rank(b)).then_with(|| {
            let order = match spec.key {
                SortKey::Name => by_name(a, b),
                SortKey::Extension => a.extension().cmp(&b.extension()).then_with(|| by_name(a, b)),
                SortKey::Size => a.size.cmp(&b.size).then_with(|| by_name(a, b)),
                SortKey::Modified => a.modified.cmp(&b.modified).then_with(|| by_name(a, b)),
            };
            if spec.descending { order.reverse() } else { order }
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmc_vfs::EntryKind;
    use proptest::prelude::*;

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["a10", "a2", "A1", "a02", "b", "a1b"];
        v.sort_by(|a, b| natural_cmp(a, b).then_with(|| a.cmp(b)));
        assert_eq!(v, ["A1", "a1b", "a2", "a02", "a10", "b"]);
    }

    #[test]
    fn directories_first_and_dotdot_on_top() {
        let mut entries = vec![
            Entry::new("zeta.txt", EntryKind::File),
            Entry::new("alpha", EntryKind::Directory),
            Entry::new("..", EntryKind::Directory),
            Entry::new("beta.txt", EntryKind::File),
        ];
        sort_entries(&mut entries, SortSpec { key: SortKey::Name, descending: true });
        assert_eq!(names(&entries), ["..", "alpha", "zeta.txt", "beta.txt"]);
    }

    #[test]
    fn by_size_and_extension() {
        let mut entries = vec![
            Entry::new("b.rs", EntryKind::File).with_size(5),
            Entry::new("a.toml", EntryKind::File).with_size(1),
            Entry::new("c.md", EntryKind::File).with_size(9),
        ];
        sort_entries(&mut entries, SortSpec { key: SortKey::Size, descending: false });
        assert_eq!(names(&entries), ["a.toml", "b.rs", "c.md"]);
        sort_entries(&mut entries, SortSpec { key: SortKey::Extension, descending: false });
        assert_eq!(names(&entries), ["c.md", "b.rs", "a.toml"]);
    }

    proptest! {
        /// natural_cmp is a total order: antisymmetric and transitive.
        #[test]
        fn natural_cmp_is_consistent(a in "[a-c0-9]{0,6}", b in "[a-c0-9]{0,6}", c in "[a-c0-9]{0,6}") {
            prop_assert_eq!(natural_cmp(&a, &b), natural_cmp(&b, &a).reverse());
            if natural_cmp(&a, &b) != Ordering::Greater && natural_cmp(&b, &c) != Ordering::Greater {
                prop_assert_ne!(natural_cmp(&a, &c), Ordering::Greater);
            }
        }
    }
}
