//! The private certificate authority of a controller, and the bundles it
//! issues to servers.
//!
//! The controller's folder (`pnxr tls init`, normally in the configuration
//! folder) holds:
//!
//! | File | What |
//! |------|------|
//! | `ca.pem`, `ca.key` | the authority |
//! | `controller.pem`, `controller.key` | the controller's client certificate |
//! | `token.key` | the Ed25519 key that signs transfer tokens |
//!
//! A server's bundle (`pnxr tls server NAME... --out DIR`, copied to the
//! server and given to `pnxr serve --tls DIR`) holds:
//!
//! | File | What |
//! |------|------|
//! | `ca.pem` | the authority, to check the other side |
//! | `server.pem`, `server.key` | the server's certificate, valid for its names, for serving and for contacting other servers |
//! | `names` | those names, one per line |
//! | `controller.pem` | the controller's certificate: connections presenting it may do everything |
//! | `controller-token.pub` | the key that checks transfer tokens |
//!
//! Certificates are ECDSA P-256. Private keys are written readable by the
//! owner only.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};

use crate::token::{self, Grant, TokenError};
use crate::{Error, Result, crypto_provider};

const CA_NAME: &str = "Pairnox CA";

/// Who is on the other end of a connection to `pnxr serve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The controller: may do everything the server's limits allow.
    Controller,
    /// Another server of the same authority: needs a transfer token for
    /// every path it touches.
    Server,
}

/// The controller's authority, certificate and token key.
pub struct ControllerIdentity {
    dir: PathBuf,
    ca: CertificateDer<'static>,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    token_key: Ed25519KeyPair,
}

impl ControllerIdentity {
    /// Whether `dir` holds an identity already.
    pub fn exists(dir: &Path) -> bool {
        dir.join("ca.pem").exists()
    }

    /// Creates a new authority and controller certificate in `dir`. Refuses
    /// to replace an existing one: servers trust it.
    pub fn create(dir: &Path) -> Result<Self> {
        if Self::exists(dir) {
            return Err(Error::Invalid(format!(
                "{}: a certificate authority exists already",
                dir.display()
            )));
        }
        fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;

        let ca_key = KeyPair::generate()?;
        let ca = ca_params().self_signed(&ca_key)?;
        let issuer = Issuer::new(ca_params(), ca_key);

        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.distinguished_name.push(DnType::CommonName, "Pairnox controller");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let key = KeyPair::generate()?;
        let cert = params.signed_by(&key, &issuer)?;

        write(&dir.join("ca.pem"), ca.pem().as_bytes(), false)?;
        write(&dir.join("ca.key"), issuer_key_pem(&issuer).as_bytes(), true)?;
        write(&dir.join("controller.pem"), cert.pem().as_bytes(), false)?;
        write(&dir.join("controller.key"), key.serialize_pem().as_bytes(), true)?;
        write(&dir.join("token.key"), &token::generate_key(), true)?;
        Self::load(dir)
    }

    /// Loads the identity in `dir`.
    pub fn load(dir: &Path) -> Result<Self> {
        let token_bytes = read(&dir.join("token.key"))?;
        let token_key = Ed25519KeyPair::from_pkcs8(&token_bytes).map_err(|_| {
            Error::Invalid(format!("{}: not a token key", dir.join("token.key").display()))
        })?;
        Ok(ControllerIdentity {
            dir: dir.to_path_buf(),
            ca: load_cert(&dir.join("ca.pem"))?,
            cert: load_cert(&dir.join("controller.pem"))?,
            key: load_key(&dir.join("controller.key"))?,
            token_key,
        })
    }

    /// Writes a bundle for a server reachable as `names` (host names or IP
    /// addresses, as the controller and the other servers will write them)
    /// into `out`.
    pub fn issue_server(&self, names: &[String], out: &Path) -> Result<()> {
        if names.is_empty() {
            return Err(Error::Invalid("a server needs at least one name".into()));
        }
        let ca_key = KeyPair::from_pem(&read_text(&self.dir.join("ca.key"))?)?;
        let issuer = Issuer::new(ca_params(), ca_key);
        let mut params = CertificateParams::new(names.to_vec())?;
        params.distinguished_name.push(DnType::CommonName, names[0].as_str());
        // Serving, and contacting other servers for direct transfers.
        params.extended_key_usages =
            vec![ExtendedKeyUsagePurpose::ServerAuth, ExtendedKeyUsagePurpose::ClientAuth];
        let key = KeyPair::generate()?;
        let cert = params.signed_by(&key, &issuer)?;

        fs::create_dir_all(out).map_err(|e| io_error(out, e))?;
        fs::copy(self.dir.join("ca.pem"), out.join("ca.pem")).map_err(|e| io_error(out, e))?;
        fs::copy(self.dir.join("controller.pem"), out.join("controller.pem"))
            .map_err(|e| io_error(out, e))?;
        write(&out.join("server.pem"), cert.pem().as_bytes(), false)?;
        write(&out.join("server.key"), key.serialize_pem().as_bytes(), true)?;
        write(&out.join("names"), format!("{}\n", names.join("\n")).as_bytes(), false)?;
        let public = hex(self.token_key.public_key().as_ref());
        write(&out.join("controller-token.pub"), format!("{public}\n").as_bytes(), false)
    }

