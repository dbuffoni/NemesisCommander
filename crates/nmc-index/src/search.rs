//! Recursive search by name, size, date and content.
//!
//! [`search`] walks a tree on any provider and sends matches through a
//! channel as soon as they are found, so a UI can show results while the
//! search continues. Content checks run concurrently on several files.

use std::sync::Arc;
use std::time::SystemTime;

use futures::StreamExt;
use futures::future::BoxFuture;
use memchr::memmem;
use nmc_vfs::{Entry, EntryKind, Provider, Result, VfsError, VfsPath, walk};
use regex::bytes::{Regex, RegexBuilder};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

use crate::mask::Mask;

/// Files whose content is checked at the same time.
const CONTENT_PARALLELISM: usize = 8;

/// Longest line considered by regex content search; longer lines are cut.
const MAX_LINE: usize = 1024 * 1024;

/// What to look for inside files.
#[derive(Debug, Clone)]
pub struct ContentQuery {
    pattern: Pattern,
}

#[derive(Debug, Clone)]
enum Pattern {
    /// Literal bytes, matched case-sensitively with SIMD `memmem`.
    Literal(Vec<u8>),
    /// A regex (also used for case-insensitive literal search).
    Regex(Regex),
}

impl ContentQuery {
    /// Searches for literal text.
    pub fn text(text: &str, case_sensitive: bool) -> Self {
        if case_sensitive {
            ContentQuery { pattern: Pattern::Literal(text.as_bytes().to_vec()) }
        } else {
            let regex = RegexBuilder::new(&regex::escape(text))
                .case_insensitive(true)
                .build()
                .expect("an escaped literal is a valid regex");
            ContentQuery { pattern: Pattern::Regex(regex) }
        }
    }

    /// What is typed in a search dialog: text, or a regular expression
    /// when it starts with `re:`.
    pub fn parse(text: &str, case_sensitive: bool) -> std::result::Result<Self, regex::Error> {
        match text.strip_prefix(crate::mask::REGEX_PREFIX) {
            Some(pattern) => Self::regex(pattern, case_sensitive),
            None => Ok(Self::text(text, case_sensitive)),
        }
    }

    /// Searches for a regular expression (matched line by line).
    pub fn regex(pattern: &str, case_sensitive: bool) -> std::result::Result<Self, regex::Error> {
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(!case_sensitive)
            .multi_line(true)
            .build()?;
        Ok(ContentQuery { pattern: Pattern::Regex(regex) })
    }
}

/// Conditions an entry must meet. Every set condition must hold.
#[derive(Debug, Clone, Default)]
pub struct SearchCriteria {
    /// Name mask (empty = any name).
    pub name: Mask,
    /// Content to find (files only).
    pub content: Option<ContentQuery>,
    /// Minimum size in bytes.
    pub min_size: Option<u64>,
    /// Maximum size in bytes.
    pub max_size: Option<u64>,
    /// Modified at or after.
    pub modified_after: Option<SystemTime>,
    /// Modified at or before.
    pub modified_before: Option<SystemTime>,
    /// Also report directories whose name matches (never with `content`).
    pub include_directories: bool,
    /// Limit recursion depth (`None` = unlimited).
    pub max_depth: Option<usize>,
    /// Also search inside the archives met on the way (needs an
    /// [`ArchiveOpener`], see [`search_in_archives`]).
    pub archives: bool,
}

impl SearchCriteria {
    /// Checks every condition except content.
    fn matches_metadata(&self, entry: &Entry) -> bool {
        let kind_ok = match entry.kind {
            EntryKind::File => true,
            EntryKind::Directory => self.include_directories && self.content.is_none(),
            EntryKind::Symlink | EntryKind::Other => self.content.is_none(),
        };
        let has_size_filter = self.min_size.is_some() || self.max_size.is_some();
        kind_ok
            && self.name.matches(&entry.name)
            && (!has_size_filter || entry.kind == EntryKind::File)
            && self.min_size.is_none_or(|min| entry.size >= min)
            && self.max_size.is_none_or(|max| entry.size <= max)
            && self.modified_after.is_none_or(|t| entry.modified.is_some_and(|m| m >= t))
            && self.modified_before.is_none_or(|t| entry.modified.is_some_and(|m| m <= t))
    }
}

