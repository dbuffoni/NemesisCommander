//! [`VfsPath`]: a provider-independent location written as a URI.
//!
//! # Syntax
//!
//! | Example                              | Meaning                                   |
//! |--------------------------------------|-------------------------------------------|
//! | `/home/me/notes.txt`                 | local path (shorthand for `file://`)      |
//! | `file:///home/me/notes.txt`          | local path                                |
//! | `sftp://me@host:2222/srv/www`        | remote path; `me@host:2222` is the authority |
//! | `agent://build01/var/log`            | path served by a remote `pnxr` agent        |
//! | `zip:///tmp/a.zip!/docs/readme.md`   | entry `/docs/readme.md` inside `/tmp/a.zip` |
//! | `zip://sftp://h/a.zip!/x`            | archive stored on an SFTP server          |
//! | `zip://zip:///a.zip!/b.zip!/c`       | archive nested inside another archive     |
//!
//! Archive schemes (see [`ARCHIVE_SCHEMES`]) wrap a *container* path: the
//! text between `scheme://` and the **last** `!/` is the container, the rest
//! is the path inside it. That rule makes nesting unambiguous.
//!
//! Paths are always absolute, use `/` as separator, and are normalized on
//! construction: `.` segments and repeated slashes are dropped, `..` removes
//! the previous segment (never going above the root).
//!
//! On Windows a local path `C:\Users` is stored as `/C:/Users`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{Result, VfsError};

/// URI schemes whose paths point *inside* another file (an archive).
pub const ARCHIVE_SCHEMES: &[&str] = &["zip", "tar", "7z", "rar", "gz", "xz", "bz2", "zst"];

/// Separator between an archive's container and the path inside it.
const CONTAINER_SEPARATOR: &str = "!/";

/// A normalized, absolute location in any provider.
///
/// `VfsPath` is cheap to clone for local and remote paths and compares by
/// value. See the [module documentation](self) for the textual syntax.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct VfsPath {
    scheme: String,
    authority: String,
    container: Option<Box<VfsPath>>,
    path: String,
}

impl VfsPath {
    /// Parses a URI or a bare absolute local path.
    ///
    /// ```
    /// use pnxr_vfs::VfsPath;
    /// let p = VfsPath::parse("sftp://me@host/srv/../www/").unwrap();
    /// assert_eq!(p.scheme(), "sftp");
    /// assert_eq!(p.authority(), "me@host");
    /// assert_eq!(p.path(), "/www");
    /// ```
    pub fn parse(input: &str) -> Result<Self> {
        let invalid = |reason| VfsError::InvalidPath { input: input.to_owned(), reason };

        let Some((scheme, rest)) = split_scheme(input) else {
            // No scheme: a bare local path.
            return Self::local_from_str(input)
                .ok_or_else(|| invalid("local paths must be absolute"));
        };

        if ARCHIVE_SCHEMES.contains(&scheme.as_str()) {
            let (container, inner) = rest
                .rsplit_once(CONTAINER_SEPARATOR)
                .ok_or_else(|| invalid("archive paths need a '!/' after the archive file"))?;
            let container = VfsPath::parse(container)?;
            if container.is_root() {
                return Err(invalid("an archive container cannot be a root directory"));
            }
            return Ok(VfsPath {
                scheme,
                authority: String::new(),
                container: Some(Box::new(container)),
                path: normalize(inner),
            });
        }

        // scheme://authority/path ; authority may be empty (file:///x).
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        if scheme == "file" && !authority.is_empty() && authority != "localhost" {
            return Err(invalid("file:// URIs cannot name a remote host"));
        }
        let authority = if scheme == "file" { "" } else { authority };
        Ok(VfsPath {
            scheme,
            authority: authority.to_owned(),
            container: None,
            path: normalize(path),
        })
    }

