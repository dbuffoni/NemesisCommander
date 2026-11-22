//! Audio and video files as an information sheet: tags, duration, codecs,
//! picture size. Nothing is played or decoded: the sheet is read from the
//! headers of the file, in pure Rust (`lofty` for audio; MP4/MOV,
//! Matroska/WebM and AVI are read here).

use std::io::Cursor;

use crate::rich::{Block, BlockKind};

/// Audio formats read by their extension.
const AUDIO: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "wav", "wave", "aac", "m4a", "aif", "aiff", "aifc", "ape",
    "wv", "mpc", "spx",
];
/// Video formats read by their extension.
const VIDEO: &[&str] = &["mp4", "m4v", "mov", "3gp", "mkv", "webm", "avi"];

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

/// Whether `name` is an audio file this module reads.
pub fn is_audio_name(name: &str) -> bool {
    extension(name).is_some_and(|e| AUDIO.contains(&e.as_str()))
}

/// Whether `name` is a video file this module reads.
pub fn is_video_name(name: &str) -> bool {
    extension(name).is_some_and(|e| VIDEO.contains(&e.as_str()))
}

/// A line of the sheet.
type Rows = Vec<Vec<String>>;

fn row(rows: &mut Rows, name: &str, value: impl Into<String>) {
    let value = value.into();
    if !value.trim().is_empty() {
        rows.push(vec![name.to_owned(), value]);
    }
}

/// `1:02:03` or `2:03` for `seconds`.
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn sheet(title: &str, name: &str, size: Option<usize>, sections: Vec<(&str, Rows)>) -> Vec<Block> {
    let mut blocks = vec![Block::heading(1, title.to_owned())];
    let mut file = vec![vec!["File".to_owned(), name.to_owned()]];
    if let Some(size) = size {
        file.push(vec!["Size read".into(), format!("{size} bytes")]);
    }
    blocks.push(Block::new(BlockKind::Table { rows: file, header: false }, Vec::new()));
    for (heading, rows) in sections {
        if rows.is_empty() {
            continue;
        }
        blocks.push(Block::heading(2, heading.to_owned()));
        blocks.push(Block::new(BlockKind::Table { rows, header: false }, Vec::new()));
    }
    blocks
}

// ---------------------------------------------------------------- audio

/// The sheet of an audio file.
pub fn audio(name: &str, data: &[u8], complete: bool) -> Result<Vec<Block>, String> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::prelude::Accessor;

    let probe = lofty::probe::Probe::new(Cursor::new(data))
        .guess_file_type()
        .map_err(|e| format!("cannot read this audio file: {e}"))?;
    let tagged = probe.read().map_err(|e| format!("cannot read this audio file: {e}"))?;
    let properties = tagged.properties();
    let mut about: Rows = Vec::new();
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        row(&mut about, "Title", tag.title().map(|v| v.into_owned()).unwrap_or_default());
        row(&mut about, "Artist", tag.artist().map(|v| v.into_owned()).unwrap_or_default());
        row(&mut about, "Album", tag.album().map(|v| v.into_owned()).unwrap_or_default());
        row(&mut about, "Year", tag.date().map(|d| d.to_string()).unwrap_or_default());
        row(&mut about, "Track", tag.track().map(|t| t.to_string()).unwrap_or_default());
        row(&mut about, "Genre", tag.genre().map(|v| v.into_owned()).unwrap_or_default());
        if tag.picture_count() > 0 {
            row(&mut about, "Cover", format!("{} picture(s) inside", tag.picture_count()));
        }
    }
    let mut sound: Rows = Vec::new();
    row(&mut sound, "Format", format!("{:?}", tagged.file_type()));
    let duration = properties.duration().as_secs_f64();
    if duration > 0.0 {
        let note = if complete { "" } else { " (estimated from the start of the file)" };
        row(&mut sound, "Duration", format!("{}{note}", clock(duration)));
    }
    row(
        &mut sound,
        "Bit rate",
        properties.audio_bitrate().map(|b| format!("{b} kbit/s")).unwrap_or_default(),
    );
    row(
        &mut sound,
        "Sample rate",
        properties.sample_rate().map(|r| format!("{r} Hz")).unwrap_or_default(),
    );
    row(&mut sound, "Channels", properties.channels().map(|c| c.to_string()).unwrap_or_default());
    row(
        &mut sound,
        "Bit depth",
        properties.bit_depth().map(|b| format!("{b} bits")).unwrap_or_default(),
    );
    let title = about
        .iter()
        .find(|r| r[0] == "Title")
        .map_or_else(|| "Audio file".to_owned(), |r| r[1].clone());
    Ok(sheet(&title, name, Some(data.len()), vec![("Tags", about), ("Sound", sound)]))
}

