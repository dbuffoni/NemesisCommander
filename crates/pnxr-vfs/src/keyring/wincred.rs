//! The Windows Credential Manager, through the Win32 credential API. A login
//! is a generic credential named `pairnox:HOST:USER`, with the user
//! name and the password (UTF-16, like the Windows tools keep them), saved
//! for this user on this machine.
#![allow(unsafe_code)]

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredEnumerateW,
    CredFree, CredReadW, CredWriteW,
};

use super::SERVICE;
use crate::netrc::Login;

pub(super) const STORE_NAME: &str = "the Windows Credential Manager";
pub(super) const MISSING: &str = "unavailable";

const ERROR_NOT_FOUND: u32 = 1168;

pub(super) fn available() -> bool {
    true
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn target(host: &str, user: &str) -> String {
    format!("{SERVICE}:{host}:{user}")
}

fn last_error(what: &str) -> io::Error {
    // SAFETY: GetLastError has no preconditions.
    let code = unsafe { GetLastError() };
    io::Error::other(format!("{what}: Windows error {code}"))
}

pub(super) fn store(host: &str, user: &str, password: &str) -> io::Result<()> {
    let mut name = wide(&target(host, user));
    let mut user_name = wide(user);
    let mut blob: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let Ok(size) = u32::try_from(blob.len()) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the password is too long"));
    };
    // SAFETY: a zeroed CREDENTIALW is valid; the pointers stay valid for the
    // call, and CredWriteW copies what it needs.
    let written = unsafe {
        let mut credential: CREDENTIALW = std::mem::zeroed();
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = name.as_mut_ptr();
        credential.UserName = user_name.as_mut_ptr();
        credential.CredentialBlobSize = size;
        credential.CredentialBlob = blob.as_mut_ptr();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        CredWriteW(&credential, 0)
    };
    if written == 0 {
        return Err(last_error("the Credential Manager refused it"));
    }
    Ok(())
}

pub(super) fn clear(host: &str, user: &str) -> io::Result<()> {
    let name = wide(&target(host, user));
    // SAFETY: `name` is a NUL-terminated UTF-16 string.
    let deleted = unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) };
    // SAFETY: no preconditions.
    if deleted == 0 && unsafe { GetLastError() } != ERROR_NOT_FOUND {
        return Err(last_error("cannot remove the login"));
    }
    Ok(())
}

/// The user name and password inside a credential returned by the API.
///
/// # Safety
/// `credential` must point to a valid CREDENTIALW.
unsafe fn read_credential(credential: *const CREDENTIALW) -> Option<Login> {
    // SAFETY: the caller guarantees the pointer; the blob and user name are
    // valid for the sizes the structure states.
    unsafe {
        let credential = &*credential;
        if credential.UserName.is_null() || credential.CredentialBlob.is_null() {
            return None;
        }
        let mut length = 0;
        while *credential.UserName.add(length) != 0 {
            length += 1;
        }
        let user =
            String::from_utf16(std::slice::from_raw_parts(credential.UserName, length)).ok()?;
        let bytes = std::slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        );
        let units: Vec<u16> =
            bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        let password = String::from_utf16(&units).ok()?;
        (!password.is_empty()).then_some(Login { user, password })
    }
}

pub(super) fn lookup(host: &str, user: Option<&str>) -> Option<Login> {
    if let Some(user) = user {
        let name = wide(&target(host, user));
        let mut found: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: `found` receives a buffer that CredFree releases.
        unsafe {
            if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) == 0 {
                return None;
            }
            let login = read_credential(found);
            CredFree(found.cast());
            return login;
        }
    }
    let filter = wide(&format!("{SERVICE}:{host}:*"));
    let (mut count, mut list) = (0u32, ptr::null_mut::<*mut CREDENTIALW>());
    // SAFETY: the list holds `count` valid pointers until CredFree.
    unsafe {
        if CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut list) == 0 {
            return None;
        }
        let login = (0..count as usize).find_map(|i| read_credential(*list.add(i)));
        CredFree(list.cast());
        login
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real Credential Manager (runs on the Windows runner of the nightly CI):
    /// a throwaway host name, removed at the end.
    #[test]
    fn a_login_is_stored_found_and_cleared() {
        let host = format!("pnxr-test-{}.invalid", std::process::id());
        store(&host, "anna", "s3 \"cret\\ é").unwrap();
        let login = lookup(&host, Some("anna")).unwrap();
        assert_eq!((login.user.as_str(), login.password.as_str()), ("anna", "s3 \"cret\\ é"));
        assert_eq!(lookup(&host, Some("bob")), None);
        assert_eq!(lookup(&host, None).unwrap().user, "anna");
        store(&host, "anna", "second").unwrap();
        assert_eq!(lookup(&host, Some("anna")).unwrap().password, "second");
        clear(&host, "anna").unwrap();
        assert_eq!(lookup(&host, Some("anna")), None);
        clear(&host, "anna").unwrap();
    }
}
