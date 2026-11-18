//! Creating archives from files on any provider.
//!
//! The async side walks the sources and streams each file's data; a
//! blocking thread runs the (synchronous) archive writer. They are connected
//! by bounded channels: one for the list of members, and one per file for
//! its data, so memory use does not depend on file sizes.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use futures::StreamExt;
use nmc_vfs::{EntryKind, ProviderRegistry, Result, VfsError, VfsPath, walk};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::ArchiveFormat;
use crate::compression::Compression;
use crate::pipe::ChannelReader;
use crate::time;

/// Options for [`pack`].
#[derive(Clone)]
pub struct PackOptions {
    /// Archive format to create.
    pub format: ArchiveFormat,
    /// Compression level, 1 (fastest) to 9 (smallest).
    pub level: u32,
    /// Called with the number of source bytes consumed.
    pub progress: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    /// Cancels packing (the partial output is removed).
    pub cancel: CancellationToken,
}

impl PackOptions {
    /// Default level 6 for `format`.
    pub fn new(format: ArchiveFormat) -> Self {
        PackOptions { format, level: 6, progress: None, cancel: CancellationToken::new() }
    }
}

const WRITER_STOPPED: &str = "archive writer stopped";

/// One member sent to the writer thread.
enum Member {
    Directory {
        name: String,
        modified: Option<SystemTime>,
        mode: Option<u32>,
    },
    File {
        name: String,
        size: u64,
        modified: Option<SystemTime>,
        mode: Option<u32>,
        data: mpsc::Receiver<Bytes>,
    },
    Symlink {
        name: String,
        target: String,
    },
}

/// Packs `sources` (files and whole directories) into the archive file
/// `output`. Each source appears at the archive root under its own name.
/// Returns the number of members written.
pub async fn pack(
    registry: &ProviderRegistry,
    sources: &[VfsPath],
    output: &VfsPath,
    options: PackOptions,
) -> Result<u64> {
    if !options.format.can_write() {
        return Err(VfsError::unsupported("create archive", options.format.scheme()));
    }
    let single = matches!(options.format, ArchiveFormat::Compressed(_));

    // Write locally: next to the target if it is local, else in a temp file
    // that is uploaded at the end.
    let (local_target, temp) = match output.to_local_path() {
        Some(path) => {
            let name =
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (path.with_file_name(format!(".{name}.nmc-part")), None)
        }
        None => {
            let temp = tempfile::NamedTempFile::new()
                .map_err(|e| VfsError::Other(e.to_string()))?
                .into_temp_path();
            (temp.to_path_buf(), Some(temp))
        }
    };

    let (tx, rx) = mpsc::channel::<Member>(16);
    let (format, level, target) = (options.format, options.level, local_target.clone());
    let writer = tokio::task::spawn_blocking(move || write_archive(&target, format, level, rx));

    let feed = feed_members(registry, sources, &options, single, tx).await;
    let written = writer.await.map_err(|e| VfsError::Other(e.to_string()))?;
    let count = match (feed, written) {
        (Ok(()), Ok(count)) => count,
        (feed, written) => {
            let _ = std::fs::remove_file(&local_target);
            // Report the root cause: if the writer failed first, the feeder
            // only saw a closed channel.
            return Err(match (feed, written) {
                (Err(VfsError::Other(m)), Err(w)) if m == WRITER_STOPPED => w,
                (Err(e), _) | (Ok(()), Err(e)) => e,
                (Ok(()), Ok(_)) => unreachable!(),
            });
        }
    };

    match (output.to_local_path(), temp) {
        (Some(final_path), _) => {
            std::fs::rename(&local_target, &final_path).map_err(|e| VfsError::from_io(output, e))?
        }
        (None, Some(temp)) => upload(registry, &temp, output).await?,
        (None, None) => unreachable!("non-local output always uses a temp file"),
    }
    Ok(count)
}

