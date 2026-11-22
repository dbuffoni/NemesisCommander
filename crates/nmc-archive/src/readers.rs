//! Reading archives: building the index and extracting one member.
//!
//! Everything here is blocking and runs on tokio's blocking pool.

use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nmc_vfs::{Entry, EntryKind};

use crate::index::{ArchiveIndex, IndexEntry, Locator, normalize_member};
use crate::{ArchiveFormat, Compression, password, time};

fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

/// Reads the table of contents of `file`. `key` names the archive for its
/// password (see [`password`]).
pub(crate) fn build_index(
    file: &Path,
    format: ArchiveFormat,
    key: &str,
) -> io::Result<ArchiveIndex> {
    match format {
        ArchiveFormat::Zip => zip_index(file),
        ArchiveFormat::Tar(_) => tar_index(file),
        ArchiveFormat::SevenZ => sevenz_index(file, key),
        ArchiveFormat::Rar => rar_index(file),
        ArchiveFormat::Compressed(c) => single_index(file, c),
    }
}

/// Writes the data of one member to `out`.
pub(crate) fn extract(
    file: &Path,
    format: ArchiveFormat,
    item: &IndexEntry,
    out: &mut dyn Write,
    key: &str,
) -> io::Result<()> {
    match (&item.locator, format) {
        (Locator::Synthetic, _) => Err(io::Error::new(io::ErrorKind::IsADirectory, "directory")),
        (Locator::Zip(index), _) => zip_extract(file, *index, out, key),
        (Locator::Named(name), ArchiveFormat::Tar(_)) => tar_extract(file, name, out),
        (Locator::Named(name), ArchiveFormat::SevenZ) => sevenz_extract(file, name, out, key),
        (Locator::Named(name), ArchiveFormat::Rar) => rar_extract(file, name, out),
        (Locator::Single, ArchiveFormat::Compressed(c)) => {
            let mut decoder = c.decoder(BufReader::new(File::open(file)?))?;
            io::copy(&mut decoder, out).map(|_| ())
        }
        _ => Err(invalid("index does not match archive format")),
    }
}

// ---------------------------------------------------------------- zip

fn zip_time(t: zip::DateTime) -> Option<SystemTime> {
    time::from_civil(
        i64::from(t.year()),
        u32::from(t.month()),
        u32::from(t.day()),
        u32::from(t.hour()),
        u32::from(t.minute()),
        u32::from(t.second()),
    )
}

/// Writes member `number` of a zip file to `out`, decrypting it with the
/// archive's password when it is encrypted.
fn zip_extract(file: &Path, number: usize, out: &mut dyn Write, key: &str) -> io::Result<()> {
    use zip::result::ZipError;
    let mut archive = zip::ZipArchive::new(BufReader::new(File::open(file)?)).map_err(invalid)?;
    let encrypted = archive.by_index_raw(number).map_err(invalid)?.encrypted();
    let opened = if encrypted {
        match password::password_for(key) {
            Some(password) => archive.by_index_decrypt(number, password.as_bytes()),
            None => {
                password::lock(key);
                return Err(password::problem(false));
            }
        }
    } else {
        archive.by_index(number)
    };
    let mut member = match opened {
        Ok(member) => member,
        Err(ZipError::InvalidPassword) => {
            password::refuse(key);
            return Err(password::problem(true));
        }
        Err(ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)) => {
            password::lock(key);
            return Err(password::problem(false));
        }
        Err(e) => return Err(invalid(e)),
    };
    // With ZipCrypto a wrong password may only show while reading.
    io::copy(&mut member, out).map(|_| ()).map_err(|e| {
        if encrypted && e.kind() == io::ErrorKind::InvalidData {
            password::refuse(key);
            password::problem(true)
        } else {
            e
        }
    })
}

fn zip_index(file: &Path) -> io::Result<ArchiveIndex> {
    let mut archive = zip::ZipArchive::new(BufReader::new(File::open(file)?)).map_err(invalid)?;
    let mut index = ArchiveIndex::default();
    for i in 0..archive.len() {
        let member = archive.by_index_raw(i).map_err(invalid)?;
        index.locked |= member.encrypted();
        let name = member.name().map_err(invalid)?.into_owned();
        let kind = if member.is_dir() {
            EntryKind::Directory
        } else if member.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::File
        };
        let mut entry =
            Entry::new("", kind).with_size(if kind == EntryKind::File { member.size() } else { 0 });
        entry.modified = member.last_modified().and_then(zip_time);
        entry.permissions = member.unix_mode().map(|m| m & 0o7777);
        drop(member);
        if kind == EntryKind::Symlink {
            let mut target = String::new();
            archive.by_index(i).map_err(invalid)?.read_to_string(&mut target)?;
            entry.symlink_target = Some(target);
        }
        index.insert(&name, entry, Locator::Zip(i));
    }
    Ok(index)
}

