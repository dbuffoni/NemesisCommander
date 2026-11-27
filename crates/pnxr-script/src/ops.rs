//! The Rust side of the `pnxr` API: one op per function of `js/pnxr.d.ts`.
//!
//! Every op resolves its locations against the script's folder, checks
//! them against the script's [`Permissions`] and then uses the same
//! providers and copy engine as the panels.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use deno_core::{OpState, op2};
use deno_error::JsErrorBox;
use pnxr_ops::{ConflictPolicy, ExecuteOptions, Executor, PlanOptions, Step, plan_copy};
use pnxr_vfs::{
    Entry, EntryKind, Provider, ProviderRegistry, VfsError, VfsPath, WriteOptions, collect_listing,
};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::permissions::{Access, Permissions};

/// Largest file `readText` reads: scripts that need more should copy.
pub const READ_TEXT_LIMIT: u64 = 256 * 1024 * 1024;

/// Receives what the script prints.
pub type Output = Arc<dyn Fn(&str) + Send + Sync>;

/// What the ops need, kept in the runtime's op state.
pub(crate) struct ScriptState {
    pub registry: Arc<ProviderRegistry>,
    /// The folder relative locations start from.
    pub base: PathBuf,
    pub permissions: Permissions,
    pub args: Vec<String>,
    pub output: Output,
    pub invocation: Option<Invocation>,
}

/// Where a plugin command was started: `pnxr.context` in the script.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Invocation {
    /// The command's id from the manifest.
    pub command: String,
    /// The folder of the active panel.
    pub panel: Option<String>,
    /// The folder of the other panel.
    pub other: Option<String>,
    /// The selected (or current) entries of the active panel.
    pub selection: Vec<String>,
    /// Whether the program that started the command can show a list of
    /// results (`pnxr.showResults`); the window and the terminal can.
    pub results: bool,
}

impl Invocation {
    /// The context of `command` run with these panels and selection.
    pub fn new(
        command: &str,
        panel: Option<&VfsPath>,
        other: Option<&VfsPath>,
        selection: &[VfsPath],
    ) -> Self {
        Invocation {
            command: command.to_owned(),
            panel: panel.map(display),
            other: other.map(display),
            selection: selection.iter().map(display).collect(),
            results: std::env::var_os("PNXR_RESULTS").is_some_and(|v| v == "1"),
        }
    }
}

deno_core::extension!(
    pnxr_script,
    ops = [
        op_pnxr_version,
        op_pnxr_args,
        op_pnxr_context,
        op_pnxr_print,
        op_pnxr_list,
        op_pnxr_stat,
        op_pnxr_exists,
        op_pnxr_read_text,
        op_pnxr_image_hash,
        op_pnxr_write_text,
        op_pnxr_write_bytes,
        op_pnxr_mkdir,
        op_pnxr_rename,
        op_pnxr_remove,
        op_pnxr_copy,
        op_pnxr_run,
    ],
    options = { state: ScriptState },
    state = |op_state, options| op_state.put(options.state),
);

/// An entry as scripts see it.
#[derive(Serialize)]
struct ScriptEntry {
    name: String,
    path: String,
    kind: &'static str,
    size: f64,
    modified: Option<f64>,
}

impl ScriptEntry {
    fn new(entry: &Entry, path: &VfsPath) -> Self {
        ScriptEntry {
            name: entry.name.clone(),
            path: display(path),
            kind: match entry.kind {
                EntryKind::File => "file",
                EntryKind::Directory => "dir",
                EntryKind::Symlink => "symlink",
                EntryKind::Other => "other",
            },
            size: entry.size as f64,
            modified: entry
                .modified
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|age| age.as_millis() as f64),
        }
    }
}

/// What `copy` reports. Counts are of files (and links): folders are
/// created as needed and not counted.
#[derive(Serialize)]
struct CopyReport {
    done: usize,
    skipped: usize,
    failed: Vec<String>,
    bytes: f64,
}