async fn feed_members(
    registry: &ProviderRegistry,
    sources: &[VfsPath],
    options: &PackOptions,
    single: bool,
    tx: mpsc::Sender<Member>,
) -> Result<()> {
    let closed = || VfsError::Other(WRITER_STOPPED.into());
    for source in sources {
        let provider = registry.provider_for(source).await?;
        let root = provider.stat(source).await?;
        let base = source.file_name().unwrap_or("root").to_owned();
        if single && (root.kind != EntryKind::File || sources.len() != 1) {
            return Err(VfsError::Other("a compressed file can hold exactly one file".into()));
        }

        // The source itself, then (for directories) everything below it.
        let mut items = vec![(source.clone(), base.clone(), root)];
        if items[0].2.kind == EntryKind::Directory {
            let mut tree = walk(provider.as_ref(), source, None);
            while let Some(item) = tree.next().await {
                let item = item?;
                let relative = item.path.relative_to(source).unwrap_or_default();
                items.push((item.path, format!("{base}/{relative}"), item.entry));
            }
        }

        for (path, name, entry) in items {
            if options.cancel.is_cancelled() {
                return Err(VfsError::Cancelled);
            }
            match entry.kind {
                EntryKind::Directory => {
                    let member = Member::Directory {
                        name,
                        modified: entry.modified,
                        mode: entry.permissions,
                    };
                    tx.send(member).await.map_err(|_| closed())?;
                }
                EntryKind::Symlink => {
                    let target = entry.symlink_target.unwrap_or_default();
                    tx.send(Member::Symlink { name, target }).await.map_err(|_| closed())?;
                }
                EntryKind::File => {
                    let (data_tx, data_rx) = mpsc::channel(8);
                    let member = Member::File {
                        name,
                        size: entry.size,
                        modified: entry.modified,
                        mode: entry.permissions,
                        data: data_rx,
                    };
                    tx.send(member).await.map_err(|_| closed())?;
                    let mut reader = provider.read(&path, 0).await?;
                    let mut sent = 0u64;
                    loop {
                        let mut buf = vec![0u8; 256 * 1024];
                        let n =
                            reader.read(&mut buf).await.map_err(|e| VfsError::from_io(&path, e))?;
                        if n == 0 {
                            break;
                        }
                        buf.truncate(n);
                        sent += n as u64;
                        if sent > entry.size {
                            return Err(VfsError::Other(format!(
                                "{path} grew while being archived"
                            )));
                        }
                        data_tx.send(Bytes::from(buf)).await.map_err(|_| closed())?;
                        if let Some(progress) = &options.progress {
                            progress(n as u64);
                        }
                    }
                    if sent != entry.size {
                        return Err(VfsError::Other(format!("{path} shrank while being archived")));
                    }
                }
                EntryKind::Other => {}
            }
        }
    }
    Ok(())
}

async fn upload(registry: &ProviderRegistry, local: &Path, output: &VfsPath) -> Result<()> {
    let provider = registry.provider_for(output).await?;
    let mut sink = provider.write(output, nmc_vfs::WriteOptions::truncate()).await?;
    let mut file = tokio::fs::File::open(local).await.map_err(|e| VfsError::from_io(output, e))?;
    tokio::io::copy(&mut file, &mut sink).await.map_err(|e| VfsError::from_io(output, e))?;
    sink.shutdown().await.map_err(|e| VfsError::from_io(output, e))
}

// ------------------------------------------------------- blocking writers

fn other(e: impl std::fmt::Display) -> VfsError {
    VfsError::Other(e.to_string())
}

fn write_archive(
    target: &PathBuf,
    format: ArchiveFormat,
    level: u32,
    rx: mpsc::Receiver<Member>,
) -> Result<u64> {
    let file = BufWriter::new(File::create(target).map_err(other)?);
    match format {
        ArchiveFormat::Zip => write_zip(file, level, rx),
        ArchiveFormat::Tar(compression) => write_tar(file, compression, level, rx),
        ArchiveFormat::SevenZ => write_7z(File::create(target).map_err(other)?, rx),
        ArchiveFormat::Compressed(compression) => write_single(file, compression, level, rx),
        ArchiveFormat::Rar => Err(VfsError::unsupported("create archive", "rar")),
    }
}

fn write_zip(file: BufWriter<File>, level: u32, mut rx: mpsc::Receiver<Member>) -> Result<u64> {
    use zip::write::SimpleFileOptions;
    let mut zip = zip::ZipWriter::new(file);
    let mut count = 0;
    let base = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(i64::from(level.clamp(1, 9))));
    let with_meta = |modified: Option<SystemTime>, mode: Option<u32>| {
        let mut options = base;
        if let Some(t) = modified {
            let (y, mo, d, h, mi, s) = time::to_civil(t);
            if let Ok(dt) = zip::DateTime::from_date_and_time(
                y as u16, mo as u8, d as u8, h as u8, mi as u8, s as u8,
            ) {
                options = options.last_modified_time(dt);
            }
        }
        if let Some(mode) = mode {
            options = options.unix_permissions(mode);
        }
        options
    };
    while let Some(member) = rx.blocking_recv() {
        count += 1;
        match member {
            Member::Directory { name, modified, mode } => {
                zip.add_directory(name, with_meta(modified, mode)).map_err(other)?;
            }
            Member::Symlink { name, target } => {
                zip.add_symlink(name, target, base).map_err(other)?;
            }
            Member::File { name, size, modified, mode, data } => {
                let options = with_meta(modified, mode).large_file(size >= u64::from(u32::MAX));
                zip.start_file(name, options).map_err(other)?;
                io::copy(&mut ChannelReader::new(data), &mut zip).map_err(other)?;
            }
        }
    }
    zip.finish().map_err(other)?.flush().map_err(other)?;
    Ok(count)
}