/// One search result.
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Where it was found.
    pub path: VfsPath,
    /// Its metadata.
    pub entry: Entry,
    /// For content searches: the 1-based line of the first match and the
    /// line's text (lossily decoded, trimmed).
    pub first_match: Option<(u64, String)>,
}

/// Archives inside archives are searched down to this level.
const MAX_ARCHIVE_DEPTH: usize = 3;

/// Archives opened and searched at the same time.
const ARCHIVE_PARALLELISM: usize = 4;

/// Tells a search how to look inside archive files, which this crate cannot
/// do itself.
pub trait ArchiveOpener: Send + Sync {
    /// The root inside `file` if its name says it is an archive.
    fn root_of(&self, file: &VfsPath) -> Option<VfsPath>;

    /// The provider that reads the archive whose root is `root`.
    fn provider_for<'a>(&'a self, root: &'a VfsPath) -> BoxFuture<'a, Result<Arc<dyn Provider>>>;
}

/// Starts a search below `root` and returns a channel of results.
///
/// The search runs on the tokio runtime until it finishes, `cancel` fires,
/// or the receiver is dropped. Errors (unreadable folders or files) are sent
/// as `Err` items and do not stop the search.
pub fn search(
    provider: Arc<dyn Provider>,
    root: VfsPath,
    criteria: SearchCriteria,
    cancel: CancellationToken,
) -> mpsc::Receiver<Result<SearchHit>> {
    start(provider, root, criteria, cancel, None)
}

/// Like [`search`], and when `criteria.archives` is set also looks inside
/// every archive it meets (zip, tar, 7z, ...), where `opener` says how. Hits
/// inside archives have paths such as `zip:///data/a.zip!/docs/readme.txt`.
pub fn search_in_archives(
    provider: Arc<dyn Provider>,
    root: VfsPath,
    criteria: SearchCriteria,
    cancel: CancellationToken,
    opener: Arc<dyn ArchiveOpener>,
) -> mpsc::Receiver<Result<SearchHit>> {
    start(provider, root, criteria, cancel, Some(opener))
}

/// What the searches of one run share.
struct Run {
    criteria: SearchCriteria,
    cancel: CancellationToken,
    tx: mpsc::Sender<Result<SearchHit>>,
    opener: Option<Arc<dyn ArchiveOpener>>,
    /// Limits how many archives are open at once.
    archive_slots: Semaphore,
}

fn start(
    provider: Arc<dyn Provider>,
    root: VfsPath,
    criteria: SearchCriteria,
    cancel: CancellationToken,
    opener: Option<Arc<dyn ArchiveOpener>>,
) -> mpsc::Receiver<Result<SearchHit>> {
    let (tx, rx) = mpsc::channel(256);
    let opener = opener.filter(|_| criteria.archives);
    let run = Arc::new(Run {
        criteria,
        cancel,
        tx,
        opener,
        archive_slots: Semaphore::new(ARCHIVE_PARALLELISM),
    });
    tokio::spawn(search_tree(run, provider, root, 0));
    rx
}

