//! Recognizing archive formats from file names and magic bytes.

use pnxr_vfs::VfsPath;
use serde::{Deserialize, Serialize};

use crate::Compression;

/// A container format Pairnox understands natively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArchiveFormat {
    /// ZIP (also jar, apk, ...).
    Zip,
    /// tar, optionally compressed (`.tar.gz`, `.tgz`, ...).
    Tar(Compression),
    /// 7-Zip.
    SevenZ,
    /// RAR (read-only).
    Rar,
    /// A single compressed file (`notes.txt.gz`): shown as an archive with
    /// one entry.
    Compressed(Compression),
}

impl ArchiveFormat {
    /// The VFS scheme serving this format.
    pub fn scheme(self) -> &'static str {
        match self {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::Tar(_) => "tar",
            ArchiveFormat::SevenZ => "7z",
            ArchiveFormat::Rar => "rar",
            ArchiveFormat::Compressed(c) => c.extension(),
        }
    }

    /// The conventional file extension, without the leading dot.
    pub fn extension(self) -> String {
        match self {
            ArchiveFormat::Zip => "zip".into(),
            ArchiveFormat::Tar(Compression::None) => "tar".into(),
            ArchiveFormat::Tar(c) => format!("tar.{}", c.extension()),
            ArchiveFormat::SevenZ => "7z".into(),
            ArchiveFormat::Rar => "rar".into(),
            ArchiveFormat::Compressed(c) => c.extension().into(),
        }
    }

    /// Whether [`pack`](crate::pack) can create this format.
    pub fn can_write(self) -> bool {
        !matches!(self, ArchiveFormat::Rar | ArchiveFormat::Compressed(Compression::None))
    }

    /// Every format that can be created, for menus.
    pub fn writable() -> [ArchiveFormat; 11] {
        use Compression::*;
        [
            ArchiveFormat::Zip,
            ArchiveFormat::SevenZ,
            ArchiveFormat::Tar(None),
            ArchiveFormat::Tar(Gzip),
            ArchiveFormat::Tar(Xz),
            ArchiveFormat::Tar(Bzip2),
            ArchiveFormat::Tar(Zstd),
            ArchiveFormat::Compressed(Gzip),
            ArchiveFormat::Compressed(Xz),
            ArchiveFormat::Compressed(Bzip2),
            ArchiveFormat::Compressed(Zstd),
        ]
    }

    /// Parses a user-facing name such as `zip`, `tar.gz`, `tgz`, `7z`.
    pub fn parse(name: &str) -> Option<Self> {
        Self::from_file_name(&format!("x.{}", name.trim_start_matches('.')))
    }

    /// Recognizes a format from a file name's extension.
    pub fn from_file_name(name: &str) -> Option<Self> {
        use Compression::*;
        let lower = name.to_ascii_lowercase();
        let ends = |suffix: &str| lower.len() > suffix.len() && lower.ends_with(suffix);
        let found = if ends(".tar") {
            ArchiveFormat::Tar(None)
        } else if ends(".tar.gz") || ends(".tgz") {
            ArchiveFormat::Tar(Gzip)
        } else if ends(".tar.xz") || ends(".txz") {
            ArchiveFormat::Tar(Xz)
        } else if ends(".tar.bz2") || ends(".tbz2") || ends(".tbz") {
            ArchiveFormat::Tar(Bzip2)
        } else if ends(".tar.zst") || ends(".tzst") {
            ArchiveFormat::Tar(Zstd)
        } else if [
            "zip", "jar", "war", "apk", "epub", "odt", "ods", "docx", "xlsx", "pptx", "whl",
            "nupkg",
        ]
        .iter()
        .any(|ext| ends(&format!(".{ext}")))
        {
            ArchiveFormat::Zip
        } else if ends(".7z") {
            ArchiveFormat::SevenZ
        } else if ends(".rar") {
            ArchiveFormat::Rar
        } else if ends(".gz") {
            ArchiveFormat::Compressed(Gzip)
        } else if ends(".xz") {
            ArchiveFormat::Compressed(Xz)
        } else if ends(".bz2") {
            ArchiveFormat::Compressed(Bzip2)
        } else if ends(".zst") {
            ArchiveFormat::Compressed(Zstd)
        } else {
            return Option::None;
        };
        Some(found)
    }

    /// Recognizes a format from the first bytes of a file (at least 262
    /// bytes are needed to see a tar header).
    pub fn from_magic(head: &[u8]) -> Option<Self> {
        if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
            return Some(ArchiveFormat::Zip);
        }
        if head.starts_with(&[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c]) {
            return Some(ArchiveFormat::SevenZ);
        }
        if head.starts_with(b"Rar!\x1a\x07") {
            return Some(ArchiveFormat::Rar);
        }
        if head.len() >= 262 && &head[257..262] == b"ustar" {
            return Some(ArchiveFormat::Tar(Compression::None));
        }
        match Compression::from_magic(head) {
            Compression::None => None,
            c => Some(ArchiveFormat::Compressed(c)),
        }
    }
}