// ---------------------------------------------------------------- tar

/// Opens a tar file, decompressing it according to its magic bytes.
fn open_tar(file: &Path) -> io::Result<tar::Archive<Box<dyn Read>>> {
    let mut head = [0u8; 8];
    let n = File::open(file)?.read(&mut head)?;
    let compression = Compression::from_magic(&head[..n]);
    let reader = compression.decoder(BufReader::new(File::open(file)?))?;
    Ok(tar::Archive::new(reader))
}

fn tar_index(file: &Path) -> io::Result<ArchiveIndex> {
    let mut archive = open_tar(file)?;
    let mut index = ArchiveIndex::default();
    for member in archive.entries()? {
        let member = member?;
        let header = member.header();
        let name = String::from_utf8_lossy(&member.path_bytes()).into_owned();
        let (kind, locator) = match header.entry_type() {
            tar::EntryType::Directory => (EntryKind::Directory, Locator::Named(name.clone())),
            tar::EntryType::Symlink => (EntryKind::Symlink, Locator::Named(name.clone())),
            // A hard link has no data of its own: read the target member.
            tar::EntryType::Link => {
                let target =
                    member.link_name_bytes().map(|b| String::from_utf8_lossy(&b).into_owned());
                (EntryKind::File, Locator::Named(target.unwrap_or_default()))
            }
            t if t.is_file()
                || t == tar::EntryType::Continuous
                || t == tar::EntryType::GNUSparse =>
            {
                (EntryKind::File, Locator::Named(name.clone()))
            }
            // PAX/GNU long-name headers are consumed by the tar crate; anything
            // else (devices, FIFOs) is listed but has no data.
            _ => (EntryKind::Other, Locator::Named(name.clone())),
        };
        let mut entry = Entry::new("", kind).with_size(if kind == EntryKind::File {
            header.size()?
        } else {
            0
        });
        entry.modified = header.mtime().ok().map(|s| UNIX_EPOCH + Duration::from_secs(s));
        entry.permissions = header.mode().ok().map(|m| m & 0o7777);
        entry.uid = header.uid().ok().and_then(|u| u32::try_from(u).ok());
        entry.gid = header.gid().ok().and_then(|g| u32::try_from(g).ok());
        if kind == EntryKind::Symlink {
            entry.symlink_target =
                member.link_name_bytes().map(|b| String::from_utf8_lossy(&b).into_owned());
        }
        index.insert(&name, entry, locator);
    }
    Ok(index)
}

fn tar_extract(file: &Path, name: &str, out: &mut dyn Write) -> io::Result<()> {
    let wanted = normalize_member(name);
    let mut archive = open_tar(file)?;
    for member in archive.entries()? {
        let mut member = member?;
        let path = String::from_utf8_lossy(&member.path_bytes()).into_owned();
        if normalize_member(&path) == wanted && member.header().entry_type().is_file() {
            io::copy(&mut member, out)?;
            return Ok(());
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, format!("{name} not found in archive")))
}

// ---------------------------------------------------------------- 7z

/// Converts an error of the 7z library, telling password problems apart.
fn sevenz_error(e: sevenz_rust2::Error, key: &str, had_password: bool) -> io::Error {
    match e {
        sevenz_rust2::Error::PasswordRequired => {
            password::lock(key);
            password::problem(false)
        }
        sevenz_rust2::Error::MaybeBadPassword(_) if had_password => {
            password::refuse(key);
            password::problem(true)
        }
        sevenz_rust2::Error::MaybeBadPassword(_) => {
            password::lock(key);
            password::problem(false)
        }
        other => invalid(other),
    }
}

fn sevenz_password(key: &str) -> (sevenz_rust2::Password, bool) {
    match password::password_for(key) {
        Some(text) => (sevenz_rust2::Password::from(text.as_str()), true),
        None => (sevenz_rust2::Password::empty(), false),
    }
}

