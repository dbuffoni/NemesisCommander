//! The About window: logo, version, licence and where to find the source.
//!
//! The logo is part of the program (`assets/branding`): a terminal draws
//! it with half-block characters like any picture, a window shows the
//! same PNG from a file in the temporary folder.

use std::sync::OnceLock;

use image::RgbaImage;

use crate::dialogs::MessageDialog;

/// The logo without a background, for a dark theme (white arch), 256 × 256.
pub const LOGO_PNG: &[u8] =
    include_bytes!("../../../assets/branding/nemesiscommander-mark-dark.png");

/// The same for a light theme (dark arch).
pub const LOGO_LIGHT_PNG: &[u8] =
    include_bytes!("../../../assets/branding/nemesiscommander-mark-light.png");

/// The application icon (the logo on a dark tile), 256 × 256: the window's.
pub const ICON_PNG: &[u8] = include_bytes!("../../../assets/branding/nemesiscommander-256.png");

/// The version of the program.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The git commit it was built from (with `-dirty` after local changes).
pub const COMMIT: &str = env!("NMC_BUILD_COMMIT");

/// Where the source is.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// Rows of text the logo takes in a terminal.
pub const LOGO_ROWS: u16 = 10;

/// The decoded logo.
pub fn logo() -> &'static RgbaImage {
    static LOGO: OnceLock<RgbaImage> = OnceLock::new();
    LOGO.get_or_init(|| {
        image::load_from_memory(LOGO_PNG).map_or_else(|_| RgbaImage::new(1, 1), |i| i.to_rgba8())
    })
}

/// `file://` URLs of the logo for a window, written once: for a dark
/// theme and for a light one.
pub fn logo_urls() -> Option<(String, String)> {
    static URLS: OnceLock<Option<(String, String)>> = OnceLock::new();
    URLS.get_or_init(|| {
        let folder = std::env::temp_dir().join(format!("nemesiscommander-{}", std::process::id()));
        std::fs::create_dir_all(&folder).ok()?;
        let write = |name: &str, bytes: &[u8]| -> Option<String> {
            let path = folder.join(name);
            std::fs::write(&path, bytes).ok()?;
            let text = path.display().to_string().replace('\\', "/");
            Some(if text.starts_with('/') {
                format!("file://{text}")
            } else {
                format!("file:///{text}")
            })
        };
        Some((write("logo-dark.png", LOGO_PNG)?, write("logo-light.png", LOGO_LIGHT_PNG)?))
    })
    .clone()
}

/// What the About window says.
pub fn lines() -> Vec<String> {
    vec![
        format!("NemesisCommander {VERSION}  (commit {COMMIT})"),
        "A fast dual-pane file manager for local and remote files.".into(),
        String::new(),
        "License: MIT, \u{a9} 2026 escursionista2k".into(),
        format!("Source:  {REPOSITORY}"),
        String::new(),
        "Built with Rust and Qt 6 (LGPL v3, linked dynamically).".into(),
        "Release builds read RAR archives with RARLAB's unRAR library, which".into(),
        "has its own license. The third-party notices of a release list them all.".into(),
    ]
}

/// The About dialog: its text starts below the logo in a terminal.
pub fn dialog() -> MessageDialog {
    MessageDialog { title: "About".into(), lines: lines(), then: None, logo: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_is_a_square_picture() {
        let logo = logo();
        assert_eq!(logo.width(), logo.height());
        assert!(logo.width() >= 128);
    }

    #[test]
    fn the_text_names_version_licence_and_source() {
        let text = lines().join("\n");
        assert!(text.contains(VERSION));
        assert!(text.contains("MIT"));
        assert!(text.contains("github.com"));
        assert!(!COMMIT.is_empty());
    }

    #[test]
    fn the_icon_files_are_small() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/branding");
        let mut total = 0;
        for entry in std::fs::read_dir(folder).unwrap() {
            total += entry.unwrap().metadata().unwrap().len();
        }
        assert!(total < 300 * 1024, "branding assets take {total} bytes");
    }
}
