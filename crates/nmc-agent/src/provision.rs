//! Installing `nmc serve` on a server: `nmc serve install HOST`.
//!
//! Everything runs from the controller over SSH, so nothing has to be done
//! by hand on the server:
//!
//! 1. create the controller's certificate authority if there is none;
//! 2. copy the Linux `nmc` binary to `~/.local/bin/nmc`;
//! 3. issue the server's certificate bundle and copy it to
//!    `~/.config/nmc/serve-tls` (private key readable by the owner only);
//! 4. write a systemd *user* unit, enable and start it, and enable lingering
//!    so that it keeps running after the last login ends;
//! 5. connect to `scs://host:port` to prove that it works.
//!
//! Running it again updates the program and the unit and keeps the
//! certificates (unless a new one is asked for). `uninstall` removes the
//! unit and the certificates.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nmc_ssh::Target;
use nmc_tls::{ControllerIdentity, DEFAULT_PORT};

use crate::launch::{agent_binary, ssh_run};

/// Where the program is installed, relative to the home folder of the server.
const REMOTE_BINARY: &str = ".local/bin/nmc";
/// Where the server's certificate bundle goes.
const REMOTE_TLS_DIR: &str = ".config/nmc/serve-tls";
/// The name of the systemd unit.
const UNIT_NAME: &str = "nmc-serve.service";

/// What to install and how.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// The server as for SSH: `[user@]host[:ssh-port]`.
    pub authority: String,
    /// The name in the server's certificate and in `scs://` locations
    /// (default: the host of `authority`).
    pub name: Option<String>,
    /// TCP port `nmc serve` listens on.
    pub port: u16,
    /// Only serve paths below these folders (default: everything).
    pub roots: Vec<String>,
    /// Refuse every modification.
    pub read_only: bool,
    /// Issue a new certificate even if the server has one.
    pub renew: bool,
    /// The controller's certificate folder (`paths.tls_dir()`).
    pub tls_dir: PathBuf,
}

impl InstallOptions {
    /// Options with the defaults for `authority`.
    pub fn new(authority: &str, tls_dir: PathBuf) -> Self {
        InstallOptions {
            authority: authority.to_owned(),
            name: None,
            port: DEFAULT_PORT,
            roots: Vec::new(),
            read_only: false,
            renew: false,
            tls_dir,
        }
    }
}

/// Quotes `text` for a systemd command line.
fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
    format!("\"{escaped}\"")
}

