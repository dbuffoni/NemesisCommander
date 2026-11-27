//! The port to what the interfaces ask of remote systems: SSH targets, S3 keys
//! and the install of `pnxr serve`. The interfaces hold a `dyn Remotes`;
//! [`SystemRemotes`] is the real one and [`FakeRemotes`] runs nothing, for tests
//! (ADR 0061).

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;

/// The port a server install listens on unless told otherwise.
pub const DEFAULT_SERVE_PORT: u16 = 7443;

/// An SSH destination: `[user@]host[:port]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    /// Login name, if given.
    pub user: Option<String>,
    /// Host name, address or `~/.ssh/config` alias.
    pub host: String,
    /// SSH port, if not the default.
    pub port: Option<u16>,
}

impl SshTarget {
    /// `user@host`, or `host` without a user.
    pub fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }
}

/// The password of `sudo` on the server, used once. Never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct SudoPassword(pub String);

impl std::fmt::Debug for SudoPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SudoPassword(***)")
    }
}

/// What to install and how (the options of `pnxr serve install`).
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// The server as for SSH: `[user@]host[:ssh-port]`.
    pub authority: String,
    /// The name in the certificate and in `scs://` locations.
    pub name: Option<String>,
    /// TCP port `pnxr serve` listens on.
    pub port: u16,
    /// Only serve paths below these folders (default: everything).
    pub roots: Vec<String>,
    /// Refuse every modification.
    pub read_only: bool,
    /// Issue a new certificate even if the server has one.
    pub renew: bool,
    /// The controller's certificate folder.
    pub tls_dir: PathBuf,
    /// The `sudo` password, to turn lingering on when the install finds it off.
    pub sudo_password: Option<SudoPassword>,
}

impl InstallOptions {
    /// Options with the defaults for `authority`.
    pub fn new(authority: &str, tls_dir: PathBuf) -> Self {
        InstallOptions {
            authority: authority.to_owned(),
            name: None,
            port: DEFAULT_SERVE_PORT,
            roots: Vec::new(),
            read_only: false,
            renew: false,
            tls_dir,
            sudo_password: None,
        }
    }
}

/// Reports a step while something runs.
pub type Progress<'a> = &'a mut (dyn FnMut(&str) + Send);

/// What the interfaces need from remote systems.
#[async_trait]
pub trait Remotes: Send + Sync {
    /// Reads `[user@]host[:port]` as an SSH destination.
    fn ssh_target(&self, authority: &str) -> Result<SshTarget, String>;
    /// The host of an `scs://` authority.
    fn serve_host(&self, authority: &str) -> Result<String, String>;
    /// Remembers S3 keys for this session (`None`: the default profile).
    fn s3_set_session(&self, profile: Option<&str>, access: &str, secret: &str);
    /// Whether keys were typed for this session.
    fn s3_has_session(&self, profile: Option<&str>) -> bool;
    /// Saves S3 keys where the AWS tools read them; returns that file.
    fn s3_save(
        &self,
        profile: Option<&str>,
        access: &str,
        secret: &str,
    ) -> std::io::Result<PathBuf>;
    /// The sentence an install error contains when lingering is off.
    fn linger_off(&self) -> &'static str;
    /// Tests an install without changing the server.
    async fn check_serve(
        &self,
        options: &InstallOptions,
        found: Progress<'_>,
    ) -> Result<(), String>;
    /// Installs `pnxr serve`; returns its `scs://` location.
    async fn install_serve(
        &self,
        options: &InstallOptions,
        progress: Progress<'_>,
    ) -> Result<String, String>;
}

/// The real thing: SSH, the AWS files and the provisioning of the agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRemotes;

fn to_agent(options: &InstallOptions) -> pnxr_agent::provision::InstallOptions {
    pnxr_agent::provision::InstallOptions {
        authority: options.authority.clone(),
        name: options.name.clone(),
        port: options.port,
        roots: options.roots.clone(),
        read_only: options.read_only,
        renew: options.renew,
        tls_dir: options.tls_dir.clone(),
        sudo_password: options
            .sudo_password
            .as_ref()
            .map(|p| pnxr_agent::provision::SudoPassword(p.0.clone())),
    }
}

#[async_trait]
impl Remotes for SystemRemotes {
    fn ssh_target(&self, authority: &str) -> Result<SshTarget, String> {
        let target = pnxr_ssh::Target::parse(authority)?;
        Ok(SshTarget { user: target.user, host: target.host, port: target.port })
    }

    fn serve_host(&self, authority: &str) -> Result<String, String> {
        pnxr_agent::serve::host_and_port(authority).map(|(host, _)| host)
    }

    #[cfg(feature = "remote")]
    fn s3_set_session(&self, profile: Option<&str>, access: &str, secret: &str) {
        pnxr_s3::settings::set_session(profile, access, secret);
    }
    #[cfg(not(feature = "remote"))]
    fn s3_set_session(&self, _: Option<&str>, _: &str, _: &str) {}