fn sevenz_index(file: &Path, key: &str) -> io::Result<ArchiveIndex> {
    let (password, had) = sevenz_password(key);
    let archive = sevenz_rust2::Archive::open_with_password(file, &password)
        .map_err(|e| sevenz_error(e, key, had))?;
    let mut index = ArchiveIndex::default();
    for member in &archive.files {
        if member.is_anti_item {
            continue;
        }
        let kind = if member.is_directory { EntryKind::Directory } else { EntryKind::File };
        let mut entry = Entry::new("", kind).with_size(member.size);
        if member.has_last_modified_date {
            entry.modified = Some(member.last_modified_date.into());
        }
        index.insert(&member.name, entry, Locator::Named(member.name.clone()));
    }
    Ok(index)
}

fn sevenz_extract(file: &Path, name: &str, out: &mut dyn Write, key: &str) -> io::Result<()> {
    let (password, had) = sevenz_password(key);
    let mut reader =
        sevenz_rust2::ArchiveReader::open(file, password).map_err(|e| sevenz_error(e, key, had))?;
    let mut found = false;
    reader
        .for_each_entries(|entry, data| {
            if entry.name == name {
                io::copy(data, out).map_err(sevenz_rust2::Error::from)?;
                found = true;
                return Ok(false); // stop
            }
            // Solid blocks must be read through to reach later entries.
            io::copy(data, &mut io::sink()).map_err(sevenz_rust2::Error::from)?;
            Ok(true)
        })
        .map_err(|e| sevenz_error(e, key, had))?;
    if found {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, format!("{name} not in archive")))
    }
}

// ---------------------------------------------------------------- rar

#[cfg(feature = "rar")]
fn rar_index(file: &Path) -> io::Result<ArchiveIndex> {
    let archive = unrar::Archive::new(file).open_for_listing().map_err(invalid)?;
    let mut index = ArchiveIndex::default();
    for header in archive {
        let header = header.map_err(invalid)?;
        let name = header.filename.to_string_lossy().into_owned();
        let kind = if header.is_directory() { EntryKind::Directory } else { EntryKind::File };
        let entry = Entry::new("", kind).with_size(header.unpacked_size);
        index.insert(&name, entry, Locator::Named(name.clone()));
    }
    Ok(index)
}

#[cfg(feature = "rar")]
fn rar_extract(file: &Path, name: &str, out: &mut dyn Write) -> io::Result<()> {
    let mut archive = unrar::Archive::new(file).open_for_processing().map_err(invalid)?;
    while let Some(header) = archive.read_header().map_err(invalid)? {
        if header.entry().filename.to_string_lossy() == name {
            let (data, _) = header.read().map_err(invalid)?;
            return out.write_all(&data);
        }
        archive = header.skip().map_err(invalid)?;
    }
    Err(io::Error::new(io::ErrorKind::NotFound, format!("{name} not in archive")))
}

#[cfg(not(feature = "rar"))]
fn rar_index(_: &Path) -> io::Result<ArchiveIndex> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "this build has no RAR support (feature `rar`)"))
}

#[cfg(not(feature = "rar"))]
fn rar_extract(_: &Path, _: &str, _: &mut dyn Write) -> io::Result<()> {
    rar_index(Path::new("")).map(|_| ())
}

// ------------------------------------------------- single compressed file

fn single_index(file: &Path, compression: Compression) -> io::Result<ArchiveIndex> {
    let meta = std::fs::metadata(file)?;
    let file_name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let suffix = format!(".{}", compression.extension());
    let inner =
        file_name.strip_suffix(&suffix).filter(|s| !s.is_empty()).unwrap_or("data").to_owned();
    let mut entry = Entry::new("", EntryKind::File)
        .with_size(uncompressed_size(file, compression).unwrap_or(0));
    entry.modified = meta.modified().ok();
    let mut index = ArchiveIndex::default();
    index.insert(&inner, entry, Locator::Single);
    Ok(index)
}

/// The decompressed size when the format records it cheaply.
fn uncompressed_size(file: &Path, compression: Compression) -> Option<u64> {
    match compression {
        // gzip stores the size modulo 2^32 in its last four bytes.
        Compression::Gzip => {
            use std::io::{Seek, SeekFrom};
            let mut f = File::open(file).ok()?;
            f.seek(SeekFrom::End(-4)).ok()?;
            let mut size = [0u8; 4];
            f.read_exact(&mut size).ok()?;
            Some(u64::from(u32::from_le_bytes(size)))
        }
        Compression::Zstd => {
            let mut head = vec![0u8; 18];
            let n = File::open(file).ok()?.read(&mut head).ok()?;
            zstd::zstd_safe::get_frame_content_size(&head[..n]).ok().flatten()
        }
        _ => None,
    }
}
