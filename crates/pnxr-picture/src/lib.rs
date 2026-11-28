//! Pictures for the viewer and the picture hash.
//!
//! Besides the formats of the `image` crate (PNG, JPEG, GIF, WebP, BMP, ICO,
//! TIFF) this reads **HEIC/HEIF** with a decoder written in Rust (`heifer`, no C
//! library; rotation and cropping are applied) and **RAW** camera files through
//! the JPEG preview that almost every RAW file carries (the raw sensor data is
//! not developed).

use image::{DynamicImage, RgbaImage};

/// Pictures bigger than this are not decoded.
pub const PICTURE_LIMIT: u64 = 128 << 20;

/// File name extensions of HEIC/HEIF pictures.
pub const HEIC_EXTENSIONS: &[&str] = &["heic", "heif", "hif"];

/// File name extensions of RAW camera files, read through their preview.
pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef",
    "srw", "3fr", "erf", "kdc", "mrw", "x3f",
];

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

/// `true` for a HEIC/HEIF file name.
pub fn is_heic_name(name: &str) -> bool {
    extension(name).is_some_and(|e| HEIC_EXTENSIONS.contains(&e.as_str()))
}

/// `true` for a RAW camera file name.
pub fn is_raw_name(name: &str) -> bool {
    extension(name).is_some_and(|e| RAW_EXTENSIONS.contains(&e.as_str()))
}

/// `true` when `data` starts like a HEIC/HEIF file (an `ftyp` box with a
/// HEIF brand; AVIF is not included).
pub fn is_heic(data: &[u8]) -> bool {
    if data.len() < 12 || &data[4..8] != b"ftyp" {
        return false;
    }
    matches!(
        &data[8..12],
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1"
    )
}

/// Decodes a HEIC/HEIF picture.
pub fn decode_heic(data: &[u8]) -> Option<DynamicImage> {
    let picture = heifer::decode(data).ok()?;
    let rgba = RgbaImage::from_raw(picture.width, picture.height, picture.to_rgba8())?;
    Some(DynamicImage::ImageRgba8(rgba))
}

/// The biggest JPEG preview inside a RAW file (or other container): the
/// candidates start at each `FF D8 FF`, and the one that decodes to the most
/// pixels wins.
pub fn decode_raw_preview(data: &[u8]) -> Option<DynamicImage> {
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    while let Some(found) = data[at..].windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) {
        starts.push(at + found);
        at += found + 3;
        if starts.len() >= 16 {
            break;
        }
    }
    let mut best: Option<DynamicImage> = None;
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(data.len());
        let part = &data[start..end];
        let Ok(picture) = image::load_from_memory_with_format(part, image::ImageFormat::Jpeg)
        else {
            continue;
        };
        let pixels = u64::from(picture.width()) * u64::from(picture.height());
        if best.as_ref().is_none_or(|b| pixels > u64::from(b.width()) * u64::from(b.height())) {
            best = Some(picture);
        }
    }
    best
}

/// Decodes `data`, using `name` (a file name, may be empty) to tell a RAW file.
pub fn decode_named(data: &[u8], name: &str) -> Option<DynamicImage> {
    if is_heic(data) {
        return decode_heic(data);
    }
    if is_raw_name(name) {
        return decode_raw_preview(data);
    }
    load_upright(data, None)
}

/// Decodes a picture of the `image` crate and turns it upright by its EXIF
/// orientation (a phone's JPEG is stored sideways with a note to turn it).
fn load_upright(data: &[u8], format: Option<image::ImageFormat>) -> Option<DynamicImage> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data));
    match format {
        Some(format) => reader.set_format(format),
        None => reader = reader.with_guessed_format().ok()?,
    }
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(orientation) = orientation {
        picture.apply_orientation(orientation);
    }
    Some(picture)
}

/// `true` for pictures the window's own image loader cannot show (HEIC and
/// RAW): they are converted to PNG for it.
pub fn needs_conversion(name: &str) -> bool {
    is_heic_name(name) || is_raw_name(name)
}

/// Decodes `data` without a name.
pub fn decode(data: &[u8]) -> Option<DynamicImage> {
    decode_named(data, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    fn scene(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, 128])
        }))
    }

    fn jpeg(image: &DynamicImage) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn heic_is_recognised_by_its_brand_and_decoded() {
        let source = scene(96, 64);
        let rgba = source.to_rgba8();
        let data = heifer::encode_rgba8(96, 64, &rgba, &heifer::EncodeOptions::default()).unwrap();
        assert!(is_heic(&data));
        let picture = decode(&data).expect("the HEIC decodes");
        assert_eq!((picture.width(), picture.height()), (96, 64));
        // Lossy, but the same picture: the average colour is close.
        let mean = |i: &DynamicImage| {
            let rgb = i.to_rgb8();
            rgb.pixels().map(|p| u64::from(p.0[0])).sum::<u64>() / rgb.pixels().count() as u64
        };
        assert!(mean(&picture).abs_diff(mean(&source)) < 12);
    }

    #[test]
    fn names_tell_the_kinds() {
        assert!(is_heic_name("IMG_1.HEIC") && !is_heic_name("a.png"));
        assert!(is_raw_name("DSC01.ARW") && is_raw_name("x.cr3") && !is_raw_name("a.jpg"));
    }

    #[test]
    fn a_raw_file_shows_its_biggest_embedded_preview() {
        // A made-up RAW: junk, a small JPEG, junk, a big JPEG, junk.
        let mut raw = b"II*\0 not really raw data ".to_vec();
        raw.extend(jpeg(&scene(16, 12)));
        raw.extend(b"more junk");
        raw.extend(jpeg(&scene(160, 120)));
        raw.extend(b"tail");
        let picture = decode_named(&raw, "DSC0001.ARW").expect("a preview is found");
        assert_eq!((picture.width(), picture.height()), (160, 120));
        // Not a RAW name: the plain decoders do not understand it.
        assert!(decode_named(&raw, "x.bin").is_none());
    }

    #[test]
    fn damaged_heic_files_are_refused_not_crashed_on() {
        let rgba = scene(64, 48).to_rgba8();
        let data = heifer::encode_rgba8(64, 48, &rgba, &heifer::EncodeOptions::default()).unwrap();
        for cut in [12, 40, data.len() / 3, data.len() / 2, data.len() - 1] {
            let _ = decode(&data[..cut]);
        }
        let mut broken = data.clone();
        for i in (data.len() / 2..data.len()).step_by(7) {
            broken[i] ^= 0xA5;
        }
        let _ = decode(&broken);
    }

    #[test]
    fn a_jpeg_stored_sideways_is_turned_upright() {
        // A 40x20 JPEG with EXIF orientation 6 (turn 90 degrees clockwise to
        // see it): upright it is 20x40.
        let mut data = jpeg(&scene(40, 20));
        let exif: &[u8] = b"\xFF\xE1\x00\x24Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0\0\0";
        data.splice(2..2, exif.iter().copied());
        let picture = decode(&data).expect("decodes");
        assert_eq!((picture.width(), picture.height()), (20, 40));
    }

    #[test]
    fn plain_formats_and_garbage() {
        let png = {
            let mut out = Cursor::new(Vec::new());
            scene(8, 8).write_to(&mut out, ImageFormat::Png).unwrap();
            out.into_inner()
        };
        assert_eq!(decode(&png).unwrap().width(), 8);
        assert!(decode(b"not a picture").is_none());
        assert!(decode_heic(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic").is_none());
    }
}