    /// Builds a `file://` path from a local [`Path`].
    ///
    /// Relative paths are resolved against the current directory, but
    /// symlinks are *not* resolved (that is the provider's job).
    pub fn from_local(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| VfsError::InvalidPath {
                    input: path.display().to_string(),
                    reason: "cannot resolve relative path",
                })?
                .join(path)
        };
        let text = absolute.to_string_lossy();
        Self::local_from_str(&text).ok_or_else(|| VfsError::InvalidPath {
            input: text.into_owned(),
            reason: "not an absolute path",
        })
    }

    /// The root of the local file system (`/`).
    pub fn local_root() -> Self {
        VfsPath {
            scheme: "file".into(),
            authority: String::new(),
            container: None,
            path: "/".into(),
        }
    }

    fn local_from_str(input: &str) -> Option<Self> {
        // A UNC path (a share on another machine) lives under `/UNC/`.
        if let Some(inner) = crate::winpath::unc_to_inner(input) {
            return Some(VfsPath {
                scheme: "file".into(),
                authority: String::new(),
                container: None,
                path: normalize(&inner),
            });
        }
        let mut unified = input.replace('\\', "/");
        // Windows' "verbatim" form of a drive path (`\\?\C:\dir`, which
        // canonicalizing returns) is the plain drive path.
        if let Some(rest) = unified.strip_prefix("//?/").filter(|rest| is_windows_drive(rest)) {
            unified = rest.to_owned();
        }
        let path = if is_windows_drive(&unified) {
            format!("/{unified}")
        } else if unified.starts_with('/') {
            unified
        } else {
            return None;
        };
        Some(VfsPath {
            scheme: "file".into(),
            authority: String::new(),
            container: None,
            path: normalize(&path),
        })
    }

    /// The URI scheme, e.g. `"file"`, `"sftp"`, `"zip"`.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The authority (`user@host:port`), empty for local and archive paths.
    pub fn authority(&self) -> &str {
        &self.authority
    }

    /// For archive paths, the file that contains the archive.
    pub fn container(&self) -> Option<&VfsPath> {
        self.container.as_deref()
    }

    /// The absolute, normalized path inside the provider (always starts with `/`).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// `true` for `file://` paths.
    pub fn is_local(&self) -> bool {
        self.scheme == "file"
    }

    /// `true` if this is the root of its provider (or of its archive).
    pub fn is_root(&self) -> bool {
        self.path == "/"
    }

    /// The last path segment, or `None` at the root.
    pub fn file_name(&self) -> Option<&str> {
        if self.is_root() { None } else { self.path.rsplit('/').next() }
    }

    /// Returns a path with `name` appended. `name` may contain `/` to append
    /// several segments; `..` and `.` are normalized as usual.
    pub fn join(&self, name: &str) -> Self {
        let mut joined = self.clone();
        joined.path = normalize(&format!("{}/{}", self.path, name));
        joined
    }

    /// Returns a path with the same provider but a different inner path.
    pub fn with_path(&self, path: &str) -> Self {
        let mut other = self.clone();
        other.path = normalize(path);
        other
    }

    /// The parent directory.
    ///
    /// At the root of an archive this steps *out* of the archive and returns
    /// the directory containing the archive file, which is what a user
    /// pressing "go up" expects. At the root of any other provider it
    /// returns `None`.
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return self.container.as_ref().and_then(|c| c.parent());
        }
        let cut = self.path.rfind('/').unwrap_or(0);
        Some(self.with_path(if cut == 0 { "/" } else { &self.path[..cut] }))
    }

    /// `true` if `self` is `other` or lies below it (same provider).
    pub fn starts_with(&self, other: &VfsPath) -> bool {
        if self.scheme != other.scheme
            || self.authority != other.authority
            || self.container != other.container
        {
            return false;
        }
        other.is_root()
            || self.path == other.path
            || (self.path.starts_with(&other.path)
                && self.path.as_bytes().get(other.path.len()) == Some(&b'/'))
    }

    /// The path of `self` relative to `base`, if `self` is below `base`.
    /// Returns an empty string when the two are equal.
    pub fn relative_to(&self, base: &VfsPath) -> Option<String> {
        if !self.starts_with(base) {
            return None;
        }
        let rest = &self.path[base.path.len()..];
        Some(rest.trim_start_matches('/').to_owned())
    }

    /// The identity of the provider instance that serves this path: scheme,
    /// authority and container, without the inner path. Two paths with the
    /// same key are served by the same provider.
    pub fn provider_key(&self) -> String {
        let mut root = self.clone();
        root.path = "/".into();
        root.to_string()
    }

    /// The path as a person reads it: a native path for a local file, the
    /// URI for everything else (error messages use this).
    pub fn shown(&self) -> String {
        match self.to_local_path() {
            Some(local) => local.display().to_string(),
            None => self.to_string(),
        }
    }

    /// Converts a `file://` path to a native path. `None` for other schemes.
    pub fn to_local_path(&self) -> Option<PathBuf> {
        if !self.is_local() {
            return None;
        }
        let trimmed = &self.path[1..];
        #[cfg(windows)]
        if let Some(native) = crate::winpath::native_from_inner(&self.path) {
            return Some(PathBuf::from(native));
        }
        if is_windows_drive(trimmed) {
            Some(PathBuf::from(trimmed.replace('/', "\\")))
        } else {
            Some(PathBuf::from(&self.path))
        }
    }
}

