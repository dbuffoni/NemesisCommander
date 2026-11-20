//! Advanced pack (format, level, password) and the password of encrypted
//! archives.

use nmc_archive::{ArchiveFormat, Compression, PackOptions};
use nmc_vfs::VfsPath;

use super::{App, display_location, done, resolve_location};
use crate::dialogs::{ChooseDialog, ChoosePurpose, InputDialog, InputPurpose, Overlay};

/// The compression levels offered, with their names.
const LEVELS: [(&str, u32); 4] =
    [("Fastest", 1), ("Fast", 3), ("Normal (the default)", 6), ("Smallest", 9)];

/// What a format row says about itself.
fn describe(format: ArchiveFormat) -> String {
    let (name, note) = match format {
        ArchiveFormat::Zip => ("zip", "opens everywhere · password: AES-256"),
        ArchiveFormat::SevenZ => ("7z", "smallest · password: AES-256, names hidden too"),
        ArchiveFormat::Tar(Compression::None) => ("tar", "no compression"),
        ArchiveFormat::Tar(Compression::Gzip) => ("tar.gz", "fast, common on Unix"),
        ArchiveFormat::Tar(Compression::Xz) => ("tar.xz", "small, slow to pack"),
        ArchiveFormat::Tar(Compression::Bzip2) => ("tar.bz2", "older, small"),
        ArchiveFormat::Tar(Compression::Zstd) => ("tar.zst", "fast and small"),
        ArchiveFormat::Compressed(c) => {
            return format!("{:<8} one file only, no folder", c.extension());
        }
        ArchiveFormat::Rar => ("rar", "read only"),
    };
    format!("{name:<8} {note}")
}

/// Whether `format` can be protected by a password.
fn can_lock(format: ArchiveFormat) -> bool {
    matches!(format, ArchiveFormat::Zip | ArchiveFormat::SevenZ)
}

/// Whether `format` has a compression level to choose.
fn has_level(format: ArchiveFormat) -> bool {
    !matches!(format, ArchiveFormat::SevenZ | ArchiveFormat::Tar(Compression::None))
}