    /// The configuration for connecting to servers.
    pub fn client_config(&self) -> Result<Arc<ClientConfig>> {
        client_config(&self.ca, self.cert.clone(), self.key.clone_key())
    }

    /// Signs `grant` as a transfer token.
    pub fn sign(&self, grant: &Grant) -> String {
        token::sign(&self.token_key, grant)
    }
}

/// A server's bundle, as loaded by `pnxr serve`.
pub struct ServerIdentity {
    names: Vec<String>,
    ca: CertificateDer<'static>,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    controller: CertificateDer<'static>,
    token_public: Vec<u8>,
}

impl ServerIdentity {
    /// Loads the bundle in `dir`.
    pub fn load(dir: &Path) -> Result<Self> {
        let names: Vec<String> = read_text(&dir.join("names"))?
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        let token_file = dir.join("controller-token.pub");
        let token_public = unhex(read_text(&token_file)?.trim())
            .ok_or_else(|| Error::Invalid(format!("{}: not a public key", token_file.display())))?;
        Ok(ServerIdentity {
            names,
            ca: load_cert(&dir.join("ca.pem"))?,
            cert: load_cert(&dir.join("server.pem"))?,
            key: load_key(&dir.join("server.key"))?,
            controller: load_cert(&dir.join("controller.pem"))?,
            token_public,
        })
    }

    /// The names in the server's certificate.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The configuration for serving: the client must present a certificate
    /// of the same authority.
    pub fn server_config(&self) -> Result<Arc<ServerConfig>> {
        let provider = crypto_provider();
        let verifier = WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots(&self.ca)?),
            provider.clone(),
        )
        .build()
        .map_err(|e| Error::Invalid(format!("client verification: {e}")))?;
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![self.cert.clone()], self.key.clone_key())?;
        config.alpn_protocols = vec![b"h2".to_vec()];
        Ok(Arc::new(config))
    }

    /// The configuration for contacting another server (direct transfers),
    /// identified by this server's certificate.
    pub fn peer_client_config(&self) -> Result<Arc<ClientConfig>> {
        client_config(&self.ca, self.cert.clone(), self.key.clone_key())
    }

    /// The role of a client that presented `certificate` (already verified
    /// against the authority by TLS).
    pub fn role_of(&self, certificate: &CertificateDer<'_>) -> Role {
        if certificate.as_ref() == self.controller.as_ref() {
            Role::Controller
        } else {
            Role::Server
        }
    }

    /// Checks a transfer token addressed to this server.
    pub fn verify(&self, token: &str) -> std::result::Result<Grant, TokenError> {
        token::verify(&self.token_public, token, &self.names, token::now())
    }
}

/// The authority's certificate parameters. Rebuilt identically when the
/// authority issues certificates, so its key is the only state kept.
fn ca_params() -> CertificateParams {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, CA_NAME);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params
}

fn issuer_key_pem(issuer: &Issuer<'_, KeyPair>) -> String {
    issuer.key().serialize_pem()
}

fn client_config(
    ca: &CertificateDer<'static>,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<ClientConfig>> {
    let mut config = ClientConfig::builder_with_provider(crypto_provider())
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots(ca)?)
        .with_client_auth_cert(vec![cert], key)?;
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(Arc::new(config))
}

fn roots(ca: &CertificateDer<'static>) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    roots.add(ca.clone())?;
    Ok(roots)
}

fn load_cert(path: &Path) -> Result<CertificateDer<'static>> {
    CertificateDer::from_pem_file(path)
        .map_err(|e| Error::Invalid(format!("{}: not a certificate ({e})", path.display())))
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_file(path)
        .map_err(|e| Error::Invalid(format!("{}: not a private key ({e})", path.display())))
}

fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::Io { path: path.display().to_string(), source }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| io_error(path, e))
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| io_error(path, e))
}

/// Writes `data` to `path`; `secret` files are readable by the owner only.
fn write(path: &Path, data: &[u8], secret: bool) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = secret;
    use std::io::Write;
    options.open(path).and_then(|mut file| file.write_all(data)).map_err(|e| io_error(path, e))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}
