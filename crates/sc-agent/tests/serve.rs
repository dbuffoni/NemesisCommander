//! `sc serve` over real mutual TLS on local ports: what the controller may
//! do, what another server may do with and without a token, and
//! server-to-server copies along both routes.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sc_agent::route::{Outcome, Route, ServerCopy, copy_between_servers};
use sc_agent::serve::{connect, serve};
use sc_agent::{AgentProvider, AgentService, Limits};
use sc_ops::ConflictPolicy;
use sc_proto::pb;
use sc_tls::{ControllerIdentity, Grant, ServerIdentity};
use sc_vfs::{Provider, VfsError, VfsPath, WriteOptions, collect_listing};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A controller and two servers on this machine:
/// `a` reachable as `127.0.0.1:<port>`, `b` as `localhost:<port>`.
struct World {
    _dir: TempDir,
    controller: ControllerIdentity,
    a: Server,
    b: Server,
}

struct Server {
    host: &'static str,
    port: u16,
    /// The folder it serves (its only root).
    root: PathBuf,
    /// Its bundle, to act as that server in tests.
    bundle: PathBuf,
}

impl Server {
    fn url(&self, path: &Path) -> VfsPath {
        VfsPath::parse(&format!("scs://{}:{}{}", self.host, self.port, path.display())).unwrap()
    }

    fn peer(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

async fn world() -> World {
    let dir = TempDir::new().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let controller = ControllerIdentity::create(&base.join("controller")).unwrap();
    let a = start(&controller, &base, "a", "127.0.0.1").await;
    let b = start(&controller, &base, "b", "localhost").await;
    World { _dir: dir, controller, a, b }
}

async fn start(
    controller: &ControllerIdentity,
    base: &Path,
    name: &str,
    host: &'static str,
) -> Server {
    let bundle = base.join(format!("{name}-tls"));
    controller.issue_server(&[host.to_owned()], &bundle).unwrap();
    let root = base.join(name);
    fs::create_dir(&root).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let identity = ServerIdentity::load(&bundle).unwrap();
    let limits = Limits { roots: vec![root.clone()], read_only: false };
    tokio::spawn(serve(listener, identity, limits));
    Server { host, port, root, bundle }
}

impl World {
    async fn as_controller(&self, server: &Server) -> AgentProvider {
        let config = self.controller.client_config().unwrap();
        connect(config, server.host, server.port, None).await.unwrap()
    }

    /// Connects to `to` as the server `from` would, with `token`.
    async fn as_server(
        &self,
        from: &Server,
        to: &Server,
        token: Option<&str>,
    ) -> Result<AgentProvider, String> {
        let config = ServerIdentity::load(&from.bundle).unwrap().peer_client_config().unwrap();
        connect(config, to.host, to.port, token).await
    }

    fn token(&self, audience: &Server, read: &[&Path], write: &[&Path]) -> String {
        let paths = |list: &[&Path]| list.iter().map(|p| p.display().to_string()).collect();
        let grant = Grant::new(audience.host, paths(read), paths(write), Duration::from_secs(60));
        self.controller.sign(&grant)
    }
}

async fn read_all(provider: &dyn Provider, path: &VfsPath) -> Result<Vec<u8>, VfsError> {
    let mut data = Vec::new();
    provider
        .read(path, 0)
        .await?
        .read_to_end(&mut data)
        .await
        .map_err(|e| VfsError::from_io(path, e))?;
    Ok(data)
}

#[tokio::test]
async fn the_controller_works_like_with_any_agent() {
    let w = world().await;
    fs::write(w.a.root.join("hello.txt"), "hi").unwrap();
    let a = w.as_controller(&w.a).await;
    assert_eq!(a.scheme(), "scs");

    let names: Vec<String> = collect_listing(&a, &w.a.url(&w.a.root))
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["hello.txt"]);
    assert_eq!(read_all(&a, &w.a.url(&w.a.root.join("hello.txt"))).await.unwrap(), b"hi");

    let mut sink =
        a.write(&w.a.url(&w.a.root.join("new.txt")), WriteOptions::create_new()).await.unwrap();
    sink.write_all(b"written").await.unwrap();
    sink.shutdown().await.unwrap();
    assert_eq!(fs::read(w.a.root.join("new.txt")).unwrap(), b"written");

    let outside = a.stat(&w.a.url(Path::new("/etc/passwd"))).await;
    assert!(matches!(outside, Err(VfsError::PermissionDenied(_))), "{outside:?}");
}

#[tokio::test]
async fn servers_need_a_token() {
    let w = world().await;
    let refused = w.as_server(&w.b, &w.a, None).await;
    assert!(refused.unwrap_err().contains("token"));
}

#[tokio::test]
async fn a_token_opens_its_paths_once() {
    let w = world().await;
    let shared = w.a.root.join("shared");
    fs::create_dir(&shared).unwrap();
    fs::write(shared.join("data"), "d").unwrap();
    fs::write(w.a.root.join("secret"), "s").unwrap();

    let token = w.token(&w.a, &[&shared], &[]);
    let peer = w.as_server(&w.b, &w.a, Some(&token)).await.unwrap();
    assert_eq!(read_all(&peer, &w.a.url(&shared.join("data"))).await.unwrap(), b"d");
    let secret = read_all(&peer, &w.a.url(&w.a.root.join("secret"))).await;
    assert!(matches!(secret, Err(VfsError::PermissionDenied(_))), "{secret:?}");
    // Writes stream: the refusal arrives when the file is closed.
    let mut sink =
        peer.write(&w.a.url(&shared.join("x")), WriteOptions::create_new()).await.unwrap();
    let _ = sink.write_all(b"x").await;
    let closed = sink.shutdown().await.unwrap_err();
    assert!(closed.to_string().contains("permission denied"), "read-only grant: {closed}");
    assert!(!shared.join("x").exists());
    let folder = peer.create_dir(&w.a.url(&shared.join("new"))).await;
    assert!(matches!(folder, Err(VfsError::PermissionDenied(_))), "{folder:?}");

    let again = w.as_server(&w.b, &w.a, Some(&token)).await;
    assert!(again.unwrap_err().contains("already used"));

    let for_b = w.token(&w.b, &[&shared], &[]);
    let wrong = w.as_server(&w.b, &w.a, Some(&for_b)).await;
    assert!(wrong.unwrap_err().contains("issued for localhost"));
}

#[tokio::test]
async fn only_the_controller_starts_transfers() {
    let w = world().await;
    let token = w.token(&w.a, &[&w.a.root], &[]);
    let peer = w.as_server(&w.b, &w.a, Some(&token)).await.unwrap();
    let request = pb::TransferRequest { peer: w.b.peer(), ..Default::default() };
    let status = peer.raw_client().transfer(request).await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::PermissionDenied);