impl App {
    /// The advanced options of Pack (Ctrl+O in its question), first the
    /// format; the cursor starts on the one the typed name asks for.
    pub(super) fn ask_pack_format(&mut self, paths: Vec<VfsPath>, typed: &str) {
        let one = paths.len() == 1;
        let formats: Vec<ArchiveFormat> = ArchiveFormat::writable()
            .into_iter()
            // A single compressed file holds one file only.
            .filter(|f| one || !matches!(f, ArchiveFormat::Compressed(_)))
            .collect();
        let title = format!("Advanced pack: {}", Self::count_text(&paths));
        let rows = formats.iter().map(|&f| describe(f)).collect();
        let name = typed.trim().rsplit(['/', '\\']).next().unwrap_or_default();
        let start = ArchiveFormat::from_file_name(name)
            .and_then(|wanted| formats.iter().position(|&f| f == wanted));
        let mut dialog =
            ChooseDialog::new(&title, rows, ChoosePurpose::PackFormat { paths, formats });
        dialog.cursor = start.unwrap_or(0);
        dialog.footer = "Enter: choose the format   Esc: cancel".into();
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// An answer of the advanced pack's list questions.
    pub(super) fn chosen_pack(&mut self, purpose: ChoosePurpose, cursor: usize) {
        match purpose {
            ChoosePurpose::PackFormat { paths, formats } => {
                let Some(&format) = formats.get(cursor) else { return };
                if has_level(format) {
                    let rows = LEVELS.iter().map(|(name, n)| format!("{name} ({n})")).collect();
                    let mut dialog = ChooseDialog::new(
                        "Compression level",
                        rows,
                        ChoosePurpose::PackLevel { paths, format },
                    );
                    dialog.cursor = 2;
                    dialog.footer = "Enter: choose   Esc: cancel".into();
                    self.overlay = Some(Overlay::Choose(dialog));
                } else {
                    self.ask_pack_password(paths, format, 6);
                }
            }
            ChoosePurpose::PackLevel { paths, format } => {
                let level = LEVELS.get(cursor).map_or(6, |&(_, n)| n);
                self.ask_pack_password(paths, format, level);
            }
            _ => {}
        }
    }

    fn ask_pack_password(&mut self, paths: Vec<VfsPath>, format: ArchiveFormat, level: u32) {
        if !can_lock(format) {
            return self.ask_pack_destination(paths, format, level, None);
        }
        let mut dialog = InputDialog::new(
            "Advanced pack",
            "Password to protect the archive with (empty: none; it is not stored anywhere):",
            "",
            InputPurpose::PackPassword { paths, format, level },
        );
        dialog.secret = true;
        self.overlay = Some(Overlay::Input(dialog));
    }

    fn ask_pack_destination(
        &mut self,
        paths: Vec<VfsPath>,
        format: ArchiveFormat,
        level: u32,
        password: Option<String>,
    ) {
        let name = match paths.as_slice() {
            [one] => one.file_name().unwrap_or("archive").to_owned(),
            _ if self.target_override.is_some() => "basket".to_owned(),
            _ => self.workspace.active().location().file_name().unwrap_or("archive").into(),
        };
        let output = resolve_location(
            &format!("{name}.{}", format.extension()),
            self.workspace.inactive().location(),
        )
        .map(|p| display_location(&p))
        .unwrap_or_default();
        let lock = if password.is_some() { ", protected by the password" } else { "" };
        let prompt = format!("Pack {} into{lock}:", Self::count_text(&paths));
        self.ask(
            "Advanced pack",
            &prompt,
            &output,
            InputPurpose::PackAdvanced { paths, format, level, password },
        );
    }

    /// An answer of the advanced pack's typed questions, or the password of
    /// an encrypted archive.
    pub(super) fn submit_archive_input(&mut self, purpose: InputPurpose, text: &str) {
        match purpose {
            InputPurpose::PackPassword { paths, format, level } => {
                if text.is_empty() {
                    return self.ask_pack_destination(paths, format, level, None);
                }
                let mut dialog = InputDialog::new(
                    "Advanced pack",
                    "Type the password again:",
                    "",
                    InputPurpose::PackPasswordAgain {
                        paths,
                        format,
                        level,
                        first: text.to_owned(),
                    },
                );
                dialog.secret = true;
                self.overlay = Some(Overlay::Input(dialog));
            }
            InputPurpose::PackPasswordAgain { paths, format, level, first } => {
                if first == text {
                    self.ask_pack_destination(paths, format, level, Some(first));
                } else {
                    self.set_error("The two passwords differ".to_owned());
                    self.ask_pack_password(paths, format, level);
                }
            }
            InputPurpose::PackAdvanced { paths, format, level, password } => {
                let mut output =
                    match resolve_location(text.trim(), self.workspace.active().location()) {
                        Ok(output) => output,
                        Err(e) => return self.set_error(e),
                    };
                // The type is the one chosen: add its extension if it is missing.
                let wanted = format!(".{}", format.extension());
                let mut name = output.file_name().unwrap_or_default().to_owned();
                if !name.to_lowercase().ends_with(&wanted) {
                    name.push_str(&wanted);
                    output = match resolve_location(
                        &format!("{}{wanted}", display_location(&output)),
                        self.workspace.active().location(),
                    ) {
                        Ok(output) => output,
                        Err(e) => return self.set_error(e),
                    };
                }
                let registry = self.context.registry.clone();
                let mut options = PackOptions::new(format);
                options.level = level;
                options.password = password;
                self.set_status(format!("Packing {name}…"));
                self.spawn_task(async move {
                    let result = nmc_archive::pack(&registry, &paths, &output, options).await;
                    done(result.map(|count| format!("Packed {count} entries into {name}")), None)
                });
            }
            InputPurpose::ArchivePassword(container) => {
                if text.is_empty() {
                    self.password_declined.insert(container.to_string());
                    return;
                }
                nmc_archive::set_password(&container, text);
                self.context.registry.forget(&container);
                self.reload_panels(None);
            }
            _ => {}
        }
    }

    /// Asks for the password of an encrypted archive one of the panels is
    /// in, once it is known that one is needed (opening it, or reading
    /// from it, failed for that reason).
    pub(super) fn ask_archive_password(&mut self) {
        let mut shown: Vec<VfsPath> = Vec::new();
        for side in [nmc_panel::SideId::Left, nmc_panel::SideId::Right] {
            let mut next = self.workspace.side(side).panel().location().container();
            while let Some(container) = next {
                shown.push(container.clone());
                next = container.container();
            }
        }
        let keys: Vec<String> = shown.iter().map(ToString::to_string).collect();
        // Archives that were left may ask again next time.
        self.password_declined.retain(|k| keys.contains(k));
        // A viewer that could not read the file gives way to the question.
        if self.overlay.as_ref().is_some_and(|o| !matches!(o, Overlay::Viewer(_))) {
            return;
        }
        for container in shown {
            if nmc_archive::needs_password(&container)
                && !self.password_declined.contains(&container.to_string())
            {
                let prompt = if nmc_archive::was_refused(&container) {
                    format!(
                        "Wrong password. Try again for {}:",
                        container.file_name().unwrap_or("the archive")
                    )
                } else {
                    format!(
                        "{} is encrypted. Password (not stored anywhere):",
                        container.file_name().unwrap_or("The archive")
                    )
                };
                let mut dialog = InputDialog::new(
                    "Password",
                    &prompt,
                    "",
                    InputPurpose::ArchivePassword(container),
                );
                dialog.secret = true;
                self.overlay = Some(Overlay::Input(dialog));
                return;
            }
        }
    }
}
