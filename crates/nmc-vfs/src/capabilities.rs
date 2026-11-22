use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// What a [`Provider`](crate::Provider) supports natively.
    ///
    /// The operations and transfer engines read these flags to decide *how*
    /// to do something: for example a copy between two paths of a provider
    /// with [`Capabilities::SERVER_COPY`] never moves bytes through the client,
    /// and a transfer to a provider without [`Capabilities::RESUME_WRITE`]
    /// restarts interrupted files from scratch.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct Capabilities: u32 {
        /// Files can be created, written, and removed.
        const WRITE = 1 << 0;
        /// Reading can start at an arbitrary offset.
        const SEEK_READ = 1 << 1;
        /// Writing can continue an existing partial file (append at offset).
        const RESUME_WRITE = 1 << 2;
        /// `rename` is atomic and can replace an existing file.
        const ATOMIC_RENAME = 1 << 3;
        /// POSIX permission bits are meaningful and can be set.
        const POSIX_PERMISSIONS = 1 << 4;
        /// Modification time can be set.
        const SET_MTIME = 1 << 5;
        /// The provider can compute checksums without streaming the data out.
        const SERVER_HASH = 1 << 6;
        /// The provider can copy a file to another path of the same provider
        /// without streaming it through the client.
        const SERVER_COPY = 1 << 7;
        /// Symbolic links are reported and can be preserved.
        const SYMLINKS = 1 << 8;
        /// Items can be moved to a recycle bin instead of being deleted.
        const TRASH = 1 << 9;
        /// Directory changes can be watched.
        const WATCH = 1 << 10;
    }
}

impl Capabilities {
    /// `true` if the provider cannot be modified at all.
    pub fn is_read_only(self) -> bool {
        !self.contains(Capabilities::WRITE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_means_no_write_flag() {
        assert!(Capabilities::SEEK_READ.is_read_only());
        assert!(!(Capabilities::WRITE | Capabilities::SEEK_READ).is_read_only());
    }
}
