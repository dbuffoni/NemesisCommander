//! Binary files as an information sheet instead of a hex dump: what the
//! file is (from a dictionary of known formats, by its first bytes and then
//! by its name), what its header says (ELF, PE, Mach-O, ZIP, gzip, SQLite,
//! Java classes, WebAssembly, fonts, tar, ISO) and how its bytes look
//! (entropy, zeros). The hex dump stays one key away.
//!
//! Everything is read from the bytes given: nothing is run or decompressed.

use crate::rich::{Block, BlockKind};

/// A line of the sheet: name and value.
type Rows = Vec<Vec<String>>;

/// A known format: how to recognise it and what to say about it.
struct Format {
    /// The bytes it starts with, at `offset`.
    magic: &'static [u8],
    /// Where `magic` is.
    offset: usize,
    /// Its name.
    name: &'static str,
    /// What it is and what it is for.
    about: &'static str,
    /// A group for the sheet (`Executable`, `Archive`...).
    group: &'static str,
}

const fn f(
    magic: &'static [u8],
    offset: usize,
    name: &'static str,
    group: &'static str,
    about: &'static str,
) -> Format {
    Format { magic, offset, name, about, group }
}

/// The dictionary of formats known by their first bytes. The most specific
/// signatures come first.
const FORMATS: &[Format] = &[
    f(
        b"\x7fELF",
        0,
        "ELF",
        "Executable",
        "Executable, library or object file of Linux and Unix systems.",
    ),
    f(
        b"MZ",
        0,
        "DOS/Windows executable (PE)",
        "Executable",
        "Program or library (.exe, .dll, .sys) for Windows.",
    ),
    f(
        b"\xca\xfe\xba\xbe",
        0,
        "Java class or Mach-O universal binary",
        "Executable",
        "Compiled Java class, or a macOS program for several processors.",
    ),
    f(
        b"\xfe\xed\xfa\xce",
        0,
        "Mach-O (32 bit)",
        "Executable",
        "Program or library of macOS and iOS.",
    ),
    f(
        b"\xfe\xed\xfa\xcf",
        0,
        "Mach-O (64 bit)",
        "Executable",
        "Program or library of macOS and iOS.",
    ),
    f(
        b"\xce\xfa\xed\xfe",
        0,
        "Mach-O (32 bit, little endian)",
        "Executable",
        "Program or library of macOS and iOS.",
    ),
    f(
        b"\xcf\xfa\xed\xfe",
        0,
        "Mach-O (64 bit, little endian)",
        "Executable",
        "Program or library of macOS and iOS.",
    ),
    f(
        b"\0asm",
        0,
        "WebAssembly module",
        "Executable",
        "Portable binary code that runs in browsers and runtimes.",
    ),
    f(
        b"#!",
        0,
        "Script",
        "Executable",
        "A program started by the interpreter named on its first line.",
    ),
    f(
        b"PK\x03\x04",
        0,
        "ZIP archive",
        "Archive",
        "Compressed archive; also the base of .jar, .apk, .docx, .xlsx, .epub and .odt files.",
    ),
    f(b"PK\x05\x06", 0, "ZIP archive (empty)", "Archive", "A ZIP archive without any file."),
    f(b"\x1f\x8b", 0, "gzip", "Compressed", "A file compressed with gzip (also .tar.gz, .tgz)."),
    f(b"BZh", 0, "bzip2", "Compressed", "A file compressed with bzip2."),
    f(b"\xfd7zXZ\0", 0, "xz", "Compressed", "A file compressed with xz / LZMA2."),
    f(b"\x28\xb5\x2f\xfd", 0, "Zstandard", "Compressed", "A file compressed with zstd."),
    f(b"\x04\x22\x4d\x18", 0, "LZ4 frame", "Compressed", "A file compressed with LZ4."),
    f(b"7z\xbc\xaf\x27\x1c", 0, "7-Zip archive", "Archive", "Archive made by 7-Zip."),
    f(b"Rar!\x1a\x07", 0, "RAR archive", "Archive", "Archive made by WinRAR."),
    f(
        b"ustar",
        257,
        "tar archive",
        "Archive",
        "Unix tape archive: files joined one after the other.",
    ),
    f(b"!<arch>\n", 0, "ar archive", "Archive", "Static library (.a) or Debian package container."),
    f(
        b"\xed\xab\xee\xdb",
        0,
        "RPM package",
        "Archive",
        "Package of Red Hat, Fedora and SUSE systems.",
    ),
    f(
        b"SQLite format 3\0",
        0,
        "SQLite database",
        "Database",
        "A database in one file, used by many applications.",
    ),
    f(b"CD001", 0x8001, "ISO 9660 disk image", "Disk image", "Image of an optical disc (CD, DVD)."),
    f(b"\x89PNG\r\n\x1a\n", 0, "PNG image", "Image", "Lossless picture."),
    f(b"\xff\xd8\xff", 0, "JPEG image", "Image", "Photograph, lossy compression."),
    f(b"GIF8", 0, "GIF image", "Image", "Picture, possibly animated."),
    f(b"BM", 0, "BMP image", "Image", "Uncompressed Windows bitmap."),
    f(b"II*\0", 0, "TIFF image", "Image", "Picture format of scanners and cameras."),
    f(b"MM\0*", 0, "TIFF image (big endian)", "Image", "Picture format of scanners and cameras."),
    f(b"\0\0\x01\0", 0, "Windows icon", "Image", "Icon with one or more sizes."),
    f(b"8BPS", 0, "Photoshop document", "Image", "Layered picture of Adobe Photoshop."),
    f(
        b"RIFF",
        0,
        "RIFF container",
        "Media",
        "Container of WAV audio, AVI video, WebP pictures and more.",
    ),
    f(b"OggS", 0, "Ogg container", "Media", "Container of Vorbis, Opus or Theora streams."),
    f(b"fLaC", 0, "FLAC audio", "Media", "Lossless audio."),
    f(b"ID3", 0, "MP3 audio (with tags)", "Media", "Compressed audio with ID3 tags."),
    f(b"\x1a\x45\xdf\xa3", 0, "Matroska / WebM", "Media", "Video container (.mkv, .webm)."),
    f(b"%PDF-", 0, "PDF document", "Document", "Portable document."),
    f(
        b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1",
        0,
        "OLE compound file",
        "Document",
        "Old Microsoft Office document (.doc, .xls, .ppt, .msi).",
    ),
    f(b"{\\rtf", 0, "Rich Text Format", "Document", "Formatted text readable by word processors."),
    f(b"\0\x01\0\0\0", 0, "TrueType font", "Font", "Outline font (.ttf)."),
    f(b"OTTO", 0, "OpenType font", "Font", "Outline font with PostScript outlines (.otf)."),
    f(b"wOFF", 0, "WOFF font", "Font", "Font compressed for web pages."),
    f(b"wOF2", 0, "WOFF2 font", "Font", "Font compressed for web pages."),
    f(b"-----BEGIN", 0, "PEM file", "Security", "Certificate or key written as text."),
    f(b"\x30\x82", 0, "DER data", "Security", "Certificate or key in binary ASN.1 form."),
    f(b"\x03\xf3\x0d\x0a", 0, "Python bytecode", "Executable", "Compiled Python module (.pyc)."),
    f(b"\xa7\x0d\x0d\x0a", 0, "Python bytecode", "Executable", "Compiled Python module (.pyc)."),
    f(b"dex\n", 0, "Android Dalvik code", "Executable", "Code of an Android application."),
    f(b"\x93NUMPY", 0, "NumPy array", "Data", "Array saved by NumPy (.npy)."),
    f(b"\x89HDF\r\n\x1a\n", 0, "HDF5 data", "Data", "Scientific data file."),
    f(b"PAR1", 0, "Parquet data", "Data", "Columnar table of data."),
    f(b"MDMP", 0, "Windows memory dump", "Data", "Crash dump of a Windows program."),
    f(
        b"\x03\xd9\xa2\x9a\x67\xfb\x4b\xb5",
        0,
        "KeePass 2 database",
        "Security",
        "Database of KeePass.",
    ),
];

