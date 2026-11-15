//! Real mutual TLS handshakes between the certificates the authority
//! issues, over a local TCP connection.

use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use sc_tls::{ControllerIdentity, Grant, Role, ServerIdentity, TokenError};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

struct Setup {
    _dir: TempDir,
    controller: ControllerIdentity,
    /// Reachable as `localhost` and `127.0.0.1`.
    server: ServerIdentity,
    /// Another server of the same authority.
    other: ServerIdentity,
}

fn setup() -> Setup {
    let dir = TempDir::new().unwrap();
    let controller = ControllerIdentity::create(&dir.path().join("controller")).unwrap();
    let names = ["localhost".to_owned(), "127.0.0.1".to_owned()];
    controller.issue_server(&names, &dir.path().join("server")).unwrap();
    controller.issue_server(&["other.example".to_owned()], &dir.path().join("other")).unwrap();
    Setup {
        server: ServerIdentity::load(&dir.path().join("server")).unwrap(),
        other: ServerIdentity::load(&dir.path().join("other")).unwrap(),
        controller,
        _dir: dir,
    }
}

/// Connects with `client` to a server using `server`, sends a byte back
/// and forth, and returns the role the server saw (or the error).
async fn handshake(
    server: &ServerIdentity,
    client: Arc<rustls::ClientConfig>,
    name: &str,
) -> Result<Role, String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let acceptor = TlsAcceptor::from(server.server_config().unwrap());
    let accept = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = acceptor.accept(tcp).await.map_err(|e| e.to_string())?;
        let certificate = tls.get_ref().1.peer_certificates().unwrap()[0].clone().into_owned();
        let mut byte = [0u8];
        tls.read_exact(&mut byte).await.map_err(|e| e.to_string())?;
        tls.write_all(&byte).await.map_err(|e| e.to_string())?;
        tls.flush().await.map_err(|e| e.to_string())?;
        Ok::<_, String>(certificate)
    });
    let tcp = TcpStream::connect(address).await.unwrap();
    let name = ServerName::try_from(name.to_owned()).unwrap();
    let connected = async {
        let mut tls =
            TlsConnector::from(client).connect(name, tcp).await.map_err(|e| e.to_string())?;
        tls.write_all(b"x").await.map_err(|e| e.to_string())?;
        let mut byte = [0u8];
        tls.read_exact(&mut byte).await.map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    }
    .await;
    let served = tokio::time::timeout(Duration::from_secs(5), accept).await.unwrap().unwrap();
    connected?;
    Ok(server.role_of(&served?))
}

#[tokio::test]
async fn the_controller_connects_with_full_rights() {
    let s = setup();
    let role = handshake(&s.server, s.controller.client_config().unwrap(), "localhost").await;
    assert_eq!(role, Ok(Role::Controller));
    let role = handshake(&s.server, s.controller.client_config().unwrap(), "127.0.0.1").await;
    assert_eq!(role, Ok(Role::Controller), "IP addresses are names too");
}

#[tokio::test]
async fn other_servers_connect_as_servers() {
    let s = setup();
    let role = handshake(&s.server, s.other.peer_client_config().unwrap(), "localhost").await;
    assert_eq!(role, Ok(Role::Server));
}

#[tokio::test]
async fn names_outside_the_certificate_and_strangers_are_refused() {
    let s = setup();
    let wrong_name =
        handshake(&s.server, s.controller.client_config().unwrap(), "elsewhere.example").await;
    assert!(wrong_name.is_err());

    let stranger_dir = TempDir::new().unwrap();
    let stranger = ControllerIdentity::create(stranger_dir.path()).unwrap();
    let refused = handshake(&s.server, stranger.client_config().unwrap(), "localhost").await;
    assert!(refused.is_err(), "a certificate of another authority");
}

#[test]
fn an_authority_is_never_replaced() {
    let s = setup();
    let dir = s._dir.path().join("controller");
    assert!(ControllerIdentity::create(&dir).is_err());
    assert!(ControllerIdentity::load(&dir).is_ok());
}

#[test]
fn servers_check_the_controllers_tokens() {
    let s = setup();
    let grant = Grant::new("localhost", vec!["/srv".into()], vec![], Duration::from_secs(60));
    let token = s.controller.sign(&grant);
    assert_eq!(s.server.verify(&token), Ok(grant));
    assert!(matches!(s.other.verify(&token), Err(TokenError::WrongAudience(_))));

    let stranger_dir = TempDir::new().unwrap();
    let stranger = ControllerIdentity::create(stranger_dir.path()).unwrap();
    let forged =
        stranger.sign(&Grant::new("localhost", vec!["/".into()], vec![], Duration::from_secs(60)));
    assert_eq!(s.server.verify(&forged), Err(TokenError::BadSignature));
}

#[cfg(unix)]
#[test]
fn private_keys_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let s = setup();
    for file in [
        "controller/ca.key",
        "controller/controller.key",
        "controller/token.key",
        "server/server.key",
    ] {
        let mode = std::fs::metadata(s._dir.path().join(file)).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{file}");
    }
}