    // An agent over SSH has no certificates to contact other servers.
    let (controller, agent) = tokio::io::duplex(64 * 1024);
    let service = AgentService::new(Limits::default()).unwrap();
    tokio::spawn(sc_proto::transport::serve(service, agent));
    let client = sc_proto::transport::connect(controller).await.unwrap();
    let stdio = AgentProvider::handshake(client, None).await.unwrap();
    let status = stdio.raw_client().transfer(pb::TransferRequest::default()).await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::Unimplemented);
}

fn tree(root: &Path) {
    fs::create_dir_all(root.join("project/deep")).unwrap();
    fs::write(root.join("project/readme"), "read me").unwrap();
    fs::write(root.join("project/deep/big.bin"), vec![9u8; 600_000]).unwrap();
}

async fn copy(w: &World, routes: &[Route]) -> (Route, pb::TransferProgress, usize) {
    let sources = [w.a.url(&w.a.root.join("project"))];
    let destination = w.b.url(&w.b.root);
    let copy =
        ServerCopy { sources: &sources, destination: &destination, conflict: ConflictPolicy::Fail };
    assert!(copy.is_between_servers());
    let mut messages = 0;
    let outcome =
        copy_between_servers(&w.controller, &copy, routes, |_| messages += 1).await.unwrap();
    match outcome {
        Outcome::Copied(route, last) => (route, last, messages),
        Outcome::UseController(reasons) => panic!("not copied: {reasons:?}"),
    }
}

#[tokio::test]
async fn the_destination_pulls_on_the_direct_route() {
    let w = world().await;
    tree(&w.a.root);
    let (route, last, messages) = copy(&w, &[Route::Direct, Route::Reverse]).await;
    assert_eq!(route, Route::Direct);
    assert!(last.finished && last.failures.is_empty(), "{last:?}");
    assert_eq!((last.files_done, last.files_total), (2, 2));
    assert_eq!(last.bytes_done, 600_000 + 7);
    assert!(messages >= 1);
    assert_eq!(fs::read(w.b.root.join("project/readme")).unwrap(), b"read me");
    assert_eq!(fs::read(w.b.root.join("project/deep/big.bin")).unwrap().len(), 600_000);
}

#[tokio::test]
async fn the_source_pushes_on_the_reverse_route() {
    let w = world().await;
    tree(&w.a.root);
    let (route, last, _) = copy(&w, &[Route::Reverse]).await;
    assert_eq!(route, Route::Reverse);
    assert!(last.failures.is_empty(), "{last:?}");
    assert_eq!(fs::read(w.b.root.join("project/deep/big.bin")).unwrap().len(), 600_000);
}

#[tokio::test]
async fn conflicts_are_reported_not_asked() {
    let w = world().await;
    tree(&w.a.root);
    fs::create_dir_all(w.b.root.join("project")).unwrap();
    fs::write(w.b.root.join("project/readme"), "older").unwrap();
    let (_, last, _) = copy(&w, &[Route::Direct]).await;
    assert_eq!(last.failures.len(), 1, "{last:?}");
    assert_eq!(fs::read(w.b.root.join("project/readme")).unwrap(), b"older");
}

#[tokio::test]
async fn an_unreachable_peer_falls_back_to_the_next_route() {
    let w = world().await;
    tree(&w.a.root);
    // The destination is asked to pull from a server that is not running;
    // with only the direct route allowed, the caller is told to copy itself.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);
    let dead = Server {
        host: "127.0.0.1",
        port: dead_port,
        root: w.a.root.clone(),
        bundle: w.a.bundle.clone(),
    };
    let token = w.token(&dead, &[&w.a.root], &[]);
    let b = w.as_controller(&w.b).await;
    let request = pb::TransferRequest {
        peer: dead.peer(),
        token,
        sources: vec![w.a.root.join("project").display().to_string()],
        destination: w.b.root.display().to_string(),
        ..Default::default()
    };
    let status = b.raw_client().transfer(request).await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::Unavailable, "{status:?}");
}
