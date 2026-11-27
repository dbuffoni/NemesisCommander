//! What the assistant may touch: the permission model of scripts, plus the
//! checks that only matter for an assistant that cannot be trusted with
//! `..` and symbolic links, and the files it never reads.

use std::path::{Path, PathBuf};

use pnxr_script::{Access, Permissions};
use pnxr_vfs::VfsPath;

/// File names whose content is never given to an assistant (keys, tokens,
/// passwords). The same list serves the "secrets check" of other features.
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    const EXACT: [&str; 17] = [
        "application_default_credentials.json",
        "kubeconfig",
        ".dockercfg",
        ".env",
        ".netrc",
        "_netrc",
        ".git-credentials",
        ".npmrc",
        ".pypirc",
        "credentials",
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
        "secrets.json",
        "secrets.yml",
        "secrets.yaml",
    ];
    const SUFFIXES: [&str; 8] =
        [".pem", ".key", ".p12", ".pfx", ".kdbx", ".keystore", ".jks", ".gpg"];
    EXACT.contains(&lower.as_str())
        || lower.starts_with(".env.")
        || SUFFIXES.iter().any(|s| lower.ends_with(s))
}

/// Whether the file at `path` may hold secrets: by its own name, or by the
/// name of the file a symbolic link leads to.
pub fn is_secret_path(path: &VfsPath) -> bool {
    path.file_name().is_some_and(is_secret_name)
        || path
            .to_local_path()
            .and_then(|p| std::fs::canonicalize(p).ok())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .is_some_and(|n| is_secret_name(&n))
}

/// Locations allowed for reading and for writing.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    permissions: Permissions,
    /// The same grants with local folders resolved through symbolic links.
    resolved: Permissions,
}

impl Grants {
    /// Nothing allowed.
    pub fn none() -> Self {
        Grants::default()
    }

    /// Allows reading `location` and below.
    pub fn allow_read(&mut self, location: VfsPath) {
        if let Some(real) = resolve_local(&location) {
            self.resolved.allow_read(real);
        }
        self.resolved.allow_read(location.clone());
        self.permissions.allow_read(location);
    }

    /// Allows reading and changing `location` and below.
    pub fn allow_write(&mut self, location: VfsPath) {
        if let Some(real) = resolve_local(&location) {
            self.resolved.allow_write(real);
        }
        self.resolved.allow_write(location.clone());
        self.permissions.allow_write(location);
    }

    /// Checks `path`: as written (after `..` is resolved) and, for local
    /// paths, after following symbolic links, so a link inside a granted
    /// folder cannot lead out of it.
    pub fn check(&self, path: &VfsPath, access: Access) -> Result<(), String> {
        let refuse = |_: String| {
            let flag = if access == Access::Read { "--read" } else { "--write" };
            format!(
                "{path}: not allowed. The user did not grant this location; pnxr mcp {flag} DIR grants it"
            )
        };
        self.permissions.check(path, access).map_err(refuse)?;
        if let Some(real) = resolve_local(path) {
            self.resolved.check(&real, access).map_err(refuse)?;
        }
        Ok(())
    }
}

/// A local path with its symbolic links followed (the nearest existing
/// parent is resolved when the path itself does not exist yet).
fn resolve_local(path: &VfsPath) -> Option<VfsPath> {
    if path.scheme() != "file" || path.container().is_some() {
        return None;
    }
    let local = path.to_local_path()?;
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut base: &Path = &local;
    let resolved: PathBuf = loop {
        match std::fs::canonicalize(base) {
            Ok(real) => break real,
            Err(_) => {
                missing.push(base.file_name()?.to_owned());
                base = base.parent()?;
            }
        }
    };
    let mut full = resolved;
    for name in missing.iter().rev() {
        full.push(name);
    }
    VfsPath::from_local(full).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_secret_is_a_secret() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("id_rsa");
        std::fs::write(&key, "x").unwrap();
        let link = dir.path().join("notes.txt");
        std::os::unix::fs::symlink(&key, &link).unwrap();
        assert!(is_secret_path(&VfsPath::from_local(&link).unwrap()));
        let plain = dir.path().join("plain.txt");
        std::fs::write(&plain, "x").unwrap();
        assert!(!is_secret_path(&VfsPath::from_local(&plain).unwrap()));
    }

    #[test]
    fn secrets_are_recognised_by_name() {
        for name in [".env", ".ENV.local", "id_rsa", "server.key", "cert.pem", ".netrc"] {
            assert!(is_secret_name(name), "{name}");
        }
        for name in ["notes.txt", "environment.md", "keys.txt", "monkey"] {
            assert!(!is_secret_name(name), "{name}");
        }
    }

    #[test]
    fn nothing_is_allowed_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = VfsPath::from_local(dir.path()).unwrap();
        assert!(Grants::none().check(&path, Access::Read).is_err());
    }

    #[test]
    fn dotdot_and_symbolic_links_do_not_leave_the_grant() {
        let root = tempfile::tempdir().unwrap();
        let allowed = root.path().join("allowed");
        let secret = root.path().join("secret");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(secret.join("a.txt"), "x").unwrap();
        let mut grants = Grants::none();
        grants.allow_read(VfsPath::from_local(&allowed).unwrap());
        let local = |p: &Path| VfsPath::from_local(p).unwrap();
        assert!(grants.check(&local(&allowed.join("new.txt")), Access::Read).is_ok());
        assert!(
            grants.check(&local(&allowed.join("..").join("secret/a.txt")), Access::Read).is_err()
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, allowed.join("link")).unwrap();
            let through = local(&allowed.join("link").join("a.txt"));
            assert!(grants.check(&through, Access::Read).is_err(), "a link leads out");
        }
        // Reading is not writing.
        assert!(grants.check(&local(&allowed), Access::Write).is_err());
    }
}
