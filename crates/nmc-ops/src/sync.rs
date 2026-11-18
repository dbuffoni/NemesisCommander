//! Folder synchronization: compare two trees and list what has to be done
//! to make them agree, before anything is changed.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use futures::StreamExt;
use nmc_vfs::{Entry, EntryKind, ProviderRegistry, Result, VfsPath, walk};

/// File systems differ in how exactly they store times (FAT: two seconds).
const TIME_SLACK: Duration = Duration::from_secs(2);

/// How the two folders are made to agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// The right folder becomes a copy of the left one: new and changed
    /// files are copied, files only on the right are deleted.
    Mirror,
    /// New files, and files newer on the left, are copied to the right.
    /// Nothing is deleted and newer files on the right are kept.
    Update,
    /// Both folders get what only the other has, and the newer copy of a
    /// file wins. Files changed on both sides without a newer one are
    /// reported, not touched.
    TwoWay,
}

/// What happens to one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    /// Copy from the left folder to the right one.
    CopyToRight,
    /// Copy from the right folder to the left one.
    CopyToLeft,
    /// Delete from the right folder.
    DeleteRight,
    /// Not decided: the two differ and neither is newer, or a file faces a
    /// folder. Nothing is done.
    Conflict,
}

/// One line of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncItem {
    /// What to do.
    pub action: SyncAction,
    /// Path below the two folders, with `/`.
    pub path: String,
    /// A folder (copied or deleted with everything inside).
    pub is_dir: bool,
    /// Size of the file that is copied or deleted.
    pub size: u64,
    /// The copy replaces a file that is already there.
    pub replaces: bool,
}

/// Everything a synchronization would do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    /// The changes, by path.
    pub items: Vec<SyncItem>,
    /// Files that already agree.
    pub unchanged: usize,
}

impl SyncPlan {
    /// How many items have `action`.
    pub fn count(&self, action: SyncAction) -> usize {
        self.items.iter().filter(|i| i.action == action).count()
    }

    /// Whether running it changes anything.
    pub fn has_work(&self) -> bool {
        self.items.iter().any(|i| i.action != SyncAction::Conflict)
    }
}

/// A tree: relative path to entry.
pub type Tree = BTreeMap<String, Entry>;

/// The files and folders below `root`, by path relative to it (with `/`).
pub async fn list_tree(registry: &ProviderRegistry, root: &VfsPath) -> Result<Tree> {
    let provider = registry.provider_for(root).await?;
    let mut out = Tree::new();
    let mut items = walk(provider.as_ref(), root, None);
    while let Some(item) = items.next().await {
        // Unreadable sub-folders are left out of the comparison; leaving them
        // out of a mirror's deletions is the safe way to fail.
        let Ok(item) = item else { continue };
        if let Some(relative) = item.path.relative_to(root) {
            out.insert(relative, item.entry);
        }
    }
    Ok(out)
}

fn is_folder(entry: &Entry) -> bool {
    entry.kind == EntryKind::Directory
}

/// How two copies of a file compare.
#[derive(PartialEq)]
enum Age {
    Same,
    LeftNewer,
    RightNewer,
    /// Different content (size) with no telling which is newer.
    Unknown,
}

fn compare(left: &Entry, right: &Entry) -> Age {
    let newer = |a: SystemTime, b: SystemTime| a > b + TIME_SLACK;
    match (left.modified, right.modified) {
        (Some(l), Some(r)) if newer(l, r) => Age::LeftNewer,
        (Some(l), Some(r)) if newer(r, l) => Age::RightNewer,
        _ if left.size == right.size => Age::Same,
        _ => Age::Unknown,
    }
}

