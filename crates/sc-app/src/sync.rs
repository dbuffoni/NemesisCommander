//! Sending settings to another machine and fetching them from one.
//!
//! The other machine is any location SuperCommander can write to. When it
//! is reached through the `sc` agent (`agent://host`, `scs://host`), the
//! agent says where its settings live, so the location of the machine is
//! enough. Otherwise the location names the settings file itself
//! (`sftp://host/home/me/.config/supercommander/config.toml`) or the folder
//! that holds it.
//!
//! Sending replaces the other machine's settings file, after keeping the
//! old one as `config.toml.bak`. With [`SyncMode::Merge`] the other
//! machine keeps its own bookmarks, connections and shortcuts, and gets
//! these added.

use sc_config::{Config, ExportBundle, ImportMode};
use sc_vfs::{ProviderRegistry, VfsPath, WriteOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Name of the settings file in a settings folder.
pub const SETTINGS_FILE: &str = "config.toml";

/// Largest settings file read (they are a few KiB).
const READ_LIMIT: u64 = 4 * 1024 * 1024;

/// How sent settings combine with the other machine's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncMode {
    /// The other machine gets exactly these settings.
    #[default]
    Replace,
    /// Options come from here; bookmarks, connections and shortcuts are
    /// added to the other machine's (these win on name clashes).
    Merge,
}

/// The settings file of the machine at `location`.
pub async fn settings_file(
    registry: &ProviderRegistry,
    location: &VfsPath,
) -> Result<VfsPath, String> {
    let provider = registry.provider_for(location).await.map_err(|e| e.to_string())?;
    if let Some(dir) = provider.settings_dir(location) {
        return Ok(dir.join(SETTINGS_FILE));
    }
    let is_file = location.file_name().is_some_and(|name| name.ends_with(".toml"));
    if is_file {
        return Ok(location.clone());
    }
    match provider.stat(location).await {
        Ok(entry) if entry.is_dir() => Ok(location.join(SETTINGS_FILE)),
        Ok(_) => Ok(location.clone()),
        Err(e) if e.is_not_found() => Err(format!(
            "{location}: not found; give the settings file or its folder, or a machine running the sc agent"
        )),
        Err(e) => Err(e.to_string()),
    }
}

/// Sends `config` to the machine at `location`; returns the file written.
pub async fn push(
    registry: &ProviderRegistry,
    config: &Config,
    location: &VfsPath,
    mode: SyncMode,
) -> Result<VfsPath, String> {
    let file = settings_file(registry, location).await?;
    let provider = registry.provider_for(&file).await.map_err(|e| e.to_string())?;
    let existing = match read_text(registry, &file).await {
        Ok(text) => Some(text),
        Err(ReadError::NotFound) => None,
        Err(ReadError::Other(e)) => return Err(e),
    };
    let to_write = match (&existing, mode) {
        (Some(text), SyncMode::Merge) => {
            let theirs = parse(text, &file)?;
            ExportBundle::new(config).apply(&theirs, ImportMode::Merge)
        }
        _ => config.clone(),
    };
    if let Some(parent) = file.parent() {
        create_dirs(registry, &parent).await?;
    }
    if let Some(old) = &existing {
        let backup = file.with_path(&format!("{}.bak", file.path()));
        write_text(&*provider, &backup, old).await?;
    }
    write_text(&*provider, &file, &to_write.to_toml()).await?;
    Ok(file)
}

/// Reads the settings of the machine at `location` (its settings file, or
/// a file exported with "Export settings").
pub async fn pull(
    registry: &ProviderRegistry,
    location: &VfsPath,
) -> Result<(Config, VfsPath), String> {
    let file = settings_file(registry, location).await?;
    match read_text(registry, &file).await {
        Ok(text) => Ok((parse(&text, &file)?, file)),
        Err(ReadError::NotFound) => Err(format!("{file}: no settings there")),
        Err(ReadError::Other(e)) => Err(e),
    }
}

/// Settings from a settings file or an export file.
fn parse(text: &str, file: &VfsPath) -> Result<Config, String> {
    let origin = file.to_string();
    if text.contains("format = \"supercommander-settings\"") {
        return ExportBundle::parse(text, &origin).map(|b| b.settings).map_err(|e| e.to_string());
    }
    Config::from_toml(text, &origin).map_err(|e| e.to_string())
}

