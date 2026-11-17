//! Symbolic links and hard links to files, made in another folder.

use std::path::{Component, Path, PathBuf};

use sc_vfs::{ProviderRegistry, Result, VfsError, VfsPath};

/// What kind of link to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// A symbolic link whose target is written relative to the link's
    /// folder (survives moving both together).
    RelativeSymlink,
    /// A symbolic link with the full path of the target.
    AbsoluteSymlink,
    /// A second name for the same file (same drive, files only).
    HardLink,
}

/// How to get from the folder `from_dir` to `to`, without `..` beyond what is
/// needed. `None` when they have no common root (other drives).
pub fn relative_path(from_dir: &Path, to: &Path) -> Option<PathBuf> {
    let from: Vec<Component> = from_dir.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for component in &to[common..] {
        out.push(component);
    }
    Some(out)
}

/// What [`create_links`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LinkReport {
    /// Links made.
    pub created: usize,
    /// Why the others were not (`name: reason`).
    pub failed: Vec<String>,
}

/// Makes a link in `folder` for each of `sources`, named like the source.
/// Only local files and folders can be linked.
pub async fn create_links(
    registry: &ProviderRegistry,
    sources: &[VfsPath],
    folder: &VfsPath,
    kind: LinkKind,
) -> Result<LinkReport> {
    let native_folder = folder
        .to_local_path()
        .ok_or_else(|| VfsError::Other("Links can only be made in local folders".into()))?;
    let provider = registry.provider_for(folder).await?;
    let mut report = LinkReport::default();
    for source in sources {
        let name = source.file_name().unwrap_or_default().to_owned();
        let one = async || -> std::result::Result<(), String> {
            let native = source.to_local_path().ok_or("only local files can be linked")?;
            let link = folder.join(&name);
            if provider.stat(&link).await.is_ok() {
                return Err("already exists there".into());
            }
            match kind {
                LinkKind::HardLink => std::fs::hard_link(&native, native_folder.join(&name))
                    .map_err(|e| e.to_string()),
                LinkKind::AbsoluteSymlink => provider
                    .create_symlink(&link, &native.to_string_lossy())
                    .await
                    .map_err(|e| e.to_string()),
                LinkKind::RelativeSymlink => {
                    let target = relative_path(&native_folder, &native).unwrap_or(native);
                    provider
                        .create_symlink(&link, &target.to_string_lossy())
                        .await
                        .map_err(|e| e.to_string())
                }
            }
        };
        match one().await {
            Ok(()) => report.created += 1,
            Err(reason) => report.failed.push(format!("{name}: {reason}")),
        }
    }
    Ok(report)
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::Arc;

    use sc_providers::local::LocalProvider;

    use super::*;

    #[test]
    fn relative_paths() {
        let rel = |a: &str, b: &str| relative_path(Path::new(a), Path::new(b));
        assert_eq!(rel("/a/b", "/a/b/c.txt"), Some(PathBuf::from("c.txt")));
        assert_eq!(rel("/a/b", "/a/c/d.txt"), Some(PathBuf::from("../c/d.txt")));
        assert_eq!(rel("/x", "/a/d.txt"), Some(PathBuf::from("../a/d.txt")));
        assert_eq!(rel("rel", "/a"), None);
    }

    #[tokio::test]
    async fn the_three_kinds_of_link() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("dst")).unwrap();
        std::fs::write(dir.path().join("src/a.txt"), "A").unwrap();
        std::fs::write(dir.path().join("src/b.txt"), "B").unwrap();
        std::fs::write(dir.path().join("dst/b.txt"), "taken").unwrap();
        let registry = ProviderRegistry::new();
        registry.insert(&VfsPath::local_root(), Arc::new(LocalProvider::new()));
        let src = VfsPath::from_local(dir.path().join("src")).unwrap();
        let dst = VfsPath::from_local(dir.path().join("dst")).unwrap();
        let sources = vec![src.join("a.txt"), src.join("b.txt")];

        let report =
            create_links(&registry, &sources, &dst, LinkKind::RelativeSymlink).await.unwrap();
        assert_eq!(report.created, 1);
        assert_eq!(report.failed, ["b.txt: already exists there"]);
        let link = dir.path().join("dst/a.txt");
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("../src/a.txt"));
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "A");

        std::fs::remove_file(&link).unwrap();
        create_links(&registry, &sources[..1], &dst, LinkKind::AbsoluteSymlink).await.unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), dir.path().join("src/a.txt"));

        std::fs::remove_file(&link).unwrap();
        create_links(&registry, &sources[..1], &dst, LinkKind::HardLink).await.unwrap();
        assert!(std::fs::read_link(&link).is_err(), "a hard link is a plain file");
        std::fs::write(dir.path().join("src/a.txt"), "changed").unwrap();
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "changed");
    }
}