/// What the extension says when the bytes say nothing.
const EXTENSIONS: &[(&str, &str, &str)] = &[
    ("exe", "Windows program", "Executable"),
    ("dll", "Windows library", "Executable"),
    ("so", "Shared library", "Executable"),
    ("dylib", "macOS library", "Executable"),
    ("o", "Object file", "Executable"),
    ("obj", "Object file", "Executable"),
    ("a", "Static library", "Executable"),
    ("lib", "Static library", "Executable"),
    ("class", "Java class", "Executable"),
    ("jar", "Java archive", "Archive"),
    ("apk", "Android application", "Archive"),
    ("deb", "Debian package", "Archive"),
    ("rpm", "RPM package", "Archive"),
    ("iso", "Disk image", "Disk image"),
    ("img", "Disk image", "Disk image"),
    ("qcow2", "Virtual machine disk (QEMU)", "Disk image"),
    ("vmdk", "Virtual machine disk (VMware)", "Disk image"),
    ("vdi", "Virtual machine disk (VirtualBox)", "Disk image"),
    ("db", "Database", "Database"),
    ("sqlite", "SQLite database", "Database"),
    ("sqlite3", "SQLite database", "Database"),
    ("mdb", "Access database", "Database"),
    ("dat", "Data file of an application", "Data"),
    ("bin", "Raw binary data", "Data"),
    ("dump", "Dump of data or memory", "Data"),
    ("dmp", "Memory dump", "Data"),
    ("pyc", "Python bytecode", "Executable"),
    ("wasm", "WebAssembly module", "Executable"),
    ("ttf", "TrueType font", "Font"),
    ("otf", "OpenType font", "Font"),
    ("woff", "Web font", "Font"),
    ("woff2", "Web font", "Font"),
    ("ico", "Icon", "Image"),
    ("psd", "Photoshop document", "Image"),
    ("heic", "HEIF photograph", "Image"),
    ("avif", "AVIF picture", "Image"),
    ("tif", "TIFF picture", "Image"),
    ("tiff", "TIFF picture", "Image"),
    ("p12", "Certificate with private key", "Security"),
    ("pfx", "Certificate with private key", "Security"),
    ("der", "Certificate or key (binary)", "Security"),
    ("kdbx", "Password database", "Security"),
    ("gpg", "Encrypted file", "Security"),
    ("enc", "Encrypted file", "Security"),
    ("npy", "NumPy array", "Data"),
    ("parquet", "Parquet table", "Data"),
    ("pkl", "Python pickle", "Data"),
    ("onnx", "Machine learning model", "Data"),
    ("safetensors", "Machine learning weights", "Data"),
    ("gguf", "Machine learning model", "Data"),
];

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

