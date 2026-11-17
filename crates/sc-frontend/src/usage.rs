//! What takes the space: the totals of a folder's items, with and without
//! their contents ("folder sizes" and "disk usage").

use std::collections::BTreeMap;

use futures::StreamExt;
use sc_panel::format::human_size;
use sc_vfs::{Entry, ProviderRegistry, VfsPath};

/// An item of a folder with everything below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    /// The item's name.
    pub name: String,
    /// Whether it is a folder.
    pub dir: bool,
    /// Bytes in it (all files below, for a folder).
    pub bytes: u64,
    /// Files and folders below it (0 for a file).
    pub items: u64,
}

/// Groups the entries below a folder (by path relative to it, `/`
/// separated) into the totals of its direct children, biggest first.
pub fn tally(entries: impl IntoIterator<Item = (String, Entry)>) -> Vec<Usage> {
    let mut children: BTreeMap<String, Usage> = BTreeMap::new();
    for (relative, entry) in entries {
        let (head, rest) = match relative.split_once('/') {
            Some((head, rest)) => (head, Some(rest)),
            None => (relative.as_str(), None),
        };
        if head.is_empty() {
            continue;
        }
        let usage = children.entry(head.to_owned()).or_insert_with(|| Usage {
            name: head.to_owned(),
            dir: rest.is_some(),
            bytes: 0,
            items: 0,
        });
        match rest {
            None => {
                usage.dir = entry.is_dir();
                if !entry.is_dir() {
                    usage.bytes += entry.size;
                }
            }
            Some(_) => {
                usage.items += 1;
                if !entry.is_dir() {
                    usage.bytes += entry.size;
                }
            }
        }
    }
    let mut list: Vec<Usage> = children.into_values().collect();
    list.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    list
}

/// Walks `folder` and totals its direct children.
pub async fn measure(registry: &ProviderRegistry, folder: &VfsPath) -> sc_vfs::Result<Vec<Usage>> {
    let provider = registry.provider_for(folder).await?;
    let mut found = Vec::new();
    let mut items = sc_vfs::walk(provider.as_ref(), folder, None);
    while let Some(item) = items.next().await {
        // What cannot be read is left out of the totals.
        let Ok(item) = item else { continue };
        if let Some(relative) = item.path.relative_to(folder) {
            found.push((relative, item.entry));
        }
    }
    Ok(tally(found))
}

/// The bytes and the number of files and folders below `path`.
pub async fn measure_total(
    registry: &ProviderRegistry,
    path: &VfsPath,
) -> sc_vfs::Result<(u64, u64)> {
    let provider = registry.provider_for(path).await?;
    let (mut bytes, mut items) = (0, 0);
    let mut walked = sc_vfs::walk(provider.as_ref(), path, None);
    while let Some(item) = walked.next().await {
        let Ok(item) = item else { continue };
        if item.path.relative_to(path).is_some_and(|r| !r.is_empty()) {
            items += 1;
            if !item.entry.is_dir() {
                bytes += item.entry.size;
            }
        }
    }
    Ok((bytes, items))
}

/// A bar `width` characters long with `part` of `whole` filled.
pub fn bar(part: u64, whole: u64, width: usize) -> String {
    let filled =
        if whole == 0 { 0 } else { ((part as f64 / whole as f64) * width as f64).round() as usize }
            .min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// The rows of the disk usage list: a bar relative to the biggest item,
/// the share of the total, the size and the name.
pub fn rows(usages: &[Usage]) -> Vec<String> {
    let biggest = usages.iter().map(|u| u.bytes).max().unwrap_or(0);
    let total: u64 = usages.iter().map(|u| u.bytes).sum();
    usages
        .iter()
        .map(|u| {
            let share = if total == 0 { 0 } else { u.bytes * 100 / total };
            format!(
                "{} {share:>3}%  {:>10}  {}{}",
                bar(u.bytes, biggest, 12),
                human_size(u.bytes),
                u.name,
                if u.dir { "/" } else { "" }
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_vfs::EntryKind;

    fn file(size: u64) -> Entry {
        Entry::new("x", EntryKind::File).with_size(size)
    }

    fn dir() -> Entry {
        Entry::new("d", EntryKind::Directory)
    }

    #[test]
    fn children_are_totalled_biggest_first() {
        let list = tally([
            ("a.txt".to_owned(), file(10)),
            ("docs".to_owned(), dir()),
            ("docs/one".to_owned(), file(100)),
            ("docs/sub".to_owned(), dir()),
            ("docs/sub/two".to_owned(), file(50)),
            ("empty".to_owned(), dir()),
        ]);
        let names: Vec<&str> = list.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["docs", "a.txt", "empty"]);
        assert_eq!((list[0].bytes, list[0].items, list[0].dir), (150, 3, true));
        assert_eq!((list[1].bytes, list[1].items, list[1].dir), (10, 0, false));
        assert_eq!((list[2].bytes, list[2].items, list[2].dir), (0, 0, true));
    }

    #[test]
    fn bars_and_rows_show_the_shares() {
        assert_eq!(bar(5, 10, 4), "██░░");
        assert_eq!(bar(0, 0, 4), "░░░░");
        assert_eq!(bar(99, 10, 4), "████", "never wider than the bar");
        let usages = vec![
            Usage { name: "big".into(), dir: true, bytes: 3 << 20, items: 4 },
            Usage { name: "small.txt".into(), dir: false, bytes: 1 << 20, items: 0 },
        ];
        let rows = rows(&usages);
        assert!(rows[0].starts_with("████████████  75%"), "{}", rows[0]);
        assert!(rows[0].ends_with("3.0 MiB  big/"), "{}", rows[0]);
        assert!(rows[1].contains(" 25%") && rows[1].ends_with("small.txt"), "{}", rows[1]);
    }
}
