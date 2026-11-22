//! `nmc serve` and `nmc tls`: the permanent agent and its certificates.

use std::path::Path;
use std::process::ExitCode;

use nmc_app::AppContext;
use nmc_tls::ControllerIdentity;

use super::CmdResult;
use crate::cli::{ServeAction, ServeArgs, ServeInstallArgs, TlsAction, TlsArgs};

/// `nmc serve`: serve this machine over mutual TLS until stopped.
#[cfg(target_os = "linux")]
pub async fn serve(context: &AppContext, args: ServeArgs) -> CmdResult {
    match args.action {
        Some(ServeAction::Install(install)) => return install_serve(context, install).await,
        Some(ServeAction::Uninstall { host }) => return uninstall_serve(&host).await,
        None => {}
    }
    let tls = args.tls.ok_or("--tls is required")?;
    let identity = nmc_tls::ServerIdentity::load(Path::new(&tls))?;
    let limits = nmc_agent::Limits {
        roots: args.roots.iter().map(std::path::PathBuf::from).collect(),
        read_only: args.read_only,
        settings_dir: super::settings_dir(),
    };
    let listener = tokio::net::TcpListener::bind(&args.listen)
        .await
        .map_err(|e| format!("{}: {e}", args.listen))?;
    eprintln!(
        "nmc serve: listening on {} as {}",
        listener.local_addr()?,
        identity.names().join(", ")
    );
    nmc_agent::serve::serve(listener, identity, limits).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(target_os = "linux"))]
pub async fn serve(context: &AppContext, args: ServeArgs) -> CmdResult {
    match args.action {
        Some(ServeAction::Install(install)) => return install_serve(context, install).await,
        Some(ServeAction::Uninstall { host }) => return uninstall_serve(&host).await,
        None => {}
    }
    Err("nmc serve runs on Linux only; on this system use nmc as a controller".into())
}

/// `nmc serve install HOST`: set `nmc serve` up on a server.
async fn install_serve(context: &AppContext, args: ServeInstallArgs) -> CmdResult {
    let mut options =
        nmc_agent::provision::InstallOptions::new(&args.host, context.paths.tls_dir());
    options.name = args.name;
    options.port = args.port;
    options.roots = args.roots;
    options.read_only = args.read_only;
    options.renew = args.renew;
    let location = nmc_agent::provision::install_serve(&options, |step| {
        eprintln!("nmc: {step}…");
    })
    .await?;
    crate::out!("nmc serve is running; open {location}");
    crate::out!("add it with F4 in the drive list, or copy to it: nmc cp FILE {location}DIR");
    Ok(ExitCode::SUCCESS)
}

/// `nmc serve uninstall HOST`.
async fn uninstall_serve(host: &str) -> CmdResult {
    nmc_agent::provision::uninstall_serve(host, |step| eprintln!("nmc: {step}…")).await?;
    crate::out!("nmc serve was removed from {host} (the nmc program stays)");
    Ok(ExitCode::SUCCESS)
}

/// `nmc tls ...`.
pub fn tls(context: &AppContext, args: TlsArgs) -> CmdResult {
    let dir = context.paths.tls_dir();
    match args.action {
        TlsAction::Init => {
            ControllerIdentity::create(&dir)?;
            crate::out!("created the certificate authority in {}", dir.display());
            crate::out!("next: `nmc tls server NAME... --out DIR` for each server");
        }
        TlsAction::Server { names, out } => {
            let identity = if ControllerIdentity::exists(&dir) {
                ControllerIdentity::load(&dir)?
            } else {
                return Err(format!(
                    "no certificate authority in {}: run `nmc tls init` first",
                    dir.display()
                )
                .into());
            };
            identity.issue_server(&names, Path::new(&out))?;
            crate::out!("wrote the bundle for {} to {out}", names.join(", "));
            crate::out!("copy it to the server (keep server.key private) and run:");
            crate::out!("  nmc serve --tls <bundle folder>");
        }
        TlsAction::Path => crate::out!("{}", dir.display()),
    }
    Ok(ExitCode::SUCCESS)
}
