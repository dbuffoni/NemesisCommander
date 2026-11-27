//! F1 in the dialogs: the help of the selected row, or of the question,
//! one item at a time. The texts live here, next to each other, so that a
//! test can check that every kind of dialog has one.

use crate::dialogs::{ChoosePurpose, InputPurpose};
use crate::properties::PropRow;

/// A help window: title and lines.
pub type Help = (String, Vec<String>);

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|l| (*l).to_owned()).collect()
}

fn one(title: &str, text: &[&str]) -> Option<Help> {
    Some((title.to_owned(), lines(text)))
}

/// The help for the row `cursor` of a list, or of the list when it has no
/// help for rows. `None`: this list has no help.
pub fn choose_help(purpose: &ChoosePurpose, cursor: usize) -> Option<Help> {
    match purpose {
        ChoosePurpose::LinkKind(_) => match cursor {
            0 => one(
                "Relative symbolic link",
                &[
                    "A shortcut that stores the path from the link to the target.",
                    "It keeps working when both folders are moved together.",
                ],
            ),
            1 => one(
                "Absolute symbolic link",
                &[
                    "A shortcut that stores the whole path of the target.",
                    "It keeps working when the link is moved, but not when the target is.",
                ],
            ),
            _ => one(
                "Hard link",
                &[
                    "A second name for the same file data, not a shortcut.",
                    "Only for files, and only on the same drive. Deleting one name keeps the other.",
                ],
            ),
        },
        ChoosePurpose::SyncMode => match cursor {
            0 => one(
                "Mirror",
                &[
                    "The second folder becomes an exact copy of the first.",
                    "Files that exist only in the second folder are DELETED. A list of the changes",
                    "is shown before anything is done.",
                ],
            ),
            1 => one(
                "Update",
                &[
                    "Copies new and newer files of the first folder into the second.",
                    "Nothing is deleted and nothing newer is overwritten.",
                ],
            ),
            _ => one(
                "Two-way",
                &[
                    "Both folders end up with the newest version of every file.",
                    "Each one gets what only the other had. Nothing is deleted.",
                ],
            ),
        },
        ChoosePurpose::ImportMode(_) => match cursor {
            0 => one(
                "Replace all settings",
                &[
                    "Your settings, bookmarks, connections and key overrides become exactly",
                    "what the file holds. What you had is lost (export first to keep it).",
                ],
            ),
            _ => one(
                "Merge",
                &[
                    "Adds the bookmarks, connections and key overrides of the file to yours.",
                    "Settings you already have are kept.",
                ],
            ),
        },
        ChoosePurpose::MaskPresets(_) => one(
            "Ready-made masks",
            &[
                "Each row is a mask you can use as it is or change.",
                "Enter puts it in the question, where you can edit it before selecting.",
            ],
        ),
        ChoosePurpose::RenamePresets(_) => one(
            "Ready-made rename rules",
            &[
                "Each row is a rule for the multi-rename question. Enter puts it there,",
                "with the preview, so you can edit it before renaming.",
            ],
        ),
        ChoosePurpose::Checksums { verify, .. } => match (verify, cursor) {
            (Some(_), 0) => one(
                "Check the files",
                &[
                    "Reads the checksum file under the cursor and compares every file it lists",
                    "with the file on disk. Nothing is changed.",
                ],
            ),
            _ => one(
                "Write a checksum file",
                &[
                    "SHA-256 files are accepted by `sha256sum -c`, BLAKE3 ones by `b3sum --check`.",
                    "Combined: one file for everything. Per file: name.sha256 next to each file.",
                ],
            ),
        },
        ChoosePurpose::Properties(state) => {
            let row = state.rows.get(cursor).copied().unwrap_or(PropRow::Info);
            match row {
                PropRow::Info => one(
                    "Properties",
                    &[
                        "What is known about the item. Press e or F2 to change the date or",
                        "the permissions.",
                    ],
                ),
                PropRow::Modified => one(
                    "Date of the last change",
                    &["Enter to type a new date: now, 2026-11-17, 10:30 or 2026-11-17 10:30."],
                ),
                PropRow::Bit(_) => one(
                    "Permission",
                    &[
                        "Space or Enter switches this permission. The octal number follows.",
                        "Special bits: setuid and setgid run a program as its owner or group;",
                        "sticky on a folder lets only an owner delete what is inside.",
                    ],
                ),
                PropRow::Octal => one(
                    "Permissions as a number",
                    &["Enter to type them: 644 (rw-r--r--), 755 (rwxr-xr-x), 4755 (setuid)..."],
                ),
                PropRow::Recursive => one(
                    "Inside the folders",
                    &["Also changes everything inside the selected folders."],
                ),
                PropRow::Apply => one(
                    "Apply",
                    &["Makes the change (Ctrl+S does the same). Nothing is changed before."],
                ),
            }
        }
        ChoosePurpose::Bookmarks(_) => one(
            "Bookmarks and recent folders",
            &[
                "Type to filter. Enter opens the folder, Delete removes a bookmark.",
                "Bookmarks are added with the Bookmark command; recent folders are kept alone.",
            ],
        ),
        ChoosePurpose::Basket(_) => one(
            "Basket",
            &[
                "Files collected from several folders. Copy, move, delete, pack or rename",
                "work on all of them. Enter shows an item in the panel.",
            ],
        ),
        ChoosePurpose::Duplicates(_) | ChoosePurpose::Similar(_) => one(
            "Duplicates",
            &[
                "Groups of files with the same content (or similar pictures).",
                "The first of each group is kept; the first row puts the extra copies",
                "in the basket, so you decide what to do with them.",
            ],
        ),
        ChoosePurpose::Drives(_) => one(
            "Drives",
            &[
                "Disks, saved connections and places. Enter opens the place; Delete forgets",
                "a saved connection.",
            ],
        ),
        ChoosePurpose::PackFormat { .. } | ChoosePurpose::PackLevel { .. } => {
            one("Pack", &["Choose the archive format, then how much to compress."])
        }
        ChoosePurpose::ExportFormat { .. } => one(
            "Export the listing",
            &["Writes the names, sizes and dates in the format of the row."],
        ),
        ChoosePurpose::SavedSearches => {
            one("Saved searches", &["Enter runs the search again; Delete forgets it."])
        }
        ChoosePurpose::DiskUsage { .. } => {
            one("Disk usage", &["The biggest items first. Enter looks into a folder."])
        }
        _ => None,
    }
}

/// The help for a question, by what it asks.
pub fn input_help(purpose: &InputPurpose) -> Option<Help> {
    match purpose {
        InputPurpose::Rename(_) => one(
            "Rename",
            &[
                "Type the new name. It never replaces another file. Changing only the",
                "letter case works. Shift+F2 renames many files with a template.",
            ],
        ),
        InputPurpose::CopyTo(_) | InputPurpose::MoveTo(_) => one(
            "Copy and move",
            &[
                "The destination is a folder, a path, or an address such as sftp://host/dir.",
                "Tab completes the path. A name that does not exist is created as a folder.",
            ],
        ),
        InputPurpose::Attributes(_) => one(
            "Dates and permissions",
            &["Words, in any order: now, 2026-11-17, 10:30, mode=755, -r (inside folders)."],
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_have_their_own_help() {
        let a = choose_help(&ChoosePurpose::SyncMode, 0).unwrap();
        let b = choose_help(&ChoosePurpose::SyncMode, 1).unwrap();
        assert_ne!(a.0, b.0);
        assert!(a.1.iter().any(|l| l.contains("DELETED")));
        assert!(choose_help(&ChoosePurpose::LinkKind(vec![]), 2).unwrap().0.contains("Hard"));
    }
}
