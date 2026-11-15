//! Recursive search by name, size, date and content.
//!
//! [`search`] walks a tree on any provider and sends matches through a
//! channel as soon as they are found, so a UI can show results while the
//! search continues. Content checks run concurrently on several files.

use std::sync::Arc;
use std::time::SystemTime;

use futures::StreamExt;
use memchr::memmem;
use regex::bytes::{Regex, RegexBuilder};
use sc_vfs::{Entry, EntryKind, Provider, Result, VfsError, VfsPath, walk};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::mpsc;
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
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let criteria = Arc::new(criteria);
        let candidates = walk(provider.as_ref(), &root, criteria.max_depth)
            .take_until(cancel.cancelled())
            .filter_map(|item| {
                let criteria = criteria.clone();
                async move {
                    match item {
                        Ok(item) if criteria.matches_metadata(&item.entry) => Some(Ok(item)),
                        Ok(_) => None,
                        Err(e) => Some(Err(e)),
                    }
                }
            });
        let checked = candidates
            .map(|item| {
                let (provider, criteria) = (provider.clone(), criteria.clone());
                async move {
                    let item = item?;
                    let first_match = match &criteria.content {
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
            if cancel.is_cancelled() || tx.send(message).await.is_err() {
                break;
            }
        }
    });
    rx
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
    use sc_providers::memory::MemoryProvider;
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
}
