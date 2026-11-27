//! Checksum files: writing and checking files in the format of
//! `sha256sum` and `b3sum` (`<hex>  <name>`), and the BSD tag form
//! (`SHA256 (name) = <hex>`), so that they verify outside the program too.
//!
//! The text functions are pure; [`write_checksums`] and
//! [`check_checksums`] stream the files (nothing is loaded whole) through
//! any provider.

use pnxr_vfs::{
    EntryKind, HashAlgorithm, ProviderRegistry, Result, VfsError, VfsPath, WriteOptions,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How the files are listed in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One file for everything: `SHA256SUMS` / `B3SUMS`, relative paths.
    Combined,
    /// One file next to each file: `name.sha256` / `name.b3`.
    PerFile,
}

/// The extension of per-file checksum files, and the name of combined ones.
pub fn names(algorithm: HashAlgorithm) -> (&'static str, &'static str) {
    match algorithm {
        HashAlgorithm::Blake3 => ("b3", "B3SUMS"),
        HashAlgorithm::Sha256 => ("sha256", "SHA256SUMS"),
        HashAlgorithm::Xxh3 => ("xxh3", "XXH3SUMS"),
    }
}

/// One line of a checksum file: `hex  name`, with the name escaped the way
/// the GNU tools do when it holds a backslash or a newline (the line then
/// starts with a backslash).
pub fn format_line(hex: &str, name: &str) -> String {
    if name.contains('\\') || name.contains('\n') {
        let escaped = name.replace('\\', "\\\\").replace('\n', "\\n");
        format!("\\{hex}  {escaped}\n")
    } else {
        format!("{hex}  {name}\n")
    }
}

/// One entry read from a checksum file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The digest, lowercase.
    pub hex: String,
    /// The file name, as written (relative to the checksum file).
    pub name: String,
    /// The algorithm when the line says it (BSD tag form).
    pub algorithm: Option<HashAlgorithm>,
}

/// Reads the lines of a checksum file: `hex  name`, `hex *name` and
/// `ALGORITHM (name) = hex`; CRLF endings, blank lines and `#` comments are
/// accepted. Lines that are none of those are returned in `Err`.
pub fn parse(text: &str) -> std::result::Result<Vec<Listed>, Vec<usize>> {
    let mut listed = Vec::new();
    let mut bad = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(line) {
            Some(entry) => listed.push(entry),
            None => bad.push(number + 1),
        }
    }
    if bad.is_empty() { Ok(listed) } else { Err(bad) }
}

fn parse_line(line: &str) -> Option<Listed> {
    // BSD tag: SHA256 (name) = hex
    if let Some((head, hex)) = line.rsplit_once(") = ")
        && let Some((tag, name)) = head.split_once(" (")
    {
        let algorithm = pnxr_hash::parse_algorithm(tag)?;
        return is_hex(hex).then(|| Listed {
            hex: hex.to_ascii_lowercase(),
            name: name.to_owned(),
            algorithm: Some(algorithm),
        });
    }
    let (escaped, rest) = match line.strip_prefix('\\') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let (hex, name) = rest.split_once(' ')?;
    // "hex  name" (text) or "hex *name" (binary).
    let name = name.strip_prefix(' ').or_else(|| name.strip_prefix('*'))?;
    if !is_hex(hex) || name.is_empty() {
        return None;
    }
    let name = if escaped {
        let mut out = String::new();
        let mut chars = name.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        name.to_owned()
    };
    Some(Listed { hex: hex.to_ascii_lowercase(), name, algorithm: None })
}

fn is_hex(text: &str) -> bool {
    text.len() >= 16 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// The algorithm a checksum file is for: from the line when it says, else
/// from the file name (`.b3`, `B3SUMS`, `.sha256`, `SHA256SUMS`), else from
/// the length of the digest (64 hex digits are SHA-256).
pub fn algorithm_for(file_name: &str, entry: &Listed) -> Option<HashAlgorithm> {
    if let Some(algorithm) = entry.algorithm {
        return Some(algorithm);
    }
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".b3") || lower.contains("b3sums") || lower.contains("blake3") {
        return Some(HashAlgorithm::Blake3);
    }
    if lower.ends_with(".sha256") || lower.contains("sha256") {
        return Some(HashAlgorithm::Sha256);
    }
    match entry.hex.len() {
        64 => Some(HashAlgorithm::Sha256),
        16 => Some(HashAlgorithm::Xxh3),
        _ => None,
    }
}