/// Compares the folders `left` and `right` and lists what `mode` would do.
pub async fn plan_sync(
    registry: &ProviderRegistry,
    left: &VfsPath,
    right: &VfsPath,
    mode: SyncMode,
) -> Result<SyncPlan> {
    let (a, b) = (list_tree(registry, left).await?, list_tree(registry, right).await?);
    let mut plan = SyncPlan::default();
    // Folders already handled as a whole: what is inside them is not listed.
    let mut covered: Vec<String> = Vec::new();
    let mut paths: Vec<&String> = a.keys().chain(b.keys()).collect();
    paths.sort();
    paths.dedup();
    for path in paths {
        if covered.iter().any(|c| path.starts_with(&format!("{c}/"))) {
            continue;
        }
        let item = |action, entry: &Entry, replaces| SyncItem {
            action,
            path: path.clone(),
            is_dir: is_folder(entry),
            size: if is_folder(entry) { 0 } else { entry.size },
            replaces,
        };
        match (a.get(path), b.get(path)) {
            (Some(l), None) => {
                covered.push(path.clone());
                plan.items.push(item(SyncAction::CopyToRight, l, false));
            }
            (None, Some(r)) => match mode {
                SyncMode::Update => {}
                SyncMode::Mirror => {
                    covered.push(path.clone());
                    plan.items.push(item(SyncAction::DeleteRight, r, false));
                }
                SyncMode::TwoWay => {
                    covered.push(path.clone());
                    plan.items.push(item(SyncAction::CopyToLeft, r, false));
                }
            },
            (Some(l), Some(r)) if is_folder(l) && is_folder(r) => {}
            (Some(l), Some(r)) if is_folder(l) != is_folder(r) => {
                covered.push(path.clone());
                plan.items.push(item(SyncAction::Conflict, l, true));
            }
            (Some(l), Some(r)) => match (compare(l, r), mode) {
                (Age::Same, _) => plan.unchanged += 1,
                (Age::LeftNewer, _) => plan.items.push(item(SyncAction::CopyToRight, l, true)),
                (Age::RightNewer, SyncMode::Update) => plan.unchanged += 1,
                (Age::RightNewer, SyncMode::Mirror) => {
                    plan.items.push(item(SyncAction::CopyToRight, l, true));
                }
                (Age::RightNewer, SyncMode::TwoWay) => {
                    plan.items.push(item(SyncAction::CopyToLeft, r, true));
                }
                (Age::Unknown, SyncMode::Mirror) => {
                    plan.items.push(item(SyncAction::CopyToRight, l, true));
                }
                (Age::Unknown, _) => plan.items.push(item(SyncAction::Conflict, l, true)),
            },
            (None, None) => {}
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::sync::Arc;

    use nmc_providers::local::LocalProvider;

    use super::*;

    struct Fixture {
        dir: tempfile::TempDir,
        registry: ProviderRegistry,
    }

    impl Fixture {
        fn new() -> Self {
            let registry = ProviderRegistry::new();
            registry.insert(&VfsPath::local_root(), Arc::new(LocalProvider::new()));
            Fixture { dir: tempfile::tempdir().unwrap(), registry }
        }

        /// Writes `text` to `side/name`, `age` seconds before a fixed time.
        fn put(&self, side: &str, name: &str, text: &str, age: u64) {
            let path = self.dir.path().join(side).join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age);
            File::options().write(true).open(&path).unwrap().set_modified(time).unwrap();
        }

        async fn plan(&self, mode: SyncMode) -> SyncPlan {
            let side = |name: &str| {
                let path = self.dir.path().join(name);
                std::fs::create_dir_all(&path).unwrap();
                VfsPath::from_local(path).unwrap()
            };
            plan_sync(&self.registry, &side("left"), &side("right"), mode).await.unwrap()
        }
    }

    fn summary(plan: &SyncPlan) -> Vec<String> {
        let letter = |a| match a {
            SyncAction::CopyToRight => "→",
            SyncAction::CopyToLeft => "←",
            SyncAction::DeleteRight => "✗",
            SyncAction::Conflict => "?",
        };
        plan.items.iter().map(|i| format!("{}{}", letter(i.action), i.path)).collect()
    }

    fn mixed() -> Fixture {
        let f = Fixture::new();
        f.put("left", "same.txt", "same", 100);
        f.put("right", "same.txt", "same", 100);
        f.put("left", "new.txt", "new", 10);
        f.put("left", "newer-left.txt", "left", 10);
        f.put("right", "newer-left.txt", "old", 500);
        f.put("left", "newer-right.txt", "old", 500);
        f.put("right", "newer-right.txt", "right", 10);
        f.put("right", "extra.txt", "extra", 10);
        f.put("left", "dir/inside.txt", "x", 10);
        f.put("right", "gone/inside.txt", "x", 10);
        f
    }

    #[tokio::test]
    async fn mirror_makes_the_right_side_a_copy() {
        let plan = mixed().plan(SyncMode::Mirror).await;
        assert_eq!(
            summary(&plan),
            ["→dir", "✗extra.txt", "✗gone", "→new.txt", "→newer-left.txt", "→newer-right.txt"]
        );
        assert_eq!(plan.unchanged, 1);
        assert!(plan.items.iter().find(|i| i.path == "dir").unwrap().is_dir);
        assert!(!plan.items.iter().find(|i| i.path == "new.txt").unwrap().replaces);
        assert!(plan.items.iter().find(|i| i.path == "newer-left.txt").unwrap().replaces);
    }

    #[tokio::test]
    async fn update_never_deletes_or_overwrites_newer_files() {
        let plan = mixed().plan(SyncMode::Update).await;
        assert_eq!(summary(&plan), ["→dir", "→new.txt", "→newer-left.txt"]);
        assert_eq!(plan.unchanged, 2, "same.txt and the newer file on the right");
    }

    #[tokio::test]
    async fn two_way_moves_the_newest_each_way() {
        let plan = mixed().plan(SyncMode::TwoWay).await;
        assert_eq!(
            summary(&plan),
            ["→dir", "←extra.txt", "←gone", "→new.txt", "→newer-left.txt", "←newer-right.txt"]
        );
    }

    #[tokio::test]
    async fn different_files_without_a_newer_one_are_conflicts() {
        let f = Fixture::new();
        f.put("left", "a.txt", "short", 10);
        f.put("right", "a.txt", "a longer text", 10);
        f.put("left", "same-size.txt", "aaaa", 10);
        f.put("right", "same-size.txt", "bbbb", 11);
        let plan = f.plan(SyncMode::TwoWay).await;
        assert_eq!(summary(&plan), ["?a.txt"]);
        assert_eq!(plan.unchanged, 1, "within the time slack and the same size");
        assert!(!plan.has_work());
        assert_eq!(f.plan(SyncMode::Mirror).await.count(SyncAction::CopyToRight), 1);
    }

    #[tokio::test]
    async fn folders_that_match_need_nothing() {
        let f = Fixture::new();
        f.put("left", "d/a.txt", "a", 10);
        f.put("right", "d/a.txt", "a", 10);
        let plan = f.plan(SyncMode::Mirror).await;
        assert_eq!(plan, SyncPlan { items: Vec::new(), unchanged: 1 });
    }
}