/// The text of the systemd user unit that runs `nmc serve`.
pub fn systemd_unit(options: &InstallOptions) -> String {
    let mut command = format!(
        "%h/{REMOTE_BINARY} serve --tls %h/{REMOTE_TLS_DIR} --listen 0.0.0.0:{}",
        options.port
    );
    for root in &options.roots {
        command.push_str(&format!(" --root {}", quote(root)));
    }
    if options.read_only {
        command.push_str(" --read-only");
    }
    format!(
        "[Unit]\n\
         Description=NemesisCommander server (nmc serve)\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         ExecStart={command}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

/// The `scs://` location of the installed server.
pub fn location(name: &str, port: u16) -> String {
    format!("scs://{name}:{port}/")
}

/// Runs `script` on the server, optionally feeding it the contents of `input`.
async fn run(target: &Target, script: &str, input: Option<PathBuf>) -> Result<String, String> {
    ssh_run(target, script, input).await
}

/// Copies the local file `from` to `~/<relative>` on the server, with the
/// permission `mode` (octal text), through `.part` and a rename.
async fn upload(target: &Target, from: &Path, relative: &str, mode: &str) -> Result<(), String> {
    let (dir, name) = relative.rsplit_once('/').unwrap_or((".", relative));
    let script = format!(
        "d=\"$HOME/{dir}\"; mkdir -p \"$d\" && cat > \"$d/{name}.part\" && \
         chmod {mode} \"$d/{name}.part\" && mv \"$d/{name}.part\" \"$d/{name}\""
    );
    run(target, &script, Some(from.to_owned())).await.map(drop)
}

/// Checks that `nmc serve` could be installed on the server, without
/// installing anything: it only reads (the system, the port, an earlier
/// install) and creates nothing there, and no certificate authority is made
/// on this computer. `found` is told what each check found.
pub async fn check_serve(
    options: &InstallOptions,
    mut found: impl FnMut(&str),
) -> Result<(), String> {
    let target = Target::parse(&options.authority)?;
    found(&format!("Connecting to {} over SSH…", target.host));
    let system = run(
        &target,
        "uname -sm; command -v systemctl >/dev/null && echo systemd || echo no-systemd; \
         test -w \"$HOME\" && echo home-ok || echo home-readonly",
        None,
    )
    .await
    .map_err(|e| {
        format!(
            "SSH to {} fails: {}",
            options.authority,
            if e.is_empty() { "no answer" } else { &e }
        )
    })?;
    let mut lines = system.lines();
    let platform = lines.next().unwrap_or_default().to_owned();
    found(&format!("SSH works; the server runs {platform}"));
    if !platform.starts_with("Linux") {
        return Err(format!("the server runs {platform}: nmc serve needs Linux"));
    }
    match agent_binary() {
        None => {
            return Err(
                "no Linux nmc binary to copy to the server (set NMC_AGENT_BINARY to one)".into()
            );
        }
        Some(path) => found(&format!("nmc to copy: {}", path.display())),
    }
    if std::env::var(crate::launch::AGENT_BINARY_VAR).is_err()
        && platform != format!("Linux {}", std::env::consts::ARCH)
    {
        return Err(format!(
            "the server runs {platform}: set NMC_AGENT_BINARY to a matching nmc build"
        ));
    }
    if lines.next() != Some("systemd") {
        return Err("the server has no systemd (systemctl): start `nmc serve` there by hand".into());
    }
    found("systemd is available (the service runs as your user)");
    if lines.next() != Some("home-ok") {
        return Err("the home folder of the server is not writable".into());
    }
    let state = run(
        &target,
        &format!(
            "test -f \"$HOME/{REMOTE_BINARY}\" && echo binary || echo no-binary; \
             test -f \"$HOME/{REMOTE_TLS_DIR}/server.pem\" && echo cert || echo no-cert; \
             systemctl --user is-active {UNIT_NAME} 2>/dev/null || echo inactive; \
             {{ ss -ltn 2>/dev/null || netstat -ltn 2>/dev/null; }} | grep -q \":{port}[[:space:]]\" \
               && echo port-busy || echo port-free",
            port = options.port
        ),
        None,
    )
    .await?;
    let state: Vec<&str> = state.lines().collect();
    let installed = state.first() == Some(&"binary");
    let active = state.get(2) == Some(&"active");
    if installed {
        found("nmc is already installed there: it would be updated");
    } else {
        found("nmc is not installed there yet: it would be copied to ~/.local/bin");
    }
    if state.get(1) == Some(&"cert") && !options.renew {
        found("the server already has certificates: they would be kept (--renew replaces them)");
    }
    match (state.get(3), active) {
        (Some(&"port-busy"), true) => {
            found(&format!("port {} is used by the nmc serve already installed", options.port));
        }
        (Some(&"port-busy"), false) => {
            return Err(format!(
                "port {} is already used by another program on the server: choose --port",
                options.port
            ));
        }
        _ => found(&format!("port {} is free", options.port)),
    }
    found("nothing was installed or changed");
    Ok(())
}

/// Installs and starts `nmc serve` on the server. `progress` is told what
/// is being done. Returns the `scs://` location to open afterwards.
pub async fn install_serve(
    options: &InstallOptions,
    mut progress: impl FnMut(&str),
) -> Result<String, String> {
    let target = Target::parse(&options.authority)?;
    let name = options.name.clone().unwrap_or_else(|| target.host.clone());
    let binary = agent_binary()
        .ok_or("no Linux nmc binary to copy to the server (set NMC_AGENT_BINARY to one)")?;
    let identity = if ControllerIdentity::exists(&options.tls_dir) {
        ControllerIdentity::load(&options.tls_dir)
    } else {
        progress("Creating the certificate authority");
        ControllerIdentity::create(&options.tls_dir)
    }
    .map_err(|e| e.to_string())?;

    progress("Checking the server");
    let system =
        run(&target, "uname -sm; command -v systemctl >/dev/null && echo systemd", None).await?;
    let mut lines = system.lines();
    let platform = lines.next().unwrap_or_default();
    if !platform.starts_with("Linux") {
        return Err(format!("the server runs {platform}: nmc serve needs Linux"));
    }
    if std::env::var(crate::launch::AGENT_BINARY_VAR).is_err()
        && platform != format!("Linux {}", std::env::consts::ARCH)
    {
        return Err(format!(
            "the server runs {platform}: set NMC_AGENT_BINARY to a matching nmc build"
        ));
    }
    if lines.next() != Some("systemd") {
        return Err("the server has no systemd (systemctl): start `nmc serve` there by hand".into());
    }

    let scratch = std::env::temp_dir().join(format!("nmc-serve-install-{}", std::process::id()));
    let result = async {
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        progress("Copying nmc");
        upload(&target, &binary, REMOTE_BINARY, "755").await?;

        let has_certificate = run(
            &target,
            &format!("test -f \"$HOME/{REMOTE_TLS_DIR}/server.pem\" && echo yes || echo no"),
            None,
        )
        .await?
            == "yes";
        if has_certificate && !options.renew {
            progress("Keeping the certificates of the server");
        } else {
            progress("Issuing the certificate of the server");
            let bundle = scratch.join("bundle");
            identity
                .issue_server(std::slice::from_ref(&name), &bundle)
                .map_err(|e| e.to_string())?;
            run(&target, &format!("umask 077; mkdir -p \"$HOME/{REMOTE_TLS_DIR}\""), None).await?;
            for file in [
                "ca.pem",
                "controller.pem",
                "server.pem",
                "server.key",
                "names",
                "controller-token.pub",
            ] {
                let mode = if file == "server.key" { "600" } else { "644" };
                upload(&target, &bundle.join(file), &format!("{REMOTE_TLS_DIR}/{file}"), mode)
                    .await?;
            }
        }

        progress("Starting the service");
        let unit = scratch.join(UNIT_NAME);
        std::fs::write(&unit, systemd_unit(options)).map_err(|e| e.to_string())?;
        upload(&target, &unit, &format!(".config/systemd/user/{UNIT_NAME}"), "644").await?;
        run(
            &target,
            &format!(
                "systemctl --user daemon-reload && systemctl --user enable {UNIT_NAME} && \
                 systemctl --user restart {UNIT_NAME} && \
                 {{ loginctl enable-linger \"$(id -un)\" || true; }}"
            ),
            None,
        )
        .await?;
        Ok::<_, String>(())
    }
    .await;
    let _ = std::fs::remove_dir_all(&scratch);
    result?;

    progress("Connecting to the new server");
    let config = identity.client_config().map_err(|e| e.to_string())?;
    let mut last = String::new();
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        match crate::serve::connect(config.clone(), &name, options.port, None).await {
            Ok(_) => return Ok(location(&name, options.port)),
            Err(e) => last = e,
        }
    }
    Err(format!(
        "installed, but {name}:{} does not answer ({last}); is the port open in the firewall? \
         See `journalctl --user -u nmc-serve` on the server",
        options.port
    ))
}

/// Stops and removes `nmc serve` (the unit and the certificates; the
/// program stays) on the server.
pub async fn uninstall_serve(
    authority: &str,
    mut progress: impl FnMut(&str),
) -> Result<(), String> {
    let target = Target::parse(authority)?;
    progress("Stopping the service");
    run(
        &target,
        &format!(
            "systemctl --user disable --now {UNIT_NAME} 2>/dev/null; \
             rm -f \"$HOME/.config/systemd/user/{UNIT_NAME}\"; \
             systemctl --user daemon-reload; rm -rf \"$HOME/{REMOTE_TLS_DIR}\"; true"
        ),
        None,
    )
    .await
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> InstallOptions {
        InstallOptions::new("me@nas.lan", PathBuf::from("/tmp/tls"))
    }

    #[test]
    fn the_unit_runs_nmc_serve_with_the_certificates() {
        let unit = systemd_unit(&options());
        assert!(unit.contains(
            "ExecStart=%h/.local/bin/nmc serve --tls %h/.config/nmc/serve-tls --listen 0.0.0.0:7443\n"
        ));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn roots_and_read_only_reach_the_command_line() {
        let mut options = options();
        options.port = 9000;
        options.roots = vec!["/srv/data".into(), "/home/me/my \"files\"".into()];
        options.read_only = true;
        let unit = systemd_unit(&options);
        assert!(unit.contains("--listen 0.0.0.0:9000"));
        assert!(unit.contains("--root \"/srv/data\""));
        assert!(unit.contains("--root \"/home/me/my \\\"files\\\"\""));
        assert!(unit.contains(" --read-only\n"));
    }

    #[test]
    fn percent_signs_are_escaped_for_systemd() {
        assert_eq!(quote("/a%b"), "\"/a%%b\"");
    }

    #[test]
    fn the_location_names_the_server_and_port() {
        assert_eq!(location("nas.lan", 7443), "scs://nas.lan:7443/");
    }
}
