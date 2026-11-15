//! Conversions between VFS types and protocol messages.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sc_vfs::{Entry, EntryKind, VfsError, VfsPath};
use tonic::{Code, Status};

use crate::pb;

/// Metadata key carrying the precise error kind next to the gRPC code, for
/// kinds that share a code (`not-a-directory`, `is-a-directory`, ...).
pub const ERROR_KIND_KEY: &str = "sc-error-kind";

/// Nanoseconds since the Unix epoch (negative before 1970).
pub fn time_to_ns(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}

/// The inverse of [`time_to_ns`].
pub fn ns_to_time(ns: i64) -> SystemTime {
    if ns >= 0 {
        UNIX_EPOCH + Duration::from_nanos(ns as u64)
    } else {
        UNIX_EPOCH - Duration::from_nanos(ns.unsigned_abs())
    }
}

/// An entry as sent on the wire.
pub fn entry_to_pb(entry: &Entry) -> pb::Entry {
    let kind = match entry.kind {
        EntryKind::File => pb::EntryKind::File,
        EntryKind::Directory => pb::EntryKind::Directory,
        EntryKind::Symlink => pb::EntryKind::Symlink,
        EntryKind::Other => pb::EntryKind::Other,
    };
    pb::Entry {
        name: entry.name.clone(),
        kind: kind as i32,
        size: entry.size,
        modified_ns: entry.modified.map(time_to_ns),
        permissions: entry.permissions,
        uid: entry.uid,
        gid: entry.gid,
        symlink_target: entry.symlink_target.clone(),
        link_is_dir: entry.link_is_dir,
    }
}

/// An entry received from the wire. Unknown kinds become
/// [`EntryKind::Other`].
pub fn entry_from_pb(entry: pb::Entry) -> Entry {
    let kind = match pb::EntryKind::try_from(entry.kind) {
        Ok(pb::EntryKind::File) => EntryKind::File,
        Ok(pb::EntryKind::Directory) => EntryKind::Directory,
        Ok(pb::EntryKind::Symlink) => EntryKind::Symlink,
        _ => EntryKind::Other,
    };
    Entry {
        name: entry.name,
        kind,
        size: entry.size,
        modified: entry.modified_ns.map(ns_to_time),
        permissions: entry.permissions,
        uid: entry.uid,
        gid: entry.gid,
        symlink_target: entry.symlink_target,
        link_is_dir: entry.link_is_dir,
    }
}

fn with_kind(mut status: Status, kind: &'static str) -> Status {
    status.metadata_mut().insert(ERROR_KIND_KEY, kind.parse().expect("ASCII"));
    status
}

/// The status an agent returns for `error`.
pub fn error_to_status(error: &VfsError) -> Status {
    let message = error.to_string();
    match error {
        VfsError::NotFound(_) => Status::not_found(message),
        VfsError::AlreadyExists(_) => Status::already_exists(message),
        VfsError::PermissionDenied(_) => Status::permission_denied(message),
        VfsError::NotADirectory(_) => {
            with_kind(Status::failed_precondition(message), "not-a-directory")
        }
        VfsError::IsADirectory(_) => {
            with_kind(Status::failed_precondition(message), "is-a-directory")
        }
        VfsError::DirectoryNotEmpty(_) => {
            with_kind(Status::failed_precondition(message), "directory-not-empty")
        }
        VfsError::Unsupported { .. } => Status::unimplemented(message),
        VfsError::InvalidPath { .. } | VfsError::UnknownScheme(_) => {
            Status::invalid_argument(message)
        }
        VfsError::Cancelled => Status::cancelled(message),
        VfsError::Io { .. } | VfsError::Other(_) => Status::internal(message),
    }
}

/// The error a controller reports for `status` received while working on
/// `path` (a path in the controller's own `agent://` form).
pub fn status_to_error(status: &Status, path: &VfsPath) -> VfsError {
    let kind = status.metadata().get(ERROR_KIND_KEY).and_then(|v| v.to_str().ok());
    match (status.code(), kind) {
        (Code::NotFound, _) => VfsError::NotFound(path.clone()),
        (Code::AlreadyExists, _) => VfsError::AlreadyExists(path.clone()),
        (Code::PermissionDenied, _) => VfsError::PermissionDenied(path.clone()),
        (Code::FailedPrecondition, Some("not-a-directory")) => {
            VfsError::NotADirectory(path.clone())
        }
        (Code::FailedPrecondition, Some("is-a-directory")) => VfsError::IsADirectory(path.clone()),
        (Code::FailedPrecondition, Some("directory-not-empty")) => {
            VfsError::DirectoryNotEmpty(path.clone())
        }
        (Code::Unimplemented, _) => {
            VfsError::Unsupported { operation: "this operation", scheme: path.scheme().to_owned() }
        }
        (Code::Cancelled, _) => VfsError::Cancelled,
        (Code::Unavailable, _) => {
            VfsError::Other(format!("agent connection lost: {}", status.message()))
        }
        _ => VfsError::Other(format!("agent: {}", status.message())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip_including_before_1970() {
        // Multiples of 100 ns: Windows keeps times at that resolution.
        for ns in [0i64, 100, 1_700_000_000_123_456_700, -1_000_000_000] {
            assert_eq!(time_to_ns(ns_to_time(ns)), ns);
        }
    }

    #[test]
    fn entries_round_trip() {
        let mut entry = Entry::new("link", EntryKind::Symlink).with_size(42);
        entry.modified = Some(ns_to_time(1_600_000_000_000_000_000));
        entry.permissions = Some(0o755);
        entry.uid = Some(1000);
        entry.symlink_target = Some("target".into());
        entry.link_is_dir = true;
        assert_eq!(entry_from_pb(entry_to_pb(&entry)), entry);
    }

    #[test]
    fn unknown_entry_kinds_are_other() {
        let wire = pb::Entry { name: "x".into(), kind: 99, ..Default::default() };
        assert_eq!(entry_from_pb(wire).kind, EntryKind::Other);
    }

    #[test]
    fn errors_keep_their_kind_across_the_wire() {
        let remote = VfsPath::parse("file:///srv/x").unwrap();
        let local = VfsPath::parse("agent://host/srv/x").unwrap();
        let cases = [
            VfsError::NotFound(remote.clone()),
            VfsError::AlreadyExists(remote.clone()),
            VfsError::PermissionDenied(remote.clone()),
            VfsError::NotADirectory(remote.clone()),
            VfsError::IsADirectory(remote.clone()),
            VfsError::DirectoryNotEmpty(remote.clone()),
            VfsError::Cancelled,
        ];
        for error in cases {
            let back = status_to_error(&error_to_status(&error), &local);
            assert_eq!(
                std::mem::discriminant(&back),
                std::mem::discriminant(&error),
                "{error} became {back}"
            );
        }
        let back = status_to_error(&error_to_status(&VfsError::Other("boom".into())), &local);
        assert_eq!(back.to_string(), "agent: boom");
        let unsupported = VfsError::unsupported("rename", "file");
        assert!(matches!(
            status_to_error(&error_to_status(&unsupported), &local),
            VfsError::Unsupported { .. }
        ));
    }
}
