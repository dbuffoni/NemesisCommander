//! Installing `pnxr serve` on a server: `pnxr serve install HOST`.
//!
//! Everything runs from the controller over SSH, so nothing has to be done
//! by hand on the server:
//!
//! 1. create the controller's certificate authority if there is none;
//! 2. copy the Linux `pnxr` binary to `~/.local/bin/pnxr`;
//! 3. issue the server's certificate bundle and copy it to
//!    `~/.config/pnxr/serve-tls` (private key readable by the owner only);
//! 4. write a systemd *user* unit, enable and start it, and enable lingering
//!    so that it keeps running after the last login ends;
//! 5. connect to `scs://host:port` to prove that it works.
//!
//! Running it again updates the program and the unit and keeps the
//! certificates (unless a new one is asked for). `uninstall` removes the
//! unit and the certificates.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pnxr_ssh::Target;
use pnxr_tls::{ControllerIdentity, DEFAULT_PORT};

use crate::launch::{agent_binary, ssh_run};

/// Where the program is installed, relative to the home folder of the server.
const REMOTE_BINARY: &str = ".local/bin/pnxr";
/// Where the server's certificate bundle goes.
const REMOTE_TLS_DIR: &str = ".config/pnxr/serve-tls";
/// How many times the new server is contacted before giving up.
const CONNECT_ATTEMPTS: u32 = 4;
/// The name of the systemd unit.
const UNIT_NAME: &str = "pnxr-serve.service";

/// What an install error says when the service stopped because lingering
/// is off on the server (the front end then offers to turn it on).
pub const LINGER_OFF: &str = "lingering is off on the server";

/// The password of `sudo` on the server, used once to turn lingering on. It
/// is never printed, logged or saved.
#[derive(Clone, PartialEq, Eq)]
pub struct SudoPassword(pub String);

impl std::fmt::Debug for SudoPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SudoPassword(***)")
    }
}

/// What to install and how.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// The server as for SSH: `[user@]host[:ssh-port]`.
    pub authority: String,
    /// The name in the server's certificate and in `scs://` locations
    /// (default: the host of `authority`).
    pub name: Option<String>,
    /// TCP port `pnxr serve` listens on.
    pub port: u16,
    /// Only serve paths below these folders (default: everything).
    pub roots: Vec<String>,
    /// Refuse every modification.
    pub read_only: bool,
    /// Issue a new certificate even if the server has one.
    pub renew: bool,
    /// The controller's certificate folder (`paths.tls_dir()`).
    pub tls_dir: PathBuf,
    /// The `sudo` password of the server's user, to enable lingering when
    /// the install finds it off (asked after the first attempt).
    pub sudo_password: Option<SudoPassword>,
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
            sudo_password: None,
        }
    }
}

/// Quotes `text` for a systemd command line.
fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
    format!("\"{escaped}\"")
}

/// The text of the systemd user unit that runs `pnxr serve`.
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
         Description=Pairnox server (pnxr serve)\n\
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

/// The command as one short line for the log: `$ ssh me@nas  uname -sm; …`.
fn command_line(target: &Target, script: &str, input: &Option<PathBuf>) -> String {
    let flat: String = script.split_whitespace().collect::<Vec<_>>().join(" ");
    let short = if flat.chars().count() > 110 {
        format!("{}…", flat.chars().take(110).collect::<String>())
    } else {
        flat
    };
    let feeding = match input {
        Some(file) => format!("  < {}", file.display()),
        None => String::new(),
    };
    format!("$ ssh {}  {short}{feeding}", target.host)
}

/// Runs `script` on the server, optionally feeding it the contents of `input`.
/// The command line and what came of it are told to `log`, so a failure can
/// be followed step by step.
async fn run(
    target: &Target,
    script: &str,
    input: Option<PathBuf>,
    log: &mut (dyn FnMut(&str) + Send),
) -> Result<String, String> {
    log(&command_line(target, script, &input));
    let result = ssh_run(target, script, input).await;
    match &result {
        Ok(out) => {
            for line in out.lines().filter(|l| !l.trim().is_empty()).take(6) {
                log(&format!("    {line}"));
            }
        }
        Err(e) => {
            for line in e.lines().filter(|l| !l.trim().is_empty()).take(6) {
                log(&format!("    ! {line}"));
            }
        }
    }
    result
}

