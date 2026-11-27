//! The port to the store that keeps saved logins.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::netrc::Login;

/// Saves and finds logins. The interfaces hold a `dyn SecretStore`, so that
/// tests can run without touching the system's keyring.
pub trait SecretStore: Send + Sync {
    /// What the store is called, for the login question.
    fn name(&self) -> &'static str;
    /// What is missing when [`available`](Self::available) is false.
    fn missing(&self) -> &'static str;
    /// Whether logins can be saved here.
    fn available(&self) -> bool;
    /// Saves the password of `user` on `host`, replacing the one stored before.
    fn store(&self, host: &str, user: &str, password: &str) -> std::io::Result<()>;
    /// Removes the stored login (nothing happens when there is none).
    fn clear(&self, host: &str, user: &str) -> std::io::Result<()>;
    /// The stored login of `user` on `host`, or the first one of the host.
    fn lookup(&self, host: &str, user: Option<&str>) -> Option<Login>;
}

/// The system's own store: the Secret Service, the Keychain or the
/// Credential Manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSecrets;

impl SecretStore for SystemSecrets {
    fn name(&self) -> &'static str {
        crate::keyring::store_name()
    }
    fn missing(&self) -> &'static str {
        crate::keyring::missing()
    }
    fn available(&self) -> bool {
        crate::keyring::available()
    }
    fn store(&self, host: &str, user: &str, password: &str) -> std::io::Result<()> {
        crate::keyring::store(host, user, password)
    }
    fn clear(&self, host: &str, user: &str) -> std::io::Result<()> {
        crate::keyring::clear(host, user)
    }
    fn lookup(&self, host: &str, user: Option<&str>) -> Option<Login> {
        crate::keyring::lookup(host, user)
    }
}

/// A store in memory, for tests.
#[derive(Debug, Default)]
pub struct MemorySecrets {
    logins: Mutex<HashMap<(String, String), String>>,
}

impl MemorySecrets {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecrets {
    fn name(&self) -> &'static str {
        "the test store"
    }
    fn missing(&self) -> &'static str {
        ""
    }
    fn available(&self) -> bool {
        true
    }
    fn store(&self, host: &str, user: &str, password: &str) -> std::io::Result<()> {
        if host.is_empty() || user.is_empty() || password.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a host, a user and a password are needed",
            ));
        }
        let mut map = self.logins.lock().unwrap_or_else(|e| e.into_inner());
        map.insert((host.to_lowercase(), user.to_owned()), password.to_owned());
        Ok(())
    }
    fn clear(&self, host: &str, user: &str) -> std::io::Result<()> {
        let mut map = self.logins.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&(host.to_lowercase(), user.to_owned()));
        Ok(())
    }
    fn lookup(&self, host: &str, user: Option<&str>) -> Option<Login> {
        let host = host.to_lowercase();
        let map = self.logins.lock().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .filter(|((h, u), _)| *h == host && user.is_none_or(|want| want == u))
            .min_by(|a, b| a.0.1.cmp(&b.0.1))
            .map(|((_, u), p)| Login { user: u.clone(), password: p.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_round_trips_through_the_fake() {
        let store: Box<dyn SecretStore> = Box::new(MemorySecrets::new());
        assert!(store.lookup("Files.Invalid", Some("anna")).is_none());
        store.store("Files.Invalid", "anna", "s3cret").unwrap();
        let found = store.lookup("files.invalid", Some("anna")).unwrap();
        assert_eq!((found.user.as_str(), found.password.as_str()), ("anna", "s3cret"));
        assert_eq!(store.lookup("files.invalid", None).unwrap().user, "anna");
        store.clear("files.invalid", "anna").unwrap();
        assert!(store.lookup("files.invalid", Some("anna")).is_none());
        assert!(store.store("files.invalid", "", "x").is_err());
    }
}
