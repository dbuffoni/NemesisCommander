//! The question behind Select, Unselect and Keep only (`+`, `-`): the
//! patterns it understands, ready-made masks, the live count of what matches
//! and the help text.

use pnxr_index::Mask;
use pnxr_vfs::Entry;

/// The ready-made masks: name, mask.
pub const PRESETS: [(&str, &str); 9] = [
    ("All files", "*"),
    ("Images", "*.jpg;*.jpeg;*.png;*.gif;*.webp;*.bmp;*.svg;*.tif;*.tiff;*.heic"),
    (
        "Documents",
        "*.pdf;*.doc;*.docx;*.odt;*.rtf;*.txt;*.md;*.xls;*.xlsx;*.ods;*.ppt;*.pptx;*.odp",
    ),
    ("Source code", "*.rs;*.c;*.h;*.cpp;*.hpp;*.py;*.js;*.ts;*.go;*.java;*.cs;*.rb;*.sh"),
    ("Archives", "*.zip;*.7z;*.tar;*.gz;*.tgz;*.xz;*.bz2;*.zst;*.rar"),
    ("Music and video", "*.mp3;*.flac;*.ogg;*.wav;*.m4a;*.mp4;*.mkv;*.avi;*.mov;*.webm"),
    ("Hidden files", ".*"),
    ("Backups and temporary files", "*~;*.bak;*.tmp;*.swp"),
    ("Everything but hidden files", "!.*"),
];

/// The lines shown under the field: how many files match, and the first few.
/// Folders are not selected by a mask, so they are not counted.
pub fn preview(entries: &[Entry], text: &str) -> Vec<String> {
    let mask = match Mask::parse(text) {
        Ok(mask) => mask,
        Err(e) => return vec![format!("Not a valid mask: {}", e.reason)],
    };
    let files: Vec<&Entry> = entries.iter().filter(|e| e.name != ".." && !e.is_dir()).collect();
    let matching: Vec<&&Entry> = files.iter().filter(|e| mask.matches(&e.name)).collect();
    let mut lines = vec![format!("{} of {} files match", matching.len(), files.len())];
    lines.extend(matching.iter().take(4).map(|e| format!("  {}", e.name)));
    if matching.len() > 4 {
        lines.push(format!("  … and {} more", matching.len() - 4));
    }
    lines
}

/// The text of F1 in the question.
pub fn help_lines() -> Vec<String> {
    [
        "Select files by a mask. The files that match are shown while you type.",
        "",
        "  *.jpg              any name ending in .jpg   (case does not matter)",
        "  *.jpg;*.png        several masks, separated by ; or ,",
        "  report?.txt        ? is one character, [ab] one of the characters",
        "  *.log;!old*        ! leaves out what matches: all .log files except old*",
        "  has:report         names that contain the text",
        "  re:^img_\\d+       a regular expression, found anywhere in the name",
        "",
        "Keys",
        "  F2                 changes what is done: select, unselect, keep only the matches",
        "  Ctrl+O             ready-made masks (images, documents, archives...)",
        "  Enter              does it;  Esc closes",
        "",
        "Folders are not selected by a mask. The last mask is offered again.",
    ]
    .iter()
    .map(|l| (*l).to_owned())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnxr_vfs::EntryKind;

    fn file(name: &str) -> Entry {
        Entry {
            name: name.into(),
            kind: EntryKind::File,
            size: 1,
            modified: None,
            permissions: None,
            uid: None,
            gid: None,
            symlink_target: None,
            link_is_dir: false,
            os_flags: None,
        }
    }

    #[test]
    fn the_preview_counts_and_reports_errors() {
        let entries = vec![file("a.txt"), file("b.txt"), file("c.rs")];
        assert_eq!(preview(&entries, "*.txt")[0], "2 of 3 files match");
        assert_eq!(preview(&entries, "has:C")[1], "  c.rs");
        assert!(preview(&entries, "re:(")[0].starts_with("Not a valid mask"));
        for (_, mask) in PRESETS {
            assert!(Mask::parse(mask).is_ok(), "{mask}");
        }
    }
}