// ---------------------------------------------------------------- video

/// What the header of a video says.
#[derive(Debug, Default, PartialEq)]
pub struct VideoInfo {
    /// Container (`MP4`, `Matroska`, `WebM`, `AVI`).
    pub container: String,
    /// Length in seconds.
    pub duration: Option<f64>,
    /// Title, when the file has one.
    pub title: String,
    /// The tracks: kind, codec and details.
    pub tracks: Vec<Track>,
    /// The program that made the file.
    pub writer: String,
}

/// One track of a video file.
#[derive(Debug, Default, PartialEq)]
pub struct Track {
    /// `video`, `audio` or `subtitles`.
    pub kind: String,
    /// The codec, as the file names it.
    pub codec: String,
    /// Picture size.
    pub size: Option<(u32, u32)>,
    /// Pictures per second.
    pub fps: Option<f64>,
    /// Audio sample rate in Hz.
    pub rate: Option<u32>,
    /// Audio channels.
    pub channels: Option<u32>,
    /// Language code.
    pub language: String,
}

/// The sheet of a video file. `complete` is false when only the start of a
/// large file was read.
pub fn video(name: &str, data: &[u8], complete: bool) -> Result<Vec<Block>, String> {
    let info = match extension(name).as_deref() {
        Some("mkv" | "webm") => matroska(data)?,
        Some("avi") => avi(data)?,
        _ => mp4(data, complete)?,
    };
    let mut about: Rows = Vec::new();
    row(&mut about, "Container", info.container.clone());
    row(&mut about, "Title", info.title.clone());
    row(&mut about, "Duration", info.duration.map(clock).unwrap_or_default());
    row(&mut about, "Made with", info.writer.clone());
    let mut tracks: Rows = Vec::new();
    for (i, t) in info.tracks.iter().enumerate() {
        let mut parts = vec![t.codec.clone()];
        if let Some((w, h)) = t.size {
            parts.push(format!("{w}×{h}"));
        }
        if let Some(fps) = t.fps {
            parts.push(format!("{fps:.3} fps").replace(".000", ""));
        }
        if let Some(rate) = t.rate {
            parts.push(format!("{rate} Hz"));
        }
        if let Some(channels) = t.channels {
            parts.push(format!("{channels} ch"));
        }
        if !t.language.is_empty() && t.language != "und" {
            parts.push(t.language.clone());
        }
        parts.retain(|p| !p.is_empty());
        tracks.push(vec![format!("{} {}", t.kind, i + 1), parts.join(" · ")]);
    }
    let title = if info.title.is_empty() { "Video file".to_owned() } else { info.title.clone() };
    Ok(sheet(&title, name, None, vec![("Video", about), ("Tracks", tracks)]))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
fn be64(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8).map(|b| u64::from_be_bytes(b.try_into().unwrap_or([0; 8])))
}
fn le32(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn le16(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

/// The boxes of `d` as (type, payload).
fn boxes(d: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= d.len() {
        let size = be32(d, at).unwrap_or(0) as usize;
        let kind: [u8; 4] = d[at + 4..at + 8].try_into().unwrap_or([0; 4]);
        let (header, total) = match size {
            0 => (8, d.len() - at),
            1 => (16, be64(d, at + 8).unwrap_or(0) as usize),
            n => (8, n),
        };
        if total < header {
            break;
        }
        // A box cut by the end of what was read still shows what it holds.
        let end = (at + total).min(d.len());
        out.push((kind, &d[at + header..end]));
        at += total;
    }
    out
}

fn find<'a>(d: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(d).into_iter().find(|(k, _)| k == kind).map(|(_, p)| p)
}

fn fourcc(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '?' })
        .collect()
}

