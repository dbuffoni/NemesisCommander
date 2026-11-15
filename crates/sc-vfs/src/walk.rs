//! Recursive traversal of a directory tree on any provider.

use futures::stream::BoxStream;

use crate::{Entry, Provider, Result, VfsPath, collect_listing};

/// One item found by [`walk`].
#[derive(Debug, Clone)]
pub struct WalkItem {
    /// Full path of the item.
    pub path: VfsPath,
    /// Its metadata.
    pub entry: Entry,
    /// Distance from the walk root (direct children have depth 1).
    pub depth: usize,
}

/// Walks the tree below `root` depth-first, in pre-order: a directory is
/// always yielded before its contents, which is the order needed to
/// recreate a tree elsewhere. Reverse the collected items for a safe
/// deletion order.
///
/// Symbolic links are yielded but never followed, so cycles are impossible.
/// An unreadable sub-directory produces one `Err` item and the walk goes on.
/// `max_depth` limits recursion (`None` = unlimited, `Some(1)` = only the
/// direct children of `root`).
pub fn walk<'a>(
    provider: &'a dyn Provider,
    root: &VfsPath,
    max_depth: Option<usize>,
) -> BoxStream<'a, Result<WalkItem>> {
    struct State<'a> {
        provider: &'a dyn Provider,
        /// The directory to list before yielding anything else (pre-order:
        /// a directory's contents come right after the directory itself).
        to_list: Option<(VfsPath, usize)>,
        /// One frame per open directory; each frame is reversed so `pop`
        /// returns items in name order.
        stack: Vec<Vec<WalkItem>>,
        max_depth: Option<usize>,
    }

    let state = State { provider, to_list: Some((root.clone(), 0)), stack: Vec::new(), max_depth };

    Box::pin(futures::stream::unfold(state, |mut state| async move {
        if let Some((dir, depth)) = state.to_list.take() {
            match collect_listing(state.provider, &dir).await {
                Ok(mut entries) => {
                    entries.sort_by(|a, b| b.name.cmp(&a.name)); // reversed
                    state.stack.push(
                        entries
                            .into_iter()
                            .map(|entry| WalkItem {
                                path: dir.join(&entry.name),
                                entry,
                                depth: depth + 1,
                            })
                            .collect(),
                    );
                }
                Err(e) => return Some((Err(e), state)),
            }
        }
        loop {
            let frame = state.stack.last_mut()?;
            let Some(item) = frame.pop() else {
                state.stack.pop();
                continue;
            };
            let descend = item.entry.kind == crate::EntryKind::Directory
                && state.max_depth.is_none_or(|max| item.depth < max);
            if descend {
                state.to_list = Some((item.path.clone(), item.depth));
            }
            return Some((Ok(item), state));
        }
    }))
}

#[cfg(test)]
mod tests {
    // Walk is exercised against real providers in sc-providers and sc-ops;
    // here we only check the ordering logic with a tiny fake provider.
    use super::*;
    use crate::{Capabilities, EntryKind, ListStream, ReadStream, VfsError};
    use async_trait::async_trait;
    use futures::StreamExt;

    /// Tree: /a/{x, b/{y}}, /c ; listing /a/b fails if `broken` is set.
    struct Fake {
        broken: bool,
    }

    #[async_trait]
    impl Provider for Fake {
        fn scheme(&self) -> &str {
            "fake"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::empty()
        }
        async fn stat(&self, p: &VfsPath) -> Result<Entry> {
            Err(VfsError::NotFound(p.clone()))
        }
        async fn list(&self, p: &VfsPath) -> Result<ListStream> {
            let d = |n: &str| Entry::new(n, EntryKind::Directory);
            let f = |n: &str| Entry::new(n, EntryKind::File);
            let entries = match p.path() {
                "/" => vec![f("c"), d("a")],
                "/a" => vec![d("b"), f("x")],
                "/a/b" if self.broken => return Err(VfsError::PermissionDenied(p.clone())),
                "/a/b" => vec![f("y")],
                _ => vec![],
            };
            Ok(Box::pin(futures::stream::iter([Ok(entries)])))
        }
        async fn read(&self, p: &VfsPath, _: u64) -> Result<ReadStream> {
            Err(VfsError::NotFound(p.clone()))
        }
    }

    async fn paths(fake: Fake, max_depth: Option<usize>) -> Vec<String> {
        let root = VfsPath::parse("fake://h/").unwrap();
        walk(&fake, &root, max_depth)
            .map(|r| r.map(|i| i.path.path().to_owned()).unwrap_or_else(|e| format!("ERR {e}")))
            .collect()
            .await
    }

    #[tokio::test]
    async fn pre_order_sorted() {
        assert_eq!(
            paths(Fake { broken: false }, None).await,
            ["/a", "/a/b", "/a/b/y", "/a/x", "/c"]
        );
    }

    #[tokio::test]
    async fn depth_limit() {
        assert_eq!(paths(Fake { broken: false }, Some(1)).await, ["/a", "/c"]);
    }

    #[tokio::test]
    async fn errors_do_not_stop_the_walk() {
        let got = paths(Fake { broken: true }, None).await;
        assert_eq!(got.len(), 5);
        assert!(got[2].starts_with("ERR permission denied"));
        assert_eq!(got[3], "/a/x");
    }
}
