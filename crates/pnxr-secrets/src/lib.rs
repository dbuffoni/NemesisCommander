//! # pnxr-secrets
//!
//! Where the logins of Pairnox come from, kept apart from the file system
//! abstractions of `pnxr-vfs`:
//!
//! * [`keyring`]: the system's own secret store (ADR 0051);
//! * [`netrc`]: `~/.netrc`, and the logins typed for the session (ADR 0036);
//! * [`SecretStore`]: the port the interfaces use to save and look up a login,
//!   with [`SystemSecrets`] (the keyring) and [`MemorySecrets`] (a fake for
//!   tests) behind it.
//!
//! Providers that need a login call [`netrc::lookup`], which tries the session,
//! the keyring and the file in that order.

pub mod keyring;
pub mod netrc;
mod store;

pub use netrc::Login;
pub use store::{MemorySecrets, SecretStore, SystemSecrets};