/// Detects a format by name, falling back to magic bytes.
pub fn detect_format(name: &str, head: &[u8]) -> Option<ArchiveFormat> {
    ArchiveFormat::from_file_name(name).or_else(|| ArchiveFormat::from_magic(head))
}

/// The path of the root *inside* an archive file, if its name looks like
/// an archive: `/tmp/a.zip` → `zip:///tmp/a.zip!/`.
pub fn archive_root(file: &VfsPath) -> Option<VfsPath> {
    let format = ArchiveFormat::from_file_name(file.file_name()?)?;
    VfsPath::parse(&format!("{}://{}!/", format.scheme(), file)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        use Compression::*;
        let f = ArchiveFormat::from_file_name;
        assert_eq!(f("a.ZIP"), Some(ArchiveFormat::Zip));
        assert_eq!(f("a.tar.gz"), Some(ArchiveFormat::Tar(Gzip)));
        assert_eq!(f("a.tgz"), Some(ArchiveFormat::Tar(Gzip)));
        assert_eq!(f("a.tar.zst"), Some(ArchiveFormat::Tar(Zstd)));
        assert_eq!(f("notes.txt.xz"), Some(ArchiveFormat::Compressed(Xz)));
        assert_eq!(f("a.7z"), Some(ArchiveFormat::SevenZ));
        assert_eq!(f("a.rar"), Some(ArchiveFormat::Rar));
        assert_eq!(f("readme.txt"), Option::None);
        assert_eq!(f(".zip"), Option::None, "a bare extension is a hidden file, not an archive");
        assert_eq!(ArchiveFormat::parse("tar.bz2"), Some(ArchiveFormat::Tar(Bzip2)));
    }

    #[test]
    fn extension_roundtrip() {
        for format in ArchiveFormat::writable() {
            assert_eq!(ArchiveFormat::parse(&format.extension()), Some(format), "{format:?}");
            assert!(format.can_write());
        }
        assert!(!ArchiveFormat::Rar.can_write());
    }

    #[test]
    fn magic() {
        assert_eq!(ArchiveFormat::from_magic(b"PK\x03\x04rest"), Some(ArchiveFormat::Zip));
        let mut tar = vec![0u8; 512];
        tar[257..262].copy_from_slice(b"ustar");
        assert_eq!(ArchiveFormat::from_magic(&tar), Some(ArchiveFormat::Tar(Compression::None)));
        assert_eq!(
            detect_format("data.bin", &[0x1f, 0x8b, 8]),
            Some(ArchiveFormat::Compressed(Compression::Gzip))
        );
        assert_eq!(ArchiveFormat::from_magic(b"hello"), None);
    }

    #[test]
    fn roots() {
        let root = archive_root(&VfsPath::parse("/tmp/a.tar.gz").unwrap()).unwrap();
        assert_eq!(root.scheme(), "tar");
        assert!(root.is_root());
        assert_eq!(root.container().unwrap().path(), "/tmp/a.tar.gz");
        assert!(archive_root(&VfsPath::parse("/tmp/a.txt").unwrap()).is_none());
    }
}