/// Splits `"scheme://rest"` into its parts. The scheme is made of letters,
/// digits, `+`, `-` and `.` and is lowercased. Unlike RFC 3986 it may start
/// with a digit, so that `7z://` works.
fn split_scheme(input: &str) -> Option<(String, &str)> {
    let (scheme, rest) = input.split_once("://")?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    // A single letter is a Windows drive (`C://x` is a mangled `C:/x`), not a scheme.
    (valid && scheme.len() > 1).then(|| (scheme.to_ascii_lowercase(), rest))
}

fn is_windows_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/')
}

/// Normalizes a `/`-separated path: always absolute, no empty or `.`
/// segments, `..` pops a segment, no trailing slash (except the root).
fn normalize(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    let mut out = String::with_capacity(path.len() + 1);
    for s in &segments {
        out.push('/');
        out.push_str(s);
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.container {
            Some(container) => write!(f, "{}://{}!{}", self.scheme, container, self.path),
            None => write!(f, "{}://{}{}", self.scheme, self.authority, self.path),
        }
    }
}

impl fmt::Debug for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfsPath({self})")
    }
}

impl FromStr for VfsPath {
    type Err = VfsError;
    fn from_str(s: &str) -> Result<Self> {
        VfsPath::parse(s)
    }
}

impl From<VfsPath> for String {
    fn from(p: VfsPath) -> String {
        p.to_string()
    }
}

