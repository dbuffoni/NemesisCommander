//! Turning "copy these items there" into a list of concrete steps.

use std::time::SystemTime;

use futures::StreamExt;
use sc_vfs::{
    EntryKind, Provider, ProviderRegistry, Result, SetAttributes, VfsError, VfsPath, walk,
};
use serde::{Deserialize, Serialize};

/// One idempotent unit of work. Running a step twice has the same effect
/// as running it once, which is what makes journaled plans resumable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    /// Create a directory (succeeds if it already exists).
    CreateDir {
        /// Directory to create.
        path: VfsPath,
    },
    /// Copy one regular file.
    CopyFile {
        /// Source file.
        from: VfsPath,
        /// Destination file.
        to: VfsPath,
        /// Size at planning time, for progress estimates.
        size: u64,
        /// Modification time to restore on the copy, if preserved.
        modified: Option<SystemTime>,
        /// Permission bits to restore on the copy, if preserved.
        permissions: Option<u32>,
    },
    /// Recreate a symbolic link. If the destination provider does not
    /// support links, the file the link points to is copied instead.
    CopySymlink {
        /// The source link.
        from: VfsPath,
        /// Where to create the new link.
        to: VfsPath,
        /// The link target, as stored in the link.
        target: String,
    },
    /// Restore directory attributes after its contents were written
    /// (writing into a directory changes its modification time).
    SetAttributes {
        /// Directory to update.
        path: VfsPath,
        /// Attributes to apply.
        attributes: SetAttributes,
    },
    /// Rename within one provider, falling back to copy + delete when the
    /// provider cannot rename between those two paths (e.g. across disks).
    Move {
        /// Item to move.
        from: VfsPath,
        /// New location.
        to: VfsPath,
    },
    /// Delete a file, link, or (empty) directory.
    Remove {
        /// What to delete.
        path: VfsPath,
        /// Whether it is a directory.
        is_dir: bool,
    },
}

impl Step {
    /// `true` for steps that may run concurrently with their neighbours.
    pub fn is_parallel(&self) -> bool {
        matches!(self, Step::CopyFile { .. } | Step::CopySymlink { .. })
    }

    /// The path this step writes to or removes, for display.
    pub fn target(&self) -> &VfsPath {
        match self {
            Step::CreateDir { path }
            | Step::SetAttributes { path, .. }
            | Step::Remove { path, .. } => path,
            Step::CopyFile { to, .. } | Step::CopySymlink { to, .. } | Step::Move { to, .. } => to,
        }
    }
}

/// A complete, ordered list of steps plus totals for progress reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Steps in execution order.
    pub steps: Vec<Step>,
    /// Sum of file sizes to copy.
    pub total_bytes: u64,
    /// Number of files and links to copy.
    pub total_files: u64,
    /// Problems found while planning (unreadable folders, ...). The plan is
    /// still valid for everything else.
    pub warnings: Vec<String>,
}

/// Choices that shape a copy plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanOptions {
    /// Keep modification times.
    pub preserve_mtime: bool,
    /// Keep POSIX permission bits.
    pub preserve_permissions: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        PlanOptions { preserve_mtime: true, preserve_permissions: true }
    }
}

/// Plans copying `sources` to `destination`.
///
/// * If `destination` is an existing directory, each source is copied
///   *into* it, keeping its name.
/// * If it does not exist and there is one source, the copy gets the name
///   `destination` (copy-and-rename).
/// * If it does not exist and there are several sources, it is created as
///   a directory first.
pub async fn plan_copy(
    registry: &ProviderRegistry,
    sources: &[VfsPath],
    destination: &VfsPath,
    options: PlanOptions,
) -> Result<Plan> {
    let mut builder = Builder::new(options);
    for (source, target) in
        resolve_targets(registry, sources, destination, &mut builder.plan).await?
    {
        let provider = registry.provider_for(&source).await?;
        builder.add_copy(provider.as_ref(), &source, &target).await?;
    }
    Ok(builder.finish())
}

