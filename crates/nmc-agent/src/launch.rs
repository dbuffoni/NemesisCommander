//! Starting a remote agent: `ssh host nmc agent --stdio`, installing `nmc` in
//! `~/.cache/nmc/` first when it is missing.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nmc_proto::transport::{Duplex, connect};
use nmc_ssh::{Remote, Target, ssh_args};
use nmc_vfs::{Provider, ProviderFactory, ProviderRegistry, Result, VfsError, VfsPath};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::client::AgentProvider;

/// Environment variable that replaces `ssh` with another command line.
/// `{host}` is replaced by the authority of the `agent://` URI. Tests use it
/// to run `nmc agent --stdio` locally; it is also handy for `docker exec`.
pub const AGENT_COMMAND_VAR: &str = "NMC_AGENT_COMMAND";

/// Environment variable naming the Linux `nmc` binary to install on servers
/// that do not have it.
pub const AGENT_BINARY_VAR: &str = "NMC_AGENT_BINARY";

/// Printed by the remote start script when no suitable `nmc` exists.
const MISSING_MARKER: &str = "nmc-agent-missing";

/// How long the handshake may take (SSH login included).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

/// The shell script run on the server: start the cached or installed `nmc`
/// of the right version, or report that it is missing.
pub fn remote_start_script(version: &str) -> String {
    format!(
        "p=\"$HOME/.cache/nmc/nmc-{version}\"; \
         if [ -x \"$p\" ]; then exec \"$p\" agent --stdio; fi; \
         if command -v nmc >/dev/null 2>&1 && [ \"$(nmc --version 2>/dev/null)\" = \"nmc {version}\" ]; \
         then exec nmc agent --stdio; fi; \
         echo {MISSING_MARKER} >&2; exit 127"
    )
}

/// The command line that starts the agent for `authority`.
fn agent_command(authority: &str) -> std::result::Result<(String, Vec<String>), String> {
    if let Some(command) = nmc_ssh::command_override(AGENT_COMMAND_VAR, authority) {
        return Ok(command);
    }
    let target = Target::parse(authority)?;
    let script = remote_start_script(crate::VERSION);
    let args = ssh_args(&target, Remote::Script(&script))
        .map_err(|e| format!("agent on {authority}: {e}"))?;
    Ok(("ssh".to_owned(), args))
}

/// Why starting failed.
enum StartError {
    /// No suitable `nmc` on the server: installing it may help.
    Missing,
    /// Anything else, with a readable explanation.
    Other(String),
}

/// Starts the agent process and performs the handshake.
async fn start(authority: &str) -> std::result::Result<AgentProvider, StartError> {
    let (program, args) = agent_command(authority).map_err(StartError::Other)?;
    let env = nmc_ssh::password_env(authority);
    let mut child = nmc_ssh::spawn_env(&program, &args, &env)
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

/// The Linux `nmc` binary to install on servers: `$NMC_AGENT_BINARY`, then
/// `nmc-agent-linux-<arch>` or `nmc` next to this program, then this program
/// itself when it is `nmc`.
pub(crate) fn agent_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(AGENT_BINARY_VAR) {
        return Some(PathBuf::from(path));
    }
    if !cfg!(target_os = "linux") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidates =
        [dir.join(format!("nmc-agent-linux-{}", std::env::consts::ARCH)), dir.join("nmc")];
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    (exe.file_stem()? == "nmc").then_some(exe)
}

/// Runs a one-off command on `target` and returns its standard output.
pub(crate) async fn ssh_run(
    target: &Target,
    script: &str,
    input: Option<PathBuf>,
) -> std::result::Result<String, String> {
    // On this computer itself the script runs directly: installing on
    // `localhost` must not need an SSH server.
    let mut command = if target.is_this_computer() {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    } else {
        let mut command = Command::new("ssh");
        command.envs(nmc_ssh::target_env(target)).args(ssh_args(target, Remote::Script(script))?);
        command
    };
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
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

/// Copies `nmc` to `~/.cache/nmc/nmc-<version>` on the server.
async fn install(authority: &str) -> std::result::Result<(), String> {
    let target = Target::parse(authority)?;
    let binary = agent_binary().ok_or(
        "nmc is not installed on the server and no Linux nmc binary is available to copy \
         (set NMC_AGENT_BINARY)",
    )?;
    let system = ssh_run(&target, "uname -sm", None).await?;
    let expected = format!("Linux {}", std::env::consts::ARCH);
    if std::env::var(AGENT_BINARY_VAR).is_err() && system != expected {
        return Err(format!(
            "the server runs {system}; install nmc there or set NMC_AGENT_BINARY to a matching build"
        ));
    }
    let version = crate::VERSION;
    let script = format!(
        "d=\"$HOME/.cache/nmc\"; mkdir -p \"$d\" && cat > \"$d/nmc-{version}.part\" && \
         chmod 755 \"$d/nmc-{version}.part\" && mv \"$d/nmc-{version}.part\" \"$d/nmc-{version}\""
    );
    ssh_run(&target, &script, Some(binary)).await.map(|_| ())
}

/// Connects to the agent for `authority`, installing it once if needed.
pub async fn connect_agent(authority: &str) -> std::result::Result<AgentProvider, String> {
    match start(authority).await {
        Ok(provider) => Ok(provider),
        Err(StartError::Other(message)) => Err(message),
        Err(StartError::Missing) => {
            install(authority).await.map_err(|e| format!("installing nmc on {authority}: {e}"))?;
            match start(authority).await {
                Ok(provider) => Ok(provider),
                Err(StartError::Missing) => Err(format!("nmc on {authority} does not start")),
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
        assert!(script.contains("$HOME/.cache/nmc/nmc-1.2.3"));
        assert!(script.contains("\"nmc 1.2.3\""));
        assert!(!script.contains('\''), "the script is wrapped in single quotes");
    }

    #[tokio::test]
    async fn localhost_needs_no_ssh_server() {
        // No `ssh` is started for this computer itself.
        let target = Target::parse("localhost").unwrap();
        assert!(target.is_this_computer());
        assert_eq!(ssh_run(&target, "echo $((40 + 2))", None).await.unwrap(), "42");
        assert!(
            ssh_run(&target, "echo oops >&2; exit 3", None).await.unwrap_err().contains("oops")
        );
        // Another port or another account is not this computer.
        assert!(!Target::parse("localhost:2222").unwrap().is_this_computer());
        assert!(!Target::parse("someone-else-123@localhost").unwrap().is_this_computer());
        assert!(!Target::parse("nas.lan").unwrap().is_this_computer());
    }
}
