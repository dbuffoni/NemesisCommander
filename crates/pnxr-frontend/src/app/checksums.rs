//! Checksums: the digests of the file under the cursor, and writing and
//! checking checksum files (`sha256sum` / `b3sum` format) for a selection.

use pnxr_ops::checksums::{self, Layout, Verdict};
use pnxr_vfs::{HashAlgorithm, VfsPath};

use super::{App, AppEvent, done};
use crate::dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, MessageDialog, Overlay,
};

/// What to write, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumJob {
    /// The folder the names are relative to.
    pub base: VfsPath,
    /// The files and folders to hash.
    pub sources: Vec<VfsPath>,
    /// The algorithm.
    pub algorithm: HashAlgorithm,
    /// One file or one per file.
    pub layout: Layout,
    /// Where the checksum files go.
    pub folder: VfsPath,
}

/// The rows of the list, in order.
const WRITE_ROWS: [(&str, HashAlgorithm, Layout); 4] = [
    ("SHA-256, one file for all (SHA256SUMS)", HashAlgorithm::Sha256, Layout::Combined),
    ("SHA-256, one file for each file (name.sha256)", HashAlgorithm::Sha256, Layout::PerFile),
    ("BLAKE3, one file for all (B3SUMS)", HashAlgorithm::Blake3, Layout::Combined),
    ("BLAKE3, one file for each file (name.b3)", HashAlgorithm::Blake3, Layout::PerFile),
];

/// Whether a file name looks like a checksum file.
fn is_checksum_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".sha256")
        || lower.ends_with(".b3")
        || lower.ends_with(".sha256sum")
        || lower.ends_with("sums")
        || lower.ends_with("sums.txt")
}

impl App {
    /// The list "Write checksum files" for `sources` (and "Check" for a
    /// checksum file under the cursor).
    fn checksum_list(&self, sources: Vec<VfsPath>, verify: Option<VfsPath>) -> ChooseDialog {
        let base = self.workspace.active().location().clone();
        let mut items: Vec<String> = Vec::new();
        if let Some(file) = &verify {
            items.push(format!("Check the files listed in {}", file.file_name().unwrap_or("")));
        }
        items.extend(WRITE_ROWS.iter().map(|(label, ..)| (*label).to_owned()));
        let title = if verify.is_some() {
            "Check or write checksums".to_owned()
        } else if sources.len() == 1 {
            "Write a checksum file".to_owned()
        } else {
            format!("Write checksum files for {}", Self::count_text(&sources))
        };
        let mut dialog =
            ChooseDialog::new(&title, items, ChoosePurpose::Checksums { base, sources, verify });
        dialog.footer = crate::hints::list("choose", &[], "close");
        dialog
    }

    /// Ctrl+K: for one file its digests (Enter then offers to write a
    /// checksum file); for a selection or a folder the list of checksum
    /// files to write; on a checksum file, also to check it.
    pub(super) fn checksum_current(&mut self) {
        let panel = self.workspace.active();
        let targets = panel.targets();
        let one_file = targets.len() == 1
            && panel.selection_summary().dirs == 0
            && panel.current_entry().is_some_and(|e| !e.is_dir());
        let Some(first) = targets.first().cloned() else { return };
        if !one_file {
            let dialog = self.checksum_list(targets, None);
            self.overlay = Some(Overlay::Choose(dialog));
            return;
        }
        let path = first;
        let verify = path.file_name().filter(|n| is_checksum_file(n)).map(|_| path.clone());
        let list = self.checksum_list(vec![path.clone()], verify.clone());
        if verify.is_some() {
            self.overlay = Some(Overlay::Choose(list));
            return;
        }
        let registry = self.context.registry.clone();
        self.set_status("Computing checksums…");
        self.spawn_task(async move {
            let result = async {
                let provider = registry.provider_for(&path).await?;
                let mut lines = vec![format!("{}", path.file_name().unwrap_or(""))];
                for (name, algorithm) in
                    [("BLAKE3", HashAlgorithm::Blake3), ("SHA-256", HashAlgorithm::Sha256)]
                {
                    let digest = pnxr_ops::checksum(provider.as_ref(), &path, algorithm).await?;
                    lines.push(format!("{name:<8} {digest}"));
                }
                Ok::<_, pnxr_vfs::VfsError>(lines)
            }
            .await;
            match result {
                Ok(lines) => AppEvent::Show(Box::new(Overlay::Message(MessageDialog {
                    title: "Checksums".into(),
                    lines,
                    then: None,
                    then_choose: Some(Box::new(list)),
                    back_to_settings: None,
                    back: false,
                    logo: false,
                    busy: false,
                    fixed: true,
                }))),
                Err(e) => done(Err(e), None),
            }
        });
    }