/// Plans moving `sources` to `destination`, with the same destination
/// rules as [`plan_copy`].
///
/// Items that stay on the same provider become [`Step::Move`] (a rename,
/// with automatic fallback). Items that change provider become a copy
/// followed by the removal of the source.
pub async fn plan_move(
    registry: &ProviderRegistry,
    sources: &[VfsPath],
    destination: &VfsPath,
    options: PlanOptions,
) -> Result<Plan> {
    let mut builder = Builder::new(options);
    let mut removals = Plan::default();
    for (source, target) in
        resolve_targets(registry, sources, destination, &mut builder.plan).await?
    {
        if source.provider_key() == target.provider_key() {
            builder.plan.steps.push(Step::Move { from: source, to: target });
        } else {
            let provider = registry.provider_for(&source).await?;
            builder.add_copy(provider.as_ref(), &source, &target).await?;
            add_removal(provider.as_ref(), &source, &mut removals).await?;
        }
    }
    let mut plan = builder.finish();
    plan.steps.extend(removals.steps);
    plan.warnings.extend(removals.warnings);
    Ok(plan)
}

/// Plans deleting `paths` and everything below them (children first).
pub async fn plan_delete(registry: &ProviderRegistry, paths: &[VfsPath]) -> Result<Plan> {
    let mut plan = Plan::default();
    for path in paths {
        let provider = registry.provider_for(path).await?;
        add_removal(provider.as_ref(), path, &mut plan).await?;
    }
    Ok(plan)
}

/// Pairs every source with the exact path it will be copied/moved to.
async fn resolve_targets(
    registry: &ProviderRegistry,
    sources: &[VfsPath],
    destination: &VfsPath,
    plan: &mut Plan,
) -> Result<Vec<(VfsPath, VfsPath)>> {
    let dst_provider = registry.provider_for(destination).await?;
    let into_directory = match dst_provider.stat(destination).await {
        Ok(entry) if entry.is_dir() => true,
        Ok(_) if sources.len() > 1 => return Err(VfsError::NotADirectory(destination.clone())),
        Ok(_) => false, // single source onto an existing file: a conflict, resolved later
        Err(e) if e.is_not_found() && sources.len() > 1 => {
            plan.steps.push(Step::CreateDir { path: destination.clone() });
            true
        }
        Err(e) if e.is_not_found() => false,
        Err(e) => return Err(e),
    };

    let mut pairs = Vec::with_capacity(sources.len());
    for source in sources {
        let target = if into_directory {
            let name = source.file_name().ok_or_else(|| VfsError::InvalidPath {
                input: source.to_string(),
                reason: "cannot copy a root directory into another directory",
            })?;
            destination.join(name)
        } else {
            destination.clone()
        };
        if &target == source {
            return Err(VfsError::InvalidPath {
                input: source.to_string(),
                reason: "source and destination are the same",
            });
        }
        if target.starts_with(source) {
            return Err(VfsError::InvalidPath {
                input: target.to_string(),
                reason: "cannot copy a directory into itself",
            });
        }
        pairs.push((source.clone(), target));
    }
    Ok(pairs)
}

/// Accumulates steps; directories first, then files, then directory
/// attributes (deepest first), so files can be copied in parallel.
struct Builder {
    options: PlanOptions,
    plan: Plan,
    files: Vec<Step>,
    dir_attributes: Vec<Step>,
}

impl Builder {
    fn new(options: PlanOptions) -> Self {
        Builder { options, plan: Plan::default(), files: Vec::new(), dir_attributes: Vec::new() }
    }

    fn attributes(&self, modified: Option<SystemTime>, permissions: Option<u32>) -> SetAttributes {
        SetAttributes {
            modified: modified.filter(|_| self.options.preserve_mtime),
            permissions: permissions.filter(|_| self.options.preserve_permissions),
        }
    }

