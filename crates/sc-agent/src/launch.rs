//! Starting a remote agent: `ssh host sc agent --stdio`, installing `sc` in
//! `~/.cache/sc/` first when it is missing.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sc_proto::transport::{Duplex, connect};
use sc_ssh::{Remote, Target, ssh_args};
use sc_vfs::{Provider, ProviderFactory, ProviderRegistry, Result, VfsError, VfsPath};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::client::AgentProvider;

/// Environment variable that replaces `ssh` with another command line.
/// `{host}` is replaced by the authority of the `agent://` URI. Tests use it
/// to run `sc agent --stdio` locally; it is also handy for `docker exec`.
pub const AGENT_COMMAND_VAR: &str = "SC_AGENT_COMMAND";

/// Environment variable naming the Linux `sc` binary to install on servers
/// that do not have it.
pub const AGENT_BINARY_VAR: &str = "SC_AGENT_BINARY";

/// Printed by the remote start script when no suitable `sc` exists.
const MISSING_MARKER: &str = "sc-agent-missing";

/// How long the handshake may take (SSH login included).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

/// The shell script run on the server: start the cached or installed `sc`
/// of the right version, or report that it is missing.
pub fn remote_start_script(version: &str) -> String {
    format!(
        "p=\"$HOME/.cache/sc/sc-{version}\"; \
         if [ -x \"$p\" ]; then exec \"$p\" agent --stdio; fi; \
         if command -v sc >/dev/null 2>&1 && [ \"$(sc --version 2>/dev/null)\" = \"sc {version}\" ]; \
         then exec sc agent --stdio; fi; \
         echo {MISSING_MARKER} >&2; exit 127"
    )
}

/// The command line that starts the agent for `authority`.
fn agent_command(authority: &str) -> std::result::Result<(String, Vec<String>), String> {
    if let Some(command) = sc_ssh::command_override(AGENT_COMMAND_VAR, authority) {
        return Ok(command);
    }
    let target = Target::parse(authority)?;
    let script = remote_start_script(crate::VERSION);
    Ok(("ssh".to_owned(), ssh_args(&target, Remote::Script(&script))))
}

/// Why starting failed.
enum StartError {
    /// No suitable `sc` on the server: installing it may help.
    Missing,
    /// Anything else, with a readable explanation.
    Other(String),
}

/// Starts the agent process and performs the handshake.
async fn start(authority: &str) -> std::result::Result<AgentProvider, StartError> {
    let (program, args) = agent_command(authority).map_err(StartError::Other)?;
    let mut child = sc_ssh::spawn(&program, &args)
        .map_err(|e| StartError::Other(format!("cannot start {program}: {e}")))?;
    let stdin = child.stdin.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");

    let attempt = async {
        let client = connect(Duplex::new(stdout, stdin)).await.map_err(|e| e.to_string())?;
        AgentProvider::handshake(client, None).await
    };
    let result = tokio::time::timeout(HANDSHAKE_TIMEOUT, attempt).await;
    match result {
        Ok(Ok(provider)) => {
            // Keep the process (and its stderr, so it never blocks on a full
            // pipe) alive as long as the provider.
            tokio::spawn(async move {
                let mut sink = Vec::new();
                let _ = stderr.read_to_end(&mut sink).await;
            });
            Ok(provider.keeping(child))
        }
        failure => {
            let mut message = String::new();
            let _ =
                tokio::time::timeout(Duration::from_secs(5), stderr.read_to_string(&mut message))
                    .await;
            let _ = child.start_kill();
            if message.contains(MISSING_MARKER) {
                return Err(StartError::Missing);
            }
            let reason = match failure {
                Ok(Err(e)) => e,
                Err(_) => "timed out".into(),
                Ok(Ok(_)) => unreachable!(),
            };
            let detail = message.trim();
            Err(StartError::Other(if detail.is_empty() {
                format!("agent on {authority}: {reason}")
            } else {
                format!("agent on {authority}: {detail}")
            }))
        }
    }
}