    /// A row of the checksum list was chosen.
    pub(super) fn checksum_chosen(
        &mut self,
        base: VfsPath,
        sources: Vec<VfsPath>,
        verify: Option<VfsPath>,
        cursor: usize,
    ) {
        let (cursor, checking) = match (&verify, cursor) {
            (Some(_), 0) => (0, true),
            (Some(_), n) => (n - 1, false),
            (None, n) => (n, false),
        };
        if checking {
            if let Some(file) = verify {
                self.check_checksum_file(file);
            }
            return;
        }
        let Some((_, algorithm, layout)) = WRITE_ROWS.get(cursor).copied() else { return };
        let folder = base.clone();
        self.write_checksum_files(ChecksumJob { base, sources, algorithm, layout, folder }, false);
    }

    /// Writes the checksum files; asks before replacing one that exists.
    pub(super) fn write_checksum_files(&mut self, job: ChecksumJob, overwrite: bool) {
        let registry = self.context.registry.clone();
        self.set_status("Writing checksum files…");
        self.spawn_task(async move {
            let result = checksums::write_checksums(
                &registry,
                &job.base,
                &job.sources,
                job.algorithm,
                job.layout,
                &job.folder,
                overwrite,
            )
            .await;
            match result {
                Ok(written) => {
                    let names: Vec<String> = written
                        .files
                        .iter()
                        .filter_map(|f| f.file_name().map(str::to_owned))
                        .collect();
                    let shown = if names.len() <= 3 {
                        names.join(", ")
                    } else {
                        format!("{} and {} more", names[..2].join(", "), names.len() - 2)
                    };
                    done(Ok(format!("Wrote {shown} ({} files hashed)", written.hashed)), None)
                }
                Err(pnxr_vfs::VfsError::AlreadyExists(path)) => {
                    AppEvent::Show(Box::new(Overlay::Confirm(ConfirmDialog {
                        title: "Checksum file exists".into(),
                        lines: vec![format!(
                            "{} already exists. Replace it (and the other checksum files)?",
                            path.file_name().unwrap_or("")
                        )],
                        purpose: ConfirmPurpose::ChecksumOverwrite(Box::new(job)),
                    })))
                }
                Err(e) => done(Err(e), None),
            }
        });
    }

    /// Checks the files a checksum file lists and shows the result.
    fn check_checksum_file(&mut self, file: VfsPath) {
        let registry = self.context.registry.clone();
        self.set_status("Checking files…");
        self.spawn_task(async move {
            match checksums::check_checksums(&registry, &file).await {
                Ok(results) => {
                    let failed = results.iter().filter(|(_, v)| *v == Verdict::Failed).count();
                    let missing =
                        results.iter().filter(|(_, v)| matches!(v, Verdict::Missing(_))).count();
                    let mut lines = vec![format!(
                        "{}: {} OK, {failed} FAILED, {missing} missing",
                        file.file_name().unwrap_or(""),
                        results.len() - failed - missing
                    )];
                    for (name, verdict) in &results {
                        lines.push(match verdict {
                            Verdict::Ok => format!("OK       {name}"),
                            Verdict::Failed => format!("FAILED   {name}"),
                            Verdict::Missing(why) => format!("MISSING  {name} ({why})"),
                        });
                    }
                    AppEvent::Show(Box::new(Overlay::Message(
                        MessageDialog::new("Check checksums", lines).with_fixed_font(),
                    )))
                }
                Err(e) => done(Err(e), None),
            }
        });
    }
}