    fn add_entry(&mut self, from: VfsPath, to: VfsPath, entry: &sc_vfs::Entry) {
        let attributes = self.attributes(entry.modified, entry.permissions);
        match entry.kind {
            EntryKind::Directory => {
                self.plan.steps.push(Step::CreateDir { path: to.clone() });
                if attributes != SetAttributes::default() {
                    self.dir_attributes.push(Step::SetAttributes { path: to, attributes });
                }
            }
            EntryKind::File => {
                self.plan.total_bytes += entry.size;
                self.plan.total_files += 1;
                self.files.push(Step::CopyFile {
                    from,
                    to,
                    size: entry.size,
                    modified: attributes.modified,
                    permissions: attributes.permissions,
                });
            }
            EntryKind::Symlink => {
                self.plan.total_files += 1;
                let target = entry.symlink_target.clone().unwrap_or_default();
                self.files.push(Step::CopySymlink { from, to, target });
            }
            EntryKind::Other => {
                self.plan.warnings.push(format!("{from}: special files are not copied"));
            }
        }
    }

    async fn add_copy(
        &mut self,
        provider: &dyn Provider,
        source: &VfsPath,
        target: &VfsPath,
    ) -> Result<()> {
        let entry = provider.stat(source).await?;
        self.add_entry(source.clone(), target.clone(), &entry);
        if entry.kind != EntryKind::Directory {
            return Ok(());
        }
        let mut items = walk(provider, source, None);
        while let Some(item) = items.next().await {
            match item {
                Ok(item) => {
                    let relative =
                        item.path.relative_to(source).expect("walk stays below its root");
                    self.add_entry(item.path, target.join(&relative), &item.entry);
                }
                Err(e) => self.plan.warnings.push(e.to_string()),
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Plan {
        self.plan.steps.append(&mut self.files);
        self.dir_attributes.reverse();
        self.plan.steps.append(&mut self.dir_attributes);
        self.plan
    }
}

/// Appends removal steps for `path` and its contents, children first.
async fn add_removal(provider: &dyn Provider, path: &VfsPath, plan: &mut Plan) -> Result<()> {
    let entry = provider.stat(path).await?;
    if entry.kind == EntryKind::Directory {
        let mut below = Vec::new();
        let mut items = walk(provider, path, None);
        while let Some(item) = items.next().await {
            match item {
                Ok(item) => below.push(Step::Remove {
                    is_dir: item.entry.kind == EntryKind::Directory,
                    path: item.path,
                }),
                Err(e) => plan.warnings.push(e.to_string()),
            }
        }
        below.reverse();
        plan.steps.extend(below);
    }
    plan.steps
        .push(Step::Remove { path: path.clone(), is_dir: entry.kind == EntryKind::Directory });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_providers::memory::MemoryProvider;
    use std::sync::Arc;

    fn setup() -> (ProviderRegistry, MemoryProvider, MemoryProvider) {
        let registry = ProviderRegistry::new();
        let (a, b) = (MemoryProvider::new(), MemoryProvider::new());
        a.put_file("/src/one.txt", "1");
        a.put_file("/src/sub/two.txt", "22");
        a.put_file("/single.txt", "333");
        b.put_file("/existing/keep", "");
        registry.insert(&p("a", "/"), Arc::new(a.clone()));
        registry.insert(&p("b", "/"), Arc::new(b.clone()));
        (registry, a, b)
    }

    fn p(host: &str, path: &str) -> VfsPath {
        VfsPath::parse(&format!("mem://{host}{path}")).unwrap()
    }

    fn describe(plan: &Plan) -> Vec<String> {
        plan.steps
            .iter()
            .map(|s| match s {
                Step::CreateDir { path } => format!("mkdir {}", path.path()),
                Step::CopyFile { from, to, .. } => format!("copy {} -> {}", from.path(), to.path()),
                Step::CopySymlink { to, .. } => format!("link {}", to.path()),
                Step::SetAttributes { path, .. } => format!("attrs {}", path.path()),
                Step::Move { from, to } => format!("move {} -> {}", from.path(), to.path()),
                Step::Remove { path, .. } => format!("rm {}", path.path()),
            })
            .collect()
    }

    #[tokio::test]
    async fn copy_directory_into_existing_dir() {
        let (registry, _, _) = setup();
        let plan =
            plan_copy(&registry, &[p("a", "/src")], &p("b", "/existing"), PlanOptions::default())
                .await
                .unwrap();
        assert_eq!(
            describe(&plan),
            [
                "mkdir /existing/src",
                "mkdir /existing/src/sub",
                "copy /src/one.txt -> /existing/src/one.txt",
                "copy /src/sub/two.txt -> /existing/src/sub/two.txt",
                "attrs /existing/src/sub",
                "attrs /existing/src",
            ]
        );
        assert_eq!(plan.total_bytes, 3);
        assert_eq!(plan.total_files, 2);
    }

    #[tokio::test]
    async fn single_file_to_new_name() {
        let (registry, _, _) = setup();
        let plan = plan_copy(
            &registry,
            &[p("a", "/single.txt")],
            &p("b", "/renamed.txt"),
            PlanOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(describe(&plan), ["copy /single.txt -> /renamed.txt"]);
    }

    #[tokio::test]
    async fn many_sources_to_new_dir_create_it() {
        let (registry, _, _) = setup();
        let sources = [p("a", "/single.txt"), p("a", "/src/one.txt")];
        let plan =
            plan_copy(&registry, &sources, &p("b", "/new"), PlanOptions::default()).await.unwrap();
        assert_eq!(
            describe(&plan),
            [
                "mkdir /new",
                "copy /single.txt -> /new/single.txt",
                "copy /src/one.txt -> /new/one.txt"
            ]
        );
    }

    #[tokio::test]
    async fn rejects_copy_into_itself() {
        let (registry, _, _) = setup();
        let err =
            plan_copy(&registry, &[p("a", "/src")], &p("a", "/src/sub"), PlanOptions::default())
                .await
                .unwrap_err();
        assert!(matches!(
            err,
            VfsError::InvalidPath { reason: "cannot copy a directory into itself", .. }
        ));
        let err = plan_copy(&registry, &[p("a", "/src")], &p("a", "/src"), PlanOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(err, VfsError::InvalidPath { .. }));
    }

    #[tokio::test]
    async fn attributes_can_be_dropped() {
        let (registry, _, _) = setup();
        let options = PlanOptions { preserve_mtime: false, preserve_permissions: false };
        let plan = plan_copy(&registry, &[p("a", "/src")], &p("b", "/x"), options).await.unwrap();
        assert!(!plan.steps.iter().any(|s| matches!(s, Step::SetAttributes { .. })));
        assert!(plan.steps.iter().all(|s| !matches!(s, Step::CopyFile { modified: Some(_), .. })));
    }

    #[tokio::test]
    async fn move_same_provider_is_a_rename_and_cross_provider_is_copy_plus_remove() {
        let (registry, _, _) = setup();
        let same = plan_move(&registry, &[p("a", "/src")], &p("a", "/dst"), PlanOptions::default())
            .await
            .unwrap();
        assert_eq!(describe(&same), ["move /src -> /dst"]);

        let cross =
            plan_move(&registry, &[p("a", "/src")], &p("b", "/dst"), PlanOptions::default())
                .await
                .unwrap();
        let steps = describe(&cross);
        assert_eq!(steps.first().unwrap(), "mkdir /dst");
        assert_eq!(
            &steps[steps.len() - 4..],
            ["rm /src/sub/two.txt", "rm /src/sub", "rm /src/one.txt", "rm /src"]
        );
    }

    #[tokio::test]
    async fn delete_removes_children_first() {
        let (registry, _, _) = setup();
        let plan = plan_delete(&registry, &[p("a", "/src"), p("a", "/single.txt")]).await.unwrap();
        assert_eq!(
            describe(&plan),
            ["rm /src/sub/two.txt", "rm /src/sub", "rm /src/one.txt", "rm /src", "rm /single.txt"]
        );
    }

    #[tokio::test]
    async fn plans_serialize() {
        // Plans are journaled by sc-transfer, so they must round-trip.
        let (registry, _, _) = setup();
        let plan = plan_copy(&registry, &[p("a", "/src")], &p("b", "/x"), PlanOptions::default())
            .await
            .unwrap();
        fn assert_serde<T: Serialize + for<'de> Deserialize<'de>>(_: &T) {}
        assert_serde(&plan);
    }
}