/// Searches the tree below `root`; archives met on the way are searched by
/// tasks of their own (`archive_depth` counts how deep inside archives this
/// tree is).
async fn search_tree(
    run: Arc<Run>,
    provider: Arc<dyn Provider>,
    root: VfsPath,
    archive_depth: usize,
) {
    let criteria = &run.criteria;
    let candidates = walk(provider.as_ref(), &root, criteria.max_depth)
        .take_until(run.cancel.cancelled())
        .filter_map(|item| {
            let run = run.clone();
            async move {
                match item {
                    Ok(item) => {
                        if archive_depth < MAX_ARCHIVE_DEPTH && item.entry.kind == EntryKind::File {
                            spawn_archive(&run, &item.path, archive_depth + 1);
                        }
                        run.criteria.matches_metadata(&item.entry).then_some(Ok(item))
                    }
                    Err(e) => Some(Err(e)),
                }
            }
        });
    let checked = candidates
        .map(|item| {
            let (provider, run) = (provider.clone(), run.clone());
            async move {
                let item = item?;
                let first_match = match &run.criteria.content {
                    None => None,
                    Some(query) => {
                        match find_content(provider.as_ref(), &item.path, query).await? {
                            Some(found) => Some(found),
                            None => return Ok(None),
                        }
                    }
                };
                Ok(Some(SearchHit { path: item.path, entry: item.entry, first_match }))
            }
        })
        .buffered(CONTENT_PARALLELISM);
    futures::pin_mut!(checked);
    while let Some(result) = checked.next().await {
        let message = match result {
            Ok(Some(hit)) => Ok(hit),
            Ok(None) => continue,
            Err(e) => Err(e),
        };
        if run.cancel.is_cancelled() || run.tx.send(message).await.is_err() {
            break;
        }
    }
}

/// Starts searching inside `file` if it is an archive and archives are asked
/// for.
fn spawn_archive(run: &Arc<Run>, file: &VfsPath, depth: usize) {
    let Some(opener) = &run.opener else { return };
    let Some(root) = opener.root_of(file) else { return };
    let run = run.clone();
    let opener = opener.clone();
    tokio::spawn(async move {
        let Ok(_slot) = run.archive_slots.acquire().await else { return };
        if run.cancel.is_cancelled() {
            return;
        }
        match opener.provider_for(&root).await {
            Ok(provider) => search_tree(run.clone(), provider, root, depth).await,
            // An archive that cannot be opened does not stop the search.
            Err(e) => {
                let _ = run.tx.send(Err(e)).await;
            }
        }
    });
}

