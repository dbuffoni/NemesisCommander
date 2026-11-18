//! Finding pictures that are resized or re-encoded copies of each other,
//! by a perceptual hash (a "difference hash" of a 9×8 grey thumbnail).

use image::DynamicImage;
use image::imageops::FilterType;
use nmc_vfs::VfsPath;

/// Hashes that differ in at most this many of 64 bits count as similar.
pub const MAX_DISTANCE: u32 = 6;

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

/// A picture that was hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Where it is.
    pub path: VfsPath,
    /// Its perceptual hash.
    pub hash: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Size of the file in bytes.
    pub size: u64,
}

impl Picture {
    /// Hashes `data`; `None` when it is not a picture, or a flat one (all
    /// flat pictures would look alike).
    pub fn from_bytes(path: VfsPath, data: &[u8]) -> Option<Picture> {
        let image = image::load_from_memory(data).ok()?;
        let hash = dhash(&image);
        (hash != 0).then(|| Picture {
            path,
            hash,
            width: image.width(),
            height: image.height(),
            size: data.len() as u64,
        })
    }

    /// Pixels, then file size: the better copy has more of both.
    fn quality(&self) -> (u64, u64) {
        (u64::from(self.width) * u64::from(self.height), self.size)
    }

    /// `4000×3000, 2.1 MiB`.
    pub fn describe(&self) -> String {
        format!("{}×{}, {}", self.width, self.height, nmc_panel::format::human_size(self.size))
    }
}

/// Groups of similar pictures. Inside a group the best copy (most pixels,
/// then largest file) is first; groups are ordered by their size.
pub fn group(mut pictures: Vec<Picture>, max_distance: u32) -> Vec<Vec<Picture>> {
    let count = pictures.len();
    let mut parent: Vec<usize> = (0..count).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..count {
        for j in i + 1..count {
            if (pictures[i].hash ^ pictures[j].hash).count_ones() <= max_distance {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<Picture>> = Default::default();
    for i in (0..count).rev() {
        let key = root(&mut parent, i);
        groups.entry(key).or_default().push(pictures.swap_remove(i));
    }
    let mut groups: Vec<Vec<Picture>> = groups.into_values().filter(|g| g.len() > 1).collect();
    for group in &mut groups {
        group.sort_by(|a, b| {
            b.quality().cmp(&a.quality()).then_with(|| a.path.to_string().cmp(&b.path.to_string()))
        });
    }
    groups.sort_by(|a, b| {
        b.len().cmp(&a.len()).then_with(|| a[0].path.to_string().cmp(&b[0].path.to_string()))
    });
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    /// A picture with a diagonal gradient and a bright blob at `(cx, cy)`.
    fn scene(width: u32, height: u32, cx: f32, cy: f32) -> DynamicImage {
        let image = ImageBuffer::from_fn(width, height, |x, y| {
            let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
            let blob = (-(((u - cx).powi(2) + (v - cy).powi(2)) * 40.0)).exp();
            let base = u * 120.0 + v * 60.0;
            let value = (base + blob * 120.0).min(255.0) as u8;
            Rgb([value, value / 2, 255 - value])
        });
        DynamicImage::ImageRgb8(image)
    }

    fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn picture(name: &str, image: &DynamicImage, format: ImageFormat) -> Picture {
        let path = VfsPath::parse("file:///pics").unwrap().join(name);
        Picture::from_bytes(path, &encode(image, format)).expect("a real picture")
    }

    #[test]
    fn resized_and_reencoded_copies_group_with_the_best_first() {
        let big = scene(400, 300, 0.3, 0.3);
        let small = big.resize_exact(100, 75, FilterType::Lanczos3);
        let other = scene(400, 300, 0.8, 0.7);
        let pictures = vec![
            picture("small.png", &small, ImageFormat::Png),
            picture("big.png", &big, ImageFormat::Png),
            picture("big.jpg", &big, ImageFormat::Jpeg),
            picture("other.png", &other, ImageFormat::Png),
        ];
        let groups = group(pictures, MAX_DISTANCE);
        assert_eq!(groups.len(), 1, "the other scene stays alone");
        let names: Vec<&str> = groups[0].iter().map(|p| p.path.file_name().unwrap()).collect();
        assert_eq!(names.len(), 3);
        assert_eq!(names[2], "small.png", "fewest pixels last");
        assert_eq!(names[0], "big.png", "same pixels: the larger file");
    }

    #[test]
    fn flat_pictures_and_other_files_are_ignored() {
        let flat = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(20, 20, Rgb([9, 9, 9])));
        let path = VfsPath::parse("file:///pics/flat.png").unwrap();
        assert!(Picture::from_bytes(path.clone(), &encode(&flat, ImageFormat::Png)).is_none());
        assert!(Picture::from_bytes(path, b"not a picture").is_none());
    }
}
