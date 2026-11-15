//! # sc-tls
//!
//! The security pieces of `sc serve` (see ADR 0014):
//!
//! * [`pki`]: a private certificate authority owned by the controller (the
//!   desktop that drives the servers). `sc tls init` creates it with the
//!   controller's own certificate; `sc tls server` issues a bundle for each
//!   server. Every connection is mutual TLS against that authority.
//! * [`token`]: one-time transfer tokens. For a server-to-server copy the
//!   controller signs a token naming the server that will be contacted, the
//!   paths, and an expiry; that server accepts the other server's
//!   connection only for those paths, and only on one connection.
//!
//! Nothing here touches the network: the configurations are handed to
//! `tokio-rustls` by the agent.

pub mod pki;
pub mod token;

pub use pki::{ControllerIdentity, Role, ServerIdentity};
pub use token::{Grant, TokenError};

/// The port `sc serve` listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7443;

/// Errors of this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// What went wrong.
        source: std::io::Error,
    },
    /// A file exists but does not hold what it should.
    #[error("{0}")]
    Invalid(String),
    /// Certificate generation failed.
    #[error("creating a certificate: {0}")]
    Certificate(#[from] rcgen::Error),
    /// The TLS library refused the configuration.
    #[error("TLS configuration: {0}")]
    Tls(#[from] rustls::Error),
}

/// Result of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The crypto provider of every configuration made here (`ring`: no C
/// toolchain beyond a C compiler, and it builds for static musl agents).
pub fn crypto_provider() -> std::sync::Arc<rustls::crypto::CryptoProvider> {
    std::sync::Arc::new(rustls::crypto::ring::default_provider())
}
