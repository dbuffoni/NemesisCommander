//! The interfaces depend on ports, not on adapters (ADR 0061).
//!
//! The front-end crates (`pnxr-frontend`, `pnxr-tui`) may not list a crate that
//! talks to the outside world (SSH, SFTP, S3, WebDAV, SMB, the agent, TLS,
//! libarchive) among their dependencies. What they need from those crates
//! comes through a port of `pnxr-app` (`Remotes`, `SecretStore`) or of the
//! front end itself (`HostTools`); `pnxr-app` is the only place that wires an
//! adapter to its port. Tests may use adapters: only `[dependencies]` count.

use std::path::Path;

/// Crates that are adapters to the outside.
const ADAPTERS: &[&str] = &[
    "pnxr-agent",
    "pnxr-ssh",
    "pnxr-sftp",
    "pnxr-s3",
    "pnxr-webdav",
    "pnxr-smb",
    "pnxr-tls",
    "pnxr-libarchive",
    "pnxr-proto",
    "pnxr-mcp",
];

/// The names listed under `[dependencies]` (and `[target.*.dependencies]`).
fn dependencies(manifest: &str) -> Vec<String> {
    let mut inside = false;
    let mut names = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[dependencies]"
                || (line.starts_with("[target.") && line.ends_with(".dependencies]"));
            continue;
        }
        if inside && let Some((name, _)) = line.split_once('=') {
            let name = name.trim().split('.').next().unwrap_or("").trim();
            if !name.is_empty() && !name.starts_with('#') {
                names.push(name.to_owned());
            }
        }
    }
    names
}

fn manifest_of(crate_dir: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(crate_dir).join("Cargo.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_front_ends_depend_on_no_adapter_crate() {
    for crate_dir in ["pnxr-frontend", "pnxr-tui"] {
        let found: Vec<String> = dependencies(&manifest_of(crate_dir))
            .into_iter()
            .filter(|name| ADAPTERS.contains(&name.as_str()))
            .collect();
        assert!(
            found.is_empty(),
            "{crate_dir} depends on adapter crates {found:?}: go through a port of pnxr-app instead (ADR 0061)"
        );
    }
}

#[test]
fn the_guard_reads_dependencies_and_ignores_dev_dependencies() {
    let manifest = "[package]\nname = \"x\"\n[dependencies]\npnxr-vfs.workspace = true\n\
                    pnxr-s3 = { workspace = true }\n[dev-dependencies]\npnxr-agent.workspace = true\n";
    assert_eq!(dependencies(manifest), ["pnxr-vfs", "pnxr-s3"]);
}

#[test]
fn pnxr_app_is_where_adapters_are_wired() {
    let listed = dependencies(&manifest_of("pnxr-app"));
    for adapter in ["pnxr-agent", "pnxr-ssh", "pnxr-secrets"] {
        assert!(listed.iter().any(|n| n == adapter), "pnxr-app wires {adapter}");
    }
}