/// A location as a script writes it: local paths stay plain paths.
fn display(path: &VfsPath) -> String {
    if path.scheme() == "file" && path.container().is_none() {
        // Windows paths are kept as `C:/dir`, which every function here
        // and Windows itself read back.
        let text = path.path();
        match text.strip_prefix('/') {
            Some(rest) if is_drive(rest) => rest.to_owned(),
            _ => text.to_owned(),
        }
    } else {
        path.to_string()
    }
}

fn vfs_error(error: VfsError) -> JsErrorBox {
    JsErrorBox::generic(error.to_string())
}

/// `C:` or `C:/...`: a Windows drive.
fn is_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes.get(2).is_none_or(|b| *b == b'/' || *b == b'\\')
}

/// Resolves `text` (a URI, an absolute path, or a path relative to `base`).
fn resolve(base: &Path, text: &str) -> Result<VfsPath, JsErrorBox> {
    let parsed = if text.contains("://") {
        VfsPath::parse(text)
    } else if cfg!(windows) && (is_drive(text) || text.strip_prefix('/').is_some_and(is_drive)) {
        // A drive path is absolute even where `Path` does not know drives.
        VfsPath::from_local(text.trim_start_matches('/'))
    } else {
        VfsPath::from_local(base.join(text))
    };
    parsed.map_err(|e| JsErrorBox::type_error(e.to_string()))
}

/// Resolves and checks a location; returns it with the registry.
fn locate(
    state: &Rc<RefCell<OpState>>,
    text: &str,
    access: Access,
) -> Result<(Arc<ProviderRegistry>, VfsPath), JsErrorBox> {
    let state = state.borrow();
    let script = state.borrow::<ScriptState>();
    let path = resolve(&script.base, text)?;
    script.permissions.check(&path, access).map_err(|e| JsErrorBox::new("PermissionDenied", e))?;
    Ok((script.registry.clone(), path))
}

/// The provider and checked path for `text`.
async fn open(
    state: &Rc<RefCell<OpState>>,
    text: &str,
    access: Access,
) -> Result<(Arc<dyn Provider>, VfsPath), JsErrorBox> {
    let (registry, path) = locate(state, text, access)?;
    let provider = registry.provider_for(&path).await.map_err(vfs_error)?;
    Ok((provider, path))
}

#[op2]
#[string]
fn op_pnxr_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[op2]
#[serde]
fn op_pnxr_args(state: &mut OpState) -> Vec<String> {
    state.borrow::<ScriptState>().args.clone()
}

#[op2]
#[serde]
fn op_pnxr_context(state: &mut OpState) -> Option<Invocation> {
    state.borrow::<ScriptState>().invocation.clone()
}

#[op2(fast)]
fn op_pnxr_print(state: &mut OpState, #[string] text: &str) {
    (state.borrow::<ScriptState>().output)(text);
}

#[op2]
#[serde]
async fn op_pnxr_list(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<Vec<ScriptEntry>, JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Read).await?;
    let entries = collect_listing(provider.as_ref(), &path).await.map_err(vfs_error)?;
    Ok(entries.iter().map(|entry| ScriptEntry::new(entry, &path.join(&entry.name))).collect())
}

#[op2]
#[serde]
async fn op_pnxr_stat(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<ScriptEntry, JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Read).await?;
    let entry = provider.stat(&path).await.map_err(vfs_error)?;
    Ok(ScriptEntry::new(&entry, &path))
}

#[op2]
async fn op_pnxr_exists(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<bool, JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Read).await?;
    match provider.stat(&path).await {
        Ok(_) => Ok(true),
        Err(VfsError::NotFound(_)) => Ok(false),
        Err(e) => Err(vfs_error(e)),
    }
}

