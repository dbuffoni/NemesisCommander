//! Records the git commit the program is built from, for the About window.

use std::process::Command;

fn main() {
    // A release build may name the commit itself (no git checkout there).
    let commit = std::env::var("PNXR_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| {
            let out = Command::new("git")
                .args(["describe", "--tags", "--always", "--dirty"])
                .output()
                .ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=PNXR_BUILD_COMMIT={commit}");
    println!("cargo:rerun-if-env-changed=PNXR_COMMIT");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
}
