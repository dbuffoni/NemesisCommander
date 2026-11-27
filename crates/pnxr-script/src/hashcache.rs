//! Remembers the hashes of pictures between runs (`pnxr.imageHash`), so a
//! second look at a big folder only decodes the pictures that are new or
//! changed. A picture is known by its location, size and modification time.
//!
//! The cache is the text file `image-hashes.tsv` in the data folder: one line
//! per picture, appended as they are hashed. A line that cannot be read is
//! ignored; deleting the file is always safe.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::imagehash::ImageInfo;

/// Where the cache lives: `PNXR_DATA_DIR`, or the default data folder.
fn file() -> Option<PathBuf> {
    Some(pnxr_config::AppPaths::detect().data_dir.join("image-hashes.tsv"))
}

struct Cache {
    file: Option<PathBuf>,
    known: HashMap<String, ImageInfo>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let file = file();
        let known = file
            .as_deref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|text| text.lines().filter_map(parse_line).collect())
            .unwrap_or_default();
        Mutex::new(Cache { file, known })
    })
}

/// `key TAB hash TAB width TAB height` (the size is part of the key).
fn parse_line(line: &str) -> Option<(String, ImageInfo)> {
    let mut parts = line.rsplitn(4, '\t');
    let height = parts.next()?.parse().ok()?;
    let width = parts.next()?.parse().ok()?;
    let hash = parts.next()?.to_owned();
    let key = parts.next()?.to_owned();
    if hash.len() != 16 {
        return None;
    }
    let bytes = key.rsplit('\t').nth(1)?.parse().ok()?;
    Some((key, ImageInfo { hash, width, height, bytes }))
}

/// The cache key of a picture, or `None` when it cannot be cached (no
/// modification time, or a name with a tab or a line break).
pub fn key(path: &str, size: u64, modified: Option<SystemTime>) -> Option<String> {
    if path.contains(['\t', '\n', '\r']) {
        return None;
    }
    let millis = modified?.duration_since(UNIX_EPOCH).ok()?.as_millis();
    Some(format!("{path}\t{size}\t{millis}"))
}

/// What the cache knows about `key`.
pub fn get(key: &str) -> Option<ImageInfo> {
    cache().lock().ok()?.known.get(key).cloned()
}

/// Remembers `info` for `key`, in memory and in the file.
pub fn put(key: &str, info: &ImageInfo) {
    let Ok(mut cache) = cache().lock() else { return };
    if cache.known.insert(key.to_owned(), info.clone()).is_some() {
        return;
    }
    if let Some(file) = &cache.file {
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
            let _ = writeln!(out, "{key}\t{}\t{}\t{}", info.hash, info.width, info.height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_read_back_and_a_damaged_one_is_ignored() {
        let key =
            key("/p/a.jpg", 1234, Some(UNIX_EPOCH + std::time::Duration::from_millis(5))).unwrap();
        assert_eq!(key, "/p/a.jpg\t1234\t5");
        let line = format!("{key}\t00ff00ff00ff00ff\t640\t480");
        let (read_key, info) = parse_line(&line).unwrap();
        assert_eq!(read_key, key);
        assert_eq!(
            info,
            ImageInfo { hash: "00ff00ff00ff00ff".into(), width: 640, height: 480, bytes: 1234 }
        );
        assert!(parse_line("garbage").is_none());
        assert!(parse_line(&format!("{key}\tshort\t1\t2")).is_none());
        assert!(super::key("/a\tb", 1, Some(UNIX_EPOCH)).is_none(), "a tab in a name");
        assert!(super::key("/a", 1, None).is_none(), "no modification time");
    }
}
