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

/// One cell of the logo drawn with half blocks: the colours of its upper and
/// lower half (`None`: transparent, the terminal's own background shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoCell {
    /// The upper half.
    pub upper: Option<[u8; 3]>,
    /// The lower half.
    pub lower: Option<[u8; 3]>,
}

/// The sizes of the text logo, in rows (it is twice as many pixels high and
/// as many wide): the largest that fits is used.
pub const LOGO_SIZES: [u16; 2] = [LOGO_ROWS, 6];

const BLUE: [u8; 3] = [0x1E, 0x88, 0xE5];

/// The colour of the logo at `(x, y)`, in a square where the disc has radius
/// 1 around the origin; `px` is the size of one pixel in those units, so that
/// thin strokes (the ring, the chevrons) never get thinner than a pixel.
fn logo_color(x: f64, y: f64, px: f64) -> Option<[u8; 3]> {
    let r = x.hypot(y);
    if r > 1.0 {
        return None;
    }
    let ring = (0.096_f64).max(px * 1.05);
    if r > 1.0 - ring {
        return Some(BLUE);
    }
    // The chevrons: two strokes each, blue, as in the picture (> <).
    let half = (0.056_f64).max(px * 0.55);
    let segment = |ax: f64, ay: f64, bx: f64, by: f64| {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        (x - (ax + t * dx)).hypot(y - (ay + t * dy))
    };
    let (tip, near, far) = (0.44, 0.136, 0.264);
    let on_chevron = |sign: f64| {
        let (nx, tx) = (sign * near, sign * tip);
        segment(nx, -far, tx, 0.0) <= half || segment(tx, 0.0, nx, far) <= half
    };
    if on_chevron(-1.0) || on_chevron(1.0) {
        return Some(BLUE);
    }
    Some(if x < 0.0 { [255, 255, 255] } else { [0, 0, 0] })
}

/// The logo for a terminal, drawn with half blocks from the shape itself
/// (not from a shrunken picture), `rows` cells high and as many wide:
/// a crisp ring and chevrons, and the terminal's background around it.
pub fn logo_cells(rows: u16) -> Vec<Vec<LogoCell>> {
    let size = usize::from(rows) * 2;
    let px = 2.0 / size as f64;
    const SAMPLES: usize = 4;
    let pixel = |col: usize, row: usize| -> Option<[u8; 3]> {
        let (mut sum, mut covered) = ([0u32; 3], 0u32);
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let x = -1.0 + (col as f64 + (sx as f64 + 0.5) / SAMPLES as f64) * px;
                let y = -1.0 + (row as f64 + (sy as f64 + 0.5) / SAMPLES as f64) * px;
                if let Some(c) = logo_color(x, y, px) {
                    covered += 1;
                    for i in 0..3 {
                        sum[i] += u32::from(c[i]);
                    }
                }
            }
        }
        // More than half covered: the pixel is drawn, in the average colour.
        (covered * 2 > (SAMPLES * SAMPLES) as u32)
            .then(|| [0, 1, 2].map(|i| (sum[i] / covered) as u8))
    };
    (0..usize::from(rows))
        .map(|row| {
            (0..size)
                .map(|col| LogoCell { upper: pixel(col, row * 2), lower: pixel(col, row * 2 + 1) })
                .collect()
        })
        .collect()
}

/// The decoded logo.
pub fn logo() -> &'static RgbaImage {
    static LOGO: OnceLock<RgbaImage> = OnceLock::new();
    LOGO.get_or_init(|| {
        // The 32 pixel icon has the thick ring, which survives being drawn
        // in 20 half-block cells.
        image::load_from_memory(ICON_PNGS[1])
            .map_or_else(|_| RgbaImage::new(1, 1), |i| i.to_rgba8())
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
        fixed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_logo_has_a_ring_a_white_half_and_a_black_half() {
        let cells = logo_cells(10);
        assert_eq!(cells.len(), 10);
        assert!(cells.iter().all(|row| row.len() == 20));
        // The corners are outside the disc: nothing is drawn (no black box).
        for corner in [(0, 0), (0, 19), (9, 0), (9, 19)] {
            let cell = cells[corner.0][corner.1];
            assert!(cell.upper.is_none() || cell.lower.is_none(), "{corner:?} {cell:?}");
        }
        assert_eq!((cells[0][0].upper, cells[9][19].lower), (None, None));
        let colors: Vec<[u8; 3]> =
            cells.iter().flatten().flat_map(|c| [c.upper, c.lower]).flatten().collect();
        for wanted in [BLUE, [255, 255, 255], [0, 0, 0]] {
            assert!(colors.contains(&wanted), "{wanted:?} is missing");
        }
        // The ring closes: every row touches blue at both ends of the disc.
        let ring = |row: &Vec<LogoCell>| {
            row.iter().flat_map(|c| [c.upper, c.lower]).flatten().any(|c| c == BLUE)
        };
        assert!(cells.iter().all(ring));
    }

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
