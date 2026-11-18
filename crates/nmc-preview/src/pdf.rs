//! PDF pages to pictures, with `hayro` (pure Rust, no system libraries).

use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use image::RgbaImage;

/// The number of pages of a PDF file.
pub fn page_count(data: &Arc<Vec<u8>>) -> Result<usize, String> {
    Ok(open(data)?.pages().len())
}

fn open(data: &Arc<Vec<u8>>) -> Result<Pdf, String> {
    Pdf::new(data.clone()).map_err(|e| format!("cannot read this PDF: {e:?}"))
}

/// Page `index` (from 0) drawn to fit in `width` × `height` pixels, on
/// white.
pub fn render_page(
    data: &Arc<Vec<u8>>,
    index: usize,
    width: u32,
    height: u32,
) -> Result<RgbaImage, String> {
    let pdf = open(data)?;
    let pages = pdf.pages();
    let page = pages.get(index).ok_or_else(|| format!("the PDF has no page {}", index + 1))?;
    let (page_width, page_height) = page.render_dimensions();
    if page_width <= 0.0 || page_height <= 0.0 {
        return Err("the page has no size".into());
    }
    // Pages are measured in points; never draw a page larger than about
    // 4000 pixels on its longer side.
    let scale = f32::min(width.max(16) as f32 / page_width, height.max(16) as f32 / page_height)
        .min(4000.0 / page_width.max(page_height));
    let settings = PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &settings,
    );
    let (w, h) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    // On a white background the premultiplied pixels are the plain ones.
    RgbaImage::from_raw(w, h, pixmap.data_as_u8_slice().to_vec())
        .ok_or_else(|| "the page could not be drawn".into())
}