/// Returns the first matching line of `path`, if any.
async fn find_content(
    provider: &dyn Provider,
    path: &VfsPath,
    query: &ContentQuery,
) -> Result<Option<(u64, String)>> {
    let reader = provider.read(path, 0).await?;
    let io = |e| VfsError::from_io(path, e);
    match &query.pattern {
        Pattern::Literal(needle) => {
            if needle.is_empty() {
                return Ok(Some((1, String::new())));
            }
            // Chunked search; keep the last needle.len()-1 bytes so a match
            // spanning two chunks is found. Track line numbers as we go.
            let finder = memmem::Finder::new(needle);
            let mut reader = BufReader::with_capacity(256 * 1024, reader);
            let mut window: Vec<u8> = Vec::new();
            let mut line_base = 1u64; // line number at window[0]
            let mut chunk = vec![0u8; 256 * 1024];
            loop {
                let n = reader.read(&mut chunk).await.map_err(io)?;
                if n == 0 {
                    return Ok(None);
                }
                window.extend_from_slice(&chunk[..n]);
                if let Some(pos) = finder.find(&window) {
                    let line =
                        line_base + memchr::memchr_iter(b'\n', &window[..pos]).count() as u64;
                    let start =
                        window[..pos].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
                    let end =
                        memchr::memchr(b'\n', &window[pos..]).map_or(window.len(), |i| pos + i);
                    return Ok(Some((
                        line,
                        String::from_utf8_lossy(&window[start..end]).trim().to_owned(),
                    )));
                }
                let keep = needle.len() - 1;
                if window.len() > keep {
                    let cut = window.len() - keep;
                    line_base += memchr::memchr_iter(b'\n', &window[..cut]).count() as u64;
                    window.drain(..cut);
                }
            }
        }
        Pattern::Regex(regex) => {
            let mut reader = BufReader::with_capacity(256 * 1024, reader);
            let mut line = Vec::new();
            let mut line_number = 1u64;
            loop {
                line.clear();
                // Bound each read so a huge file without newlines cannot
                // exhaust memory; an over-long line is checked in pieces.
                let n = (&mut reader)
                    .take(MAX_LINE as u64)
                    .read_until(b'\n', &mut line)
                    .await
                    .map_err(io)?;
                if n == 0 {
                    return Ok(None);
                }
                if regex.is_match(&line) {
                    return Ok(Some((
                        line_number,
                        String::from_utf8_lossy(&line).trim().to_owned(),
                    )));
                }
                if line.last() == Some(&b'\n') {
                    line_number += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmc_providers::memory::MemoryProvider;
    use std::time::Duration;

    fn tree() -> Arc<MemoryProvider> {
        let m = MemoryProvider::new();
        m.put_file("/src/main.rs", "fn main() {\n    println!(\"Hello\");\n}\n");
        m.put_file("/src/lib.rs", "pub fn add() {}\n");
        m.put_file("/docs/readme.md", "# Title\nsee main.rs\n");
        m.put_file("/big.bin", vec![b'x'; 10_000]);
        Arc::new(m)
    }

    async fn run(criteria: SearchCriteria) -> Vec<SearchHit> {
        let root = VfsPath::parse("mem://t/").unwrap();
        let mut rx = search(tree(), root, criteria, CancellationToken::new());
        let mut hits = Vec::new();
        while let Some(hit) = rx.recv().await {
            hits.push(hit.unwrap());
        }
        hits
    }

    fn paths(hits: &[SearchHit]) -> Vec<&str> {
        hits.iter().map(|h| h.path.path()).collect()
    }

    #[tokio::test]
    async fn by_name() {
        let hits =
            run(SearchCriteria { name: Mask::parse("*.rs").unwrap(), ..Default::default() }).await;
        assert_eq!(paths(&hits), ["/src/lib.rs", "/src/main.rs"]);
    }

    #[tokio::test]
    async fn directories_only_when_asked() {
        let criteria = SearchCriteria { name: Mask::parse("src").unwrap(), ..Default::default() };
        assert!(run(criteria.clone()).await.is_empty());
        let hits = run(SearchCriteria { include_directories: true, ..criteria }).await;
        assert_eq!(paths(&hits), ["/src"]);
    }

    #[tokio::test]
    async fn by_size() {
        let hits = run(SearchCriteria { min_size: Some(1000), ..Default::default() }).await;
        assert_eq!(paths(&hits), ["/big.bin"]);
        let hits = run(SearchCriteria { max_size: Some(18), ..Default::default() }).await;
        assert_eq!(paths(&hits), ["/src/lib.rs"]);
    }

    #[tokio::test]
    async fn by_date() {
        let future = SystemTime::now() + Duration::from_secs(3600);
        assert!(
            run(SearchCriteria { modified_after: Some(future), ..Default::default() })
                .await
                .is_empty()
        );
        assert_eq!(
            run(SearchCriteria { modified_before: Some(future), ..Default::default() }).await.len(),
            4
        );
    }

    #[tokio::test]
    async fn by_content_literal_and_case() {
        let hits = run(SearchCriteria {
            content: Some(ContentQuery::text("main", true)),
            ..Default::default()
        })
        .await;
        assert_eq!(paths(&hits), ["/docs/readme.md", "/src/main.rs"]);
        assert_eq!(hits[0].first_match, Some((2, "see main.rs".into())));
        assert_eq!(hits[1].first_match, Some((1, "fn main() {".into())));

        let sensitive = run(SearchCriteria {
            content: Some(ContentQuery::text("hello", true)),
            ..Default::default()
        })
        .await;
        assert!(sensitive.is_empty());
        let insensitive = run(SearchCriteria {
            content: Some(ContentQuery::text("hello", false)),
            ..Default::default()
        })
        .await;
        assert_eq!(insensitive[0].first_match.as_ref().unwrap().0, 2);
    }

    #[tokio::test]
    async fn by_content_regex() {
        let query = ContentQuery::regex(r"pub\s+fn\s+\w+", true).unwrap();
        let hits = run(SearchCriteria { content: Some(query), ..Default::default() }).await;
        assert_eq!(paths(&hits), ["/src/lib.rs"]);
    }

    #[tokio::test]
    async fn literal_match_across_chunk_boundary() {
        let m = MemoryProvider::new();
        let mut data = vec![b'a'; 256 * 1024 - 2];
        data.extend_from_slice(b"\nNEEDLE");
        m.put_file("/f", data);
        let path = VfsPath::parse("mem://t/f").unwrap();
        let found = find_content(&m, &path, &ContentQuery::text("NEEDLE", true)).await.unwrap();
        assert_eq!(found, Some((2, "NEEDLE".into())));
    }

    #[tokio::test]
    async fn cancellation_stops_results() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut rx =
            search(tree(), VfsPath::parse("mem://t/").unwrap(), SearchCriteria::default(), cancel);
        assert!(rx.recv().await.is_none());
    }

    /// Treats every `*.pack` file as an archive whose content is `inner`.
    struct FakeArchives {
        inner: Arc<MemoryProvider>,
    }

    impl ArchiveOpener for FakeArchives {
        fn root_of(&self, file: &VfsPath) -> Option<VfsPath> {
            file.file_name()?.ends_with(".pack").then(|| VfsPath::parse("mem://pack/").unwrap())
        }

        fn provider_for<'a>(
            &'a self,
            _root: &'a VfsPath,
        ) -> BoxFuture<'a, Result<Arc<dyn Provider>>> {
            let inner: Arc<dyn Provider> = self.inner.clone();
            Box::pin(async move { Ok(inner) })
        }
    }

    async fn run_with_archives(criteria: SearchCriteria) -> Vec<SearchHit> {
        let outer = MemoryProvider::new();
        outer.put_file("/data/inner.pack", "binary");
        outer.put_file("/data/plain.txt", "needle outside");
        let inner = MemoryProvider::new();
        inner.put_file("/docs/readme.txt", "needle inside\n");
        inner.put_file("/docs/other.txt", "nothing\n");
        let opener = Arc::new(FakeArchives { inner: Arc::new(inner) });
        let root = VfsPath::parse("mem://t/").unwrap();
        let mut rx =
            search_in_archives(Arc::new(outer), root, criteria, CancellationToken::new(), opener);
        let mut hits = Vec::new();
        while let Some(hit) = rx.recv().await {
            hits.push(hit.unwrap());
        }
        hits
    }

    #[tokio::test]
    async fn archives_are_searched_only_when_asked() {
        let content = || Some(ContentQuery::text("needle", true));
        let without =
            run_with_archives(SearchCriteria { content: content(), ..Default::default() }).await;
        assert_eq!(paths(&without), ["/data/plain.txt"]);

        let with = run_with_archives(SearchCriteria {
            content: content(),
            archives: true,
            ..Default::default()
        })
        .await;
        let mut found = paths(&with);
        found.sort_unstable();
        assert_eq!(found, ["/data/plain.txt", "/docs/readme.txt"], "{found:?}");
        let inside = with.iter().find(|h| h.path.path() == "/docs/readme.txt").unwrap();
        assert_eq!(inside.first_match.as_ref().unwrap().1, "needle inside");
    }

    #[tokio::test]
    async fn archives_are_searched_by_name_too() {
        let hits = run_with_archives(SearchCriteria {
            name: Mask::parse("*.txt").unwrap(),
            archives: true,
            ..Default::default()
        })
        .await;
        assert_eq!(hits.len(), 3, "plain.txt, readme.txt, other.txt: {:?}", paths(&hits));
    }
}
