//! The perceptual hash behind `pnxr.imageHash` (used by the image-duplicates
//! plugin): a "difference hash" of a 9×8 grey thumbnail. Pictures that are
//! resized or re-encoded copies of each other have hashes that differ in a few
//! of the 64 bits.

use image::DynamicImage;
use image::imageops::FilterType;

/// Pictures bigger than this are not decoded.
pub const PICTURE_LIMIT: u64 = 48 << 20;

/// What `pnxr.imageHash` returns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ImageInfo {
    /// The hash, as 16 hexadecimal digits.
    pub hash: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Size of the file in bytes.
    pub bytes: u64,
}

/// The difference hash of a picture: one bit for each pair of neighbouring
/// pixels of a 9×8 grey version, set when the left one is brighter.
pub fn dhash(image: &DynamicImage) -> u64 {
    let small = image.resize_exact(9, 8, FilterType::Triangle).to_luma8();
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            hash <<= 1;
            if small.get_pixel(x, y).0[0] > small.get_pixel(x + 1, y).0[0] {
                hash |= 1;
            }
        }
    }
    hash
}

/// Hashes `data`; `None` when it is not a picture, or a flat one (all flat
/// pictures would look alike).
pub fn describe(data: &[u8]) -> Option<ImageInfo> {
    let image = image::load_from_memory(data).ok()?;
    let hash = dhash(&image);
    (hash != 0).then(|| ImageInfo {
        hash: format!("{hash:016x}"),
        width: image.width(),
        height: image.height(),
        bytes: data.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    /// A picture with a diagonal gradient and a bright blob at `(cx, cy)`.
    fn scene(width: u32, height: u32, cx: f32, cy: f32) -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(width, height, |x, y| {
            let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
            let blob = (-(((u - cx).powi(2) + (v - cy).powi(2)) * 40.0)).exp();
            let value = (u * 120.0 + v * 60.0 + blob * 120.0).min(255.0) as u8;
            Rgb([value, value / 2, 255 - value])
        }))
    }

    fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn distance(a: &ImageInfo, b: &ImageInfo) -> u32 {
        let value = |i: &ImageInfo| u64::from_str_radix(&i.hash, 16).unwrap();
        (value(a) ^ value(b)).count_ones()
    }

    #[test]
    fn resized_and_reencoded_copies_are_close_and_other_scenes_are_far() {
        let big = scene(400, 300, 0.3, 0.3);
        let small = big.resize_exact(100, 75, FilterType::Lanczos3);
        let other = scene(400, 300, 0.8, 0.7);
        let original = describe(&encode(&big, ImageFormat::Png)).unwrap();
        let jpeg = describe(&encode(&big, ImageFormat::Jpeg)).unwrap();
        let thumb = describe(&encode(&small, ImageFormat::Png)).unwrap();
        let different = describe(&encode(&other, ImageFormat::Png)).unwrap();
        assert_eq!(original.hash.len(), 16);
        assert_eq!((thumb.width, thumb.height), (100, 75));
        assert!(distance(&original, &jpeg) <= 6, "{}", distance(&original, &jpeg));
        assert!(distance(&original, &thumb) <= 6, "{}", distance(&original, &thumb));
        assert!(distance(&original, &different) > 6, "{}", distance(&original, &different));
    }

    #[test]
    fn flat_pictures_and_other_files_have_no_hash() {
        let flat = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(20, 20, Rgb([9, 9, 9])));
        assert!(describe(&encode(&flat, ImageFormat::Png)).is_none());
        assert!(describe(b"not a picture").is_none());
    }
}