/// MP4, MOV, M4V and 3GP.
fn mp4(data: &[u8], complete: bool) -> Result<VideoInfo, String> {
    let top = boxes(data);
    let ftyp = top.iter().find(|(k, _)| k == b"ftyp").ok_or("not an MP4/MOV file (no ftyp)")?;
    let brand = fourcc(ftyp.1.get(..4).unwrap_or(b""));
    let mut info = VideoInfo {
        container: if brand.trim() == "qt" {
            "QuickTime (MOV)".into()
        } else {
            format!("MP4 ({})", brand.trim())
        },
        ..VideoInfo::default()
    };
    let Some(moov) = top.iter().find(|(k, _)| k == b"moov").map(|(_, p)| *p) else {
        return Err(if complete {
            "the file has no movie header (moov)".into()
        } else {
            "the movie header is at the end of this large file, which was not read; the container is ".to_owned() + &info.container
        });
    };
    if let Some(mvhd) = find(moov, b"mvhd") {
        let version = mvhd.first().copied().unwrap_or(0);
        let (scale, length) = if version == 1 {
            (be32(mvhd, 20), be64(mvhd, 24))
        } else {
            (be32(mvhd, 12), be32(mvhd, 16).map(u64::from))
        };
        if let (Some(scale), Some(length)) = (scale, length)
            && scale > 0
        {
            info.duration = Some(length as f64 / f64::from(scale));
        }
    }
    for (kind, trak) in boxes(moov) {
        if &kind != b"trak" {
            continue;
        }
        let mut track = Track::default();
        if let Some(tkhd) = find(trak, b"tkhd") {
            let version = tkhd.first().copied().unwrap_or(0);
            let at = if version == 1 { 88 } else { 76 };
            if let (Some(w), Some(h)) = (be32(tkhd, at), be32(tkhd, at + 4)) {
                let (w, h) = (w >> 16, h >> 16);
                if w > 0 && h > 0 {
                    track.size = Some((w, h));
                }
            }
        }
        let Some(mdia) = find(trak, b"mdia") else { continue };
        let handler =
            find(mdia, b"hdlr").and_then(|h| h.get(8..12)).map(fourcc).unwrap_or_default();
        track.kind = match handler.as_str() {
            "vide" => "video",
            "soun" => "audio",
            "text" | "sbtl" | "subt" | "clcp" => "subtitles",
            _ => continue,
        }
        .to_owned();
        let mut media_seconds = None;
        if let Some(mdhd) = find(mdia, b"mdhd") {
            let version = mdhd.first().copied().unwrap_or(0);
            let (scale, length, lang_at) = if version == 1 {
                (be32(mdhd, 20), be64(mdhd, 24), 32)
            } else {
                (be32(mdhd, 12), be32(mdhd, 16).map(u64::from), 20)
            };
            if let Some(code) = mdhd.get(lang_at..lang_at + 2) {
                let packed = u16::from_be_bytes([code[0], code[1]]);
                let letters: String = (0..3)
                    .map(|i| char::from(((packed >> (10 - 5 * i)) & 0x1f) as u8 + 0x60))
                    .collect();
                if letters.chars().all(|c| c.is_ascii_lowercase()) {
                    track.language = letters;
                }
            }
            if let (Some(scale), Some(length)) = (scale, length)
                && scale > 0
            {
                media_seconds = Some((length as f64 / f64::from(scale), scale));
            }
        }
        if let Some(stbl) = find(mdia, b"minf").and_then(|m| find(m, b"stbl")) {
            if let Some(stsd) = find(stbl, b"stsd")
                && let Some(entry) = stsd.get(8..)
                && let Some((kind, payload)) = boxes(entry).into_iter().next()
            {
                track.codec = fourcc(&kind);
                if track.kind == "audio" {
                    // Version 0 sound entry: channels at 16, rate (16.16) at 24.
                    track.channels =
                        payload.get(16..18).map(|c| u32::from(u16::from_be_bytes([c[0], c[1]])));
                    track.rate = be32(payload, 24).map(|r| r >> 16).filter(|r| *r > 0);
                }
            }
            // Frames per second from the number of samples and the length.
            if track.kind == "video"
                && let Some(stts) = find(stbl, b"stts")
                && let Some((seconds, _)) = media_seconds
            {
                let entries = be32(stts, 4).unwrap_or(0) as usize;
                let samples: u64 = (0..entries.min(10_000))
                    .filter_map(|i| be32(stts, 8 + i * 8))
                    .map(u64::from)
                    .sum();
                if samples > 0 && seconds > 0.0 {
                    track.fps = Some((samples as f64 / seconds * 1000.0).round() / 1000.0);
                }
            }
        }
        info.tracks.push(track);
    }
    Ok(info)
}

