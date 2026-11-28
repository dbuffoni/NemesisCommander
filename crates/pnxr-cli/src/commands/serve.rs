//! `pnxr serve` and `pnxr tls`: the permanent agent and its certificates.

use std::path::Path;
use std::process::ExitCode;

use pnxr_app::AppContext;
use pnxr_tls::ControllerIdentity;

use super::CmdResult;
use crate::cli::{ServeAction, ServeArgs, ServeInstallArgs, TlsAction, TlsArgs};

/// `pnxr serve`: serve this machine over mutual TLS until stopped.
#[cfg(target_os = "linux")]
pub async fn serve(context: &AppContext, args: ServeArgs) -> CmdResult {
    match args.action {
        Some(ServeAction::Install(install)) => return install_serve(context, install).await,
        Some(ServeAction::Uninstall { host }) => return uninstall_serve(&host).await,
        None => {}
    }
    let tls = args.tls.ok_or("--tls is required")?;
    let identity = pnxr_tls::ServerIdentity::load(Path::new(&tls))?;
    let limits = pnxr_agent::Limits {
        roots: args.roots.iter().map(std::path::PathBuf::from).collect(),
        read_only: args.read_only,
        settings_dir: super::settings_dir(),
    };
    let listener = tokio::net::TcpListener::bind(&args.listen)
        .await
        .map_err(|e| format!("{}: {e}", args.listen))?;
    eprintln!(
        "pnxr serve: listening on {} as {}",
        listener.local_addr()?,
        identity.names().join(", ")
    );
    pnxr_agent::serve::serve(listener, identity, limits).await?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(target_os = "linux"))]
pub async fn serve(context: &AppContext, args: ServeArgs) -> CmdResult {
    match args.action {
        Some(ServeAction::Install(install)) => return install_serve(context, install).await,
        Some(ServeAction::Uninstall { host }) => return uninstall_serve(&host).await,
        None => {}
    }
    Err("pnxr serve runs on Linux only; on this system use pnxr as a controller".into())
}

/// `pnxr serve install HOST`: set `pnxr serve` up on a server.
async fn install_serve(context: &AppContext, args: ServeInstallArgs) -> CmdResult {
    let mut options =
        pnxr_agent::provision::InstallOptions::new(&args.host, context.paths.tls_dir());
    options.name = args.name;
    options.port = args.port;
    options.roots = args.roots;
    options.read_only = args.read_only;
    options.renew = args.renew;
    let with_settings = args.with_settings;
    let location = pnxr_agent::provision::install_serve(&options, |step| {
        eprintln!("pnxr: {step}…");
    })
    .await?;
    crate::out!("pnxr serve is running; open {location}");
    crate::out!("add it with F4 in the drive list, or copy to it: pnxr cp FILE {location}DIR");
    if with_settings {
        let target = crate::location::parse(&location).map_err(|e| e.to_string())?;
        let file = pnxr_app::sync::push(
            &context.registry,
            &context.config,
            &target,
            pnxr_app::sync::SyncMode::Replace,
        )
        .await?;
        crate::out!("settings sent to {file}");
    }
    Ok(ExitCode::SUCCESS)
}

/// `pnxr serve uninstall HOST`.
async fn uninstall_serve(host: &str) -> CmdResult {
    pnxr_agent::provision::uninstall_serve(host, |step| eprintln!("pnxr: {step}…")).await?;
    crate::out!("pnxr serve was removed from {host} (the pnxr program stays)");
    Ok(ExitCode::SUCCESS)
}

/// `pnxr tls ...`.
pub fn tls(context: &AppContext, args: TlsArgs) -> CmdResult {
    let dir = context.paths.tls_dir();
    match args.action {
        TlsAction::Init => {
            ControllerIdentity::create(&dir)?;
            crate::out!("created the certificate authority in {}", dir.display());
            crate::out!("next: `pnxr tls server NAME... --out DIR` for each server");
        }
        TlsAction::Server { names, out } => {
            let identity = if ControllerIdentity::exists(&dir) {
                ControllerIdentity::load(&dir)?
            } else {
                return Err(format!(
                    "no certificate authority in {}: run `pnxr tls init` first",
                    dir.display()
                )
                .into());
            };
            identity.issue_server(&names, Path::new(&out))?;
            crate::out!("wrote the bundle for {} to {out}", names.join(", "));
            crate::out!("copy it to the server (keep server.key private) and run:");
            crate::out!("  pnxr serve --tls <bundle folder>");
        }
        TlsAction::Path => crate::out!("{}", dir.display()),
    }
    Ok(ExitCode::SUCCESS)
}
