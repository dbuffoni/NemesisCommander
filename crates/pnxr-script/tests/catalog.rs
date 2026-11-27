//! A local Git repository with two plugins, used as a plugin repository.

use std::fs;
use std::path::Path;
use std::process::Command;

use pnxr_script::catalog::{self, RepositorySpec};
use pnxr_script::source::{self, Lock, Updated};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn plugin(root: &Path, name: &str, version: &str) {
    let dir = root.join("plugins").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"name":"{name}","version":"{version}","description":"{name}",
               "commands":[{{"id":"go","title":"Go"}}],"permissions":{{"run":["bsdtar"]}}}}"#
        ),
    )
    .unwrap();
    fs::write(dir.join("main.ts"), "pnxr.print('hi');").unwrap();
}

#[test]
fn two_plugins_install_separately_and_update_from_their_repository() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let work = tempfile::tempdir().unwrap();
    let repo = work.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    plugin(&repo, "alpha", "1.0.0");
    plugin(&repo, "beta", "1.0.0");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "one"]);

    let url = format!("file://{}", repo.display());
    let url = url.replace('\\', "/");
    let spec = RepositorySpec::parse(&url).unwrap();
    let cache = work.path().join("cache");
    let (entries, problems) = catalog::available(&cache, std::slice::from_ref(&spec), false);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["alpha", "beta"]);
    assert!(entries.iter().all(|e| !e.official));

    let plugins = work.path().join("installed");
    let mut lock = Lock::load(&work.path().join("lock.json")).unwrap();
    let alpha =
        catalog::install(catalog::find(&entries, "alpha").unwrap(), &plugins, &mut lock).unwrap();
    catalog::install(catalog::find(&entries, "beta").unwrap(), &plugins, &mut lock).unwrap();
    assert_eq!(alpha.manifest.permissions.describe(), ["may run: bsdtar"]);
    assert_eq!(lock.get("alpha").unwrap().subdir.as_deref(), Some("plugins/alpha"));
    assert_eq!(lock.get("beta").unwrap().url, spec.url);

    // A new version of beta only.
    plugin(&repo, "beta", "1.1.0");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "two"]);
    let beta = source::update("beta", &plugins, &mut lock).unwrap();
    assert!(matches!(beta, Updated::Newer { .. }));
    assert_eq!(lock.get("beta").unwrap().version, "1.1.0");
    assert_eq!(lock.get("alpha").unwrap().version, "1.0.0");

    // Forced refresh sees the new version in the catalogue.
    let (entries, _) = catalog::available(&cache, std::slice::from_ref(&spec), true);
    assert_eq!(catalog::find(&entries, "beta").unwrap().version, "1.1.0");
}