// ------------------------------------------------------------- Matroska

/// A variable-length number of EBML; `keep_marker` for element ids.
fn vint(d: &[u8], at: usize, keep_marker: bool) -> Option<(u64, usize)> {
    let first = *d.get(at)?;
    let length = first.leading_zeros() as usize + 1;
    if length > 8 || at + length > d.len() {
        return None;
    }
    let mut value = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first) & ((1u64 << (8 - length)) - 1)
    };
    let mut all_ones =
        (first & ((1u16 << (8 - length)) - 1) as u8) == ((1u16 << (8 - length)) - 1) as u8;
    for b in &d[at + 1..at + length] {
        value = (value << 8) | u64::from(*b);
        all_ones &= *b == 0xff;
    }
    // An all-ones size means "unknown": the rest of the data.
    if !keep_marker && all_ones {
        value = u64::MAX;
    }
    Some((value, length))
}

/// The elements of `d` as (id, payload).
fn elements(d: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((id, id_len)) = vint(d, at, true) {
        let Some((size, size_len)) = vint(d, at + id_len, false) else { break };
        let start = at + id_len + size_len;
        let end = if size == u64::MAX {
            d.len()
        } else {
            start.saturating_add(size as usize).min(d.len())
        };
        if start > d.len() {
            break;
        }
        out.push((id, &d[start..end]));
        at = end;
        if at >= d.len() {
            break;
        }
    }
    out
}

fn uint(d: &[u8]) -> u64 {
    d.iter().fold(0, |acc, b| (acc << 8) | u64::from(*b))
}

fn float(d: &[u8]) -> Option<f64> {
    match d.len() {
        4 => Some(f64::from(f32::from_be_bytes([d[0], d[1], d[2], d[3]]))),
        8 => Some(f64::from_be_bytes(d.try_into().ok()?)),
        _ => None,
    }
}

fn text(d: &[u8]) -> String {
    String::from_utf8_lossy(d).trim_end_matches('\0').to_owned()
}