/// The format `data` starts with, by the dictionary: `(name, group, about)`.
pub fn recognise(data: &[u8]) -> Option<(&'static str, &'static str, &'static str)> {
    FORMATS
        .iter()
        .find(|f| data.get(f.offset..f.offset + f.magic.len()) == Some(f.magic))
        .map(|f| (f.name, f.group, f.about))
}

/// Whether the dictionary knows `data` as a picture (the viewer shows
/// those as pictures when it can decode them).
pub fn is_picture(data: &[u8]) -> bool {
    recognise(data).is_some_and(|(_, group, _)| group == "Image")
}

fn row(rows: &mut Rows, name: &str, value: impl Into<String>) {
    let value = value.into();
    if !value.trim().is_empty() {
        rows.push(vec![name.to_owned(), value]);
    }
}

/// `1234567` as `1.2 MiB (1234567 bytes)`.
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{value:.1} {} ({bytes} bytes)", UNITS[unit])
    }
}

/// Shannon entropy of `data` in bits per byte (0 to 8).
pub fn entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let total = data.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total;
            -p * p.log2()
        })
        .sum()
}

fn u16_at(data: &[u8], at: usize, big: bool) -> Option<u16> {
    let b: [u8; 2] = data.get(at..at + 2)?.try_into().ok()?;
    Some(if big { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
}

fn u32_at(data: &[u8], at: usize, big: bool) -> Option<u32> {
    let b: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
    Some(if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
}

fn u64_at(data: &[u8], at: usize, big: bool) -> Option<u64> {
    let b: [u8; 8] = data.get(at..at + 8)?.try_into().ok()?;
    Some(if big { u64::from_be_bytes(b) } else { u64::from_le_bytes(b) })
}

// ------------------------------------------------------------------ ELF

fn elf(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    let big = data.get(5) == Some(&2);
    row(&mut rows, "Word size", if data.get(4) == Some(&2) { "64 bit" } else { "32 bit" });
    row(&mut rows, "Byte order", if big { "big endian" } else { "little endian" });
    row(
        &mut rows,
        "Operating system",
        match data.get(7) {
            Some(0) => "System V (Linux, Unix)",
            Some(3) => "Linux",
            Some(6) => "Solaris",
            Some(9) => "FreeBSD",
            _ => "",
        },
    );
    row(
        &mut rows,
        "Kind",
        match u16_at(data, 16, big) {
            Some(1) => "relocatable object (.o)",
            Some(2) => "executable",
            Some(3) => "shared object (library or position independent program)",
            Some(4) => "core dump",
            _ => "",
        },
    );
    row(
        &mut rows,
        "Processor",
        match u16_at(data, 18, big) {
            Some(0x03) => "x86",
            Some(0x3e) => "x86-64",
            Some(0x28) => "ARM",
            Some(0xb7) => "AArch64 (ARM 64 bit)",
            Some(0xf3) => "RISC-V",
            Some(0x08) => "MIPS",
            Some(0x14) => "PowerPC",
            Some(0x15) => "PowerPC 64 bit",
            Some(0x16) => "IBM S/390",
            _ => "",
        },
    );
    let entry = if data.get(4) == Some(&2) {
        u64_at(data, 24, big)
    } else {
        u32_at(data, 24, big).map(u64::from)
    };
    row(&mut rows, "Entry point", entry.map(|e| format!("0x{e:x}")).unwrap_or_default());
    rows
}

// ------------------------------------------------------------------- PE

fn pe(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    let Some(at) = u32_at(data, 0x3c, false).map(|v| v as usize) else { return rows };
    if data.get(at..at + 4) != Some(b"PE\0\0") {
        row(&mut rows, "Kind", "old DOS program");
        return rows;
    }
    row(
        &mut rows,
        "Processor",
        match u16_at(data, at + 4, false) {
            Some(0x14c) => "x86",
            Some(0x8664) => "x86-64",
            Some(0x1c0) | Some(0x1c4) => "ARM",
            Some(0xaa64) => "ARM64",
            _ => "",
        },
    );
    row(
        &mut rows,
        "Sections",
        u16_at(data, at + 6, false).map(|n| n.to_string()).unwrap_or_default(),
    );
    if let Some(stamp) = u32_at(data, at + 8, false).filter(|s| *s > 0) {
        row(&mut rows, "Built", date_from_unix(i64::from(stamp)));
    }
    let characteristics = u16_at(data, at + 22, false).unwrap_or(0);
    row(&mut rows, "Kind", if characteristics & 0x2000 != 0 { "library (DLL)" } else { "program" });
    let optional = at + 24;
    let subsystem = u16_at(data, optional + 68, false);
    row(
        &mut rows,
        "Subsystem",
        match subsystem {
            Some(1) => "native (driver)",
            Some(2) => "window application",
            Some(3) => "console application",
            _ => "",
        },
    );
    rows
}

// ---------------------------------------------------------------- Mach-O

fn macho(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    let big = matches!(data.get(..4), Some(b"\xfe\xed\xfa\xce" | b"\xfe\xed\xfa\xcf"));
    row(
        &mut rows,
        "Processor",
        match u32_at(data, 4, big).map(|c| c & 0xff_ffff) {
            Some(7) => "x86 / x86-64",
            Some(12) => "ARM / ARM64",
            Some(18) => "PowerPC",
            _ => "",
        },
    );
    row(
        &mut rows,
        "Kind",
        match u32_at(data, 12, big) {
            Some(1) => "object file",
            Some(2) => "executable",
            Some(6) => "dynamic library",
            Some(8) => "bundle",
            _ => "",
        },
    );
    rows
}

// ------------------------------------------------------------------ ZIP

/// Entries of a ZIP file, read from its central directory, when the whole
/// file is there.
fn zip_entries(data: &[u8]) -> Option<(usize, Vec<String>)> {
    let search_from = data.len().saturating_sub(65_557);
    let eocd = data[search_from..].windows(4).rposition(|w| w == b"PK\x05\x06")? + search_from;
    let total = u16_at(data, eocd + 10, false)? as usize;
    let mut at = u32_at(data, eocd + 16, false)? as usize;
    let mut names = Vec::new();
    for _ in 0..total.min(10_000) {
        if data.get(at..at + 4) != Some(b"PK\x01\x02") {
            break;
        }
        let name_len = u16_at(data, at + 28, false)? as usize;
        let extra = u16_at(data, at + 30, false)? as usize;
        let comment = u16_at(data, at + 32, false)? as usize;
        let name = data.get(at + 46..at + 46 + name_len)?;
        if names.len() < 8 {
            names.push(String::from_utf8_lossy(name).into_owned());
        }
        at += 46 + name_len + extra + comment;
    }
    Some((total, names))
}

fn zip(data: &[u8], complete: bool) -> Rows {
    let mut rows = Rows::new();
    if !complete {
        row(&mut rows, "Entries", "the file is longer than what was read");
        return rows;
    }
    if let Some((total, names)) = zip_entries(data) {
        row(&mut rows, "Entries", total.to_string());
        row(&mut rows, "First entries", names.join(", "));
        let flavour = if names.iter().any(|n| n == "AndroidManifest.xml") {
            "Android application"
        } else if names.iter().any(|n| n == "META-INF/MANIFEST.MF") {
            "Java archive"
        } else if names.iter().any(|n| n == "mimetype") {
            "OpenDocument or EPUB package"
        } else if names.iter().any(|n| n.starts_with("word/")) {
            "Word document"
        } else if names.iter().any(|n| n.starts_with("xl/")) {
            "Excel workbook"
        } else if names.iter().any(|n| n.starts_with("ppt/")) {
            "PowerPoint presentation"
        } else {
            ""
        };
        row(&mut rows, "Looks like", flavour);
    }
    rows
}

// ----------------------------------------------------------------- gzip

fn gzip(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    row(&mut rows, "Method", if data.get(2) == Some(&8) { "deflate" } else { "" });
    if let Some(stamp) = u32_at(data, 4, false).filter(|s| *s > 0) {
        row(&mut rows, "Date of the file", date_from_unix(i64::from(stamp)));
    }
    row(
        &mut rows,
        "System",
        match data.get(9) {
            Some(0) => "FAT",
            Some(3) => "Unix",
            Some(7) => "Macintosh",
            Some(11) => "NTFS",
            _ => "",
        },
    );
    if data.get(3).is_some_and(|flags| flags & 0x08 != 0) {
        // The name follows the 10 byte header (and the extra field, if any).
        let mut at = 10;
        if data.get(3).is_some_and(|flags| flags & 0x04 != 0) {
            at += 2 + u16_at(data, at, false).unwrap_or(0) as usize;
        }
        if let Some(rest) = data.get(at..)
            && let Some(end) = rest.iter().position(|&b| b == 0)
        {
            row(&mut rows, "Original name", String::from_utf8_lossy(&rest[..end]).into_owned());
        }
    }
    rows
}

// --------------------------------------------------------------- SQLite

fn sqlite(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    let page = u16_at(data, 16, true).map(|p| if p == 1 { 65_536 } else { u32::from(p) });
    row(&mut rows, "Page size", page.map(|p| format!("{p} bytes")).unwrap_or_default());
    if let Some(pages) = u32_at(data, 28, true).filter(|p| *p > 0) {
        row(&mut rows, "Pages", pages.to_string());
    }
    row(
        &mut rows,
        "Text encoding",
        match u32_at(data, 56, true) {
            Some(1) => "UTF-8",
            Some(2) => "UTF-16 little endian",
            Some(3) => "UTF-16 big endian",
            _ => "",
        },
    );
    row(
        &mut rows,
        "User version",
        u32_at(data, 60, true).map(|v| v.to_string()).unwrap_or_default(),
    );
    row(
        &mut rows,
        "Write mode",
        if data.get(18) == Some(&2) { "write-ahead log (WAL)" } else { "rollback journal" },
    );
    rows
}

// ------------------------------------------------------------ Java class

fn java_class(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    if let Some(major) = u16_at(data, 6, true).filter(|m| (45..=80).contains(m)) {
        row(&mut rows, "Class format", format!("version {major}"));
        row(
            &mut rows,
            "Needs Java",
            match major {
                45 => "1.1".to_owned(),
                46..=52 => format!("1.{}", major - 44),
                m => (m - 44).to_string(),
            },
        );
    }
    rows
}

fn wasm(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    row(&mut rows, "Version", u32_at(data, 4, false).map(|v| v.to_string()).unwrap_or_default());
    rows
}

fn font(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    if matches!(data.get(..4), Some(b"wOFF" | b"wOF2")) {
        row(&mut rows, "Tables", u16_at(data, 12, true).map(|n| n.to_string()).unwrap_or_default());
    } else {
        row(&mut rows, "Tables", u16_at(data, 4, true).map(|n| n.to_string()).unwrap_or_default());
    }
    rows
}

fn tar(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    let name = data.get(..100).map(|n| {
        let end = n.iter().position(|&b| b == 0).unwrap_or(n.len());
        String::from_utf8_lossy(&n[..end]).into_owned()
    });
    row(&mut rows, "First entry", name.unwrap_or_default());
    rows
}

fn iso(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    if let Some(label) = data.get(0x8028..0x8048) {
        row(&mut rows, "Volume name", String::from_utf8_lossy(label).trim().to_owned());
    }
    rows
}

/// A date for a Unix time (UTC), without any time library.
fn date_from_unix(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", rest / 3600, rest / 60 % 60)
}

/// The header details of a format, by its name in the dictionary.
fn details(name: &str, data: &[u8], complete: bool) -> Rows {
    match name {
        "ELF" => elf(data),
        "DOS/Windows executable (PE)" => pe(data),
        n if n.starts_with("Mach-O") => macho(data),
        "Java class or Mach-O universal binary" => {
            // A class has a small major version after the magic number.
            let mut rows = java_class(data);
            if rows.is_empty() {
                row(
                    &mut rows,
                    "Number of processors",
                    u32_at(data, 4, true).map(|n| n.to_string()).unwrap_or_default(),
                );
            }
            rows
        }
        "WebAssembly module" => wasm(data),
        "ZIP archive" => zip(data, complete),
        "gzip" => gzip(data),
        "SQLite database" => sqlite(data),
        "TrueType font" | "OpenType font" | "WOFF font" | "WOFF2 font" => font(data),
        "tar archive" => tar(data),
        "ISO 9660 disk image" => iso(data),
        _ => Rows::new(),
    }
}

/// How the bytes look, from a sample of the file.
fn content_rows(data: &[u8]) -> Rows {
    let mut rows = Rows::new();
    let sample = &data[..data.len().min(1 << 20)];
    let entropy = entropy(sample);
    let verdict = if entropy > 7.5 {
        "very high: compressed or encrypted data"
    } else if entropy > 6.0 {
        "high: packed or dense binary data"
    } else if entropy > 3.0 {
        "medium: ordinary binary data"
    } else {
        "low: repetitive data"
    };
    row(&mut rows, "Randomness", format!("{entropy:.2} of 8 bits per byte, {verdict}"));
    let zeros = sample.iter().filter(|&&b| b == 0).count();
    row(
        &mut rows,
        "Zero bytes",
        format!("{:.0}%", zeros as f64 * 100.0 / sample.len().max(1) as f64),
    );
    let printable = sample.iter().filter(|b| b.is_ascii_graphic() || **b == b' ').count();
    row(
        &mut rows,
        "Readable characters",
        format!("{:.0}%", printable as f64 * 100.0 / sample.len().max(1) as f64),
    );
    let first: Vec<String> = data.iter().take(16).map(|b| format!("{b:02x}")).collect();
    row(&mut rows, "First bytes", first.join(" "));
    rows
}

/// Readable strings in `data` (at least 6 characters), the first few.
fn strings(data: &[u8], count: usize) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = String::new();
    for &b in data.iter().take(1 << 20) {
        if b.is_ascii_graphic() || b == b' ' {
            current.push(b as char);
        } else {
            if current.len() >= 6 {
                found.push(std::mem::take(&mut current));
                if found.len() == count {
                    return found;
                }
            }
            current.clear();
        }
    }
    if current.len() >= 6 && found.len() < count {
        found.push(current);
    }
    found
}

/// Whether the sheet is the right view for `data`: binary content. Callers
/// check for text and pictures first.
pub fn describe(
    name: &str,
    data: &[u8],
    total_size: Option<u64>,
    file_rows: Vec<Vec<String>>,
) -> Vec<Block> {
    let complete = total_size.is_none_or(|t| t as usize <= data.len());
    let known = recognise(data);
    let by_name = extension(name).and_then(|e| {
        EXTENSIONS.iter().find(|(x, _, _)| *x == e).map(|(_, what, group)| (*what, *group))
    });

    let title = match (known, by_name) {
        (Some((format, _, _)), _) => format.to_owned(),
        (None, Some((what, _))) => what.to_owned(),
        (None, None) => "Binary file".to_owned(),
    };
    let mut blocks = vec![Block::heading(1, title)];

    let mut file: Rows = vec![vec!["Name".to_owned(), name.to_owned()]];
    file.extend(file_rows);
    row(&mut file, "Size", total_size.map_or_else(|| size_text(data.len() as u64), size_text));
    blocks.push(Block::new(BlockKind::Table { rows: file, header: false }, Vec::new()));

    let mut kind: Rows = Vec::new();
    match (known, by_name) {
        (Some((format, group, about)), by_name) => {
            row(&mut kind, "Format", format);
            row(&mut kind, "Group", group);
            row(&mut kind, "What it is", about);
            if let Some((what, _)) = by_name.filter(|(what, _)| *what != format) {
                row(&mut kind, "By its name", what);
            }
        }
        (None, Some((what, group))) => {
            row(&mut kind, "By its name", what);
            row(&mut kind, "Group", group);
            row(&mut kind, "Note", "the contents do not start with a known signature");
        }
        (None, None) => {
            row(
                &mut kind,
                "Format",
                "not in the dictionary of known formats: no known signature, no known extension",
            );
        }
    }
    blocks.push(Block::heading(2, "What it is".to_owned()));
    blocks.push(Block::new(BlockKind::Table { rows: kind, header: false }, Vec::new()));

    if let Some((format, _, _)) = known {
        let header = details(format, data, complete);
        if !header.is_empty() {
            blocks.push(Block::heading(2, "What its header says".to_owned()));
            blocks.push(Block::new(BlockKind::Table { rows: header, header: false }, Vec::new()));
        }
    }

    blocks.push(Block::heading(2, "How the bytes look".to_owned()));
    blocks
        .push(Block::new(BlockKind::Table { rows: content_rows(data), header: false }, Vec::new()));

    let readable = strings(data, 6);
    if !readable.is_empty() {
        blocks.push(Block::heading(2, "Readable text inside".to_owned()));
        for line in readable {
            blocks.push(Block::paragraph(line));
        }
    }
    blocks.push(Block::paragraph("Press F4 (or h) for the hex dump of the bytes."));
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(blocks: &[Block]) -> String {
        let mut out = String::new();
        for block in blocks {
            for span in &block.spans {
                out.push_str(&span.text);
                out.push('\n');
            }
            if let BlockKind::Table { rows, .. } = &block.kind {
                for row in rows {
                    out.push_str(&row.join(": "));
                    out.push('\n');
                }
            }
        }
        out
    }

    fn elf_header() -> Vec<u8> {
        let mut data = vec![0u8; 64];
        data[..4].copy_from_slice(b"\x7fELF");
        data[4] = 2; // 64 bit
        data[5] = 1; // little endian
        data[16] = 3; // shared object
        data[18] = 0x3e; // x86-64
        data
    }

    #[test]
    fn the_dictionary_knows_signatures() {
        assert_eq!(recognise(b"\x7fELF\x02").map(|r| r.0), Some("ELF"));
        assert_eq!(recognise(b"PK\x03\x04rest").map(|r| r.0), Some("ZIP archive"));
        assert_eq!(recognise(b"SQLite format 3\0").map(|r| r.1), Some("Database"));
        assert!(recognise(b"nothing known here").is_none());
        assert!(is_picture(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_picture(b"\x7fELF"));
    }

    #[test]
    fn an_elf_sheet_names_the_processor() {
        let sheet = text(&describe("prog", &elf_header(), Some(64), Vec::new()));
        assert!(sheet.contains("ELF"), "{sheet}");
        assert!(sheet.contains("x86-64"), "{sheet}");
        assert!(sheet.contains("64 bit"), "{sheet}");
        assert!(sheet.contains("hex dump"), "{sheet}");
    }

    #[test]
    fn an_unknown_file_gets_the_look_of_its_bytes() {
        let data: Vec<u8> = (0..4096u32).map(|i| (i * 7919 % 251) as u8).collect();
        let sheet = text(&describe("mystery.xyz", &data, None, Vec::new()));
        assert!(sheet.contains("not in the dictionary"), "{sheet}");
        assert!(sheet.contains("bits per byte"), "{sheet}");
    }

    #[test]
    fn the_extension_names_files_without_a_signature() {
        let sheet = text(&describe("game.dat", &[1, 2, 3, 0, 5], None, Vec::new()));
        assert!(sheet.contains("Data file of an application"), "{sheet}");
    }

    #[test]
    fn zip_entries_are_listed() {
        // A stored empty file called a.txt in a ZIP file.
        let mut zip: Vec<u8> = Vec::new();
        let local = b"PK\x03\x04\x0a\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x05\0\0\0a.txt";
        zip.extend_from_slice(local);
        let central_at = zip.len() as u32;
        let mut central = b"PK\x01\x02\x14\0\x0a\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
        central.extend_from_slice(&5u16.to_le_bytes());
        central.extend_from_slice(&[0u8; 6 + 4 + 4 + 4]);
        // name length is at offset 28 of the central record
        central.resize(46, 0);
        central[28] = 5;
        central.extend_from_slice(b"a.txt");
        let central_len = central.len() as u32;
        zip.extend_from_slice(&central);
        zip.extend_from_slice(b"PK\x05\x06\0\0\0\0\x01\0\x01\0");
        zip.extend_from_slice(&central_len.to_le_bytes());
        zip.extend_from_slice(&central_at.to_le_bytes());
        zip.extend_from_slice(&[0, 0]);
        let sheet = text(&describe("x.zip", &zip, Some(zip.len() as u64), Vec::new()));
        assert!(sheet.contains("Entries: 1"), "{sheet}");
        assert!(sheet.contains("a.txt"), "{sheet}");
    }

    #[test]
    fn unix_dates_are_written_out() {
        assert_eq!(date_from_unix(0), "1970-01-01 00:00 UTC");
        assert_eq!(date_from_unix(1_700_000_000), "2023-11-14 22:13 UTC");
    }

    #[test]
    fn entropy_goes_from_zero_to_eight() {
        assert_eq!(entropy(&[7u8; 100]), 0.0);
        let all: Vec<u8> = (0..=255u8).cycle().take(2560).collect();
        assert!((entropy(&all) - 8.0).abs() < 0.01);
    }

    #[test]
    fn sizes_are_readable() {
        assert_eq!(size_text(10), "10 bytes");
        assert_eq!(size_text(2048), "2.0 KiB (2048 bytes)");
    }
}