#[op2]
#[string]
async fn op_pnxr_read_text(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<String, JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Read).await?;
    let reader = provider.read(&path, 0).await.map_err(vfs_error)?;
    let mut data = Vec::new();
    let mut limited = reader.take(READ_TEXT_LIMIT + 1);
    limited.read_to_end(&mut data).await.map_err(|e| vfs_error(VfsError::from_io(&path, e)))?;
    if data.len() as u64 > READ_TEXT_LIMIT {
        return Err(JsErrorBox::generic(format!("{path}: too large for readText; use copy")));
    }
    String::from_utf8(data).map_err(|_| JsErrorBox::type_error(format!("{path}: not UTF-8 text")))
}

/// The perceptual hash of a picture (`null` when it is not one, or a flat one).
#[op2]
#[serde]
async fn op_pnxr_image_hash(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<Option<crate::imagehash::ImageInfo>, JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Read).await?;
    let entry = provider.stat(&path).await.map_err(vfs_error)?;
    let size = entry.size;
    if size > crate::imagehash::PICTURE_LIMIT {
        return Ok(None);
    }
    // A picture that has not changed since an earlier run keeps its hash.
    let key = crate::hashcache::key(&display(&path), size, entry.modified);
    if let Some(info) = key.as_deref().and_then(crate::hashcache::get) {
        return Ok(Some(info));
    }
    let reader = provider.read(&path, 0).await.map_err(vfs_error)?;
    let mut data = Vec::new();
    let mut limited = reader.take(crate::imagehash::PICTURE_LIMIT + 1);
    limited.read_to_end(&mut data).await.map_err(|e| vfs_error(VfsError::from_io(&path, e)))?;
    if data.len() as u64 > crate::imagehash::PICTURE_LIMIT {
        return Ok(None);
    }
    // Decoding takes a while: not on the thread of the script.
    let info = tokio::task::spawn_blocking(move || crate::imagehash::describe(&data))
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    if let (Some(key), Some(info)) = (key, &info) {
        crate::hashcache::put(&key, info);
    }
    Ok(info)
}

#[op2]
async fn op_pnxr_write_text(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
    #[string] text: String,
) -> Result<(), JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Write).await?;
    let options = WriteOptions { size_hint: Some(text.len() as u64), ..WriteOptions::truncate() };
    let mut sink = provider.write(&path, options).await.map_err(vfs_error)?;
    let io = |e| vfs_error(VfsError::from_io(&path, e));
    sink.write_all(text.as_bytes()).await.map_err(io)?;
    sink.shutdown().await.map_err(io)
}

#[op2]
async fn op_pnxr_write_bytes(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
    #[buffer(copy)] bytes: Vec<u8>,
) -> Result<(), JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Write).await?;
    let options = WriteOptions { size_hint: Some(bytes.len() as u64), ..WriteOptions::truncate() };
    let mut sink = provider.write(&path, options).await.map_err(vfs_error)?;
    let io = |e| vfs_error(VfsError::from_io(&path, e));
    sink.write_all(&bytes).await.map_err(io)?;
    sink.shutdown().await.map_err(io)
}

#[op2]
async fn op_pnxr_mkdir(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<(), JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Write).await?;
    provider.create_dir(&path).await.map_err(vfs_error)
}

#[op2]
async fn op_pnxr_rename(
    state: Rc<RefCell<OpState>>,
    #[string] from: String,
    #[string] to: String,
) -> Result<(), JsErrorBox> {
    let (provider, from) = open(&state, &from, Access::Write).await?;
    let (_, to) = locate(&state, &to, Access::Write)?;
    if from.provider_key() != to.provider_key() {
        return Err(JsErrorBox::type_error(format!(
            "{from} and {to} are in different locations: use copy, then remove"
        )));
    }
    provider.rename(&from, &to).await.map_err(vfs_error)
}

#[op2]
async fn op_pnxr_remove(
    state: Rc<RefCell<OpState>>,
    #[string] path: String,
) -> Result<(), JsErrorBox> {
    let (provider, path) = open(&state, &path, Access::Write).await?;
    let entry = provider.stat(&path).await.map_err(vfs_error)?;
    let removed = if entry.is_dir() {
        provider.remove_dir(&path).await
    } else {
        provider.remove_file(&path).await
    };
    removed.map_err(vfs_error)
}

