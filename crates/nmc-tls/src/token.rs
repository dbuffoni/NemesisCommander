//! One-time transfer tokens.
//!
//! A token is `<payload>.<signature>`, both base64url: the payload is a
//! [`Grant`] as JSON, the signature Ed25519 by the controller's token key.
//! The server named in the grant checks the signature against the
//! controller's public key from its bundle, the expiry, and its own name.
//! The agent then lets the token open one connection only (see the agent's
//! session handling), which is what makes it single-use.

use std::path::{Component, Path};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ED25519, Ed25519KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};

/// What a token allows, on one server, until it expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// Random identifier; a server accepts it on one connection only.
    pub id: String,
    /// The server that honors the token, by the name the other side uses
    /// to reach it (one of the names in its certificate).
    pub audience: String,
    /// Seconds since the Unix epoch after which the token is refused.
    pub expires: u64,
    /// Absolute paths that may be read, with everything below them.
    pub read: Vec<String>,
    /// Absolute paths that may be written (and read), with everything
    /// below them.
    pub write: Vec<String>,
}

/// Why a token was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// Not a token at all.
    #[error("malformed transfer token")]
    Malformed,
    /// Not signed by a trusted controller.
    #[error("transfer token not signed by a trusted controller")]
    BadSignature,
    /// Past its expiry.
    #[error("transfer token expired")]
    Expired,
    /// Issued for another server.
    #[error("transfer token issued for {0}")]
    WrongAudience(String),
    /// Already used on another connection.
    #[error("transfer token already used")]
    Used,
}

impl Grant {
    /// A grant for `audience`, valid for `lifetime` from now, with a fresh
    /// random identifier.
    pub fn new(audience: &str, read: Vec<String>, write: Vec<String>, lifetime: Duration) -> Self {
        let mut id = [0u8; 16];
        SystemRandom::new().fill(&mut id).expect("the system random generator works");
        Grant {
            id: id.iter().map(|b| format!("{b:02x}")).collect(),
            audience: audience.to_owned(),
            expires: now() + lifetime.as_secs(),
            read,
            write,
        }
    }

    /// Whether the grant covers `path`, for writing when `write` is set.
    /// `path` must be absolute and normalized (no `..`), as the agent makes
    /// it before checking.
    pub fn allows(&self, path: &str, write: bool) -> bool {
        let path = Path::new(path);
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return false;
        }
        let covered = |base: &String| path.starts_with(Path::new(base));
        self.write.iter().any(covered) || (!write && self.read.iter().any(covered))
    }
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Signs `grant` with the controller's token key.
pub fn sign(key: &Ed25519KeyPair, grant: &Grant) -> String {
    let payload = serde_json::to_vec(grant).expect("a grant serializes");
    let signature = key.sign(&payload);
    format!("{}.{}", URL_SAFE_NO_PAD.encode(&payload), URL_SAFE_NO_PAD.encode(signature.as_ref()))
}

/// Checks `token` against the controller's `public_key` and returns its
/// grant, if it is valid at time `now` for a server called one of `names`.
pub fn verify(
    public_key: &[u8],
    token: &str,
    names: &[String],
    now: u64,
) -> Result<Grant, TokenError> {
    let (payload, signature) = token.split_once('.').ok_or(TokenError::Malformed)?;
    let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| TokenError::Malformed)?;
    let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| TokenError::Malformed)?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&payload, &signature)
        .map_err(|_| TokenError::BadSignature)?;
    let grant: Grant = serde_json::from_slice(&payload).map_err(|_| TokenError::Malformed)?;
    if grant.expires < now {
        return Err(TokenError::Expired);
    }
    if !names.iter().any(|name| name.eq_ignore_ascii_case(&grant.audience)) {
        return Err(TokenError::WrongAudience(grant.audience));
    }
    Ok(grant)
}

/// A new Ed25519 key, as PKCS#8 bytes.
pub fn generate_key() -> Vec<u8> {
    Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .expect("the system random generator works")
        .as_ref()
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    fn key() -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(&generate_key()).unwrap()
    }

    fn grant() -> Grant {
        Grant::new(
            "src.example",
            vec!["/data/in".into()],
            vec!["/data/out".into()],
            Duration::from_secs(60),
        )
    }

    #[test]
    fn a_signed_token_verifies() {
        let key = key();
        let grant = grant();
        let token = sign(&key, &grant);
        let names = ["SRC.example".to_owned()];
        assert_eq!(verify(key.public_key().as_ref(), &token, &names, now()), Ok(grant));
    }

    #[test]
    fn tampering_expiry_and_audience_are_refused() {
        let key = key();
        let public = key.public_key().as_ref().to_vec();
        let names = ["src.example".to_owned()];
        let token = sign(&key, &grant());

        let (payload, signature) = token.split_once('.').unwrap();
        let mut forged: Grant =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
        forged.read.push("/".into());
        let forged =
            format!("{}.{signature}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged).unwrap()));
        assert_eq!(verify(&public, &forged, &names, now()), Err(TokenError::BadSignature));

        let other = key_public_of_another();
        assert_eq!(verify(&other, &token, &names, now()), Err(TokenError::BadSignature));
        assert_eq!(verify(&public, &token, &names, now() + 3600), Err(TokenError::Expired));
        let elsewhere = ["dst.example".to_owned()];
        assert!(matches!(
            verify(&public, &token, &elsewhere, now()),
            Err(TokenError::WrongAudience(_))
        ));
        assert_eq!(verify(&public, "garbage", &names, now()), Err(TokenError::Malformed));
    }

    fn key_public_of_another() -> Vec<u8> {
        key().public_key().as_ref().to_vec()
    }

    #[test]
    fn grants_cover_paths_below_their_folders() {
        let grant = grant();
        assert!(grant.allows("/data/in", false));
        assert!(grant.allows("/data/in/sub/file", false));
        assert!(!grant.allows("/data/in/sub/file", true), "read only");
        assert!(grant.allows("/data/out/new", true));
        assert!(grant.allows("/data/out/new", false), "write implies read");
        assert!(!grant.allows("/data/inside", false), "not a prefix of the name");
        assert!(!grant.allows("/data/in/../secret", false));
        assert!(!grant.allows("/etc/passwd", false));
    }

    #[test]
    fn identifiers_are_random() {
        assert_ne!(grant().id, grant().id);
        assert_eq!(grant().id.len(), 32);
    }
}
