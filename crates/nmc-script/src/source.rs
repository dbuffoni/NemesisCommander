//! Plugins from Git: `nmc plugin install URL` clones a repository, installs
//! the plugin in it and remembers where it came from in a lock file, so
//! `nmc plugin update` can fetch a newer version.
//!
//! The system's `git` does the fetching, so the user's credentials, proxies
//! and `url.insteadOf` rules apply as they do everywhere else. Nothing
//! runs from the repository while it is fetched: the files are copied into
//! the plugins folder, and the plugin still needs the user's permission
//! ([`crate::plugin::Grants`]) before it does anything.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::plugin::{self, Plugin, PluginError};

/// Where a plugin came from, as written in the lock file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The repository: whatever `git clone` accepts.
    pub url: String,
    /// A branch or tag (the repository's default branch when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// The folder of the repository holding the plugin (its root when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdir: Option<String>,
    /// The commit that is installed.
    pub commit: String,
    /// The plugin's version at that commit.
    pub version: String,
}

/// What `install` was given, when it names a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// The URL to clone.
    pub url: String,
    /// A branch or tag, from `#ref` at the end.
    pub reference: Option<String>,
}

/// The repository `text` names, or `None` when it is a folder.
///
/// Recognised: URLs (`https://`, `http://`, `ssh://`, `git://`, `file://`),
/// scp-like addresses (`git@host:owner/repo`), anything ending in `.git`
/// and `github:owner/repo`. A `#branch-or-tag` at the end picks a reference.
pub fn parse_repository(text: &str) -> Option<Repository> {
    let (address, reference) = match text.split_once('#') {
        Some((address, reference)) if !reference.is_empty() => {
            (address, Some(reference.to_owned()))
        }
        Some((address, _)) => (address, None),
        None => (text, None),
    };
    let url = if let Some(rest) = address.strip_prefix("github:") {
        format!("https://github.com/{}.git", rest.trim_end_matches(".git"))
    } else if ["https://", "http://", "ssh://", "git://", "file://"]
        .iter()
        .any(|scheme| address.starts_with(scheme))
        || address.ends_with(".git") && !Path::new(address).is_dir()
        || is_scp_like(address)
    {
        address.to_owned()
    } else {
        return None;
    };
    Some(Repository { url, reference })
}

/// `user@host:path`, which is not a Windows drive (`C:\`) either.
fn is_scp_like(text: &str) -> bool {
    match text.split_once(':') {
        Some((host, path)) => {
            host.contains('@') && !host.contains('/') && !host.contains('\\') && !path.is_empty()
        }
        None => false,
    }
}

/// A repository cloned into a temporary folder.
pub struct Checkout {
    _work: tempfile::TempDir,
    root: PathBuf,
    /// The commit that was checked out.
    pub commit: String,
}

impl Checkout {
    /// The folder holding the plugin: the repository's root, or `subdir`.
    pub fn plugin_dir(&self, subdir: Option<&str>) -> Result<PathBuf, PluginError> {
        let Some(subdir) = subdir else { return Ok(self.root.clone()) };
        let relative = Path::new(subdir);
        if relative.is_absolute()
            || relative.components().any(|c| c == std::path::Component::ParentDir)
        {
            return Err(PluginError::Source(format!("{subdir:?} must be inside the repository")));
        }
        Ok(self.root.join(relative))
    }
}