    #[cfg(feature = "remote")]
    fn s3_has_session(&self, profile: Option<&str>) -> bool {
        pnxr_s3::settings::has_session(profile)
    }
    #[cfg(not(feature = "remote"))]
    fn s3_has_session(&self, _: Option<&str>) -> bool {
        false
    }

    #[cfg(feature = "remote")]
    fn s3_save(
        &self,
        profile: Option<&str>,
        access: &str,
        secret: &str,
    ) -> std::io::Result<PathBuf> {
        pnxr_s3::settings::save(profile, access, secret)
    }
    #[cfg(not(feature = "remote"))]
    fn s3_save(&self, _: Option<&str>, _: &str, _: &str) -> std::io::Result<PathBuf> {
        Err(std::io::Error::other("this build has no S3"))
    }

    fn linger_off(&self) -> &'static str {
        pnxr_agent::provision::LINGER_OFF
    }

    async fn check_serve(
        &self,
        options: &InstallOptions,
        found: Progress<'_>,
    ) -> Result<(), String> {
        pnxr_agent::provision::check_serve(&to_agent(options), found).await
    }

    async fn install_serve(
        &self,
        options: &InstallOptions,
        progress: Progress<'_>,
    ) -> Result<String, String> {
        pnxr_agent::provision::install_serve(&to_agent(options), progress).await
    }
}

/// A [`Remotes`] that touches nothing: SSH targets are read by a simple rule,
/// S3 keys live in memory and an install says what it was asked and succeeds.
#[derive(Debug, Default)]
pub struct FakeRemotes {
    s3: Mutex<Vec<String>>,
    /// Every install asked for, as `authority` (checks are not listed).
    pub installs: Mutex<Vec<String>>,
}

impl FakeRemotes {
    /// A fake with nothing in it.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Remotes for FakeRemotes {
    fn ssh_target(&self, authority: &str) -> Result<SshTarget, String> {
        let (user, rest) = match authority.rsplit_once('@') {
            Some((user, rest)) => (Some(user.to_owned()), rest),
            None => (None, authority),
        };
        let (host, port) = match rest.rsplit_once(':') {
            Some((host, port)) => {
                (host, Some(port.parse::<u16>().map_err(|_| "invalid port".to_owned())?))
            }
            None => (rest, None),
        };
        if host.is_empty() {
            return Err("missing host".into());
        }
        Ok(SshTarget { user, host: host.to_owned(), port })
    }

    fn serve_host(&self, authority: &str) -> Result<String, String> {
        self.ssh_target(authority.rsplit_once(':').map_or(authority, |(host, _)| host))
            .map(|t| t.host)
    }

    fn s3_set_session(&self, profile: Option<&str>, _: &str, _: &str) {
        self.s3.lock().unwrap_or_else(|e| e.into_inner()).push(profile.unwrap_or("").to_owned());
    }

    fn s3_has_session(&self, profile: Option<&str>) -> bool {
        self.s3.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == profile.unwrap_or(""))
    }

    fn s3_save(&self, _: Option<&str>, access: &str, secret: &str) -> std::io::Result<PathBuf> {
        if access.is_empty() || secret.is_empty() {
            return Err(std::io::Error::other("empty keys"));
        }
        Ok(PathBuf::from("fake-credentials"))
    }

    fn linger_off(&self) -> &'static str {
        "lingering is off on the server"
    }

    async fn check_serve(&self, _: &InstallOptions, found: Progress<'_>) -> Result<(), String> {
        found("fake check");
        Ok(())
    }

    async fn install_serve(
        &self,
        options: &InstallOptions,
        progress: Progress<'_>,
    ) -> Result<String, String> {
        progress("fake install");
        self.installs.lock().unwrap_or_else(|e| e.into_inner()).push(options.authority.clone());
        Ok(format!("scs://{}:{}", options.authority, options.port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_port_is_the_agents() {
        assert_eq!(
            DEFAULT_SERVE_PORT,
            pnxr_agent::provision::InstallOptions::new("h", PathBuf::new()).port
        );
    }

    #[test]
    fn the_fake_reads_targets_like_the_real_one() {
        for text in ["me@nas.lan:2222", "nas.lan", "me@nas.lan", "nas.lan:x", "me@"] {
            assert_eq!(
                FakeRemotes::new().ssh_target(text).ok(),
                SystemRemotes.ssh_target(text).ok(),
                "{text}"
            );
        }
    }

    #[test]
    fn s3_keys_are_remembered_per_profile_in_the_fake() {
        let fake = FakeRemotes::new();
        assert!(!fake.s3_has_session(None));
        fake.s3_set_session(Some("work"), "a", "b");
        assert!(fake.s3_has_session(Some("work")) && !fake.s3_has_session(None));
    }
}