/// All the files under `sources` (folders are entered), as `(path relative to
/// base, path)`, sorted by the relative name.
async fn expand(
    registry: &ProviderRegistry,
    base: &VfsPath,
    sources: &[VfsPath],
) -> Result<Vec<(String, VfsPath)>> {
    let mut files = Vec::new();
    let mut todo: Vec<VfsPath> = sources.to_vec();
    while let Some(path) = todo.pop() {
        let provider = registry.provider_for(&path).await?;
        let entry = provider.stat(&path).await?;
        if entry.kind == EntryKind::Directory {
            for child in pnxr_vfs::collect_listing(provider.as_ref(), &path).await? {
                if child.name != ".." && child.name != "." {
                    todo.push(path.join(&child.name));
                }
            }
        } else {
            let prefix = format!("{}/", base.path().trim_end_matches('/'));
            let relative = path
                .path()
                .strip_prefix(&prefix)
                .map(str::to_owned)
                .or_else(|| path.file_name().map(str::to_owned))
                .unwrap_or_default();
            files.push((relative, path));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// What [`write_checksums`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The checksum files created.
    pub files: Vec<VfsPath>,
    /// How many files were hashed.
    pub hashed: usize,
}

/// Hashes `sources` (folders are entered) and writes the checksum files
/// into `folder` (combined: one file; per file: one file per source file, in
/// `folder`, named after the file). A name that exists is an error unless
/// `overwrite`.
pub async fn write_checksums(
    registry: &ProviderRegistry,
    base: &VfsPath,
    sources: &[VfsPath],
    algorithm: HashAlgorithm,
    layout: Layout,
    folder: &VfsPath,
    overwrite: bool,
) -> Result<Written> {
    let files = expand(registry, base, sources).await?;
    if files.is_empty() {
        return Err(VfsError::Other("There are no files to compute checksums for".into()));
    }
    let (extension, combined_name) = names(algorithm);
    let mut outputs: Vec<(VfsPath, String)> = Vec::new();
    match layout {
        Layout::Combined => {
            let mut text = String::new();
            for (relative, path) in &files {
                let provider = registry.provider_for(path).await?;
                let hex = crate::checksum(provider.as_ref(), path, algorithm).await?;
                text.push_str(&format_line(&hex, relative));
            }
            outputs.push((folder.join(combined_name), text));
        }
        Layout::PerFile => {
            for (_, path) in &files {
                let provider = registry.provider_for(path).await?;
                let hex = crate::checksum(provider.as_ref(), path, algorithm).await?;
                let name = path.file_name().unwrap_or_default();
                outputs
                    .push((folder.join(&format!("{name}.{extension}")), format_line(&hex, name)));
            }
        }
    }
    let provider = registry.provider_for(folder).await?;
    if !overwrite {
        for (target, _) in &outputs {
            if provider.stat(target).await.is_ok() {
                return Err(VfsError::AlreadyExists(target.clone()));
            }
        }
    }
    let mut written = Vec::new();
    for (target, text) in outputs {
        let options = if overwrite { WriteOptions::truncate() } else { WriteOptions::create_new() };
        let mut sink = provider.write(&target, options).await?;
        sink.write_all(text.as_bytes()).await.map_err(|e| VfsError::from_io(&target, e))?;
        sink.shutdown().await.map_err(|e| VfsError::from_io(&target, e))?;
        written.push(target);
    }
    Ok(Written { files: written, hashed: files.len() })
}

/// The result for one listed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The digest matches.
    Ok,
    /// The file is there with another digest.
    Failed,
    /// The file is not there (or cannot be read): the reason.
    Missing(String),
}

/// Checks the files listed in `checksum_file` (names are relative to its
/// folder), as `sha256sum -c` does.
pub async fn check_checksums(
    registry: &ProviderRegistry,
    checksum_file: &VfsPath,
) -> Result<Vec<(String, Verdict)>> {
    let provider = registry.provider_for(checksum_file).await?;
    let mut reader = provider.read(checksum_file, 0).await?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.map_err(|e| VfsError::from_io(checksum_file, e))?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');
    let listed = parse(text).map_err(|lines| {
        VfsError::Other(format!(
            "Not a checksum file: line {} is not in the form `hash  name`",
            lines.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
        ))
    })?;
    let folder = checksum_file.parent().unwrap_or_else(|| checksum_file.clone());
    let own_name = checksum_file.file_name().unwrap_or_default().to_owned();
    let mut results = Vec::new();
    for entry in listed {
        let Some(algorithm) = algorithm_for(&own_name, &entry) else {
            results.push((entry.name.clone(), Verdict::Missing("unknown algorithm".into())));
            continue;
        };
        let path = entry.name.split('/').fold(folder.clone(), |p, part| p.join(part));
        let verdict = match registry.provider_for(&path).await {
            Err(e) => Verdict::Missing(e.to_string()),
            Ok(provider) => match crate::checksum(provider.as_ref(), &path, algorithm).await {
                Ok(hex) if hex.eq_ignore_ascii_case(&entry.hex) => Verdict::Ok,
                Ok(_) => Verdict::Failed,
                // The report names the file already: no full address.
                Err(e) if e.is_not_found() => Verdict::Missing("not found".into()),
                Err(e) => Verdict::Missing(e.to_string()),
            },
        };
        results.push((entry.name, verdict));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_the_gnu_format_and_round_trip() {
        assert_eq!(format_line("ab12", "a b.txt"), "ab12  a b.txt\n");
        let text = format!(
            "{}{}",
            format_line(&"a".repeat(64), "sub/é ü.txt"),
            format_line(&"b".repeat(64), "we\\ird")
        );
        let listed = parse(&text).unwrap();
        assert_eq!(listed[0].name, "sub/é ü.txt");
        assert_eq!(listed[1].name, "we\\ird", "escaped names are read back");
        assert!(text.contains("\\bbbb"), "the line of an escaped name starts with a backslash");
    }

    #[test]
    fn other_forms_are_read() {
        let hex = "c".repeat(64);
        let text = format!("# comment\r\n{hex} *bin.dat\r\n\r\nSHA256 (tag name) = {hex}\r\n");
        let listed = parse(&text).unwrap();
        assert_eq!(listed[0].name, "bin.dat");
        assert_eq!(listed[1].name, "tag name");
        assert_eq!(listed[1].algorithm, Some(HashAlgorithm::Sha256));
        assert_eq!(parse("nonsense\nabc"), Err(vec![1, 2]));
    }

    #[test]
    fn the_algorithm_comes_from_the_line_the_name_or_the_length() {
        let entry = Listed { hex: "d".repeat(64), name: "x".into(), algorithm: None };
        assert_eq!(algorithm_for("B3SUMS", &entry), Some(HashAlgorithm::Blake3));
        assert_eq!(algorithm_for("x.b3", &entry), Some(HashAlgorithm::Blake3));
        assert_eq!(algorithm_for("SHA256SUMS", &entry), Some(HashAlgorithm::Sha256));
        assert_eq!(algorithm_for("sums.txt", &entry), Some(HashAlgorithm::Sha256));
    }

    fn registry() -> ProviderRegistry {
        pnxr_providers::default_registry()
    }

    #[tokio::test]
    async fn written_files_are_accepted_by_sha256sum_and_checked_here() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub dir")).unwrap();
        std::fs::write(dir.path().join("a b.txt"), "alpha\n").unwrap();
        std::fs::write(dir.path().join("sub dir/é.bin"), [0u8, 1, 2]).unwrap();
        let base = VfsPath::from_local(dir.path()).unwrap();
        let registry = registry();
        let sources = vec![base.join("a b.txt"), base.join("sub dir")];
        let done = write_checksums(
            &registry,
            &base,
            &sources,
            HashAlgorithm::Sha256,
            Layout::Combined,
            &base,
            false,
        )
        .await
        .unwrap();
        assert_eq!(done.hashed, 2);
        let text = std::fs::read_to_string(dir.path().join("SHA256SUMS")).unwrap();
        assert!(text.contains("  a b.txt\n") && text.contains("  sub dir/é.bin\n"), "{text}");
        if let Ok(out) = std::process::Command::new("sha256sum")
            .arg("-c")
            .arg("SHA256SUMS")
            .current_dir(dir.path())
            .output()
        {
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        }
        // It exists now: refused, unless overwriting is asked for.
        let again = write_checksums(
            &registry,
            &base,
            &sources,
            HashAlgorithm::Sha256,
            Layout::Combined,
            &base,
            false,
        )
        .await;
        assert!(matches!(again, Err(VfsError::AlreadyExists(_))));
        // Checked here: all good, then one file changes and one is removed.
        let ok = check_checksums(&registry, &base.join("SHA256SUMS")).await.unwrap();
        assert!(ok.iter().all(|(_, v)| *v == Verdict::Ok), "{ok:?}");
        std::fs::write(dir.path().join("a b.txt"), "changed").unwrap();
        std::fs::remove_file(dir.path().join("sub dir/é.bin")).unwrap();
        let bad = check_checksums(&registry, &base.join("SHA256SUMS")).await.unwrap();
        assert_eq!(bad[0], ("a b.txt".to_owned(), Verdict::Failed));
        assert_eq!(bad[1].1, Verdict::Missing("not found".into()), "no address");
    }

    #[tokio::test]
    async fn one_file_per_file_in_the_blake3_style() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.dat"), "x").unwrap();
        std::fs::write(dir.path().join("y.dat"), "y").unwrap();
        let base = VfsPath::from_local(dir.path()).unwrap();
        let registry = registry();
        let sources = vec![base.join("x.dat"), base.join("y.dat")];
        let done = write_checksums(
            &registry,
            &base,
            &sources,
            HashAlgorithm::Blake3,
            Layout::PerFile,
            &base,
            false,
        )
        .await
        .unwrap();
        assert_eq!(done.files.len(), 2);
        let text = std::fs::read_to_string(dir.path().join("x.dat.b3")).unwrap();
        assert!(text.ends_with("  x.dat\n"));
        let checked = check_checksums(&registry, &base.join("y.dat.b3")).await.unwrap();
        assert_eq!(checked, [("y.dat".to_owned(), Verdict::Ok)]);
    }
}
