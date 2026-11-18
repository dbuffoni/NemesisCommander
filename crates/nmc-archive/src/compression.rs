//! Stream compressors shared by tar archives and single compressed files.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// A stream compression method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Compression {
    /// No compression.
    None,
    /// gzip (DEFLATE).
    Gzip,
    /// xz (LZMA2).
    Xz,
    /// bzip2.
    Bzip2,
    /// Zstandard.
    Zstd,
}

impl Compression {
    /// Usual file extension (without dot) for a single compressed file.
    pub fn extension(self) -> &'static str {
        match self {
            Compression::None => "",
            Compression::Gzip => "gz",
            Compression::Xz => "xz",
            Compression::Bzip2 => "bz2",
            Compression::Zstd => "zst",
        }
    }

    /// Recognizes a compressed stream from its first bytes.
    pub fn from_magic(head: &[u8]) -> Compression {
        match head {
            [0x1f, 0x8b, ..] => Compression::Gzip,
            [0xfd, b'7', b'z', b'X', b'Z', 0x00, ..] => Compression::Xz,
            [b'B', b'Z', b'h', ..] => Compression::Bzip2,
            [0x28, 0xb5, 0x2f, 0xfd, ..] => Compression::Zstd,
            _ => Compression::None,
        }
    }

    /// Wraps `inner` in a decompressor.
    pub fn decoder<'a, R: Read + 'a>(self, inner: R) -> io::Result<Box<dyn Read + 'a>> {
        Ok(match self {
            Compression::None => Box::new(inner),
            // MultiGzDecoder also reads concatenated gzip members (pigz output).
            Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(inner)),
            Compression::Xz => Box::new(liblzma::read::XzDecoder::new_multi_decoder(inner)),
            Compression::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(inner)),
            Compression::Zstd => Box::new(zstd::stream::read::Decoder::new(inner)?),
        })
    }

    /// Wraps `inner` in a compressor. `level` is the usual 1 (fast) to 9
    /// (small) scale; it is mapped to each method's own range.
    pub fn encoder<'a, W: Write + 'a>(self, inner: W, level: u32) -> io::Result<Encoder<'a, W>> {
        self.encoder_with_size(inner, level, None)
    }

    /// Like [`encoder`](Self::encoder), but records the uncompressed size in
    /// the header when the format supports it (zstd), so viewers can show
    /// the size without decompressing.
    pub fn encoder_with_size<'a, W: Write + 'a>(
        self,
        inner: W,
        level: u32,
        size: Option<u64>,
    ) -> io::Result<Encoder<'a, W>> {
        let level = level.clamp(1, 9);
        Ok(match self {
            Compression::None => Encoder::Plain(inner),
            Compression::Gzip => {
                Encoder::Gzip(flate2::write::GzEncoder::new(inner, flate2::Compression::new(level)))
            }
            Compression::Xz => Encoder::Xz(liblzma::write::XzEncoder::new(inner, level)),
            Compression::Bzip2 => {
                Encoder::Bzip2(bzip2::write::BzEncoder::new(inner, bzip2::Compression::new(level)))
            }
            // zstd levels go to 19+; map 1..9 onto 1..19.
            Compression::Zstd => {
                let mut encoder = zstd::stream::write::Encoder::new(inner, (level * 2 + 1) as i32)?;
                if size.is_some() {
                    encoder.set_pledged_src_size(size)?;
                    encoder.include_contentsize(true)?;
                }
                Encoder::Zstd(encoder)
            }
        })
    }
}

/// A compressing writer. Call [`Encoder::finish`] to write the trailer.
pub enum Encoder<'a, W: Write> {
    /// No compression.
    Plain(W),
    /// gzip.
    Gzip(flate2::write::GzEncoder<W>),
    /// xz.
    Xz(liblzma::write::XzEncoder<W>),
    /// bzip2.
    Bzip2(bzip2::write::BzEncoder<W>),
    /// zstd.
    Zstd(zstd::stream::write::Encoder<'a, W>),
}

impl<W: Write> Encoder<'_, W> {
    /// Flushes everything and returns the inner writer.
    pub fn finish(self) -> io::Result<W> {
        match self {
            Encoder::Plain(mut w) => {
                w.flush()?;
                Ok(w)
            }
            Encoder::Gzip(e) => e.finish(),
            Encoder::Xz(e) => e.finish(),
            Encoder::Bzip2(e) => e.finish(),
            Encoder::Zstd(e) => e.finish(),
        }
    }
}

impl<W: Write> Write for Encoder<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Encoder::Plain(w) => w.write(buf),
            Encoder::Gzip(e) => e.write(buf),
            Encoder::Xz(e) => e.write(buf),
            Encoder::Bzip2(e) => e.write(buf),
            Encoder::Zstd(e) => e.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Encoder::Plain(w) => w.flush(),
            Encoder::Gzip(e) => e.flush(),
            Encoder::Xz(e) => e.flush(),
            Encoder::Bzip2(e) => e.flush(),
            Encoder::Zstd(e) => e.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Compression; 5] = [
        Compression::None,
        Compression::Gzip,
        Compression::Xz,
        Compression::Bzip2,
        Compression::Zstd,
    ];

    #[test]
    fn roundtrip_and_magic() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 97) as u8).collect();
        for method in ALL {
            let mut encoder = method.encoder(Vec::new(), 6).unwrap();
            encoder.write_all(&data).unwrap();
            let compressed = encoder.finish().unwrap();
            if method != Compression::None {
                assert_eq!(Compression::from_magic(&compressed), method);
                assert!(compressed.len() < data.len() / 4, "{method:?} did not compress");
            }
            let mut out = Vec::new();
            method.decoder(compressed.as_slice()).unwrap().read_to_end(&mut out).unwrap();
            assert_eq!(out, data, "{method:?}");
        }
    }
}
