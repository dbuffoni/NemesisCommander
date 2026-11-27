//! The About window: logo, version, licence and where to find the source.
//!
//! The logo is part of the program (`assets/branding`): a terminal draws
//! it with half-block characters like any picture, a window shows the
//! same PNG from a file in the temporary folder.

use std::sync::OnceLock;

use image::RgbaImage;

use crate::dialogs::MessageDialog;

/// The round logo with a transparent background, 256 × 256. It reads on
/// light and dark themes alike (blue ring, white and black halves).
pub const LOGO_PNG: &[u8] = include_bytes!("../../../assets/branding/pairnox-256.png");

/// The same for a light theme: the one picture serves both.
pub const LOGO_LIGHT_PNG: &[u8] = LOGO_PNG;

/// The application icon in the sizes a window picks from: 16, 32, 48, 64,
/// 128 and 256 pixels (16 and 32 are the thick small variant).
pub const ICON_PNGS: [&[u8]; 6] = [
    include_bytes!("../../../assets/branding/pairnox-16.png"),
    include_bytes!("../../../assets/branding/pairnox-32.png"),
    include_bytes!("../../../assets/branding/pairnox-48.png"),
    include_bytes!("../../../assets/branding/pairnox-64.png"),
    include_bytes!("../../../assets/branding/pairnox-128.png"),
    include_bytes!("../../../assets/branding/pairnox-256.png"),
];

/// The version of the program.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The git commit it was built from (with `-dirty` after local changes).
pub const COMMIT: &str = env!("PNXR_BUILD_COMMIT");

/// Where the source is.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// Rows of text the logo takes in a terminal.
pub const LOGO_ROWS: u16 = 10;

/// The decoded logo.
pub fn logo() -> &'static RgbaImage {
    static LOGO: OnceLock<RgbaImage> = OnceLock::new();
    LOGO.get_or_init(|| {
        // The 32 pixel icon has the thick ring, which survives being drawn
        // in 20 half-block cells.
        image::load_from_memory(ICON_PNGS[1]).map_or_else(|_| RgbaImage::new(1, 1), |i| i.to_rgba8())
    })
}

/// `file://` URLs of the logo for a window, written once: for a dark
/// theme and for a light one.
pub fn logo_urls() -> Option<(String, String)> {
    static URLS: OnceLock<Option<(String, String)>> = OnceLock::new();
    URLS.get_or_init(|| {
        let folder = std::env::temp_dir().join(format!("pairnox-{}", std::process::id()));
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
        format!("Pairnox {VERSION}  (commit {COMMIT})"),
        "A fast dual-pane file manager for local and remote files.".into(),
        String::new(),
        "License: MIT, \u{a9} 2026 Daniele Buffoni".into(),
        format!("Source:  {REPOSITORY}"),
        String::new(),
        "Built with Rust and Qt 6 (LGPL v3, linked dynamically).".into(),
        // One line per paragraph: each interface wraps it to its width.
        "Release builds read RAR archives with libarchive (BSD-2-Clause). The \
         third-party notices of a release list every library and its license."
            .into(),
    ]
}

/// The About dialog: its text starts below the logo in a terminal.
pub fn dialog() -> MessageDialog {
    MessageDialog {
        title: "About".into(),
        lines: lines(),
        then: None,
        then_choose: None,
        back: false,
        back_to_settings: None,
        logo: true,
        busy: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_is_a_square_picture() {
        let logo = logo();
        assert_eq!(logo.width(), logo.height());
        // The thick-ring icon (32 px) is what a terminal can draw.
        assert!(logo.width() >= 32);
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
        // What is built into the program: the logo and the window icon sizes.
        let total: usize = ICON_PNGS.iter().map(|p| p.len()).sum::<usize>() + LOGO_PNG.len();
        assert!(total < 100 * 1024, "built-in branding takes {total} bytes");
    }
}
