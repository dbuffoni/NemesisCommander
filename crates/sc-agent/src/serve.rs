//! `sc serve`: the agent as a permanent server, on its own port, over
//! mutual TLS (ADR 0014).
//!
//! Every connection must present a certificate of the controller's
//! authority. The controller's own certificate gets the same rights as
//! `sc agent --stdio`; another server's certificate gets nothing until it
//! presents a transfer token, and then only the token's paths. The limits
//! given on the command line (roots, read-only) apply to everyone.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sc_tls::{ControllerIdentity, DEFAULT_PORT, Role, ServerIdentity};
use sc_vfs::{Provider, ProviderFactory, ProviderRegistry, Result, VfsError, VfsPath};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::client::AgentProvider;
use crate::server::{AgentService, Limits, ServeState, Session, resolve_roots};

/// How long a client has to complete the TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Serves connections from `listener` until it fails. Each connection runs
/// on its own task; problems with one connection are reported on standard
/// error and do not stop the server.
pub async fn serve(
    listener: TcpListener,
    identity: ServerIdentity,
    limits: Limits,
) -> io::Result<()> {
    let limits = Arc::new(resolve_roots(limits)?);
    let acceptor = TlsAcceptor::from(identity.server_config().map_err(io::Error::other)?);
    let state = Arc::new(ServeState::new(identity));
    loop {
        let (tcp, address) = listener.accept().await?;
        let _ = tcp.set_nodelay(true);
        let (acceptor, state, limits) = (acceptor.clone(), state.clone(), limits.clone());
        tokio::spawn(async move {
            let tls = match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
                Ok(Ok(tls)) => tls,
                Ok(Err(e)) => return eprintln!("sc serve: {address}: {e}"),
                Err(_) => return eprintln!("sc serve: {address}: TLS handshake timed out"),
            };
            let role = tls
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|certificates| certificates.first())
                .map(|certificate| state.identity.role_of(certificate));
            let session = match role {
                Some(Role::Controller) => Session::Controller { serve: Some(state) },
                Some(Role::Server) => Session::Peer { serve: state, grant: Mutex::new(None) },
                // The verifier requires a certificate; this cannot happen.
                None => return,
            };
            let service = AgentService::with_session(limits, session);
            if let Err(e) = sc_proto::transport::serve(service, tls).await {
                eprintln!("sc serve: {address}: {e}");
            }
        });
    }
}

/// Connects to `sc serve` on `host:port` with `config` (the controller's
/// or, for server-to-server copies, a server's), presenting `token` if
/// given. `host` must be one of the names in the server's certificate.
pub async fn connect(
    config: Arc<ClientConfig>,
    host: &str,
    port: u16,
    token: Option<&str>,
) -> std::result::Result<AgentProvider, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{host}:{port}: {e}");
    let tcp = TcpStream::connect((host, port)).await.map_err(|e| fail(&e))?;
    let _ = tcp.set_nodelay(true);
    let name = ServerName::try_from(host.to_owned()).map_err(|e| fail(&e))?;
    let tls = TlsConnector::from(config).connect(name, tcp).await.map_err(|e| fail(&e))?;
    let client = sc_proto::transport::connect(tls).await.map_err(|e| fail(&e))?;
    AgentProvider::handshake_with(client, "scs", token, None).await.map_err(|e| fail(&e))
}

/// Splits `host[:port]` (the port defaults to [`DEFAULT_PORT`]).
pub fn host_and_port(authority: &str) -> std::result::Result<(String, u16), String> {
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    match authority.rsplit_once(':') {
        Some((host, port)) => {
            let port = port.parse().map_err(|_| format!("{authority}: invalid port"))?;
            Ok((host.to_owned(), port))
        }
        None => Ok((authority.to_owned(), DEFAULT_PORT)),
    }
}

/// Creates providers for `scs://host[:port]/path` with the controller's
/// identity from `dir` (see `sc tls init`).
pub struct ServeFactory {
    dir: PathBuf,
}

impl ServeFactory {
    /// A factory using the controller identity in `dir`.
    pub fn new(dir: PathBuf) -> Self {
        ServeFactory { dir }
    }

    /// The controller identity, with a hint when there is none yet.
    pub fn identity(&self) -> std::result::Result<ControllerIdentity, String> {
        if !ControllerIdentity::exists(&self.dir) {
            return Err(format!(
                "no certificates in {}: run `sc tls init`, then `sc tls server` for each server",
                self.dir.display()
            ));
        }
        ControllerIdentity::load(&self.dir).map_err(|e| e.to_string())
    }
}

#[async_trait]
impl ProviderFactory for ServeFactory {
    fn scheme(&self) -> &str {
        "scs"
    }

    async fn create(&self, path: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        let (host, port) = host_and_port(path.authority()).map_err(VfsError::Other)?;
        let config = self.identity().and_then(|id| id.client_config().map_err(|e| e.to_string()));
        let provider = connect(config.map_err(VfsError::Other)?, &host, port, None)
            .await
            .map_err(VfsError::Other)?;
        Ok(Arc::new(provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_default_to_the_serve_port() {
        assert_eq!(host_and_port("files.example"), Ok(("files.example".into(), DEFAULT_PORT)));
        assert_eq!(host_and_port("files.example:9000"), Ok(("files.example".into(), 9000)));
        assert_eq!(host_and_port("me@10.0.0.2:9000"), Ok(("10.0.0.2".into(), 9000)));
        assert!(host_and_port("host:x").is_err());
    }
}