/// Copies the local file `from` to `~/<relative>` on the server, with the
/// permission `mode` (octal text), through `.part` and a rename.
async fn upload(
    target: &Target,
    from: &Path,
    relative: &str,
    mode: &str,
    log: &mut (dyn FnMut(&str) + Send),
) -> Result<(), String> {
    let (dir, name) = relative.rsplit_once('/').unwrap_or((".", relative));
    let script = format!(
        "d=\"$HOME/{dir}\"; mkdir -p \"$d\" && cat > \"$d/{name}.part\" && \
         chmod {mode} \"$d/{name}.part\" && mv \"$d/{name}.part\" \"$d/{name}\""
    );
    run(target, &script, Some(from.to_owned()), log).await.map(drop)
}

/// Checks that `pnxr serve` could be installed on the server, without
/// installing anything: it only reads (the system, the port, an earlier
/// install) and creates nothing there, and no certificate authority is made
/// on this computer. `found` is told what each check found.
pub async fn check_serve(
    options: &InstallOptions,
    mut found: impl FnMut(&str) + Send,
) -> Result<(), String> {
    let target = Target::parse(&options.authority)?;
    found(&format!("Connecting to {} over SSH…", target.host));
    let system = run(
        &target,
        "uname -sm; command -v systemctl >/dev/null && echo systemd || echo no-systemd; \
         test -w \"$HOME\" && echo home-ok || echo home-readonly",
        None,
        &mut found,
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
        return Err(format!("the server runs {platform}: pnxr serve needs Linux"));
    }
    match agent_binary() {
        None => {
            return Err(
                "no Linux pnxr binary to copy to the server (set PNXR_AGENT_BINARY to one)".into(),
            );
        }
        Some(path) => found(&format!("pnxr to copy: {}", path.display())),
    }
    if std::env::var(crate::launch::AGENT_BINARY_VAR).is_err()
        && platform != format!("Linux {}", std::env::consts::ARCH)
    {
        return Err(format!(
            "the server runs {platform}: set PNXR_AGENT_BINARY to a matching pnxr build"
        ));
    }
    if lines.next() != Some("systemd") {
        return Err(
            "the server has no systemd (systemctl): start `pnxr serve` there by hand".into()
        );
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
        &mut found,
    )
    .await?;
    let state: Vec<&str> = state.lines().collect();
    let installed = state.first() == Some(&"binary");
    let active = state.get(2) == Some(&"active");
    if installed {
        found("pnxr is already installed there: it would be updated");
    } else {
        found("pnxr is not installed there yet: it would be copied to ~/.local/bin");
    }
    if state.get(1) == Some(&"cert") && !options.renew {
        found("the server already has certificates: they would be kept (--renew replaces them)");
    }
    match (state.get(3), active) {
        (Some(&"port-busy"), true) => {
            found(&format!("port {} is used by the pnxr serve already installed", options.port));
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

/// Installs and starts `pnxr serve` on the server. `progress` is told what
/// is being done. Returns the `scs://` location to open afterwards.
pub async fn install_serve(
    options: &InstallOptions,
    mut progress: impl FnMut(&str) + Send,
) -> Result<String, String> {
    let target = Target::parse(&options.authority)?;
    let name = options.name.clone().unwrap_or_else(|| target.host.clone());
    let binary = agent_binary()
        .ok_or("no Linux pnxr binary to copy to the server (set PNXR_AGENT_BINARY to one)")?;
    let identity = if ControllerIdentity::exists(&options.tls_dir) {
        ControllerIdentity::load(&options.tls_dir)
    } else {
        progress("Creating the certificate authority");
        ControllerIdentity::create(&options.tls_dir)
    }
    .map_err(|e| e.to_string())?;

    progress("Checking the server");
    let system = run(
        &target,
        "uname -sm; command -v systemctl >/dev/null && echo systemd",
        None,
        &mut progress,
    )
    .await?;
    let mut lines = system.lines();
    let platform = lines.next().unwrap_or_default();
    if !platform.starts_with("Linux") {
        return Err(format!("the server runs {platform}: pnxr serve needs Linux"));
    }
    if std::env::var(crate::launch::AGENT_BINARY_VAR).is_err()
        && platform != format!("Linux {}", std::env::consts::ARCH)
    {
        return Err(format!(
            "the server runs {platform}: set PNXR_AGENT_BINARY to a matching pnxr build"
        ));
    }
    if lines.next() != Some("systemd") {
        return Err(
            "the server has no systemd (systemctl): start `pnxr serve` there by hand".into()
        );
    }

    let scratch = std::env::temp_dir().join(format!("pnxr-serve-install-{}", std::process::id()));
    let result = async {
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        progress("Copying pnxr");
        upload(&target, &binary, REMOTE_BINARY, "755", &mut progress).await?;

        let has_certificate = run(
            &target,
            &format!("test -f \"$HOME/{REMOTE_TLS_DIR}/server.pem\" && echo yes || echo no"),
            None,
            &mut progress,
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
            run(
                &target,
                &format!("umask 077; mkdir -p \"$HOME/{REMOTE_TLS_DIR}\""),
                None,
                &mut progress,
            )
            .await?;
            for file in [
                "ca.pem",
                "controller.pem",
                "server.pem",
                "server.key",
                "names",
                "controller-token.pub",
            ] {
                let mode = if file == "server.key" { "600" } else { "644" };
                upload(
                    &target,
                    &bundle.join(file),
                    &format!("{REMOTE_TLS_DIR}/{file}"),
                    mode,
                    &mut progress,
                )
                .await?;
            }
        }

        progress("Starting the service");
        let unit = scratch.join(UNIT_NAME);
        std::fs::write(&unit, systemd_unit(options)).map_err(|e| e.to_string())?;
        upload(&target, &unit, &format!(".config/systemd/user/{UNIT_NAME}"), "644", &mut progress)
            .await?;
        run(
            &target,
            &format!(
                "systemctl --user daemon-reload && systemctl --user enable {UNIT_NAME} && \
                 systemctl --user restart {UNIT_NAME} && \
                 {{ loginctl enable-linger \"$(id -un)\" || true; }}"
            ),
            None,
            &mut progress,
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
    let mut answered = false;
    let mut linger_off = false;
    for attempt in 1..=CONNECT_ATTEMPTS {
        tokio::time::sleep(Duration::from_secs(1)).await;
        progress(&format!(
            "Contacting {name}:{} with TLS (attempt {attempt} of {CONNECT_ATTEMPTS}, \
             {} s at most)",
            options.port,
            crate::serve::CONNECT_TIMEOUT.as_secs()
        ));
        match crate::serve::connect(config.clone(), &name, options.port, None).await {
            Ok(_) => {
                progress("    it answers");
                answered = true;
                break;
            }
            Err(e) => {
                progress(&format!("    no answer: {e}"));
                last = e;
            }
        }
    }
    if answered {
        // The install was done through SSH sessions that are over now: a
        // user service without lingering is stopped with the last session,
        // so a first answer proves little. Check lingering, wait, ask again.
        progress("Checking that the service survives the end of the SSH sessions");
        let linger = run(
            &target,
            "loginctl show-user \"$(id -un)\" -p Linger --value 2>&1 || echo unknown",
            None,
            &mut progress,
        )
        .await
        .unwrap_or_default();
        let mut linger = linger.trim().to_owned();
        if linger == "no" {
            progress("    lingering is OFF: the service stops when the last login ends");
            if let Some(password) = &options.sudo_password {
                progress("Turning lingering on with sudo");
                match enable_linger(&target, &password.0, &mut progress).await {
                    Ok(()) => linger = "yes".into(),
                    Err(e) => progress(&format!("    sudo failed: {e}")),
                }
            }
        }
        linger_off = linger == "no";
        tokio::time::sleep(Duration::from_secs(6)).await;
        progress(&format!("Contacting {name}:{} again, after the sessions closed", options.port));
        match crate::serve::connect(config.clone(), &name, options.port, None).await {
            Ok(_) => {
                progress("    still answering: it stays up");
                return Ok(location(&name, options.port));
            }
            Err(e) => {
                progress(&format!("    no answer now: {e}"));
                last = format!("it answered right after the install, then stopped: {e}");
            }
        }
    }
    // Nothing answers: ask the server itself what it sees.
    progress("Asking the server why (service state, listening ports, its log)");
    let port = options.port;
    let report = run(
        &target,
        &format!(
            "echo \"service: $(systemctl --user is-active {UNIT_NAME} 2>&1)\"; \
             {{ ss -ltn 2>/dev/null || netstat -ltn 2>/dev/null; }} | grep \":{port}[[:space:]]\" \
               || echo \"nothing listens on port {port}\"; \
             journalctl --user -u pnxr-serve -n 12 --no-pager 2>&1 | tail -n 12"
        ),
        None,
        &mut progress,
    )
    .await
    .unwrap_or_default();
    let listening =
        report.lines().any(|l| l.contains(&format!(":{port}")) && !l.contains("nothing"));
    let advice = if linger_off {
        format!(
            "{LINGER_OFF}: the service stops with the last login. Give the sudo password to turn \
             it on, or run `sudo loginctl enable-linger $USER` there"
        )
    } else if listening {
        format!(
            "the service listens on port {port}, but this computer cannot reach it: open TCP \
             port {port} in the firewall of the server (or of the network in between)"
        )
    } else {
        format!(
            "the service does not listen on port {port}: read its log above, or run \
             `journalctl --user -u pnxr-serve` on the server; if lingering is off, run \
             `sudo loginctl enable-linger $USER` there so the service survives logout"
        )
    };
    Err(format!("installed, but {name}:{port} does not answer ({last}); {advice}"))
}

/// Turns lingering on for the user of `target` with `sudo`, reading the
/// password from a file only this user can read, which is deleted at once.
/// The password is not part of the command line nor of the log.
pub async fn enable_linger(
    target: &Target,
    password: &str,
    log: &mut (dyn FnMut(&str) + Send),
) -> Result<(), String> {
    let file = std::env::temp_dir().join(format!("pnxr-sudo-{}", std::process::id()));
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut handle = options.open(&file).map_err(|e| e.to_string())?;
        writeln!(handle, "{password}").map_err(|e| e.to_string())?;
    }
    let result = run(
        target,
        "sudo -S -p \"\" loginctl enable-linger \"$(id -un)\" 2>&1; \
         echo \"lingering: $(loginctl show-user \"$(id -un)\" -p Linger --value 2>&1)\"",
        Some(file.clone()),
        log,
    )
    .await;
    let _ = std::fs::remove_file(&file);
    let out = result?;
    if out.lines().any(|l| l.trim() == "lingering: yes") {
        Ok(())
    } else {
        Err(out
            .lines()
            .find(|l| !l.starts_with("lingering:"))
            .unwrap_or("sudo did not accept the password")
            .to_owned())
    }
}

/// Stops and removes `pnxr serve` (the unit and the certificates; the
/// program stays) on the server.
pub async fn uninstall_serve(
    authority: &str,
    mut progress: impl FnMut(&str) + Send,
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
        &mut progress,
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
    fn the_unit_runs_pnxr_serve_with_the_certificates() {
        let unit = systemd_unit(&options());
        assert!(unit.contains(
            "ExecStart=%h/.local/bin/pnxr serve --tls %h/.config/pnxr/serve-tls --listen 0.0.0.0:7443\n"
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
