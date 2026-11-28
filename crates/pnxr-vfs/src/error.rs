use std::io;

use crate::VfsPath;

/// Convenience alias used across the VFS.
pub type Result<T, E = VfsError> = std::result::Result<T, E>;

/// Errors any provider can report.
///
/// Providers translate their native errors into these variants so the rest
/// of the application can react uniformly (for example, conflict handling
/// matches on [`VfsError::AlreadyExists`] regardless of the backend).
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    /// The path does not exist.
    #[error("not found: {}", .0.shown())]
    NotFound(VfsPath),
    /// The path already exists and the operation would not replace it.
    #[error("already exists: {}", .0.shown())]
    AlreadyExists(VfsPath),
    /// The provider refused access.
    #[error("permission denied: {}", .0.shown())]
    PermissionDenied(VfsPath),
    /// A directory was expected.
    #[error("not a directory: {}", .0.shown())]
    NotADirectory(VfsPath),
    /// A file was expected.
    #[error("is a directory: {}", .0.shown())]
    IsADirectory(VfsPath),
    /// A directory could not be removed because it is not empty.
    #[error("directory not empty: {}", .0.shown())]
    DirectoryNotEmpty(VfsPath),
    /// The provider does not implement this operation.
    #[error("{operation} is not supported by {scheme}://")]
    Unsupported {
        /// Name of the operation, e.g. `"rename"`.
        operation: &'static str,
        /// The provider's URI scheme.
        scheme: String,
    },
    /// No provider is registered for the scheme.
    #[error("no provider for scheme {0:?}")]
    UnknownScheme(String),
    /// A string could not be parsed as a [`VfsPath`].
    #[error("invalid path {input:?}: {reason}")]
    InvalidPath {
        /// The offending input.
        input: String,
        /// A human readable reason.
        reason: &'static str,
    },
    /// The operation was cancelled by the user.
    #[error("cancelled")]
    Cancelled,
    /// A low-level I/O error on a specific path.
    #[error("{}: {source}", path.shown())]
    Io {
        /// The path that was being accessed.
        path: VfsPath,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// An error reported by a remote peer or library, already formatted.
    #[error("{0}")]
    Other(String),
}

impl VfsError {
    /// Maps an [`io::Error`] to the most specific variant for `path`.
    pub fn from_io(path: &VfsPath, err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::NotFound => VfsError::NotFound(path.clone()),
            io::ErrorKind::AlreadyExists => VfsError::AlreadyExists(path.clone()),
            io::ErrorKind::PermissionDenied => VfsError::PermissionDenied(path.clone()),
            io::ErrorKind::NotADirectory => VfsError::NotADirectory(path.clone()),
            io::ErrorKind::IsADirectory => VfsError::IsADirectory(path.clone()),
            io::ErrorKind::DirectoryNotEmpty => VfsError::DirectoryNotEmpty(path.clone()),
            _ => VfsError::Io { path: path.clone(), source: err },
        }
    }

    /// Turns the error into an [`io::Error`] whose kind matches the
    /// variant, for code that must report through an I/O interface (a
    /// [`WriteSink`](crate::WriteSink), for example). [`from_io`](Self::from_io)
    /// maps it back to the same variant.
    pub fn into_io(self) -> io::Error {
        let kind = match &self {
            VfsError::NotFound(_) => io::ErrorKind::NotFound,
            VfsError::AlreadyExists(_) => io::ErrorKind::AlreadyExists,
            VfsError::PermissionDenied(_) => io::ErrorKind::PermissionDenied,
            VfsError::NotADirectory(_) => io::ErrorKind::NotADirectory,
            VfsError::IsADirectory(_) => io::ErrorKind::IsADirectory,
            VfsError::DirectoryNotEmpty(_) => io::ErrorKind::DirectoryNotEmpty,
            VfsError::Io { source, .. } => source.kind(),
            _ => io::ErrorKind::Other,
        };
        match self {
            VfsError::Io { source, .. } => source,
            other => io::Error::new(kind, other.to_string()),
        }
    }

    /// Shorthand for [`VfsError::Unsupported`].
    pub fn unsupported(operation: &'static str, scheme: impl Into<String>) -> Self {
        VfsError::Unsupported { operation, scheme: scheme.into() }
    }

    /// `true` if the error means "the path does not exist".
    pub fn is_not_found(&self) -> bool {
        matches!(self, VfsError::NotFound(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_round_trip_keeps_the_kind() {
        let path = VfsPath::parse("/tmp/x").unwrap();
        for error in [
            VfsError::NotFound(path.clone()),
            VfsError::AlreadyExists(path.clone()),
            VfsError::DirectoryNotEmpty(path.clone()),
        ] {
            let text = format!("{error:?}");
            let back = VfsError::from_io(&path, error.into_io());
            assert_eq!(format!("{back:?}"), text);
        }
    }

    #[test]
    fn io_errors_map_to_specific_variants() {
        let p = VfsPath::parse("/tmp/x").unwrap();
        let e = VfsError::from_io(&p, io::Error::from(io::ErrorKind::NotFound));
        assert!(e.is_not_found());
        let e = VfsError::from_io(&p, io::Error::from(io::ErrorKind::AlreadyExists));
        assert!(matches!(e, VfsError::AlreadyExists(_)));
        let e = VfsError::from_io(&p, io::Error::other("boom"));
        assert!(matches!(e, VfsError::Io { .. }));
    }
}