fn write_tar(
    file: BufWriter<File>,
    compression: Compression,
    level: u32,
    mut rx: mpsc::Receiver<Member>,
) -> Result<u64> {
    let encoder = compression.encoder(file, level).map_err(other)?;
    let mut tar = tar::Builder::new(encoder);
    let mut count = 0;
    let header_for =
        |kind: tar::EntryType, size: u64, modified: Option<SystemTime>, mode: Option<u32>| {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(kind);
            header.set_size(size);
            header.set_mtime(modified.map(time::unix_seconds).unwrap_or(0));
            header.set_mode(mode.unwrap_or(if kind == tar::EntryType::Directory {
                0o755
            } else {
                0o644
            }));
            header
        };
    while let Some(member) = rx.blocking_recv() {
        count += 1;
        match member {
            Member::Directory { name, modified, mode } => {
                let mut header = header_for(tar::EntryType::Directory, 0, modified, mode);
                tar.append_data(&mut header, format!("{name}/"), io::empty()).map_err(other)?;
            }
            Member::Symlink { name, target } => {
                let mut header = header_for(tar::EntryType::Symlink, 0, None, Some(0o777));
                tar.append_link(&mut header, name, target).map_err(other)?;
            }
            Member::File { name, size, modified, mode, data } => {
                let mut header = header_for(tar::EntryType::Regular, size, modified, mode);
                tar.append_data(&mut header, name, ChannelReader::new(data)).map_err(other)?;
            }
        }
    }
    tar.into_inner().map_err(other)?.finish().map_err(other)?.flush().map_err(other)?;
    Ok(count)
}

fn write_7z(file: File, mut rx: mpsc::Receiver<Member>) -> Result<u64> {
    let mut archive = sevenz_rust2::ArchiveWriter::new(file).map_err(other)?;
    let mut count = 0;
    let nt_time = |t: SystemTime| {
        // 100 ns intervals since 1601-01-01.
        sevenz_rust2::NtTime::new((time::unix_seconds(t) + 11_644_473_600) * 10_000_000)
    };
    while let Some(member) = rx.blocking_recv() {
        count += 1;
        match member {
            Member::Directory { name, modified, .. } => {
                let mut entry = sevenz_rust2::ArchiveEntry::new_directory(&name);
                if let Some(t) = modified {
                    entry.has_last_modified_date = true;
                    entry.last_modified_date = nt_time(t);
                }
                archive.push_archive_entry::<&[u8]>(entry, None).map_err(other)?;
            }
            Member::Symlink { name, target } => {
                // 7z has no portable symlink entry: store the link target as text.
                let entry = sevenz_rust2::ArchiveEntry::new_file(&name);
                archive.push_archive_entry(entry, Some(target.as_bytes())).map_err(other)?;
            }
            Member::File { name, modified, data, .. } => {
                let mut entry = sevenz_rust2::ArchiveEntry::new_file(&name);
                if let Some(t) = modified {
                    entry.has_last_modified_date = true;
                    entry.last_modified_date = nt_time(t);
                }
                archive.push_archive_entry(entry, Some(ChannelReader::new(data))).map_err(other)?;
            }
        }
    }
    archive.finish().map_err(other)?;
    Ok(count)
}

fn write_single(
    file: BufWriter<File>,
    compression: Compression,
    level: u32,
    mut rx: mpsc::Receiver<Member>,
) -> Result<u64> {
    // Exactly one file member is sent (checked by the feeder).
    while let Some(member) = rx.blocking_recv() {
        if let Member::File { data, size, .. } = member {
            let mut encoder =
                compression.encoder_with_size(file, level, Some(size)).map_err(other)?;
            io::copy(&mut ChannelReader::new(data), &mut encoder).map_err(other)?;
            encoder.finish().map_err(other)?.flush().map_err(other)?;
            return Ok(1);
        }
    }
    Err(VfsError::Other("nothing to compress".into()))
}
