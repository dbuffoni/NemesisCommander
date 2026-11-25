//! A minimal reader for RAR archives (RAR 3/4 and RAR 5) on top of the
//! bundled libarchive. It lists and extracts; nothing here writes archives.

#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_int, c_long, c_void};
use std::path::Path;
use std::ptr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[repr(C)]
struct RawArchive {
    _private: [u8; 0],
}

#[repr(C)]
struct RawEntry {
    _private: [u8; 0],
}

const ARCHIVE_OK: c_int = 0;
const ARCHIVE_EOF: c_int = 1;
const ARCHIVE_WARN: c_int = -20;
const AE_IFMT: u32 = 0o170000;
const AE_IFDIR: u32 = 0o040000;
const AE_IFLNK: u32 = 0o120000;

unsafe extern "C" {
    fn archive_read_new() -> *mut RawArchive;
    fn archive_read_free(a: *mut RawArchive) -> c_int;
    fn archive_read_support_format_rar(a: *mut RawArchive) -> c_int;
    fn archive_read_support_format_rar5(a: *mut RawArchive) -> c_int;
    #[cfg(not(windows))]
    fn archive_read_open_filename(a: *mut RawArchive, name: *const c_char, block: usize) -> c_int;
    #[cfg(windows)]
    fn archive_read_open_filename_w(a: *mut RawArchive, name: *const u16, block: usize) -> c_int;
    fn archive_read_next_header(a: *mut RawArchive, entry: *mut *mut RawEntry) -> c_int;
    fn archive_read_data(a: *mut RawArchive, buffer: *mut c_void, len: usize) -> isize;
    fn archive_error_string(a: *mut RawArchive) -> *const c_char;
    fn archive_entry_pathname_utf8(e: *mut RawEntry) -> *const c_char;
    fn archive_entry_pathname(e: *mut RawEntry) -> *const c_char;
    fn archive_entry_size(e: *mut RawEntry) -> i64;
    fn archive_entry_filetype(e: *mut RawEntry) -> u32;
    fn archive_entry_mtime(e: *mut RawEntry) -> c_long;
    fn archive_entry_mtime_is_set(e: *mut RawEntry) -> c_int;
    fn archive_entry_is_encrypted(e: *mut RawEntry) -> c_int;
}

/// What went wrong while reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The archive (or one member) is encrypted.
    Encrypted,
    /// libarchive's own message.
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Encrypted => f.write_str("the archive is encrypted"),
            Error::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

/// One member of the archive, as read from its header.
#[derive(Debug, Clone)]
pub struct Member {
    /// Path inside the archive, as UTF-8 (lossy when it is not).
    pub path: String,
    /// Size in bytes after extraction.
    pub size: u64,
    /// A folder.
    pub is_dir: bool,
    /// A symbolic link.
    pub is_symlink: bool,
    /// Modification time, when the archive has one.
    pub modified: Option<SystemTime>,
    /// The data is encrypted.
    pub encrypted: bool,
}

/// An open RAR archive, read front to back.
pub struct RarReader {
    archive: *mut RawArchive,
}

// The handle is used by one thread at a time (it is not `Sync`).
unsafe impl Send for RarReader {}

impl RarReader {
    /// Opens the RAR file at `path`.
    pub fn open(path: &Path) -> Result<Self, Error> {
        // SAFETY: the pointers passed are valid for the call; the archive is
        // freed in `Drop`.
        unsafe {
            let archive = archive_read_new();
            if archive.is_null() {
                return Err(Error::Other("out of memory".into()));
            }
            let reader = RarReader { archive };
            if archive_read_support_format_rar(archive) < ARCHIVE_WARN
                || archive_read_support_format_rar5(archive) < ARCHIVE_WARN
            {
                return Err(reader.error());
            }
            #[cfg(not(windows))]
            let code = {
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::CString::new(path.as_os_str().as_bytes())
                    .map_err(|_| Error::Other("the path contains a NUL byte".into()))?;
                archive_read_open_filename(archive, name.as_ptr(), 128 * 1024)
            };
            #[cfg(windows)]
            let code = {
                use std::os::windows::ffi::OsStrExt;
                let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
                archive_read_open_filename_w(archive, name.as_ptr(), 128 * 1024)
            };
            if code < ARCHIVE_OK {
                return Err(reader.error());
            }
            Ok(reader)
        }
    }

    fn error(&self) -> Error {
        // SAFETY: `archive` is valid until `Drop`; the string is copied.
        let text = unsafe {
            let message = archive_error_string(self.archive);
            if message.is_null() {
                "unknown error".to_owned()
            } else {
                CStr::from_ptr(message).to_string_lossy().into_owned()
            }
        };
        let lower = text.to_lowercase();
        if lower.contains("encrypt") || lower.contains("passphrase") || lower.contains("password") {
            Error::Encrypted
        } else {
            Error::Other(text)
        }
    }

    /// The next member, or `None` at the end. Read its data (or not) before
    /// calling this again.
    pub fn next_member(&mut self) -> Result<Option<Member>, Error> {
        // SAFETY: `entry` is set by libarchive and valid until the next header.
        unsafe {
            let mut entry: *mut RawEntry = ptr::null_mut();
            let code = archive_read_next_header(self.archive, &mut entry);
            if code == ARCHIVE_EOF {
                return Ok(None);
            }
            if code < ARCHIVE_WARN {
                return Err(self.error());
            }
            let name = {
                let utf8 = archive_entry_pathname_utf8(entry);
                let raw = if utf8.is_null() { archive_entry_pathname(entry) } else { utf8 };
                if raw.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(raw).to_string_lossy().into_owned()
                }
            };
            let kind = archive_entry_filetype(entry) & AE_IFMT;
            let modified = (archive_entry_mtime_is_set(entry) != 0).then(|| {
                let seconds = archive_entry_mtime(entry) as i64;
                if seconds >= 0 {
                    UNIX_EPOCH + Duration::from_secs(seconds as u64)
                } else {
                    UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs())
                }
            });
            Ok(Some(Member {
                path: name,
                size: archive_entry_size(entry).max(0) as u64,
                is_dir: kind == AE_IFDIR,
                is_symlink: kind == AE_IFLNK,
                modified,
                encrypted: archive_entry_is_encrypted(entry) != 0,
            }))
        }
    }

    /// Reads data of the current member into `buffer`; 0 at its end.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        // SAFETY: the buffer is valid for `len` bytes.
        let n =
            unsafe { archive_read_data(self.archive, buffer.as_mut_ptr().cast(), buffer.len()) };
        if n < 0 { Err(self.error()) } else { Ok(n as usize) }
    }
}

impl Drop for RarReader {
    fn drop(&mut self) {
        // SAFETY: freed once; `archive` is not used afterwards.
        unsafe {
            archive_read_free(self.archive);
        }
    }
}
