//! Passwords of encrypted archives, kept in memory only.
//!
//! NemesisCommander never writes a password to disk. An archive that needs
//! one marks itself here when it is opened or read; the application asks
//! the user ([`needs_password`]) and hands the answer back
//! ([`set_password`]), after which reading goes on.

use std::collections::HashMap;
use std::io;
use std::sync::{Mutex, MutexGuard, OnceLock};

use nmc_vfs::VfsPath;

#[derive(Default)]
struct Slot {
    /// Something in the archive is encrypted.
    locked: bool,
    /// The password given by the user.
    password: Option<String>,
    /// The password given did not fit.
    wrong: bool,
}

fn store() -> MutexGuard<'static, HashMap<String, Slot>> {
    static STORE: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    STORE
        .get_or_init(Mutex::default)
        .lock()
        // The map stays consistent even if a thread panicked holding it.
        .unwrap_or_else(|e| e.into_inner())
}

/// The key of an archive: its container's location.
pub(crate) fn key_of(container: &VfsPath) -> String {
    container.to_string()
}

/// Whether the archive in `container` is encrypted and has no (working)
/// password yet.
pub fn needs_password(container: &VfsPath) -> bool {
    store().get(&key_of(container)).is_some_and(|s| s.locked && (s.password.is_none() || s.wrong))
}

/// Whether the last password given for `container` was refused.
pub fn was_refused(container: &VfsPath) -> bool {
    store().get(&key_of(container)).is_some_and(|s| s.wrong)
}

/// Gives the password of the archive in `container`.
pub fn set_password(container: &VfsPath, password: &str) {
    let mut store = store();
    let slot = store.entry(key_of(container)).or_default();
    slot.password = Some(password.to_owned());
    slot.wrong = false;
}

/// Forgets what is known about the archive in `container` (and its password).
pub fn forget(container: &VfsPath) {
    store().remove(&key_of(container));
}

pub(crate) fn lock(key: &str) {
    store().entry(key.to_owned()).or_default().locked = true;
}

pub(crate) fn refuse(key: &str) {
    let mut store = store();
    let slot = store.entry(key.to_owned()).or_default();
    slot.locked = true;
    slot.wrong = true;
}

pub(crate) fn password_for(key: &str) -> Option<String> {
    store().get(key).and_then(|s| s.password.clone())
}

/// Why a member could not be read, as an I/O error that says so.
#[derive(Debug)]
pub(crate) struct PasswordProblem {
    pub wrong: bool,
}

impl std::fmt::Display for PasswordProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.wrong {
            "wrong password for this encrypted archive"
        } else {
            "this archive is encrypted: a password is needed"
        })
    }
}

impl std::error::Error for PasswordProblem {}

pub(crate) fn problem(wrong: bool) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, PasswordProblem { wrong })
}