fn git(args: &[&str], dir: Option<&Path>) -> Result<String, PluginError> {
    let mut command = Command::new("git");
    command.args(args).env("GIT_TERMINAL_PROMPT", "0");
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let output = command.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PluginError::Source("installing from Git needs `git` on the PATH".into())
        } else {
            PluginError::Source(format!("cannot run git: {e}"))
        }
    })?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        let why = why.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("git failed");
        return Err(PluginError::Source(format!("git {}: {}", args[0], why.trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Clones `url` (at `reference`, a branch or tag) into a temporary folder.
pub fn fetch(url: &str, reference: Option<&str>) -> Result<Checkout, PluginError> {
    let work = tempfile::tempdir().map_err(|e| PluginError::Source(e.to_string()))?;
    let root = work.path().join("repository");
    let target = root.to_string_lossy().into_owned();
    let mut args = vec!["clone", "--quiet", "--depth", "1"];
    if let Some(reference) = reference {
        args.extend(["--branch", reference]);
    }
    args.extend(["--", url, &target]);
    git(&args, None)?;
    let commit = git(&["rev-parse", "HEAD"], Some(&root))?;
    Ok(Checkout { _work: work, root, commit })
}

/// Installs the plugin of the repository `repository` (in `subdir` of it)
/// into `plugins_dir` and records where it came from in `lock`.
pub fn install(
    repository: &Repository,
    subdir: Option<&str>,
    plugins_dir: &Path,
    lock: &mut Lock,
) -> Result<Plugin, PluginError> {
    let checkout = fetch(&repository.url, repository.reference.as_deref())?;
    install_checkout(&checkout, repository, subdir, plugins_dir, lock)
}

fn install_checkout(
    checkout: &Checkout,
    repository: &Repository,
    subdir: Option<&str>,
    plugins_dir: &Path,
    lock: &mut Lock,
) -> Result<Plugin, PluginError> {
    let folder = checkout.plugin_dir(subdir)?;
    if !folder.join(plugin::MANIFEST).is_file() {
        return Err(PluginError::Source(format!(
            "the repository has no {} {} (use --subdir when the plugin is in a folder of it)",
            plugin::MANIFEST,
            if subdir.is_some() { "in that folder" } else { "at its top" },
        )));
    }
    let plugin = plugin::install(&folder, plugins_dir)?;
    lock.record(
        plugin.name(),
        Source {
            url: repository.url.clone(),
            reference: repository.reference.clone(),
            subdir: subdir.map(str::to_owned),
            commit: checkout.commit.clone(),
            version: plugin.manifest.version.clone(),
        },
    )?;
    Ok(plugin)
}

/// What an update found.
#[derive(Debug)]
pub enum Updated {
    /// The installed commit is the latest one.
    Current {
        /// The installed version.
        version: String,
    },
    /// A newer commit was installed.
    Newer {
        /// The plugin as installed now.
        plugin: Box<Plugin>,
        /// The version and commit it replaced.
        from: (String, String),
    },
}

/// Fetches the latest commit of the source of plugin `name` and installs it
/// when it is not the one installed.
pub fn update(name: &str, plugins_dir: &Path, lock: &mut Lock) -> Result<Updated, PluginError> {
    let source = lock.get(name).cloned().ok_or_else(|| {
        PluginError::Source(format!(
            "{name} was not installed from Git (nothing to update from): see `nmc plugin list`"
        ))
    })?;
    let repository = Repository { url: source.url.clone(), reference: source.reference.clone() };
    let checkout = fetch(&repository.url, repository.reference.as_deref())?;
    if checkout.commit == source.commit {
        return Ok(Updated::Current { version: source.version });
    }
    let plugin =
        install_checkout(&checkout, &repository, source.subdir.as_deref(), plugins_dir, lock)?;
    Ok(Updated::Newer { plugin: Box::new(plugin), from: (source.version, source.commit) })
}

/// Where each Git-installed plugin came from: the lock file.
#[derive(Debug, Clone, Default)]
pub struct Lock {
    path: PathBuf,
    sources: BTreeMap<String, Source>,
}

impl Lock {
    /// The lock stored in `path` (empty when the file does not exist).
    pub fn load(path: &Path) -> Result<Self, PluginError> {
        let failed =
            |message: String| PluginError::Source(format!("{}: {message}", path.display()));
        let sources = match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| failed(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(failed(e.to_string())),
        };
        Ok(Lock { path: path.to_path_buf(), sources })
    }

    /// The source of plugin `name`, when it came from Git.
    pub fn get(&self, name: &str) -> Option<&Source> {
        self.sources.get(name)
    }

    /// The names of the plugins that came from Git.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sources.keys().map(String::as_str)
    }

    /// Records where plugin `name` came from, and saves.
    pub fn record(&mut self, name: &str, source: Source) -> Result<(), PluginError> {
        self.sources.insert(name.to_owned(), source);
        self.save()
    }

    /// Forgets plugin `name` (a copy installed from a folder replaces the
    /// Git one; removing a plugin removes it), and saves when it was there.
    pub fn forget(&mut self, name: &str) -> Result<(), PluginError> {
        if self.sources.remove(name).is_some() {
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> Result<(), PluginError> {
        let failed =
            |message: String| PluginError::Source(format!("{}: {message}", self.path.display()));
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| failed(e.to_string()))?;
        }
        let text =
            serde_json::to_string_pretty(&self.sources).map_err(|e| failed(e.to_string()))?;
        fs::write(&self.path, text).map_err(|e| failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repositories_are_told_from_folders() {
        let repo = |text: &str| parse_repository(text).map(|r| (r.url, r.reference));
        assert_eq!(
            repo("https://github.com/me/tidy.git#v1.2"),
            Some(("https://github.com/me/tidy.git".into(), Some("v1.2".into())))
        );
        assert_eq!(repo("github:me/tidy"), Some(("https://github.com/me/tidy.git".into(), None)));
        assert_eq!(repo("git@example.com:me/tidy"), Some(("git@example.com:me/tidy".into(), None)));
        assert_eq!(
            repo("ssh://git@example.com/me/tidy"),
            Some(("ssh://git@example.com/me/tidy".into(), None))
        );
        assert_eq!(repo("./my-plugin"), None);
        assert_eq!(repo("my-plugin"), None);
        assert_eq!(repo("C:\\plugins\\tidy"), None);
        assert_eq!(repo("/home/me/tidy"), None);
    }
}