#[op2]
#[serde]
async fn op_pnxr_copy(
    state: Rc<RefCell<OpState>>,
    #[serde] sources: Vec<String>,
    #[string] destination: String,
    #[string] conflict: String,
) -> Result<CopyReport, JsErrorBox> {
    let conflict: ConflictPolicy = conflict_policy(&conflict)?;
    let mut paths = Vec::new();
    for source in &sources {
        paths.push(locate(&state, source, Access::Read)?.1);
    }
    let (registry, destination) = locate(&state, &destination, Access::Write)?;
    let plan = plan_copy(&registry, &paths, &destination, PlanOptions::default())
        .await
        .map_err(vfs_error)?;
    let options = ExecuteOptions { conflict, ..ExecuteOptions::default() };
    let report = Executor::new(registry, options).run(&plan).await.map_err(vfs_error)?;
    let is_file = |index: usize| {
        matches!(plan.steps.get(index), Some(Step::CopyFile { .. } | Step::CopySymlink { .. }))
    };
    let files = (0..plan.steps.len()).filter(|&index| is_file(index)).count();
    let failed_files = report.failed.iter().filter(|(index, _)| is_file(*index)).count();
    let mut failed = plan.warnings.clone();
    failed.extend(report.failed.into_iter().map(|(_, message)| message));
    Ok(CopyReport {
        done: files.saturating_sub(failed_files + report.skipped),
        skipped: report.skipped,
        failed,
        bytes: report.bytes as f64,
    })
}

/// A conflict policy by its name in the API.
fn conflict_policy(name: &str) -> Result<ConflictPolicy, JsErrorBox> {
    let policy = match name {
        "fail" => ConflictPolicy::Fail,
        "overwrite" => ConflictPolicy::Overwrite,
        "overwrite-if-newer" => ConflictPolicy::OverwriteIfNewer,
        "overwrite-if-different-size" => ConflictPolicy::OverwriteIfDifferentSize,
        "skip" => ConflictPolicy::Skip,
        "keep-both" => ConflictPolicy::KeepBoth,
        _ => return Err(JsErrorBox::type_error(format!("unknown conflict policy {name:?}"))),
    };
    Ok(policy)
}

/// What `run` reports.
#[derive(Serialize)]
struct RunReport {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Largest output kept from a program; the rest is dropped.
const RUN_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

/// Starts a program the plugin was granted, without a shell: the arguments
/// reach it unchanged.
#[op2]
#[serde]
async fn op_pnxr_run(
    state: Rc<RefCell<OpState>>,
    #[string] program: String,
    #[serde] args: Vec<String>,
    #[string] cwd: Option<String>,
) -> Result<RunReport, JsErrorBox> {
    let (base, allowed) = {
        let state = state.borrow();
        let script = state.borrow::<ScriptState>();
        (script.base.clone(), script.permissions.check_run(&program))
    };
    allowed.map_err(|e| JsErrorBox::new("PermissionDenied", e))?;
    let dir = match cwd {
        Some(text) => {
            let path = locate(&state, &text, Access::Read)?.1;
            path.to_local_path()
                .ok_or_else(|| JsErrorBox::type_error("cwd must be a local folder"))?
        }
        None => base,
    };
    let output = tokio::process::Command::new(&program)
        .args(&args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                JsErrorBox::generic(format!("{program} is not installed (not found on the PATH)"))
            } else {
                JsErrorBox::generic(format!("cannot run {program}: {e}"))
            }
        })?;
    let text = |bytes: &[u8]| {
        String::from_utf8_lossy(&bytes[..bytes.len().min(RUN_OUTPUT_LIMIT)]).into_owned()
    };
    Ok(RunReport {
        code: output.status.code().unwrap_or(-1),
        stdout: text(&output.stdout),
        stderr: text(&output.stderr),
    })
}