enum ReadError {
    NotFound,
    Other(String),
}

async fn read_text(registry: &ProviderRegistry, file: &VfsPath) -> Result<String, ReadError> {
    let other = |e: String| ReadError::Other(e);
    let provider = registry.provider_for(file).await.map_err(|e| other(e.to_string()))?;
    let reader = match provider.read(file, 0).await {
        Ok(reader) => reader,
        Err(e) if e.is_not_found() => return Err(ReadError::NotFound),
        Err(e) => return Err(other(e.to_string())),
    };
    let mut data = Vec::new();
    reader
        .take(READ_LIMIT)
        .read_to_end(&mut data)
        .await
        .map_err(|e| other(format!("{file}: {e}")))?;
    String::from_utf8(data).map_err(|_| other(format!("{file}: not a text file")))
}

async fn write_text(
    provider: &dyn sc_vfs::Provider,
    file: &VfsPath,
    text: &str,
) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{file}: {e}");
    let options = WriteOptions { size_hint: Some(text.len() as u64), ..WriteOptions::truncate() };
    let mut sink = provider.write(file, options).await.map_err(|e| e.to_string())?;
    sink.write_all(text.as_bytes()).await.map_err(io)?;
    sink.shutdown().await.map_err(io)
}

/// Creates `dir` and its missing parents.
async fn create_dirs(registry: &ProviderRegistry, dir: &VfsPath) -> Result<(), String> {
    let provider = registry.provider_for(dir).await.map_err(|e| e.to_string())?;
    let mut missing = Vec::new();
    let mut current = Some(dir.clone());
    while let Some(path) = current {
        match provider.stat(&path).await {
            Ok(_) => break,
            Err(e) if e.is_not_found() => {
                current = path.parent();
                missing.push(path);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    for path in missing.into_iter().rev() {
        provider.create_dir(&path).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ProviderRegistry {
        sc_providers::default_registry()
    }

    #[tokio::test]
    async fn sends_and_fetches_settings_with_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let other = VfsPath::from_local(dir.path().join("other/.config/supercommander")).unwrap();
        let mut mine = Config::default();
        mine.transfers.parallel_jobs = 1;
        mine.keys.insert("files.copy".into(), vec!["Ctrl+C".into()]);

        // A folder that does not exist yet: given as the file itself.
        let file =
            push(&registry(), &mine, &other.join("config.toml"), SyncMode::Replace).await.unwrap();
        assert_eq!(file, other.join("config.toml"));
        let (fetched, _) = pull(&registry(), &other).await.unwrap();
        assert_eq!(fetched, mine);

        // Sending again keeps the previous file, and merging keeps their bookmarks.
        let mut theirs = Config::default();
        theirs.bookmarks.push(sc_config::Bookmark {
            name: "logs".into(),
            path: "/var/log".into(),
            targets: vec![],
        });
        push(&registry(), &theirs, &other, SyncMode::Replace).await.unwrap();
        assert!(dir.path().join("other/.config/supercommander/config.toml.bak").exists());
        push(&registry(), &mine, &other, SyncMode::Merge).await.unwrap();
        let (merged, _) = pull(&registry(), &other).await.unwrap();
        assert_eq!(merged.transfers.parallel_jobs, 1);
        assert_eq!(merged.bookmarks.len(), 1, "their bookmark stays");
        assert_eq!(merged.keys["files.copy"], ["Ctrl+C"]);
    }

    #[tokio::test]
    async fn export_files_can_be_fetched_and_missing_places_are_explained() {
        let dir = tempfile::tempdir().unwrap();
        let mut mine = Config::default();
        mine.viewer.tab_width = 8;
        ExportBundle::new(&mine).write(&dir.path().join("export.toml")).unwrap();
        let export = VfsPath::from_local(dir.path().join("export.toml")).unwrap();
        assert_eq!(pull(&registry(), &export).await.unwrap().0.viewer.tab_width, 8);
        let nowhere = VfsPath::from_local(dir.path().join("nowhere")).unwrap();
        assert!(pull(&registry(), &nowhere).await.unwrap_err().contains("not found"));
    }
}