/// The Linux `sc` binary to install on servers: `$SC_AGENT_BINARY`, then
/// `sc-agent-linux-<arch>` or `sc` next to this program, then this program
/// itself when it is `sc`.
fn agent_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(AGENT_BINARY_VAR) {
        return Some(PathBuf::from(path));
    }
    if !cfg!(target_os = "linux") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidates =
        [dir.join(format!("sc-agent-linux-{}", std::env::consts::ARCH)), dir.join("sc")];
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    (exe.file_stem()? == "sc").then_some(exe)
}

/// Runs a one-off command on `target` and returns its standard output.
async fn ssh_run(
    target: &Target,
    script: &str,
    input: Option<PathBuf>,
) -> std::result::Result<String, String> {
    let mut command = Command::new("ssh");
    command
        .args(ssh_args(target, Remote::Script(script)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdin(match &input {
        Some(file) => Stdio::from(std::fs::File::open(file).map_err(|e| e.to_string())?),
        None => Stdio::null(),
    });
    let output = command.output().await.map_err(|e| format!("cannot start ssh: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Copies `sc` to `~/.cache/sc/sc-<version>` on the server.
async fn install(authority: &str) -> std::result::Result<(), String> {
    let target = Target::parse(authority)?;
    let binary = agent_binary().ok_or(
        "sc is not installed on the server and no Linux sc binary is available to copy \
         (set SC_AGENT_BINARY)",
    )?;
    let system = ssh_run(&target, "uname -sm", None).await?;
    let expected = format!("Linux {}", std::env::consts::ARCH);
    if std::env::var(AGENT_BINARY_VAR).is_err() && system != expected {
        return Err(format!(
            "the server runs {system}; install sc there or set SC_AGENT_BINARY to a matching build"
        ));
    }
    let version = crate::VERSION;
    let script = format!(
        "d=\"$HOME/.cache/sc\"; mkdir -p \"$d\" && cat > \"$d/sc-{version}.part\" && \
         chmod 755 \"$d/sc-{version}.part\" && mv \"$d/sc-{version}.part\" \"$d/sc-{version}\""
    );
    ssh_run(&target, &script, Some(binary)).await.map(|_| ())
}

/// Connects to the agent for `authority`, installing it once if needed.
pub async fn connect_agent(authority: &str) -> std::result::Result<AgentProvider, String> {
    match start(authority).await {
        Ok(provider) => Ok(provider),
        Err(StartError::Other(message)) => Err(message),
        Err(StartError::Missing) => {
            install(authority).await.map_err(|e| format!("installing sc on {authority}: {e}"))?;
            match start(authority).await {
                Ok(provider) => Ok(provider),
                Err(StartError::Missing) => Err(format!("sc on {authority} does not start")),
                Err(StartError::Other(message)) => Err(message),
            }
        }
    }
}

/// Creates [`AgentProvider`]s for `agent://` paths.
pub struct AgentFactory;

#[async_trait]
impl ProviderFactory for AgentFactory {
    fn scheme(&self) -> &str {
        "agent"
    }

    async fn create(&self, path: &VfsPath, _: &ProviderRegistry) -> Result<Arc<dyn Provider>> {
        let provider = connect_agent(path.authority()).await.map_err(VfsError::Other)?;
        Ok(Arc::new(provider))
    }
}

impl AgentProvider {
    /// Ties the lifetime of `child` to the provider.
    fn keeping(self, child: Child) -> Self {
        self.with_keep_alive(Box::new(std::sync::Mutex::new(child)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_script_prefers_the_cached_binary() {
        let script = remote_start_script("1.2.3");
        assert!(script.contains("$HOME/.cache/sc/sc-1.2.3"));
        assert!(script.contains("\"sc 1.2.3\""));
        assert!(!script.contains('\''), "the script is wrapped in single quotes");
    }
}