/// Matroska and WebM.
fn matroska(data: &[u8]) -> Result<VideoInfo, String> {
    let top = elements(data);
    let header = top.iter().find(|(id, _)| *id == 0x1A45DFA3).ok_or("not a Matroska/WebM file")?;
    let doc_type = elements(header.1)
        .into_iter()
        .find(|(id, _)| *id == 0x4282)
        .map(|(_, v)| text(v))
        .unwrap_or_default();
    let mut info = VideoInfo {
        container: if doc_type == "webm" { "WebM".into() } else { "Matroska".into() },
        ..VideoInfo::default()
    };
    let segment = top.iter().find(|(id, _)| *id == 0x18538067).ok_or("the file has no segment")?.1;
    let mut scale = 1_000_000u64;
    let mut duration = None;
    for (id, payload) in elements(segment) {
        match id {
            0x1549A966 => {
                for (id, v) in elements(payload) {
                    match id {
                        0x2AD7B1 => scale = uint(v).max(1),
                        0x4489 => duration = float(v),
                        0x7BA9 => info.title = text(v),
                        0x5741 => info.writer = text(v),
                        _ => {}
                    }
                }
            }
            0x1654AE6B => {
                for (id, entry) in elements(payload) {
                    if id != 0xAE {
                        continue;
                    }
                    let mut track = Track::default();
                    for (id, v) in elements(entry) {
                        match id {
                            0x83 => {
                                track.kind = match uint(v) {
                                    1 => "video",
                                    2 => "audio",
                                    17 => "subtitles",
                                    _ => "other",
                                }
                                .into()
                            }
                            0x86 => {
                                track.codec = text(v)
                                    .trim_start_matches("V_")
                                    .trim_start_matches("A_")
                                    .trim_start_matches("S_")
                                    .to_owned()
                            }
                            0x22B59C => track.language = text(v),
                            0x23E383 => {
                                let ns = uint(v);
                                if ns > 0 {
                                    track.fps = Some((1e9 / ns as f64 * 1000.0).round() / 1000.0);
                                }
                            }
                            0xE0 => {
                                let (mut w, mut h) = (0, 0);
                                for (id, v) in elements(v) {
                                    match id {
                                        0xB0 => w = uint(v) as u32,
                                        0xBA => h = uint(v) as u32,
                                        _ => {}
                                    }
                                }
                                if w > 0 && h > 0 {
                                    track.size = Some((w, h));
                                }
                            }
                            0xE1 => {
                                for (id, v) in elements(v) {
                                    match id {
                                        0xB5 => track.rate = float(v).map(|r| r as u32),
                                        0x9F => track.channels = Some(uint(v) as u32),
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    info.tracks.push(track);
                }
            }
            // Pictures and sound come after the headers.
            0x1F43B675 => break,
            _ => {}
        }
    }
    info.duration = duration.map(|d| d * scale as f64 / 1e9);
    Ok(info)
}

// ------------------------------------------------------------------ AVI

/// AVI (RIFF).
fn avi(data: &[u8]) -> Result<VideoInfo, String> {
    if data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"AVI ") {
        return Err("not an AVI file".into());
    }
    let mut info = VideoInfo { container: "AVI".into(), ..VideoInfo::default() };
    let mut micros = 0u32;
    let mut frames = 0u32;
    // Walk the chunks of the header list.
    fn walk(
        d: &[u8],
        info: &mut VideoInfo,
        micros: &mut u32,
        frames: &mut u32,
        current: &mut Track,
    ) {
        let mut at = 0;
        while at + 8 <= d.len() {
            let id = &d[at..at + 4];
            let size = le32(d, at + 4).unwrap_or(0) as usize;
            let start = at + 8;
            let end = (start + size).min(d.len());
            let body = &d[start..end];
            match id {
                b"LIST" => {
                    let kind = body.get(..4).unwrap_or(b"");
                    if kind == b"hdrl" || kind == b"strl" {
                        if kind == b"strl" {
                            *current = Track::default();
                        }
                        walk(&body[4..], info, micros, frames, current);
                        if kind == b"strl" && !current.kind.is_empty() {
                            info.tracks.push(std::mem::take(current));
                        }
                    }
                }
                b"avih" => {
                    *micros = le32(body, 0).unwrap_or(0);
                    *frames = le32(body, 16).unwrap_or(0);
                    if info.tracks.is_empty() {
                        // The overall size is repeated by the video stream.
                    }
                }
                b"strh" => {
                    let kind = fourcc(body.get(..4).unwrap_or(b""));
                    current.kind = match kind.as_str() {
                        "vids" => "video",
                        "auds" => "audio",
                        "txts" => "subtitles",
                        _ => "other",
                    }
                    .into();
                    if current.kind == "video" {
                        current.codec = fourcc(body.get(4..8).unwrap_or(b"")).trim().to_owned();
                        let (scale, rate) =
                            (le32(body, 20).unwrap_or(0), le32(body, 24).unwrap_or(0));
                        if scale > 0 && rate > 0 {
                            current.fps = Some(
                                (f64::from(rate) / f64::from(scale) * 1000.0).round() / 1000.0,
                            );
                        }
                    }
                }
                b"strf" => {
                    if current.kind == "video" {
                        if let (Some(w), Some(h)) = (le32(body, 4), le32(body, 8)) {
                            current.size = Some((w, (h as i32).unsigned_abs()));
                        }
                        let comp = fourcc(body.get(16..20).unwrap_or(b""));
                        if !comp.trim_matches(|c: char| c == '?' || c == ' ').is_empty() {
                            current.codec = comp.trim().to_owned();
                        }
                    } else if current.kind == "audio" {
                        let tag = le16(body, 0).unwrap_or(0);
                        current.codec = match tag {
                            0x0001 => "PCM",
                            0x0055 => "MP3",
                            0x2000 => "AC-3",
                            0x00ff | 0x1610 => "AAC",
                            0x0161 | 0x0162 => "WMA",
                            _ => "audio",
                        }
                        .into();
                        current.channels = le16(body, 2).map(u32::from);
                        current.rate = le32(body, 4);
                    }
                }
                _ => {}
            }
            at = end + (size & 1);
        }
    }
    let mut current = Track::default();
    walk(&data[12..], &mut info, &mut micros, &mut frames, &mut current);
    if micros > 0 && frames > 0 {
        info.duration = Some(f64::from(micros) * f64::from(frames) / 1e6);
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn mp4_gives_duration_and_tracks() {
        let mut mvhd = vec![0u8; 4];
        mvhd.extend_from_slice(&[0; 8]);
        mvhd.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        mvhd.extend_from_slice(&90_000u32.to_be_bytes()); // 90 s
        mvhd.extend_from_slice(&[0; 80]);
        let mut tkhd = vec![0u8; 76];
        tkhd.extend_from_slice(&(1920u32 << 16).to_be_bytes());
        tkhd.extend_from_slice(&(1080u32 << 16).to_be_bytes());
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"vide");
        hdlr.extend_from_slice(&[0; 12]);
        let mut mdhd = vec![0u8; 12];
        mdhd.extend_from_slice(&25u32.to_be_bytes());
        mdhd.extend_from_slice(&2250u32.to_be_bytes()); // 90 s
        mdhd.extend_from_slice(&0x55c4u16.to_be_bytes()); // und
        mdhd.extend_from_slice(&[0; 2]);
        let mut avc = vec![0u8; 78];
        avc[0] = 0;
        let mut stsd = vec![0u8; 8];
        stsd.extend_from_slice(&mp4_box(b"avc1", &avc));
        let mut stts = vec![0u8; 4];
        stts.extend_from_slice(&1u32.to_be_bytes());
        stts.extend_from_slice(&2250u32.to_be_bytes());
        stts.extend_from_slice(&1u32.to_be_bytes());
        let stbl = [mp4_box(b"stsd", &stsd), mp4_box(b"stts", &stts)].concat();
        let minf = mp4_box(b"stbl", &stbl);
        let mdia =
            [mp4_box(b"mdhd", &mdhd), mp4_box(b"hdlr", &hdlr), mp4_box(b"minf", &minf)].concat();
        let trak = [mp4_box(b"tkhd", &tkhd), mp4_box(b"mdia", &mdia)].concat();
        let moov = [mp4_box(b"mvhd", &mvhd), mp4_box(b"trak", &trak)].concat();
        let file = [mp4_box(b"ftyp", b"isom\0\0\0\0isom"), mp4_box(b"moov", &moov)].concat();
        let info = mp4(&file, true).unwrap();
        assert_eq!(info.container, "MP4 (isom)");
        assert_eq!(info.duration, Some(90.0));
        assert_eq!(info.tracks.len(), 1);
        assert_eq!(info.tracks[0].kind, "video");
        assert_eq!(info.tracks[0].codec, "avc1");
        assert_eq!(info.tracks[0].size, Some((1920, 1080)));
        assert_eq!(info.tracks[0].fps, Some(25.0));
        let blocks = video("a.mp4", &file, true).unwrap();
        let text = format!("{blocks:?}");
        assert!(text.contains("1:30") && text.contains("1920×1080"), "{text}");
    }

    #[test]
    fn mp4_with_the_header_at_the_end_explains_itself() {
        let file = mp4_box(b"ftyp", b"isom\0\0\0\0isom");
        assert!(mp4(&file, false).unwrap_err().contains("end of this large file"));
        assert!(mp4(b"nothing here", true).is_err());
    }

    fn element(id: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        assert!(payload.len() < 127);
        out.push(0x80 | payload.len() as u8);
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn matroska_gives_duration_and_tracks() {
        let info = [
            element(&[0x2A, 0xD7, 0xB1], &1_000_000u32.to_be_bytes()),
            element(&[0x44, 0x89], &120_000f32.to_be_bytes()),
            element(&[0x7B, 0xA9], b"My film"),
        ]
        .concat();
        let video = [
            element(&[0x83], &[1]),
            element(&[0x86], b"V_MPEG4/ISO/AVC"),
            element(
                &[0xE0],
                &[
                    element(&[0xB0], &1280u16.to_be_bytes()),
                    element(&[0xBA], &720u16.to_be_bytes()),
                ]
                .concat(),
            ),
        ]
        .concat();
        let audio = [
            element(&[0x83], &[2]),
            element(&[0x86], b"A_AAC"),
            element(&[0x22, 0xB5, 0x9C], b"eng"),
            element(
                &[0xE1],
                &[element(&[0xB5], &48_000f32.to_be_bytes()), element(&[0x9F], &[2])].concat(),
            ),
        ]
        .concat();
        let tracks = [element(&[0xAE], &video), element(&[0xAE], &audio)].concat();
        let segment = [
            element(&[0x15, 0x49, 0xA9, 0x66], &info),
            element(&[0x16, 0x54, 0xAE, 0x6B], &tracks),
        ]
        .concat();
        let file = [
            element(&[0x1A, 0x45, 0xDF, 0xA3], &element(&[0x42, 0x82], b"matroska")),
            element(&[0x18, 0x53, 0x80, 0x67], &segment),
        ]
        .concat();
        let info = matroska(&file).unwrap();
        assert_eq!(info.container, "Matroska");
        assert_eq!(info.title, "My film");
        assert_eq!(info.duration, Some(120.0));
        assert_eq!(info.tracks[0].codec, "MPEG4/ISO/AVC");
        assert_eq!(info.tracks[0].size, Some((1280, 720)));
        assert_eq!((info.tracks[1].rate, info.tracks[1].channels), (Some(48_000), Some(2)));
        assert_eq!(info.tracks[1].language, "eng");
    }

    #[test]
    fn avi_gives_length_and_picture_size() {
        let mut avih = vec![0u8; 56];
        avih[0..4].copy_from_slice(&40_000u32.to_le_bytes());
        avih[16..20].copy_from_slice(&250u32.to_le_bytes());
        let chunk = |id: &[u8; 4], body: &[u8]| {
            let mut c = id.to_vec();
            c.extend_from_slice(&(body.len() as u32).to_le_bytes());
            c.extend_from_slice(body);
            c
        };
        let mut strh = vec![0u8; 56];
        strh[0..4].copy_from_slice(b"vids");
        strh[4..8].copy_from_slice(b"XVID");
        strh[20..24].copy_from_slice(&1u32.to_le_bytes());
        strh[24..28].copy_from_slice(&25u32.to_le_bytes());
        let mut strf = vec![0u8; 40];
        strf[4..8].copy_from_slice(&640u32.to_le_bytes());
        strf[8..12].copy_from_slice(&480u32.to_le_bytes());
        strf[16..20].copy_from_slice(b"XVID");
        let strl = [b"strl".to_vec(), chunk(b"strh", &strh), chunk(b"strf", &strf)].concat();
        let hdrl = [b"hdrl".to_vec(), chunk(b"avih", &avih), chunk(b"LIST", &strl)].concat();
        let body = [b"AVI ".to_vec(), chunk(b"LIST", &hdrl)].concat();
        let file = [b"RIFF".to_vec(), (body.len() as u32).to_le_bytes().to_vec(), body].concat();
        let info = avi(&file).unwrap();
        assert_eq!(info.duration, Some(10.0));
        assert_eq!(info.tracks[0].size, Some((640, 480)));
        assert_eq!(info.tracks[0].codec, "XVID");
        assert_eq!(info.tracks[0].fps, Some(25.0));
    }

    #[test]
    fn audio_needs_real_audio() {
        assert!(audio("a.mp3", b"this is not audio at all", true).is_err());
        assert!(is_audio_name("x.FLAC") && is_video_name("y.MKV") && !is_audio_name("z.txt"));
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(59.6), "1:00");
    }

    #[test]
    fn a_wav_file_is_described() {
        // 1 s of silence, 8 kHz, 16 bit mono.
        let samples = vec![0u8; 16_000];
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        wav.extend_from_slice(&samples);
        let blocks = audio("silence.wav", &wav, true).unwrap();
        let text = format!("{blocks:?}");
        assert!(text.contains("8000 Hz") && text.contains("0:01"), "{text}");
    }
}
