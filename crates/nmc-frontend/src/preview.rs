//! What the viewer shows besides text, hex and plain pictures: formatted
//! documents (Markdown, Office), PDF pages and 3D models, through
//! `nmc-preview`.
//!
//! Loading a file *prepares* it in the background ([`prepare`]): documents
//! become blocks, PDFs and models are opened. Pages and views of models are
//! pictures *rendered* in the background too ([`render`]) whenever the page
//! or the angle changes. Every picture shown also gets a PNG file when the
//! interface is a window ([`write_png`]), which it can draw at full quality.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use image::RgbaImage;
use nmc_preview::model::{Camera, Mesh};
use nmc_preview::rich::{Block, Line};
use nmc_preview::{Kind, ebook, markdown, media, office, pdf};

/// Files previewed as documents, PDFs or models are read up to this size.
pub const PREVIEW_LIMIT: u64 = 256 * 1024 * 1024;

/// Audio and video files are described from their first bytes only.
pub const MEDIA_LIMIT: u64 = 32 * 1024 * 1024;

/// How much of a file to read to preview it.
pub fn read_limit(name: &str) -> Option<u64> {
    match kind_of(name)? {
        Kind::Audio | Kind::Video => Some(MEDIA_LIMIT),
        _ => Some(PREVIEW_LIMIT),
    }
}

/// Pictures of pages and models are rendered to fit this box in the
/// viewer, and a smaller one in the quick view.
pub const RENDER_SIZE: (u32, u32) = (1600, 1600);
/// See [`RENDER_SIZE`].
pub const QUICK_RENDER_SIZE: (u32, u32) = (800, 800);

/// What the file system says about the file shown: it goes on the
/// information sheet of binary files.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The whole size of the file (it may be longer than what was read).
    pub size: Option<u64>,
    /// Rows of the sheet: modified, permissions...
    pub rows: Vec<Vec<String>>,
}

impl Facts {
    /// The facts of a listing entry.
    pub fn of(entry: &nmc_vfs::Entry) -> Self {
        let mut rows = Vec::new();
        if let Some(modified) = entry.modified {
            rows.push(vec!["Modified".to_owned(), nmc_panel::format::local_time(modified)]);
        }
        if let Some(mode) = entry.permissions {
            rows.push(vec![
                "Permissions".to_owned(),
                format!("{} ({:o})", nmc_panel::format::permissions(mode), mode & 0o7777),
            ]);
        }
        Facts { size: Some(entry.size), rows }
    }
}

/// A loaded file, ready to show.
#[derive(Debug, Clone)]
pub enum Prepared {
    /// Text, a picture or binary data: the viewer decides from the bytes.
    Plain,
    /// A formatted document.
    Rich(Vec<Block>),
    /// A PDF and its number of pages (the viewer keeps the file's bytes).
    Pdf {
        /// Pages in it.
        pages: usize,
    },
    /// A 3D model.
    Model(Arc<Mesh>),
}

/// The preview kind of a file name, if it has a native preview.
pub fn kind_of(name: &str) -> Option<Kind> {
    Kind::of(name)
}

/// Turns the bytes of `name` into what the viewer shows. Runs in the
/// background: Office files and models can take a moment.
pub fn prepare(
    name: &str,
    data: &Arc<Vec<u8>>,
    truncated: bool,
    facts: &Facts,
) -> Result<Prepared, String> {
    let Some(kind) = kind_of(name) else {
        // Binary data is described, not dumped: the hex dump is F4.
        let picture = nmc_preview::binary::is_picture(data) || crate::viewer::is_image_name(name);
        if !picture && !crate::viewer::looks_like_text(data) {
            return Ok(Prepared::Rich(nmc_preview::binary::describe(
                name,
                data,
                facts.size,
                facts.rows.clone(),
            )));
        }
        return Ok(Prepared::Plain);
    };
    // Audio and video are described from their start, however long they are.
    if truncated && !matches!(kind, Kind::Audio | Kind::Video) {
        return Err(format!("the file is too large to preview (over {} MiB)", PREVIEW_LIMIT >> 20));
    }
    Ok(match kind {
        Kind::Markdown => Prepared::Rich(markdown::parse(&String::from_utf8_lossy(data))),
        Kind::Office(format) => Prepared::Rich(office::read(format, data)?),
        Kind::Pdf => {
            let pages = pdf::page_count(data)?;
            if pages == 0 {
                return Err("the PDF has no pages".into());
            }
            Prepared::Pdf { pages }
        }
        Kind::Model => Prepared::Model(Arc::new(Mesh::load(name, data)?)),
        Kind::Ebook(format) => Prepared::Rich(ebook::read(format, data)?),
        Kind::Audio => Prepared::Rich(media::audio(name, data, !truncated)?),
        Kind::Video => Prepared::Rich(media::video(name, data, !truncated)?),
    })
}

/// What a rendered picture is made from.
#[derive(Debug, Clone)]
pub enum Source {
    /// A decoded picture, only written as PNG for the window.
    Picture(Arc<RgbaImage>),
    /// Page `page` (from 0) of a PDF.
    Page {
        /// The PDF.
        data: Arc<Vec<u8>>,
        /// The page.
        page: usize,
    },
    /// A model seen from a camera.
    Model {
        /// The model.
        mesh: Arc<Mesh>,
        /// Where from.
        camera: Camera,
    },
}