impl TryFrom<String> for VfsPath {
    type Error = VfsError;
    fn try_from(s: String) -> Result<Self> {
        VfsPath::parse(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn p(s: &str) -> VfsPath {
        VfsPath::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn bare_local_paths_become_file_uris() {
        let path = p("/home/me/../you/./docs//");
        assert_eq!(path.scheme(), "file");
        assert_eq!(path.path(), "/home/you/docs");
        assert_eq!(path.to_string(), "file:///home/you/docs");
    }

    #[test]
    fn verbatim_windows_paths_are_plain_drive_paths() {
        let path = VfsPath::local_from_str(r"\\?\C:\Users\me").unwrap();
        assert_eq!(path.path(), "/C:/Users/me");
    }

    #[test]
    fn relative_bare_paths_are_rejected() {
        assert!(VfsPath::parse("docs/readme").is_err());
    }

    #[test]
    fn remote_uri_parts() {
        let path = p("SFTP://me@host:2222/srv/www");
        assert_eq!(path.scheme(), "sftp");
        assert_eq!(path.authority(), "me@host:2222");
        assert_eq!(path.path(), "/srv/www");
        assert_eq!(path.provider_key(), "sftp://me@host:2222/");
    }

    #[test]
    fn uri_without_path_is_root() {
        assert!(p("sftp://host").is_root());
    }

    #[test]
    fn file_uri_rejects_remote_hosts() {
        assert!(VfsPath::parse("file://server/share").is_err());
        assert_eq!(p("file://localhost/etc").path(), "/etc");
    }

    #[test]
    fn archive_paths_have_a_container() {
        let path = p("zip:///tmp/a.zip!/docs/readme.md");
        assert_eq!(path.container().unwrap(), &p("/tmp/a.zip"));
        assert_eq!(path.path(), "/docs/readme.md");
        assert_eq!(path.to_string(), "zip://file:///tmp/a.zip!/docs/readme.md");
    }

    #[test]
    fn schemes_may_start_with_a_digit() {
        let path = p("7z:///tmp/a.7z!/x");
        assert_eq!(path.scheme(), "7z");
        assert_eq!(path.container().unwrap(), &p("/tmp/a.7z"));
    }

    #[test]
    fn nested_archives_split_at_last_separator() {
        let path = p("zip://zip:///a.zip!/b.zip!/c");
        let inner = path.container().unwrap();
        assert_eq!(inner.scheme(), "zip");
        assert_eq!(inner.path(), "/b.zip");
        assert_eq!(inner.container().unwrap(), &p("/a.zip"));
    }

    #[test]
    fn archive_without_separator_is_invalid() {
        assert!(VfsPath::parse("zip:///tmp/a.zip").is_err());
        assert!(VfsPath::parse("zip:///!/x").is_err());
    }

    #[test]
    fn parent_steps_out_of_archives() {
        let root = p("zip:///tmp/a.zip!/");
        assert_eq!(root.parent().unwrap(), p("/tmp"));
        assert_eq!(p("zip:///tmp/a.zip!/x/y").parent().unwrap(), p("zip:///tmp/a.zip!/x"));
        assert_eq!(p("/").parent(), None);
        assert_eq!(p("/etc").parent().unwrap(), p("/"));
    }

    #[test]
    fn join_and_file_name() {
        let base = p("/home");
        assert_eq!(base.join("me/docs").path(), "/home/me/docs");
        assert_eq!(base.join("..").path(), "/");
        assert_eq!(base.join("me").file_name(), Some("me"));
        assert_eq!(p("/").file_name(), None);
    }

    #[test]
    fn starts_with_respects_segment_boundaries() {
        assert!(p("/home/me").starts_with(&p("/home")));
        assert!(p("/home").starts_with(&p("/home")));
        assert!(p("/home").starts_with(&p("/")));
        assert!(!p("/homework").starts_with(&p("/home")));
        assert!(!p("sftp://h/home").starts_with(&p("/home")));
        assert_eq!(p("/home/me/x").relative_to(&p("/home")).as_deref(), Some("me/x"));
        assert_eq!(p("/home").relative_to(&p("/home")).as_deref(), Some(""));
        assert_eq!(p("/etc").relative_to(&p("/home")), None);
    }

    #[test]
    fn windows_drive_paths() {
        let path = p("C:\\Users\\me");
        assert_eq!(path.path(), "/C:/Users/me");
        assert_eq!(path.to_local_path().unwrap(), PathBuf::from("C:\\Users\\me"));
    }

    #[test]
    fn serde_uses_the_uri_string() {
        let path = p("sftp://h/x");
        let json = serde_json_like(&path);
        assert_eq!(json, "sftp://h/x");
    }

    // Avoid a serde_json dependency: round-trip through the String conversions
    // serde is configured to use.
    fn serde_json_like(path: &VfsPath) -> String {
        let s: String = path.clone().into();
        assert_eq!(&VfsPath::try_from(s.clone()).unwrap(), path);
        s
    }

    proptest! {
        /// Displaying and re-parsing a path gives back the same path.
        #[test]
        fn display_parse_roundtrip(segments in proptest::collection::vec("[a-zA-Z0-9 _.-]{1,8}", 0..6),
                                   scheme in prop_oneof![Just("file"), Just("sftp"), Just("agent")]) {
            let inner = format!("/{}", segments.join("/"));
            let input = if scheme == "file" { inner.clone() } else { format!("{scheme}://host{inner}") };
            let path = VfsPath::parse(&input).unwrap();
            prop_assert_eq!(VfsPath::parse(&path.to_string()).unwrap(), path);
        }

        /// Normalization never produces empty segments or a trailing slash.
        #[test]
        fn normalized_shape(raw in "[a-z/.]{0,30}") {
            let n = normalize(&raw);
            prop_assert!(n.starts_with('/'));
            prop_assert!(n == "/" || !n.ends_with('/'));
            prop_assert!(!n.contains("//"));
            prop_assert!(!n.split('/').any(|s| s == "." || s == ".."));
        }

        /// The parent of a joined child is the original directory.
        #[test]
        fn parent_of_join(name in "[a-zA-Z0-9_]{1,10}") {
            let base = VfsPath::parse("/data/dir").unwrap();
            prop_assert_eq!(base.join(&name).parent().unwrap(), base);
        }
    }
}
