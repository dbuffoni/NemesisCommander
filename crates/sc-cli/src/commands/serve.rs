//! `sc serve` and `sc tls`: the permanent agent and its certificates.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sc_app::AppContext;
use sc_tls::ControllerIdentity;

use super::CmdResult;
use crate::cli::{ServeArgs, TlsAction, TlsArgs};

/// `sc serve`: serve this machine over mutual TLS until stopped.
#[cfg(target_os = "linux")]
pub async fn serve(args: ServeArgs) -> CmdResult {
    let identity = sc_tls::ServerIdentity::load(Path::new(&args.tls))?;
    let limits = sc_agent::Limits {
        roots: args.roots.iter().map(PathBuf::from).collect(),
        read_only: args.read_only,
    };
    let listener = tokio::net::TcpListener::bind(&args.listen)
        .await
        .map_err(|e| format!("{}: {e}", args.listen))?;
    eprintln!(
        "sc serve: listening on {} as {}",
        listener.local_addr()?,
        identity.names().join(", ")
    );
    sc_agent::serve::serve(listener, identity, limits).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(target_os = "linux"))]
pub async fn serve(_args: ServeArgs) -> CmdResult {
    Err("sc serve runs on Linux only; on this system use sc as a controller".into())
}

/// `sc tls ...`.
pub fn tls(context: &AppContext, args: TlsArgs) -> CmdResult {
    let dir = context.paths.tls_dir();
    match args.action {
        TlsAction::Init => {
            ControllerIdentity::create(&dir)?;
            crate::out!("created the certificate authority in {}", dir.display());
            crate::out!("next: `sc tls server NAME... --out DIR` for each server");
        }
        TlsAction::Server { names, out } => {
            let identity = if ControllerIdentity::exists(&dir) {
                ControllerIdentity::load(&dir)?
            } else {
                return Err(format!(
                    "no certificate authority in {}: run `sc tls init` first",
                    dir.display()
                )
                .into());
            };
            identity.issue_server(&names, Path::new(&out))?;
            crate::out!("wrote the bundle for {} to {out}", names.join(", "));
            crate::out!("copy it to the server (keep server.key private) and run:");
            crate::out!("  sc serve --tls <bundle folder>");
        }
        TlsAction::Path => crate::out!("{}", dir.display()),
    }
    Ok(ExitCode::SUCCESS)
}