/// Draws `source` to fit `size`. Runs in the background.
pub fn render(source: &Source, size: (u32, u32)) -> Result<Arc<RgbaImage>, String> {
    match source {
        Source::Picture(picture) => Ok(picture.clone()),
        Source::Page { data, page } => pdf::render_page(data, *page, size.0, size.1).map(Arc::new),
        Source::Model { mesh, camera } => {
            Ok(Arc::new(nmc_preview::model::render(mesh, *camera, size.0, size.1 * 3 / 4)))
        }
    }
}

/// Writes `picture` as a PNG file in this program's temporary folder and
/// returns its path. Each call makes a new file, so a window showing the
/// previous one notices the change.
pub fn write_png(picture: &RgbaImage) -> Result<PathBuf, String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let folder = std::env::temp_dir().join(format!("nemesiscommander-{}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = folder.join(format!("view-{number}.png"));
    picture.save_with_format(&path, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Deletes a PNG made by [`write_png`] (quietly: it is only a cache).
pub fn forget_png(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// A formatted document and its lines at the current width.
#[derive(Debug, Clone, Default)]
pub struct RichText {
    /// The document.
    pub blocks: Vec<Block>,
    /// Its lines, laid out for `width` columns.
    pub lines: Vec<Line>,
    /// The width of the layout (0 before the first one).
    pub width: usize,
}

impl RichText {
    /// A document not laid out yet.
    pub fn new(blocks: Vec<Block>) -> Self {
        RichText { blocks, lines: Vec::new(), width: 0 }
    }

    /// Lays the document out for `width` columns, if it changed.
    pub fn layout(&mut self, width: usize) {
        let width = width.max(20);
        if width != self.width {
            self.lines = nmc_preview::rich::layout(&self.blocks, width);
            self.width = width;
        }
    }
}

/// The part of a picture shown: everything at zoom 1 ("fit"), a smaller
/// window around `center` when zoomed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    /// 1.0 fits the whole picture; 2.0 shows half of each side.
    pub factor: f32,
    /// The point shown in the middle, as fractions of width and height.
    pub center: (f32, f32),
}

impl Default for Zoom {
    fn default() -> Self {
        Zoom { factor: 1.0, center: (0.5, 0.5) }
    }
}

impl Zoom {
    /// Largest zoom.
    pub const MAX: f32 = 16.0;

    /// Zooms in (`steps` > 0) or out by factors of √2, between fit and
    /// [`Zoom::MAX`].
    pub fn change(&mut self, steps: i32) {
        self.factor = (self.factor * 2f32.sqrt().powi(steps)).clamp(1.0, Self::MAX);
        if self.factor <= 1.0 {
            self.center = (0.5, 0.5);
        }
        self.clamp();
    }

    /// Moves the shown window by a tenth of its size per step.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        let step = 0.1 / self.factor;
        self.center.0 += dx as f32 * step;
        self.center.1 += dy as f32 * step;
        self.clamp();
    }

    fn clamp(&mut self) {
        let half = 0.5 / self.factor;
        self.center.0 = self.center.0.clamp(half, 1.0 - half);
        self.center.1 = self.center.1.clamp(half, 1.0 - half);
    }

    /// The shown rectangle of a `width` × `height` picture: x, y, width,
    /// height in pixels.
    pub fn region(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let w = ((width as f32 / self.factor).round() as u32).clamp(1, width.max(1));
        let h = ((height as f32 / self.factor).round() as u32).clamp(1, height.max(1));
        let x = ((self.center.0 * width as f32) - w as f32 / 2.0).round().max(0.0) as u32;
        let y = ((self.center.1 * height as f32) - h as f32 / 2.0).round().max(0.0) as u32;
        (x.min(width - w), y.min(height - h), w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_stays_inside_the_picture() {
        let mut zoom = Zoom::default();
        assert_eq!(zoom.region(400, 200), (0, 0, 400, 200));
        zoom.change(2);
        assert!((zoom.factor - 2.0).abs() < 1e-4, "{}", zoom.factor);
        zoom.factor = 2.0;
        let (x, y, w, h) = zoom.region(400, 200);
        assert_eq!((w, h), (200, 100));
        assert_eq!((x, y), (100, 50), "centred");
        zoom.pan(100, 100);
        assert_eq!(zoom.region(400, 200), (200, 100, 200, 100), "stops at the corner");
        zoom.change(-10);
        assert_eq!(zoom, Zoom::default(), "back to fit");
        zoom.change(100);
        assert_eq!(zoom.factor, Zoom::MAX);
    }

    #[test]
    fn plain_files_need_no_preparation() {
        let bytes = |text: &[u8]| Arc::new(text.to_vec());
        assert!(matches!(
            prepare("a.txt", &bytes(b"hello"), false, &Facts::default()),
            Ok(Prepared::Plain)
        ));
        assert!(
            matches!(prepare("a.md", &bytes(b"# Hi"), false, &Facts::default()), Ok(Prepared::Rich(blocks)) if blocks.len() == 1)
        );
        assert!(
            prepare("a.pdf", &bytes(b"%PDF-1.4"), true, &Facts::default())
                .unwrap_err()
                .contains("too large")
        );
        assert!(prepare("a.stl", &bytes(b"nonsense"), false, &Facts::default()).is_err());
    }
}
