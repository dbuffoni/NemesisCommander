//! Turning command-line arguments into [`VfsPath`]s.

use sc_vfs::{Result, VfsPath};

/// Parses a location typed by the user: a URI (`sftp://host/x`,
/// `zip:///a.zip!/`) or a local path, which may be relative to the current
/// directory.
pub fn parse(text: &str) -> Result<VfsPath> {
    if text.contains("://") { VfsPath::parse(text) } else { VfsPath::from_local(text) }
}

/// Parses several locations.
pub fn parse_all(texts: &[String]) -> Result<Vec<VfsPath>> {
    texts.iter().map(|t| parse(t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_relative_paths() {
        assert_eq!(parse("sftp://h/x").unwrap().scheme(), "sftp");
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(parse("sub").unwrap(), VfsPath::from_local(cwd.join("sub")).unwrap());
    }
}
