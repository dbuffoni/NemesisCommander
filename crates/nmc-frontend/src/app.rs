//! The application state and everything that changes it.
//!
//! [`App`] never touches the terminal. It receives key presses
//! ([`App::handle_key`]) and results of background work
//! ([`App::handle_event`]); slow work (listing, copying, hashing, ...) runs
//! in tokio tasks that report back through an [`AppEvent`] channel. This
//! keeps the interface responsive and makes the whole application testable
//! without a terminal.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use nmc_app::AppContext;
use nmc_archive::{ArchiveFormat, PackOptions, archive_root};
use nmc_config::{Action, Bookmark, ExportBundle, ImportMode, Key, Keymap};
use nmc_index::{Mask, SearchCriteria, SearchHit, SortKey, SortSpec};
use nmc_ops::{
    Conflict, ConflictAction, ConflictResolver, DeleteOptions, RenameRule, Resolution,
    apply_renames, delete_paths, preview_renames, rename_conflicts, renamed,
};
use nmc_panel::{Listing, LoadRequest, Panel, SideId, Workspace};
use nmc_transfer::{JobEvent, JobId, JobInfo, JobKind, JobSpec, JobState, TransferManager};
use nmc_vfs::{Capabilities, EntryKind, HashAlgorithm, VfsPath, collect_listing};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::dialogs::{
    ChooseDialog, ChoosePurpose, ConfirmDialog, ConfirmPurpose, Forget, InputDialog, InputPurpose,
    JobsView, JumpRow, MessageDialog, Overlay, PendingConflict, Secret,
};
use crate::input::KeyPress;
use crate::viewer::{self, QUICK_VIEW_LIMIT, VIEW_LIMIT, Viewer};

mod archives;
mod diskwork;
mod features;
mod generate;
mod logins;
mod palette;
mod plugin_store;
mod plugin_views;
mod plugins;
mod pointer;
mod settings_screen;
mod terminal;

pub use plugin_store::{RepoRow, StoreRow};
pub use plugin_views::{ColumnCells, ColumnHead};
pub use plugins::{PluginChoice, plugin_short_name};

/// Results of background work, delivered to [`App::handle_event`].
#[derive(Debug)]
pub enum AppEvent {
    /// A directory listing arrived.
    Listing(Listing),
    /// The transfer queue changed.
    Job(JobEvent),
    /// A copy needs the user's decision.
    Conflict(PendingConflict),
    /// A background task finished.
    TaskDone {
        /// Status line text.
        message: String,
        /// Whether it is an error.
        error: bool,
        /// Reload the panels.
        reload: bool,
        /// Put the cursor of the focused panel on this name after reloading.
        focus: Option<String>,
    },
    /// Unpacking found the archive encrypted and without a (right) password:
    /// ask for it, then unpack again.
    NeedPassword(VfsPath),
    /// The plugin columns of a folder were computed.
    PluginCells(Box<ColumnCells>),
    /// The text of a folder's `nmc_folder.md` arrived.
    FolderText {
        /// The folder.
        path: VfsPath,
        /// The document, `None` when the file could not be read.
        blocks: Option<Vec<nmc_preview::rich::Block>>,
    },
    /// The git status of a folder arrived (`None`: not a repository).
    Git {
        /// The folder.
        path: VfsPath,
        /// Its status.
        status: Option<Box<crate::git::GitStatus>>,
    },
    /// A background task wants to show a window.
    Show(Box<Overlay>),
    /// A line for the log window titled `title`, which is still open while
    /// its work goes on.
    Progress {
        /// The title of the window.
        title: String,
        /// The line, as it happened.
        line: String,
    },
    /// A task changed files in a way Ctrl+Z can take back; the event is the
    /// task's own result.
    Undoable(UndoStep, Box<AppEvent>),
    /// One search result.
    SearchHit(SearchHit),
    /// The search ended (with an error message, if it failed).
    SearchDone(Option<String>),
    /// Contents for the viewer or the quick view.
    Loaded {
        /// Which file.
        path: VfsPath,
        /// For the quick view (otherwise the full viewer).
        quick: bool,
        /// Data, "file is longer" and its preview, or an error.
        result: Result<(Arc<Vec<u8>>, bool, crate::preview::Prepared), String>,
    },
    /// A picture for the viewer or quick view was rendered.
    Rendered {
        /// Which file.
        path: VfsPath,
        /// For the quick view (otherwise the full viewer).
        quick: bool,
        /// The request it answers.
        generation: u64,
        /// The picture and its PNG file (windows only), or an error.
        result: Result<(Arc<image::RgbaImage>, Option<PathBuf>), String>,
    },
    /// Claude wrote a multi-rename rule for these items (or failed).
    AiRenameDone {
        /// The items to rename.
        paths: Vec<VfsPath>,
        /// The rule and Claude's explanation, or why there is none.
        result: Result<(RenameRule, String), String>,
    },
    /// A synchronization deleted what it had to; these copies are next.
    SyncCopies {
        /// Sources and the folder they are copied into.
        copies: Vec<(Vec<VfsPath>, VfsPath)>,
        /// How many items were deleted.
        deleted: usize,
        /// What went wrong, if something did.
        error: Option<String>,
    },
    /// A comparison by content: select these names.
    Compared {
        /// The names to select on each side.
        marks: Vec<(SideId, Vec<String>)>,
        /// The result for the status line.
        message: String,
    },
    /// The plugin list was loaded.
    PluginStore {
        /// The rows and their text.
        rows: Vec<(String, StoreRow)>,
        /// What went wrong with some repositories.
        problems: Vec<String>,
    },
    /// A plugin repository to add was checked.
    PluginRepoChecked {
        /// The repository.
        repository: nmc_config::PluginRepository,
        /// How many plugins it lists, or why it cannot be used.
        result: Result<usize, String>,
    },
    /// A plugin command or script ended.
    ScriptDone {
        /// What ran, for messages.
        title: String,
        /// What it printed.
        lines: Vec<String>,
        /// Why it failed, if it did.
        error: Option<String>,
    },
}

impl AppEvent {
    /// Whether this event ends one background task started with
    /// [`App::spawn_task`].
    fn ends_task(&self) -> bool {
        if let AppEvent::Undoable(_, inner) = self {
            return inner.ends_task();
        }
        matches!(
            self,
            AppEvent::TaskDone { .. }
                | AppEvent::NeedPassword(_)
                | AppEvent::Git { .. }
                | AppEvent::FolderText { .. }
                | AppEvent::PluginCells(_)
                | AppEvent::Show(_)
                | AppEvent::SearchDone(_)
                | AppEvent::Loaded { .. }
                | AppEvent::Rendered { .. }
                | AppEvent::ScriptDone { .. }
                | AppEvent::PluginStore { .. }
                | AppEvent::PluginRepoChecked { .. }
                | AppEvent::AiRenameDone { .. }
                | AppEvent::SyncCopies { .. }
                | AppEvent::Compared { .. }
        )
    }
}

/// The status line message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Text.
    pub text: String,
    /// Shown as an error.
    pub error: bool,
}

/// A program to run with the terminal handed over (the editor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    /// Program and arguments.
    pub command: Vec<String>,
    /// The settings file was edited: reload settings afterwards.
    pub reload_settings: bool,
    /// The file of names being edited for "rename in the editor", and the
    /// items they belong to.
    pub rename: Option<(PathBuf, Vec<VfsPath>)>,
}

/// Rows PageUp and PageDown move the text of a dialog: less than the
/// smallest window shows, so that nothing is jumped over.
const DIALOG_PAGE: usize = 6;

/// Where `press` scrolls the text of `overlay` to, from `at`; `None` when
/// the key is not for scrolling it. Messages (not logs, which follow their
/// end), questions to answer with yes or no and, in a window, the main
/// shortcuts (the terminal lays them out to fit) scroll with the arrows,
/// PageUp/PageDown and Home/End; the lines
/// under a typed answer with the arrows and PageUp/PageDown (Home and End
/// move in the answer).
fn scrolled_dialog(
    overlay: &Overlay,
    press: &KeyPress,
    at: usize,
    graphical: bool,
) -> Option<usize> {
    let message = match overlay {
        Overlay::Message(dialog) if !dialog.is_log() => true,
        Overlay::Confirm(_) => true,
        Overlay::Help if graphical => true,
        Overlay::Input(_) => false,
        _ => return None,
    };
    Some(match press.plain()? {
        Key::Up => at.saturating_sub(1),
        Key::Down => at.saturating_add(1),
        Key::PageUp => at.saturating_sub(DIALOG_PAGE),
        Key::PageDown => at.saturating_add(DIALOG_PAGE),
        Key::Home if message => 0,
        Key::End if message => usize::MAX,
        _ => return None,
    })
}

/// What identifies a question of a series: a window and what it asks.
/// `None` for windows that are not steps (viewer, lists of commands...).
fn step_of(overlay: &Overlay) -> Option<(u8, String, String)> {
    match overlay {
        Overlay::Input(d) if d.purpose.settings_screen().is_none() => {
            Some((0, d.title.clone(), d.prompt.clone()))
        }
        Overlay::Choose(d) => Some((1, d.title.clone(), String::new())),
        Overlay::Confirm(d) => {
            Some((2, d.title.clone(), d.lines.first().cloned().unwrap_or_default()))
        }
        Overlay::Message(d) if !d.busy => Some((3, d.title.clone(), String::new())),
        _ => None,
    }
}

/// A copy of a window that is a step, to come back to.
fn snapshot(overlay: &Overlay) -> Option<Overlay> {
    step_of(overlay)?;
    Some(match overlay {
        Overlay::Input(d) => Overlay::Input(d.clone()),
        Overlay::Choose(d) => Overlay::Choose(d.clone()),
        Overlay::Confirm(d) => Overlay::Confirm(d.clone()),
        Overlay::Message(d) => Overlay::Message(d.clone()),
        _ => return None,
    })
}

/// Asks the user about copy conflicts through the event channel.
struct DialogResolver {
    events: mpsc::UnboundedSender<AppEvent>,
}

#[async_trait]
impl ConflictResolver for DialogResolver {
    async fn resolve(&self, conflict: &Conflict) -> Resolution {
        let (reply, answer) = oneshot::channel();
        let pending = PendingConflict { conflict: conflict.clone(), reply };
        if self.events.send(AppEvent::Conflict(pending)).is_err() {
            return Resolution { action: ConflictAction::Abort, apply_to_all: true };
        }
        // If the interface goes away, stop rather than guess.
        answer.await.unwrap_or(Resolution { action: ConflictAction::Abort, apply_to_all: true })
    }
}

/// When a job was first seen, started and ended, and the bytes it moved.
#[derive(Debug, Clone)]
pub struct JobClock {
    /// First seen in the queue.
    pub queued: std::time::Instant,
    /// Started running or moving data.
    pub started: Option<std::time::Instant>,
    /// Reached a final state.
    pub ended: Option<std::time::Instant>,
    /// Bytes reported by progress events.
    pub moved: u64,
}

impl JobClock {
    fn new() -> Self {
        Self { queued: std::time::Instant::now(), started: None, ended: None, moved: 0 }
    }
}

/// The whole interactive application, without its drawing.
///
/// The public fields are what a front end draws; it changes only `scroll`
/// and `page`, which describe its own window.
pub struct App {
    /// Settings, key map and providers.
    pub context: AppContext,
    /// The two panels with their tabs.
    pub workspace: Workspace,
    /// The persistent transfer queue.
    pub manager: TransferManager,
    /// The open dialog or window, if any.
    pub overlay: Option<Overlay>,
    /// Copy conflicts waiting for an answer, oldest first.
    pub conflicts: VecDeque<PendingConflict>,
    /// Servers the install of `nmc serve` was already offered for.
    install_offered: HashSet<String>,
    /// The questions that led to the one on screen, for going back.
    history: Vec<Overlay>,
    /// The message of the status line.
    pub status: Option<Status>,
    /// The jobs of the transfer queue, as last reported.
    pub jobs: Vec<JobInfo>,
    /// Bytes done of running jobs, updated more often than `jobs`.
    pub live_bytes: HashMap<JobId, u64>,
    /// When the job list was last read because a running job had no size yet.
    sizes_refreshed: Option<std::time::Instant>,
    /// How the data of each job travelled between two servers, once known
    /// (`direct` or `reverse`).
    pub job_routes: HashMap<JobId, String>,
    /// Transfer speed of the running jobs.
    pub speed: crate::speed::SpeedMeter,
    /// Times and bytes of the jobs seen in this session, for the queue window.
    pub job_clock: HashMap<JobId, JobClock>,
    /// Readouts that change at most every three seconds.
    pub readouts: crate::speed::Steady,
    /// The quick view (Ctrl+Q) and the file it shows.
    pub quick_view: Option<Option<Viewer>>,
    /// First visible row of each side's list, kept by the renderer.
    pub scroll: [usize; 2],
    /// Rows per page, measured by the renderer.
    pub page: usize,
    /// Rows the text of the open dialog is scrolled down. The keys move it;
    /// the renderer, which knows how much text there is, keeps it in range.
    pub dialog_scroll: usize,
    /// Columns of text the viewer shows, measured by the renderer
    /// (formatted documents are laid out for it).
    pub columns: usize,
    /// The front end is a desktop window, not a terminal: "Edit" then opens
    /// a graphical editor instead of `$EDITOR`, which usually needs a
    /// terminal.
    pub graphical: bool,
    events: mpsc::UnboundedSender<AppEvent>,
    /// The basket: items collected from several folders to act on together.
    pub basket: Vec<VfsPath>,
    /// Text waiting to go to the system clipboard.
    clipboard: Option<String>,
    /// Where the terminal interface writes the folder it ends in.
    cwd_file: Option<std::path::PathBuf>,
    /// Encrypted archives whose password the user refused to give (until
    /// they are left).
    password_declined: HashSet<String>,
    /// An archive being unpacked that needs its password first.
    password_wanted: Option<VfsPath>,
    /// How often and how recently folders were opened (the jump command).
    frecency: crate::frecency::Frecency,
    /// The installed font families, for the font settings (empty in a terminal).
    fonts: Vec<String>,
    /// The folder last recorded for each side.
    visited: [Option<String>; 2],
    /// The last search, to save it.
    last_search: Option<nmc_config::SavedSearch>,
    /// Items for the commands to act on instead of the panel's selection
    /// (the basket's).
    target_override: Option<Vec<VfsPath>>,
    /// The selection in the panels comes from comparing folders: Esc
    /// clears it in both.
    compare_marks: bool,
    /// Ctrl, Alt and Shift as held right now (the function key bar shows
    /// what those keys do).
    held: (bool, bool, bool),
    /// Git status of what each side shows, with the folder it is for.
    pub git: [Option<(VfsPath, Arc<crate::git::GitStatus>)>; 2],
    /// The plugin columns of the folder of each panel.
    pub column_cells: [ColumnCells; 2],
    /// The text of `nmc_folder.md` in the folder of each panel.
    pub folder_text: [Option<(VfsPath, Vec<nmc_preview::rich::Block>)>; 2],
    /// Rows the text of `nmc_folder.md` takes in each panel, measured by the
    /// renderer (the mouse needs them).
    pub note_rows: [usize; 2],
    /// Per panel: its last listing failed with a lost connection, and no
    /// listing has worked since (the next one opens the connection again).
    pub connection_lost: [bool; 2],
    undo: Vec<UndoStep>,
    undo_jobs: HashSet<JobId>,
    tasks: usize,
    search_cancel: Option<CancellationToken>,
    external: Option<External>,
    running: bool,
    /// `nmc pick`: the application is a chooser (see [`PickMode`]).
    picker: Option<PickMode>,
    /// What the user chose in pick mode (set together with `running = false`).
    picked: Option<Vec<VfsPath>>,
}

/// How the application behaves when it is started as a chooser, to give
/// paths to another program (`nmc pick`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PickMode {
    /// Choose a folder (the one shown) instead of files.
    pub folder: bool,
    /// Several items may be chosen (the marked ones).
    pub multiple: bool,
}

/// Something Ctrl+Z can take back: each item is `(path now, path before)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoStep {
    /// Items renamed.
    Renamed(Vec<(VfsPath, VfsPath)>),
    /// A folder created.
    Created(VfsPath),
    /// Items moved by a finished job.
    Moved(Vec<(VfsPath, VfsPath)>),
}

/// The file whose Markdown text is shown at the top of a panel:
/// `nmc_folder.md`.
pub const FOLDER_TEXT_NAMES: [&str; 1] = ["nmc_folder.md"];

/// The largest folder text read.
const FOLDER_TEXT_LIMIT: u64 = 64 * 1024;

/// How many steps Ctrl+Z remembers.
const UNDO_DEPTH: usize = 50;

impl App {
    /// Asks git, in the background, about a local folder that was just listed.
    fn refresh_git(&mut self, path: &VfsPath) {
        if !self.context.config.panels.git_status || path.scheme() != "file" {
            return;
        }
        let Some(dir) = path.to_local_path() else { return };
        let path = path.clone();
        self.spawn_task(async move {
            let status = tokio::task::spawn_blocking(move || crate::git::read(&dir))
                .await
                .ok()
                .flatten()
                .map(Box::new);
            AppEvent::Git { path, status }
        });
    }

    /// Reads `nmc_folder.md` of a folder that was just listed, if it has one.
    fn refresh_folder_text(&mut self, path: &VfsPath) {
        for side in [SideId::Left, SideId::Right] {
            if self.workspace.side(side).panel().location() == path {
                self.folder_text[side_index(side)] = None;
            }
        }
        let Some(panel) = [SideId::Left, SideId::Right]
            .into_iter()
            .map(|side| self.workspace.side(side).panel())
            .find(|panel| panel.location() == path)
        else {
            return;
        };
        let Some(entry) = panel
            .entries()
            .iter()
            .find(|e| FOLDER_TEXT_NAMES.contains(&e.name.as_str()) && !e.is_dir())
        else {
            return;
        };
        if entry.size > FOLDER_TEXT_LIMIT {
            return;
        }
        let file = path.join(&entry.name);
        let registry = self.context.registry.clone();
        let path = path.clone();
        self.spawn_task(async move {
            let blocks = match viewer::load(&registry, &file, FOLDER_TEXT_LIMIT).await {
                Ok((data, _)) => {
                    Some(nmc_preview::markdown::parse(&String::from_utf8_lossy(&data)))
                }
                Err(_) => None,
            };
            AppEvent::FolderText { path, blocks }
        });
    }

    fn remember_undo(&mut self, step: UndoStep) {
        self.undo.push(step);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    /// Takes back the last rename, new folder or move.
    fn undo_last(&mut self) {
        let Some(step) = self.undo.pop() else {
            return self.set_status("Nothing to undo");
        };
        let registry = self.context.registry.clone();
        match step {
            UndoStep::Renamed(pairs) => {
                let (now, before): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
                let names: Vec<String> =
                    before.iter().map(|p| p.file_name().unwrap_or_default().to_owned()).collect();
                let count = now.len();
                self.spawn_task(async move {
                    let result = apply_renames(&registry, &now, &names).await;
                    done(result.map(|()| format!("Undone: renamed {count} item(s) back")), None)
                });
            }
            UndoStep::Created(path) => self.spawn_task(async move {
                let result = async {
                    let provider = registry.provider_for(&path).await?;
                    provider.remove_dir(&path).await
                }
                .await;
                done(
                    result.map(|()| format!("Undone: removed {}", path.file_name().unwrap_or(""))),
                    None,
                )
            }),
            UndoStep::Moved(items) => {
                // Back to where each item came from, one job per folder.
                let mut by_folder: Vec<(VfsPath, Vec<VfsPath>)> = Vec::new();
                for (now, before) in items {
                    let Some(folder) = before.parent() else { continue };
                    match by_folder.iter_mut().find(|(f, _)| *f == folder) {
                        Some((_, list)) => list.push(now),
                        None => by_folder.push((folder, vec![now])),
                    }
                }
                for (folder, list) in by_folder {
                    // The move back is a move too; it is not remembered.
                    if let Some(id) = self.submit_job(JobKind::Move, list, folder) {
                        self.undo_jobs.insert(id);
                    }
                }
            }
        }
    }
}

/// Index of a side in per-side arrays.
pub fn side_index(side: SideId) -> usize {
    match side {
        SideId::Left => 0,
        SideId::Right => 1,
    }
}

/// One row of the bookmark list: name, folder and its targets for the
/// other panel (`→ target`, or `→ 3 targets` when there are several).
pub(crate) fn bookmark_row(bookmark: &Bookmark) -> String {
    let row = format!("{:<20} {}", bookmark.name, bookmark.path);
    match bookmark.targets.as_slice() {
        [] => row,
        [target] => format!("{row}  → {target}"),
        targets => format!("{row}  → {} targets", targets.len()),
    }
}

/// How a location is shown to the user: plain paths for local folders,
/// URIs for everything else.
pub fn display_location(path: &VfsPath) -> String {
    match path.to_local_path() {
        Some(local) if path.scheme() == "file" => {
            let home = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
            abbreviate_home(&local, home.as_deref())
        }
        _ => path.to_string(),
    }
}

/// `local` with the home folder written as `~` (which
/// [`resolve_location`] reads back).
fn abbreviate_home(local: &std::path::Path, home: Option<&std::path::Path>) -> String {
    let separator = std::path::MAIN_SEPARATOR;
    match home.filter(|home| home.parent().is_some()).and_then(|home| local.strip_prefix(home).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~{separator}{}", rest.display()),
        None => local.display().to_string(),
    }
}

/// Turns typed text into a location: a URI, `~/...`, an absolute local
/// path, or a path relative to `base` (which works for every provider).
pub fn resolve_location(text: &str, base: &VfsPath) -> Result<VfsPath, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("no location given".into());
    }
    let result = if text.contains("://") {
        VfsPath::parse(text)
    } else if text == "~" || text.starts_with("~/") || text.starts_with("~\\") {
        let home = directories::UserDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .ok_or_else(|| "cannot find the home folder".to_string())?;
        VfsPath::from_local(home.join(text.trim_start_matches('~').trim_start_matches(['/', '\\'])))
    } else if Path::new(text).is_absolute() {
        VfsPath::from_local(text)
    } else {
        Ok(base.join(text))
    };
    result.map_err(|e| e.to_string())
}

impl App {
    /// Creates the application showing `left` and `right`, starts the
    /// transfer queue and requests the first listings. Must be called inside
    /// a tokio runtime. Returns the receiver of background results, to be
    /// fed to [`handle_event`](Self::handle_event).
    pub fn new(
        context: AppContext,
        left: VfsPath,
        right: VfsPath,
    ) -> Result<(App, mpsc::UnboundedReceiver<AppEvent>), String> {
        let (events, receiver) = mpsc::unbounded_channel();
        let resolver = Arc::new(DialogResolver { events: events.clone() });
        let manager = context.transfer_manager_with(Some(resolver)).map_err(|e| e.to_string())?;
        manager.spawn();
        let mut job_events = manager.subscribe();
        let forward = events.clone();
        tokio::spawn(async move {
            loop {
                match job_events.recv().await {
                    Ok(event) => {
                        if forward.send(AppEvent::Job(event)).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let panels = &context.config.panels;
        let sort = SortSpec { key: panels.sort, descending: panels.descending };
        nmc_panel::set_hide_ds_store(panels.hide_ds_store);
        let (workspace, requests) = Workspace::new(left, right, sort, panels.show_hidden);
        let status = (!context.warnings.is_empty())
            .then(|| Status { text: context.warnings.join("; "), error: true });
        let frecency =
            crate::frecency::Frecency::load(&context.paths.data_dir.join("frecency.json"));
        let mut app = App {
            basket: Vec::new(),
            clipboard: None,
            cwd_file: None,
            password_declined: HashSet::new(),
            password_wanted: None,
            frecency,
            fonts: Vec::new(),
            visited: [None, None],
            last_search: None,
            target_override: None,
            compare_marks: false,
            held: (false, false, false),
            context,
            workspace,
            manager,
            overlay: None,
            conflicts: VecDeque::new(),
            install_offered: HashSet::new(),
            history: Vec::new(),
            status,
            jobs: Vec::new(),
            live_bytes: HashMap::new(),
            sizes_refreshed: None,
            job_routes: HashMap::new(),
            speed: crate::speed::SpeedMeter::default(),
            job_clock: HashMap::new(),
            readouts: crate::speed::Steady::default(),
            git: [None, None],
            folder_text: [None, None],
            column_cells: [ColumnCells::default(), ColumnCells::default()],
            note_rows: [0, 0],
            connection_lost: [false, false],
            undo: Vec::new(),
            undo_jobs: HashSet::new(),
            quick_view: None,
            scroll: [0, 0],
            page: 20,
            dialog_scroll: 0,
            columns: 80,
            events,
            tasks: 0,
            search_cancel: None,
            external: None,
            running: true,
            picker: None,
            picked: None,
            graphical: false,
        };
        for (_, request) in requests {
            app.load(request);
        }
        app.refresh_jobs();
        Ok((app, receiver))
    }

    // ------------------------------------------------------------ queries

    /// `false` once the user asked to quit.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Turns the application into a chooser: Enter on a file (or Ctrl+F10)
    /// ends it, and what was chosen is in [`picked`](Self::picked).
    pub fn set_picker(&mut self, mode: PickMode) {
        self.picker = Some(mode);
        self.set_status(if mode.folder {
            "Choose a folder: Ctrl+F10 picks this one, Esc cancels"
        } else {
            "Choose a file: Enter picks it, Ctrl+F10 picks a folder too, Esc cancels"
        });
    }

    /// What the user chose in pick mode: `None` when nothing was chosen
    /// (the user cancelled).
    pub fn picked(&self) -> Option<&[VfsPath]> {
        self.picked.as_deref()
    }

    /// Ends pick mode with `items`.
    fn finish_pick(&mut self, items: Vec<VfsPath>) {
        if !items.is_empty() {
            self.picked = Some(items);
            self.running = false;
        }
    }

    /// The items Ctrl+F10 chooses: the current folder in folder mode, else the
    /// marked items (several allowed) or the one under the cursor.
    fn choose_now(&mut self) {
        let Some(mode) = self.picker else {
            return self.set_status("Choosing works only in nmc pick");
        };
        let panel = self.workspace.active();
        let items = if mode.folder {
            vec![panel.location().clone()]
        } else if mode.multiple {
            panel.targets()
        } else {
            panel.current_path().into_iter().collect()
        };
        self.finish_pick(items);
    }

    /// The panels.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The window shown over the panels.
    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    /// The status line message.
    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Tells the font settings which font families are installed (the
    /// window does; a terminal has no list and uses its own font).
    pub fn set_fonts(&mut self, mut fonts: Vec<String>) {
        fonts.sort_by_key(|f| f.to_lowercase());
        fonts.dedup();
        self.fonts = fonts;
    }

    /// The settings and key map in use.
    pub fn context(&self) -> &AppContext {
        &self.context
    }

    /// The jobs as last seen.
    pub fn jobs(&self) -> &[JobInfo] {
        &self.jobs
    }

    /// The conflict waiting for an answer, if any.
    pub fn pending_conflict(&self) -> Option<&Conflict> {
        self.conflicts.front().map(|p| &p.conflict)
    }

    /// Whether background work is still going on: listings, tasks or jobs
    /// that are queued or running. Tests wait for this to become `false`.
    pub fn is_busy(&self) -> bool {
        let loading = [SideId::Left, SideId::Right]
            .iter()
            .any(|&side| self.workspace.side(side).tabs().iter().any(Panel::is_loading));
        let jobs =
            self.jobs.iter().any(|j| matches!(j.state, JobState::Queued | JobState::Running));
        loading || jobs || self.tasks > 0
    }

    /// Takes the external program the main loop should run, if any.
    pub fn take_external(&mut self) -> Option<External> {
        self.external.take()
    }

    // ----------------------------------------------------- background work

    fn load(&self, request: LoadRequest) {
        let registry = self.context.registry.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            let listing = nmc_panel::load(&registry, request).await;
            let _ = events.send(AppEvent::Listing(listing));
        });
    }

    fn load_opt(&self, request: Option<LoadRequest>) {
        if let Some(request) = request {
            self.load(request);
        }
    }

    /// The active panel could not open an `scs://` server that is not
    /// there (nothing answers, or no certificates yet): offers to install
    /// `nmc serve` instead of leaving the user with a bare error.
    fn offer_install_for(&mut self, location: &VfsPath, error: &str) {
        if location.scheme() != "scs" || self.overlay.is_some() {
            return;
        }
        let lower = error.to_lowercase();
        let missing =
            ["refused", "no answer", "timed out", "no certificates", "handshake", "certificate"]
                .iter()
                .any(|word| lower.contains(word));
        let Ok((host, _)) = nmc_agent::serve::host_and_port(location.authority()) else { return };
        if !missing || !self.install_offered.insert(host.clone()) {
            return;
        }
        self.offer_install(&host, error);
    }

    /// Runs `work` in the background; its result event ends the task.
    fn spawn_task(&mut self, work: impl Future<Output = AppEvent> + Send + 'static) {
        self.tasks += 1;
        let events = self.events.clone();
        tokio::spawn(async move {
            let _ = events.send(work.await);
        });
    }

    /// Frees the archives no tab shows any more (a panel left them). Not
    /// while transfers run: they may be reading from one.
    fn release_archives(&self) {
        if self.jobs.iter().any(|j| matches!(j.state, JobState::Queued | JobState::Running)) {
            return;
        }
        let in_use: Vec<VfsPath> = [SideId::Left, SideId::Right]
            .iter()
            .flat_map(|&side| self.workspace.side(side).tabs().iter())
            .map(|panel| panel.location().clone())
            .collect();
        self.context.registry.release_archives(&in_use);
    }

    fn reload_panels(&mut self, focus: Option<&str>) {
        let active = self.workspace.active_side();
        for side in [SideId::Left, SideId::Right] {
            let panel = self.workspace.side_mut(side).panel_mut();
            let request = match focus {
                Some(name) if side == active => panel.reload_focus(name),
                _ => panel.reload(),
            };
            self.load(request);
        }
    }

    fn refresh_jobs(&mut self) {
        match self.manager.jobs() {
            // Newest first: what was just started is on top.
            Ok(mut jobs) => {
                jobs.sort_by_key(|j| std::cmp::Reverse(j.id.0));
                self.jobs = jobs;
            }
            Err(e) => self.set_error(format!("transfer queue: {e}")),
        }
        for job in &self.jobs {
            self.job_clock.entry(job.id).or_insert_with(JobClock::new);
        }
        if crate::view::jobs_progress(&self.jobs, self).is_none() {
            self.speed.clear();
            self.readouts.forget("status");
        }
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(Status { text: text.into(), error: false });
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(Status { text: text.into(), error: true });
    }

    /// Applies the result of background work.
    pub fn handle_event(&mut self, event: AppEvent) {
        self.apply_event(event);
        self.ask_archive_password();
    }

    fn apply_event(&mut self, event: AppEvent) {
        let event = match event {
            AppEvent::Undoable(step, inner) => {
                if !matches!(*inner, AppEvent::TaskDone { error: true, .. }) {
                    self.remember_undo(step);
                }
                *inner
            }
            other => other,
        };
        if event.ends_task() {
            self.tasks = self.tasks.saturating_sub(1);
        }
        match event {
            AppEvent::PluginCells(cells) => self.set_plugin_cells(*cells),
            AppEvent::FolderText { path, blocks } => {
                for side in [SideId::Left, SideId::Right] {
                    if self.workspace.side(side).panel().location() == &path {
                        self.folder_text[side_index(side)] =
                            blocks.clone().map(|b| (path.clone(), b));
                    }
                }
            }
            AppEvent::Git { path, status } => {
                let status = status.map(|s| Arc::new(*s));
                for side in [SideId::Left, SideId::Right] {
                    if self.workspace.side(side).panel().location() == &path {
                        self.git[side_index(side)] = status.clone().map(|s| (path.clone(), s));
                    }
                }
            }
            AppEvent::Listing(listing) => {
                let listed = listing.path.clone();
                let failed = listing.result.as_ref().err().map(|e| e.to_string());
                if self.workspace.apply(listing) {
                    for side in [SideId::Left, SideId::Right] {
                        let panel = self.workspace.side(side).panel();
                        if panel.location() == &listed {
                            self.connection_lost[side_index(side)] =
                                panel.error().is_some_and(nmc_ops::is_connection_error);
                        }
                    }
                    self.record_visits(&listed);
                    // The marks of an earlier look at this folder are gone
                    // at once; git is asked again.
                    for side in [SideId::Left, SideId::Right] {
                        if self.workspace.side(side).panel().location() == &listed {
                            self.git[side_index(side)] = None;
                        }
                    }
                    self.refresh_git(&listed);
                    self.refresh_folder_text(&listed);
                    self.refresh_plugin_columns(&listed);
                    self.release_archives();
                    if let Some(error) = self.workspace.active().error() {
                        let mut error = error.to_owned();
                        if nmc_ops::is_connection_error(&error) {
                            // The next listing opens the connection again.
                            self.context.registry.forget(self.workspace.active().location());
                            error = format!(
                                "Connection lost: {error} (it is opened again on the next try)"
                            );
                        }
                        self.set_error(error);
                    }
                    // The panel stays where it was when a folder cannot be
                    // opened, so the place that failed is the listing's.
                    if let Some(error) = failed {
                        self.offer_install_for(&listed, &error);
                    }
                    self.update_quick_view();
                }
            }
            AppEvent::Job(JobEvent::Retrying { id, attempt, attempts, message }) => {
                let title = self
                    .jobs
                    .iter()
                    .find(|j| j.id == id)
                    .map(|j| j.spec.title())
                    .unwrap_or_default();
                self.set_status(format!("Retrying {title} ({attempt}/{attempts}): {message}…"));
            }
            AppEvent::Job(JobEvent::ParallelismChanged { .. }) => self.refresh_jobs(),
            AppEvent::Job(JobEvent::EndpointLimit { endpoint, limit }) => {
                self.remember_endpoint_limit(&endpoint, limit)
            }
            AppEvent::Job(JobEvent::Route { id, route }) => {
                self.job_routes.insert(id, route.clone());
                self.set_status(format!("Copying {route} between the servers (not through here)"));
            }
            AppEvent::Job(JobEvent::Progress { id, bytes }) => {
                *self.live_bytes.entry(id).or_default() += bytes;
                // The size of a job is known once it is planned: read it again then.
                let unsized_job = self.jobs.iter().any(|j| j.id == id && j.total_bytes == 0);
                let now = std::time::Instant::now();
                if unsized_job
                    && self.sizes_refreshed.is_none_or(|t| {
                        now.duration_since(t) > std::time::Duration::from_millis(500)
                    })
                {
                    self.sizes_refreshed = Some(now);
                    self.refresh_jobs();
                }
                self.speed.record(std::time::Instant::now(), bytes);
                let clock = self.job_clock.entry(id).or_insert_with(JobClock::new);
                clock.started.get_or_insert(now);
                clock.moved += bytes;
            }
            AppEvent::Job(JobEvent::StateChanged { id, state }) => {
                self.live_bytes.remove(&id);
                let clock = self.job_clock.entry(id).or_insert_with(JobClock::new);
                if state == JobState::Running {
                    clock.started.get_or_insert_with(std::time::Instant::now);
                }
                if state.is_final() {
                    clock.ended.get_or_insert_with(std::time::Instant::now);
                    self.readouts.forget(&format!("job{}", id.0));
                }
                self.refresh_jobs();
                if state.is_final() {
                    let job = self.jobs.iter().find(|j| j.id == id);
                    let title = job.map(|j| j.spec.title()).unwrap_or_default();
                    match state {
                        JobState::Completed => {
                            self.set_status(format!("Done: {title}"));
                            let own = self.undo_jobs.remove(&id);
                            if let Some(job) = self.jobs.iter().find(|j| j.id == id)
                                && !own
                                && job.spec.kind == JobKind::Move
                                && let Some(destination) = job.spec.destination.clone()
                            {
                                let items = job
                                    .spec
                                    .sources
                                    .iter()
                                    .filter_map(|s| {
                                        Some((destination.join(s.file_name()?), s.clone()))
                                    })
                                    .collect();
                                let moved = job.spec.sources.clone();
                                self.remember_undo(UndoStep::Moved(items));
                                self.basket_forget(&moved);
                            }
                        }
                        JobState::Failed => {
                            let reason = job.and_then(|j| j.errors.first()).cloned();
                            self.set_error(format!(
                                "Failed: {title}: {}",
                                reason.unwrap_or_else(|| "see the transfer queue".into())
                            ));
                        }
                        _ => self.set_status(format!("Cancelled: {title}")),
                    }
                    self.reload_panels(None);
                }
            }
            AppEvent::Conflict(pending) => self.conflicts.push_back(pending),
            AppEvent::TaskDone { message, error, reload, focus } => {
                if error {
                    self.set_error(message);
                } else if !message.is_empty() {
                    self.set_status(message);
                }
                if reload {
                    self.reload_panels(focus.as_deref());
                }
            }
            AppEvent::NeedPassword(archive) => {
                self.password_wanted = Some(archive);
                // The question comes from `ask_archive_password`.
            }
            AppEvent::Undoable(..) => {}
            AppEvent::Progress { title, line } => {
                if let Some(Overlay::Message(window)) = &mut self.overlay
                    && window.busy
                    && window.title == title
                {
                    window.lines.push(line);
                }
            }
            AppEvent::Show(overlay) => {
                self.dialog_scroll = 0;
                // "Measuring…" and the like are over once the window is there.
                if self.status.as_ref().is_some_and(|s| !s.error && s.text.ends_with('…')) {
                    self.status = None;
                }
                self.overlay = Some(*overlay);
            }
            AppEvent::SyncCopies { copies, deleted, error } => {
                let files = self.queue_sync_copies(copies);
                match error {
                    Some(e) => self.set_error(format!("Synchronize: {e}")),
                    None => self.set_status(format!(
                        "Synchronize: {deleted} deleted, {files} items queued for copying"
                    )),
                }
                self.reload_panels(None);
            }
            AppEvent::Compared { marks, message } => {
                for (side, names) in marks {
                    let panel = self.workspace.side_mut(side).panel_mut();
                    panel.clear_selection();
                    for name in &names {
                        panel.set_selected(name, true);
                    }
                }
                self.compare_marks = true;
                self.set_status(format!("{message} (differences are selected, Esc clears)"));
            }
            AppEvent::SearchHit(hit) => {
                let hint = self.basket_hint();
                if let Some(Overlay::Choose(dialog)) = &mut self.overlay
                    && let ChoosePurpose::SearchResults(paths) = &mut dialog.purpose
                {
                    let mut row = display_location(&hit.path);
                    if let Some((line, text)) = &hit.first_match {
                        row.push_str(&format!(":{line}: {text}"));
                    }
                    dialog.items.push(row);
                    paths.push(hit.path);
                    dialog.footer =
                        format!("{} found, searching…{}{hint}", paths.len(), crate::hints::SEP);
                }
            }
            AppEvent::SearchDone(error) => {
                self.search_cancel = None;
                let hint = self.basket_hint();
                if let Some(Overlay::Choose(dialog)) = &mut self.overlay
                    && let ChoosePurpose::SearchResults(paths) = &dialog.purpose
                {
                    dialog.footer = match &error {
                        Some(e) => {
                            format!(
                                "{} found, stopped: {e}{}{hint}",
                                paths.len(),
                                crate::hints::SEP
                            )
                        }
                        None => format!("{} found{}{hint}", paths.len(), crate::hints::SEP),
                    };
                }
            }
            AppEvent::AiRenameDone { paths, result } => match result {
                Ok((rule, explanation)) => self.confirm_renames(paths, &rule, Some(explanation)),
                Err(error) => self.set_error(format!("Multi-rename with AI: {error}")),
            },
            AppEvent::PluginStore { rows, problems } => self.show_plugin_store(rows, problems),
            AppEvent::PluginRepoChecked { repository, result } => {
                self.plugin_repo_checked(repository, result)
            }
            AppEvent::ScriptDone { title, lines, error } => {
                self.reload_panels(None);
                match (&error, lines.len()) {
                    (None, 0) => self.set_status(format!("{title}: done")),
                    (None, 1) => self.set_status(format!("{title}: {}", lines[0])),
                    _ => {
                        let mut text = lines;
                        if let Some(error) = &error {
                            text.push(format!("Stopped: {error}"));
                        }
                        self.overlay = Some(Overlay::Message(MessageDialog {
                            title: title.clone(),
                            lines: text,
                            then: None,
                            then_choose: None,
                            back_to_settings: None,
                            logo: false,
                            busy: false,
                        }));
                    }
                }
                if let Some(error) = error {
                    self.set_error(format!("{title}: {error}"));
                }
            }
            AppEvent::Loaded { path, quick, result } => {
                let tab_width = self.context.config.viewer.tab_width;
                let source = self.viewer_for(&path, quick).and_then(|viewer| match result {
                    Ok((data, truncated, prepared)) => {
                        viewer.set_loaded(data, truncated, prepared, tab_width)
                    }
                    Err(e) => {
                        viewer.error = Some(e);
                        None
                    }
                });
                if let Some(source) = source {
                    self.render_picture(path, quick, source);
                }
            }
            AppEvent::Rendered { path, quick, generation, result } => {
                match self.viewer_for(&path, quick) {
                    Some(viewer) => viewer.set_rendered(generation, result),
                    None => {
                        if let Ok((_, Some(file))) = result {
                            crate::preview::forget_png(&file);
                        }
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------- keys

    /// Handles one key press.
    pub fn handle_key(&mut self, press: KeyPress) {
        if !self.conflicts.is_empty() {
            self.conflict_key(press);
            return;
        }
        self.status = None;
        match self.overlay.take() {
            None => {
                self.history.clear();
                self.normal_key(press)
            }
            Some(overlay) => {
                if let Some(scroll) =
                    scrolled_dialog(&overlay, &press, self.dialog_scroll, self.graphical)
                {
                    self.dialog_scroll = scroll;
                    self.overlay = Some(overlay);
                    return;
                }
                self.dialog_scroll = 0;
                // Shift+Tab goes back to the question before this one.
                if press.chord.shift
                    && !press.chord.ctrl
                    && !press.chord.alt
                    && press.chord.key == Key::Tab
                    && step_of(&overlay).is_some()
                    && let Some(previous) = self.history.pop()
                {
                    self.overlay = Some(previous);
                    self.update_quick_view();
                    return;
                }
                let before = snapshot(&overlay);
                let keep = self.overlay_key(overlay, press);
                // An action may have opened another window; it wins.
                if self.overlay.is_none() {
                    self.overlay = keep;
                }
                self.remember_step(before);
            }
        }
        self.update_quick_view();
    }

    /// After a key: when the window on screen is the next question of a
    /// series, the one before it is kept for Shift+Tab; when the series
    /// is over, it is forgotten.
    fn remember_step(&mut self, before: Option<Overlay>) {
        let now = self.overlay.as_ref().and_then(step_of);
        match (before, now) {
            (Some(before), Some(now)) if step_of(&before).as_ref() != Some(&now) => {
                self.history.push(before);
            }
            (_, Some(_)) => {}
            (_, None) => self.history.clear(),
        }
    }

    /// Whether Shift+Tab would go back to an earlier question.
    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    /// `hint` with the key for going back added, when there is one.
    pub fn with_back(&self, hint: String) -> String {
        match (self.can_go_back(), hint.is_empty()) {
            (false, _) => hint,
            (true, true) => "Shift+Tab: back".into(),
            (true, false) => crate::hints::with_back(&hint),
        }
    }

    /// Text pasted from the system clipboard (Ctrl+V in the window, the
    /// terminal's own paste in the text UI). It goes where typing would:
    /// the field of a question, the filter of a list or the command search.
    /// Anywhere else it is ignored, so pasted text never runs commands.
    /// Only the first line is used; the questions are one line.
    pub fn paste(&mut self, text: &str) {
        let accepts = match &self.overlay {
            Some(Overlay::Input(_)) => true,
            Some(Overlay::Choose(dialog)) => dialog.query.is_some(),
            Some(Overlay::Commands(palette)) => !palette.recording,
            _ => false,
        };
        if !accepts || !self.conflicts.is_empty() {
            return;
        }
        let line = text.lines().next().unwrap_or_default();
        for c in line.chars().filter(|c| !c.is_control()) {
            self.handle_key(KeyPress::char(c));
        }
    }

    /// Which of Ctrl, Alt and Shift are held. Front ends call it when one is
    /// pressed or released; the function key bar then shows the commands of
    /// those combinations (`Shift+F5` shows what it does where `F5` copies).
    pub fn set_modifiers(&mut self, ctrl: bool, alt: bool, shift: bool) {
        self.held = (ctrl, alt, shift);
    }

    /// The modifiers held, as set by [`set_modifiers`](Self::set_modifiers).
    pub fn modifiers(&self) -> (bool, bool, bool) {
        self.held
    }

    fn normal_key(&mut self, press: KeyPress) {
        if self.quick_view_key(&press) {
            return;
        }
        let page = self.page.max(1) as isize;
        let panel = self.workspace.active_mut();
        // Shift+arrows (also with Ctrl) select as the cursor moves.
        let chord = press.chord;
        if chord.shift && !chord.alt {
            match chord.key {
                Key::Up => return panel.extend_selection(-1),
                Key::Down => return panel.extend_selection(1),
                Key::PageUp => return panel.extend_selection(-page),
                Key::PageDown => return panel.extend_selection(page),
                Key::Home => return panel.extend_selection(isize::MIN / 2),
                Key::End => return panel.extend_selection(isize::MAX / 2),
                _ => {}
            }
        }
        match press.plain() {
            Some(Key::Up) => return panel.move_cursor(-1),
            Some(Key::Down) => return panel.move_cursor(1),
            Some(Key::PageUp) => return panel.move_cursor(-page),
            Some(Key::PageDown) => return panel.move_cursor(page),
            Some(Key::Home) => return panel.set_cursor(0),
            Some(Key::End) => return panel.set_cursor(usize::MAX),
            Some(Key::Esc) if !panel.filter().is_empty() => return panel.set_filter(""),
            // The differences marked by Compare go away with Esc, in both
            // panels; any other selection only in the active one.
            Some(Key::Esc) if self.compare_marks => {
                self.compare_marks = false;
                self.workspace.active_mut().clear_selection();
                self.workspace.inactive_mut().clear_selection();
                return self.set_status("Comparison cleared");
            }
            Some(Key::Esc)
                if panel.selection_summary().files + panel.selection_summary().dirs > 0 =>
            {
                return panel.clear_selection();
            }
            // Pick mode: Esc, with nothing left to clear, cancels.
            Some(Key::Esc) if self.picker.is_some() => {
                self.running = false;
                return;
            }
            Some(Key::Left) if self.context.config.panels.arrow_navigation => {
                let request = panel.parent();
                return self.load_opt(request);
            }
            Some(Key::Right) if self.context.config.panels.arrow_navigation => {
                if let Some(request) = panel.enter() {
                    self.load(request);
                }
                return;
            }
            _ => {}
        }
        if let Some(action) = self.context.keymap.action_for(&press.chord) {
            self.perform(action);
        } else if let Some(id) = self.context.keymap.extra_for(&press.chord).map(str::to_owned)
            && !self.run_command(&id)
        {
            self.set_error(format!("{id}: no such plugin command or script"));
        }
    }

    fn conflict_key(&mut self, press: KeyPress) {
        let Some(c) = press.text.or(match press.plain() {
            Some(Key::Esc) => Some('a'),
            _ => None,
        }) else {
            return;
        };
        let apply_to_all = c.is_uppercase();
        let action = match c.to_ascii_lowercase() {
            'o' => ConflictAction::Overwrite,
            's' => ConflictAction::Skip,
            // The executor picks a free name when this one is taken.
            'r' => {
                let name = self.conflicts[0].conflict.destination.file_name().unwrap_or("file");
                ConflictAction::RenameTo(name.to_owned())
            }
            'a' => ConflictAction::Abort,
            _ => return,
        };
        if let Some(pending) = self.conflicts.pop_front() {
            let _ = pending.reply.send(Resolution { action, apply_to_all });
        }
    }

    /// Handles a key for `overlay`; returns the overlay to keep open.
    fn overlay_key(&mut self, overlay: Overlay, press: KeyPress) -> Option<Overlay> {
        let key = press.plain();
        let is_esc = key == Some(Key::Esc);
        match overlay {
            Overlay::Help => None,
            Overlay::Commands(palette) => self.palette_key(palette, press),
            Overlay::Input(mut dialog) => {
                let back = dialog.purpose.settings_screen().cloned().map(Overlay::Settings);
                if is_esc {
                    if let InputPurpose::ArchivePassword(container) = &dialog.purpose {
                        self.password_declined.insert(container.to_string());
                    }
                    if dialog.purpose == InputPurpose::QuickFilter {
                        self.workspace.active_mut().set_filter("");
                    }
                    return back;
                }
                if key == Some(Key::Enter) {
                    self.submit_input(dialog);
                    return back;
                }
                if let InputPurpose::PackInto(paths) = &dialog.purpose
                    && press.chord.ctrl
                    && press.chord.key == Key::Char('o')
                {
                    // The advanced options: the typed name picks the format.
                    self.ask_pack_format(paths.clone(), dialog.field.text());
                    return None;
                }
                if dialog.purpose.testable()
                    && press.chord.ctrl
                    && press.chord.key == Key::Char('t')
                {
                    self.test_typed(dialog);
                    return None;
                }
                if dialog.purpose == InputPurpose::InstallServe {
                    if press.chord.ctrl && press.chord.key == Key::Char('t') {
                        self.test_install(dialog);
                        return None;
                    }
                    if press.chord.key == Key::F(1) {
                        self.explain_install(dialog);
                        return None;
                    }
                }
                if dialog.field.handle(&press) {
                    match &dialog.purpose {
                        InputPurpose::NewConnection | InputPurpose::EditConnection { .. } => {
                            dialog.preview = crate::connections::hints(dialog.field.text())
                        }
                        InputPurpose::InstallServe => {
                            dialog.preview = crate::install::hints(dialog.field.text())
                        }
                        InputPurpose::QuickFilter => {
                            self.workspace.active_mut().set_filter(dialog.field.text())
                        }
                        // The new names follow the template as it is typed.
                        InputPurpose::MultiRename(paths) => {
                            dialog.preview = template_preview(paths, dialog.field.text())
                        }
                        _ => {}
                    }
                }
                Some(Overlay::Input(dialog))
            }
            Overlay::Confirm(dialog) => {
                let enter = key == Some(Key::Enter);
                let yes = matches!(press.text, Some('y' | 'Y')) || (enter && !dialog.default_no());
                let no = is_esc
                    || matches!(press.text, Some('n' | 'N'))
                    || (enter && dialog.default_no());
                if yes {
                    self.confirmed(dialog.purpose);
                    None
                } else if no {
                    None
                } else {
                    Some(Overlay::Confirm(dialog))
                }
            }
            Overlay::Choose(mut dialog) => {
                let page = self.page.max(1) as isize;
                if let Some(keep) = self.choose_extra_key(&mut dialog, &press) {
                    return keep.then_some(Overlay::Choose(dialog));
                }
                match key {
                    Some(Key::Up) => dialog.move_cursor(-1),
                    Some(Key::Down) => dialog.move_cursor(1),
                    Some(Key::PageUp) => dialog.move_cursor(-page),
                    Some(Key::PageDown) => dialog.move_cursor(page),
                    Some(Key::Home) => dialog.cursor = 0,
                    Some(Key::End) => dialog.move_cursor(isize::MAX),
                    Some(Key::Enter) => {
                        self.chosen(dialog);
                        return None;
                    }
                    Some(Key::Esc) => {
                        if let Some(cancel) = self.search_cancel.take() {
                            cancel.cancel();
                        }
                        if let ChoosePurpose::Font { screen, .. } = dialog.purpose {
                            return Some(Overlay::Settings(*screen));
                        }
                        return None;
                    }
                    Some(Key::Delete)
                        if matches!(&dialog.purpose, ChoosePurpose::Drives(drives)
                            if drives.get(dialog.cursor).is_some_and(|d| d.saved)) =>
                    {
                        if let ChoosePurpose::Drives(drives) = &dialog.purpose {
                            let drive = &drives[dialog.cursor];
                            return Some(forget_question(
                                Forget::Connection(drive.name.clone(), drive.location.clone()),
                                format!("Remove the saved connection \"{}\"?", drive.name),
                            ));
                        }
                    }
                    _ => {}
                }
                Some(Overlay::Choose(dialog))
            }
            Overlay::Message(dialog) if dialog.back_to_settings.is_some() => match key {
                Some(Key::Enter | Key::Esc) => {
                    dialog.back_to_settings.map(|s| Overlay::Settings(*s))
                }
                _ => Some(Overlay::Message(dialog)),
            },
            Overlay::Message(dialog)
                if press.chord.key == Key::Char('c')
                    && !press.chord.alt
                    && (press.chord.ctrl || press.text.is_some())
                    && dialog.can_copy() =>
            {
                // The log of a window (an install, a test) goes to the
                // clipboard as the text shown.
                self.copy_text(dialog.lines.join("\n"), "the log");
                Some(Overlay::Message(dialog))
            }
            Overlay::Message(dialog) => match key {
                Some(Key::Enter) => dialog
                    .then
                    .map(Overlay::Input)
                    .or(dialog.then_choose.map(|list| Overlay::Choose(*list))),
                Some(Key::Esc) => None,
                _ => Some(Overlay::Message(dialog)),
            },
            Overlay::Viewer(mut viewer) => {
                let page = self.page.max(1);
                let ipage = page as isize;
                if let Some(source) = viewer_picture_key(&mut viewer, key, press.text) {
                    let path = viewer.path.clone();
                    self.overlay = Some(Overlay::Viewer(viewer));
                    self.render_picture(path, false, source);
                    return self.overlay.take();
                }
                if viewer.shows_image() && viewer_zoom_key(&mut viewer, key, press.text) {
                    return Some(Overlay::Viewer(viewer));
                }
                if key == Some(Key::Enter)
                    && let Some(run) = viewer.sync.take()
                {
                    self.run_sync(*run);
                    return None;
                }
                match (key, press.text) {
                    (Some(Key::Esc | Key::F(3) | Key::F(10)), _) | (_, Some('q')) => return None,
                    (Some(Key::Up), _) => viewer.scroll_by(-1, page),
                    (Some(Key::Down), _) => viewer.scroll_by(1, page),
                    (Some(Key::PageUp), _) => viewer.scroll_by(-ipage, page),
                    (Some(Key::PageDown | Key::Space), _) => viewer.scroll_by(ipage, page),
                    (Some(Key::Home), _) => viewer.scroll = 0,
                    (Some(Key::End), _) => viewer.scroll_by(isize::MAX, page),
                    (_, Some('n')) if viewer.diff.is_some() => viewer.jump_change(1),
                    (_, Some('p')) if viewer.diff.is_some() => viewer.jump_change(-1),
                    (Some(Key::F(4)), _) | (_, Some('h')) => viewer.toggle_hex(),
                    (Some(Key::F(2)), _) | (_, Some('w')) => viewer.wrap = !viewer.wrap,
                    _ => {}
                }
                Some(Overlay::Viewer(viewer))
            }
            Overlay::Jobs(mut view) => {
                if is_esc || self.context.keymap.action_for(&press.chord) == Some(Action::Jobs) {
                    return None;
                }
                let last = self.jobs.len().saturating_sub(1);
                let selected = self.jobs.get(view.cursor).map(|j| j.id);
                let result = match (key, press.text, selected) {
                    (Some(Key::Up), _, _) => {
                        view.cursor = view.cursor.saturating_sub(1);
                        Ok(())
                    }
                    (Some(Key::Down), _, _) => {
                        view.cursor = (view.cursor + 1).min(last);
                        Ok(())
                    }
                    (_, Some('p'), Some(id)) => self.manager.pause(id),
                    (_, Some('r'), Some(id)) => self.manager.resume(id),
                    (Some(Key::Delete), _, Some(id)) | (_, Some('c'), Some(id)) => {
                        self.manager.cancel(id)
                    }
                    (_, Some('+'), Some(id)) => {
                        let priority = self.jobs[view.cursor].spec.priority + 1;
                        self.manager.set_priority(id, priority)
                    }
                    (_, Some('-'), Some(id)) => {
                        let priority = self.jobs[view.cursor].spec.priority - 1;
                        self.manager.set_priority(id, priority)
                    }
                    (_, Some(']'), Some(id)) => self
                        .manager
                        .job_parallelism(id)
                        .and_then(|n| self.manager.set_parallelism(id, n + 1)),
                    (_, Some('['), Some(id)) => self
                        .manager
                        .job_parallelism(id)
                        .and_then(|n| self.manager.set_parallelism(id, n.saturating_sub(1))),
                    (_, Some('a'), Some(id)) => {
                        let auto = self.jobs[view.cursor].spec.options.auto_parallelism;
                        self.manager.set_auto_parallelism(id, !auto)
                    }
                    (_, Some('x'), _) => self.manager.purge_finished().map(|_| ()),
                    _ => Ok(()),
                };
                if let Err(e) = result {
                    self.set_error(e.to_string());
                }
                self.refresh_jobs();
                view.cursor = view.cursor.min(self.jobs.len().saturating_sub(1));
                Some(Overlay::Jobs(view))
            }
            Overlay::Settings(screen) => self.settings_key(screen, press),
        }
    }

    // ------------------------------------------------------------- actions

    /// The items an operation acts on (selection or cursor).
    fn targets(&self) -> Vec<VfsPath> {
        if let Some(items) = &self.target_override {
            return items.clone();
        }
        self.workspace.active().targets()
    }

    fn other_location(&self) -> String {
        display_location(self.workspace.inactive().location())
    }

    fn count_text(paths: &[VfsPath]) -> String {
        match paths {
            [one] => format!("\"{}\"", one.file_name().unwrap_or("/")),
            many => format!("{} items", many.len()),
        }
    }

    /// Shows `dialog` again with `reason` under the field (the answer was
    /// not acceptable).
    fn ask_again(&mut self, mut dialog: InputDialog, reason: String) {
        dialog.preview = vec![format!("✗ {reason}")];
        self.overlay = Some(Overlay::Input(dialog));
    }

    /// Tests the connection at `location` in the background and shows what
    /// happened. Enter in the log goes back to `back`, if given.
    pub(super) fn test_connection(&mut self, location: String, back: Option<InputDialog>) {
        self.test_connection_back(location, back, None);
    }

    /// [`Self::test_connection`], coming back to a list instead of a question.
    pub(super) fn test_connection_back(
        &mut self,
        location: String,
        back: Option<InputDialog>,
        choose: Option<Box<ChooseDialog>>,
    ) {
        let registry = self.context.registry.clone();
        self.set_status(format!("Testing {location}…"));
        self.spawn_task(async move {
            let lines = crate::connections::run_test(registry, location).await;
            AppEvent::Show(Box::new(Overlay::Message(MessageDialog {
                title: "Test connection".into(),
                lines,
                then: back,
                then_choose: choose,
                back_to_settings: None,
                logo: false,
                busy: false,
            })))
        });
    }

    fn ask(&mut self, title: &str, prompt: &str, initial: &str, purpose: InputPurpose) {
        self.overlay = Some(Overlay::Input(InputDialog::new(title, prompt, initial, purpose)));
    }

    /// Bookmarks the active folder. With `with_target`, the other panel's
    /// folder becomes one of its targets; bookmarking the same folder again
    /// with another target adds that target to the list.
    fn add_bookmark(&mut self, with_target: bool) {
        let location = self.workspace.active().location().clone();
        let path = display_location(&location);
        let target = with_target.then(|| self.other_location());
        let bookmarks = &mut self.context.config.bookmarks;
        let message = match (bookmarks.iter().position(|b| b.path == path), target) {
            (Some(_), None) => return self.set_status(format!("{path} is already bookmarked")),
            (Some(index), Some(target)) => {
                let targets = &mut bookmarks[index].targets;
                if targets.contains(&target) {
                    return self.set_status(format!("{path} already has target {target}"));
                }
                targets.push(target.clone());
                format!("Bookmarked {path} with target {target} ({} targets)", targets.len())
            }
            (None, target) => {
                let message = match &target {
                    Some(target) => format!("Bookmarked {path} with target {target}"),
                    None => format!("Bookmarked {path}"),
                };
                let name = location.file_name().unwrap_or("/").to_owned();
                bookmarks.push(Bookmark { name, path, targets: target.into_iter().collect() });
                message
            }
        };
        if self.save_settings() {
            self.set_status(message);
        }
    }

    /// Opens a bookmark in the active panel, and its target in the other
    /// one: directly when there is one, with a quick picker when there are
    /// several (Esc leaves the other panel as it is).
    fn open_bookmark(&mut self, bookmark: Bookmark) {
        let here = self.workspace.active().location().clone();
        match resolve_location(&bookmark.path, &here) {
            Ok(path) => {
                let request = self.workspace.active_mut().navigate(path);
                self.load(request);
            }
            Err(e) => return self.set_error(e),
        }
        match bookmark.targets.as_slice() {
            [] => {}
            [target] => self.open_in_other(target),
            targets => {
                let title = format!("Other panel for {}", bookmark.name);
                let items = targets.to_vec();
                let mut dialog = ChooseDialog::new(
                    &title,
                    items,
                    ChoosePurpose::BookmarkTarget(targets.to_vec()),
                );
                dialog.footer = crate::hints::list("open", &[], "cancel");
                self.overlay = Some(Overlay::Choose(dialog));
            }
        }
    }

    /// Opens `location` (relative to the other panel's folder) there.
    fn open_in_other(&mut self, location: &str) {
        let here = self.workspace.inactive().location().clone();
        match resolve_location(location, &here) {
            Ok(path) => {
                let request = self.workspace.inactive_mut().navigate(path);
                self.load(request);
            }
            Err(e) => self.set_error(e),
        }
    }

    /// Runs a key-map action (from a key or the menu).
    pub fn perform(&mut self, action: Action) {
        let targets = self.targets();
        let nothing = targets.is_empty();
        match action {
            Action::View => self.view_current(),
            Action::Edit => self.edit_current(),
            Action::Copy | Action::Move if nothing => self.set_error("Nothing to copy"),
            Action::Copy => {
                let prompt = format!("Copy {} to:", Self::count_text(&targets));
                let other = self.other_location();
                self.ask("Copy", &prompt, &other, InputPurpose::CopyTo(targets));
            }
            Action::Move => {
                let prompt = format!("Move {} to:", Self::count_text(&targets));
                let other = self.other_location();
                self.ask("Move", &prompt, &other, InputPurpose::MoveTo(targets));
            }
            Action::Rename => {
                let Some(path) = self.workspace.active().current_path() else { return };
                let name = path.file_name().unwrap_or_default().to_owned();
                let mut dialog =
                    InputDialog::new("Rename", "New name:", &name, InputPurpose::Rename(path));
                // Put the cursor before the extension, where edits usually go.
                if let Some(dot) = name.rfind('.').filter(|&i| i > 0) {
                    dialog.field.set_cursor(name[..dot].chars().count());
                }
                self.overlay = Some(Overlay::Input(dialog));
            }
            Action::MultiRename if nothing => self.set_error("Nothing to rename"),
            Action::MultiRename => {
                let template = "[N][X]";
                let preview = template_preview(&targets, template);
                let mut dialog = InputDialog::new(
                    "Multi-rename",
                    "Template ([N] name, [E] extension, [X] .extension, [C] counter, [P] folder):",
                    template,
                    InputPurpose::MultiRename(targets),
                );
                dialog.preview = preview;
                self.overlay = Some(Overlay::Input(dialog));
            }
            Action::AiRename if nothing => self.set_error("Nothing to rename"),
            Action::AiRename => self.ask_ai_rename(targets),
            Action::GenerateFiles => self.generate_files(),
            Action::MakeDir => self.ask("New folder", "Name:", "", InputPurpose::MakeDir),
            Action::Delete | Action::DeleteForever if nothing => {
                self.set_error("Nothing to delete")
            }
            Action::Delete => self.ask_delete(targets, self.context.config.general.use_trash),
            Action::DeleteForever => self.ask_delete(targets, false),
            Action::Pack if nothing => self.set_error("Nothing to pack"),
            Action::Pack => {
                let name = match targets.as_slice() {
                    [one] => one.file_name().unwrap_or("archive").to_owned(),
                    // Items of many folders: no folder gives the name.
                    _ if self.target_override.is_some() => "basket".to_owned(),
                    _ => self.workspace.active().location().file_name().unwrap_or("archive").into(),
                };
                let output =
                    resolve_location(&format!("{name}.zip"), self.workspace.inactive().location())
                        .map(|p| display_location(&p))
                        .unwrap_or_default();
                let prompt = format!(
                    "Pack {} into (format from the extension):",
                    Self::count_text(&targets)
                );
                self.ask("Pack", &prompt, &output, InputPurpose::PackInto(targets));
            }
            Action::Unpack => self.unpack_current(),
            Action::Properties => self.show_properties(),
            Action::Attributes if nothing => self.set_error("Nothing to change"),
            Action::Attributes => {
                let prompt = format!(
                    "Change {} (date 2026-11-17, time 10:30, now, mode=644, -r):",
                    Self::count_text(&targets)
                );
                self.ask(
                    "Dates and permissions",
                    &prompt,
                    "now",
                    InputPurpose::Attributes(targets),
                );
            }
            Action::Undo => self.undo_last(),
            Action::Checksum => self.checksum_current(),
            Action::CopyPath => self.ask_copy_path(),
            Action::Export => self.ask_export(),
            Action::Link => self.ask_link(),
            Action::BulkRename => self.bulk_rename(),
            Action::OpenTerminal => self.open_terminal(),
            Action::BasketAdd => self.add_selection_to_basket(),
            Action::Basket => self.open_basket(),
            Action::CompareHash => self.compare_by_hash(),
            Action::Sync => self.ask_sync(),
            Action::SimilarImages => self.find_similar(),
            Action::DiskUsage => {
                let folder = self.workspace.active().location().clone();
                self.disk_usage(folder);
            }
            Action::Duplicates => self.find_duplicates(),
            Action::Disks => self.show_disks(),
            Action::SavedSearches => self.open_saved_searches(),
            Action::SaveSearch if self.last_search.is_none() => {
                self.set_error("Search first (Shift+F7), then save that search")
            }
            Action::SaveSearch => {
                self.ask("Save search", "Name for this search:", "", InputPurpose::NameSearch)
            }
            Action::ToggleSelect => self.workspace.active_mut().toggle_current(),
            Action::SelectMask => self.ask(
                "Select",
                "Select files matching (e.g. *.jpg, or re:REGEX):",
                "*",
                InputPurpose::SelectMask,
            ),
            Action::UnselectMask => {
                self.ask("Unselect", "Unselect files matching:", "*", InputPurpose::UnselectMask)
            }
            Action::InvertSelect => self.workspace.active_mut().invert_selection(),
            Action::SelectAll => self.workspace.active_mut().select_all(),
            Action::Open => {
                let request = self.workspace.active_mut().enter();
                match request {
                    Some(request) => self.load(request),
                    None if self.picker.is_some() => self.choose_file(),
                    None if self.workspace.active().current_path().is_some() => self.view_current(),
                    None => {}
                }
            }
            Action::Parent => {
                let request = self.workspace.active_mut().parent();
                self.load_opt(request);
            }
            Action::SwitchPanel => self.workspace.switch_side(),
            Action::GoTo => {
                let here = display_location(self.workspace.active().location());
                self.ask(
                    "Go to",
                    "Folder, archive or URI (sftp://, smb://, davs://, s3://, agent://, scs://, zip://…):",
                    &here,
                    InputPurpose::GoTo,
                );
            }
            Action::Back => {
                let request = self.workspace.active_mut().back();
                self.load_opt(request);
            }
            Action::Forward => {
                let request = self.workspace.active_mut().forward();
                self.load_opt(request);
            }
            Action::Bookmarks => self.open_bookmarks(),
            Action::AddBookmark => self.add_bookmark(false),
            Action::AddBookmarkTarget => self.add_bookmark(true),
            Action::NewTab => {
                let side = self.workspace.active_side();
                let request = self.workspace.side_mut(side).new_tab();
                self.load(request);
            }
            Action::CloseTab => {
                let side = self.workspace.active_side();
                if !self.workspace.side_mut(side).close_tab() {
                    self.set_status("The last tab stays open");
                }
            }
            Action::NextTab => self.cycle_tab(1),
            Action::PrevTab => self.cycle_tab(-1),
            Action::SamePath => {
                let request = self.workspace.same_location_in_other();
                self.load(request);
            }
            Action::OpenInLeft | Action::OpenInRight => {
                let side = if action == Action::OpenInLeft { SideId::Left } else { SideId::Right };
                let request = self.workspace.open_cursor_in(side);
                self.load(request);
            }
            Action::SwapPanels => {
                self.workspace.swap_sides();
                // The quick view stays beside the panel it shows: the focus
                // moves with that panel, so the two trade places.
                if self.quick_view.is_some() {
                    self.workspace.switch_side();
                }
            }
            Action::Refresh => self.reload_panels(None),
            Action::ToggleHidden => {
                let show = !self.workspace.active().show_hidden();
                for side in [SideId::Left, SideId::Right] {
                    self.workspace.side_mut(side).panel_mut().set_show_hidden(show);
                }
            }
            Action::QuickFilter => {
                let current = self.workspace.active().filter().to_owned();
                self.ask(
                    "Quick filter",
                    "Show only names containing (or a mask like *.rs):",
                    &current,
                    InputPurpose::QuickFilter,
                );
            }
            Action::SortName => self.workspace.active_mut().set_sort(SortKey::Name),
            Action::SortExtension => self.workspace.active_mut().set_sort(SortKey::Extension),
            Action::SortModified => self.workspace.active_mut().set_sort(SortKey::Modified),
            Action::QuickView => {
                self.quick_view = match self.quick_view {
                    Some(_) => None,
                    None => Some(None),
                };
            }
            Action::Search => self.ask(
                "Search",
                "File names matching (e.g. *.rs;*.toml, or re:REGEX):",
                "*",
                InputPurpose::SearchName,
            ),
            Action::Compare => self.compare(),
            Action::Jobs => {
                self.refresh_jobs();
                self.overlay = Some(Overlay::Jobs(JobsView::default()));
            }
            Action::Drives => {
                let drives = crate::drives::drives(&self.context.config);
                let mut items: Vec<String> = drives.iter().map(|d| d.row()).collect();
                items.push("New connection…".into());
                let mut dialog = ChooseDialog::new("Drives", items, ChoosePurpose::Drives(drives));
                dialog.footer = crate::hints::list(
                    "open",
                    &["F2: rename", "F4: edit", "Ctrl+T: test", "Delete: remove"],
                    "close",
                );
                self.overlay = Some(Overlay::Choose(dialog));
            }
            Action::RunScript => self.choose_plugin_command(),
            Action::AiScript => self.ask_ai_script(),
            Action::InstallServe => {
                let mut dialog = InputDialog::new(
                    "Install nmc serve",
                    crate::install::prompt(),
                    "",
                    InputPurpose::InstallServe,
                );
                dialog.preview = crate::install::hints("");
                self.overlay = Some(Overlay::Input(dialog));
            }
            Action::Help => self.overlay = Some(Overlay::Help),
            Action::Menu => self.open_commands(),
            Action::Settings => self.open_settings(),
            Action::ExportConfig => {
                let suggestion = home_file("nemesiscommander-settings.toml");
                self.ask(
                    "Export settings",
                    "Write settings, bookmarks and connections to:",
                    &suggestion,
                    InputPurpose::ExportSettings,
                );
            }
            Action::ImportConfig => {
                let suggestion = home_file("nemesiscommander-settings.toml");
                self.ask(
                    "Import settings",
                    "Read settings from:",
                    &suggestion,
                    InputPurpose::ImportSettings,
                );
            }
            Action::FlatView => {
                let request = self.workspace.active_mut().toggle_branch();
                self.load(request);
            }
            Action::Choose => self.choose_now(),
            Action::About => self.overlay = Some(Overlay::Message(crate::about::dialog())),
            Action::Quit => self.running = false,
        }
    }

    /// Enter on a file in pick mode: the marked files, or this one.
    fn choose_file(&mut self) {
        let Some(mode) = self.picker else { return };
        if mode.folder {
            return self.set_status("Choose a folder: Ctrl+F10 picks the one shown");
        }
        let panel = self.workspace.active();
        let items = if mode.multiple {
            panel.targets()
        } else {
            panel.current_path().into_iter().collect()
        };
        self.finish_pick(items);
    }

    fn cycle_tab(&mut self, delta: isize) {
        let side = self.workspace.active_side();
        self.workspace.side_mut(side).cycle_tab(delta);
    }

    fn ask_delete(&mut self, paths: Vec<VfsPath>, trash: bool) {
        if !self.context.config.general.confirm_delete {
            return self.confirmed(ConfirmPurpose::Delete { paths, trash });
        }
        let registry = self.context.registry.clone();
        self.spawn_task(async move {
            // Only promise the recycle bin when the provider has one.
            let trash = trash
                && match registry.provider_for(&paths[0]).await {
                    Ok(provider) => provider.capabilities().contains(Capabilities::TRASH),
                    Err(_) => false,
                };
            let question = if trash {
                format!("Move {} to the trash?", Self::count_text(&paths))
            } else {
                format!("Delete {} permanently? This cannot be undone.", Self::count_text(&paths))
            };
            let mut lines = vec![question];
            lines.extend(
                paths.iter().take(8).map(|p| format!("  {}", p.file_name().unwrap_or("/"))),
            );
            if paths.len() > 8 {
                lines.push(format!("  … and {} more", paths.len() - 8));
            }
            AppEvent::Show(Box::new(Overlay::Confirm(ConfirmDialog {
                title: if trash { "Delete".into() } else { "Delete permanently".into() },
                lines,
                purpose: ConfirmPurpose::Delete { paths, trash },
            })))
        });
    }

    fn submit_job(
        &mut self,
        kind: JobKind,
        sources: Vec<VfsPath>,
        destination: VfsPath,
    ) -> Option<JobId> {
        let options = self.context.job_options_to(None, Some(&destination));
        let spec = JobSpec { kind, sources, destination: Some(destination), options, priority: 0 };
        let title = spec.title();
        let id = match self.manager.submit(spec) {
            Ok(id) => {
                self.set_status(format!("Queued: {title}"));
                self.workspace.active_mut().clear_selection();
                Some(id)
            }
            Err(e) => {
                self.set_error(e.to_string());
                None
            }
        };
        self.refresh_jobs();
        id
    }

    fn submit_input(&mut self, dialog: InputDialog) {
        let text = dialog.field.text().trim().to_owned();
        let here = self.workspace.active().location().clone();
        let registry = self.context.registry.clone();
        match dialog.purpose {
            InputPurpose::CopyTo(paths) | InputPurpose::MoveTo(paths) if text.is_empty() => {
                let _ = paths;
                self.set_error("No destination given");
            }
            InputPurpose::CopyTo(paths) => match resolve_location(&text, &here) {
                Ok(destination) => drop(self.submit_job(JobKind::Copy, paths, destination)),
                Err(e) => self.set_error(e),
            },
            InputPurpose::MoveTo(paths) => match resolve_location(&text, &here) {
                Ok(destination) => drop(self.submit_job(JobKind::Move, paths, destination)),
                Err(e) => self.set_error(e),
            },
            purpose @ (InputPurpose::GenerateName
            | InputPurpose::GenerateCount(_)
            | InputPurpose::GenerateSize(..)) => self.generate_step(purpose, &text),
            InputPurpose::MakeDir => {
                if text.is_empty() {
                    return;
                }
                let first = text.split('/').next().unwrap_or(&text).to_owned();
                self.spawn_task(async move {
                    let result = make_dirs(&registry, &here, &text).await;
                    // Only the last folder is taken back: the ones above may have
                    // been there before.
                    let created = text
                        .split('/')
                        .filter(|part| !part.is_empty())
                        .fold(here.clone(), |path, part| path.join(part));
                    let step = UndoStep::Created(created);
                    undoable(step, done(result.map(|()| format!("Created {text}")), Some(first)))
                });
            }
            InputPurpose::Rename(path) => {
                let old = path.file_name().unwrap_or_default();
                if text.is_empty() || text == old || text.contains('/') {
                    return;
                }
                self.spawn_task(async move {
                    // Same rules as multi-rename: never replaces another
                    // file, and case-only changes work everywhere.
                    let result = apply_renames(
                        &registry,
                        std::slice::from_ref(&path),
                        std::slice::from_ref(&text),
                    )
                    .await;
                    let step = UndoStep::Renamed(vec![(renamed(&path, &text), path)]);
                    undoable(step, done(result.map(|()| String::new()), Some(text)))
                });
            }
            InputPurpose::MultiRename(paths) => {
                let rule = RenameRule { template: text, ..RenameRule::default() };
                self.confirm_renames(paths, &rule, None);
            }
            InputPurpose::AiRename(paths) => self.write_rename_with_ai(paths, text),
            InputPurpose::AddPluginRepo => self.plugin_repo_typed(&text),
            InputPurpose::NewConnection => match crate::connections::check(&text) {
                Ok(checked) => self.ask_name(
                    "New connection",
                    "Name of this connection in the drive list (so you recognise it later):",
                    &crate::drives::suggested_name(&checked.path),
                    InputPurpose::NameConnection {
                        uri: checked.path.to_string(),
                        secret: Secret(checked.secret.unwrap_or_default()),
                    },
                ),
                // Stay in the question, with the reason under the field.
                Err(e) => self.ask_again(dialog, e),
            },
            InputPurpose::EditConnection { name, uri } => match crate::connections::check(&text) {
                Err(e) => {
                    let purpose = InputPurpose::EditConnection { name, uri };
                    self.ask_again(InputDialog { purpose, ..dialog }, e)
                }
                Ok(checked) => self.ask_name(
                    "Edit connection",
                    "Name of this connection in the drive list:",
                    &name.clone(),
                    InputPurpose::EditConnectionName {
                        name,
                        uri,
                        new_uri: checked.path.to_string(),
                        secret: Secret(checked.secret.unwrap_or_default()),
                    },
                ),
            },
            InputPurpose::EditConnectionName { name, uri, new_uri, secret } => {
                let new_name = text.trim().to_owned();
                if new_name.is_empty() {
                    return self.set_error("no name given".to_owned());
                }
                let taken = self.context.config.connections.iter().any(|c| c.name == new_name);
                if taken && new_name != name {
                    return self
                        .set_error(format!("There is already a connection called {new_name}"));
                }
                self.finish_connection(new_name, new_uri, Some((name, uri)), secret);
            }
            InputPurpose::NameConnection { uri, secret } => {
                let name = text.trim().to_owned();
                if name.is_empty() {
                    return self.set_error("no name given".to_owned());
                }
                self.finish_connection(name, uri, None, secret);
            }
            purpose @ (InputPurpose::ConnectionUser { .. }
            | InputPurpose::ConnectionPassword { .. }
            | InputPurpose::ConnectionOpen { .. }
            | InputPurpose::S3AccessKey { .. }
            | InputPurpose::S3Secret { .. }) => {
                let dialog = InputDialog { purpose: InputPurpose::GoTo, ..dialog };
                self.submit_login(purpose, text, dialog)
            }
            InputPurpose::RenameConnection { name, uri } => {
                if text.is_empty() {
                    return self.set_error("no name given".to_owned());
                }
                let taken = self.context.config.connections.iter().any(|c| c.name == text);
                if taken && text != name {
                    return self.set_error(format!("There is already a connection called {text}"));
                }
                if let Some(c) = self
                    .context
                    .config
                    .connections
                    .iter_mut()
                    .find(|c| c.name == name && c.uri == uri)
                {
                    c.name = text.clone();
                    if self.save_settings() {
                        self.set_status(format!("Renamed to {text}"));
                    }
                }
            }
            InputPurpose::GoTo => match resolve_location(&text, &here) {
                Ok(path) => {
                    let request = self.workspace.active_mut().navigate(path);
                    self.load(request);
                }
                Err(e) => self.set_error(e),
            },
            InputPurpose::SelectMask | InputPurpose::UnselectMask => {
                let select = dialog.purpose == InputPurpose::SelectMask;
                match Mask::parse(&text) {
                    Ok(mask) => self.workspace.active_mut().select_mask(&mask, select, false),
                    Err(e) => self.set_error(e.to_string()),
                }
            }
            // Already applied while typing.
            InputPurpose::QuickFilter => {}
            InputPurpose::PackInto(paths) => {
                let output = match resolve_location(&text, &here) {
                    Ok(output) => output,
                    Err(e) => return self.set_error(e),
                };
                let name = output.file_name().unwrap_or_default().to_owned();
                let Some(format) = ArchiveFormat::from_file_name(&name) else {
                    return self.set_error(format!(
                        "Unknown archive type: {name} (try .zip, .7z, .tar.zst)"
                    ));
                };
                self.set_status(format!("Packing {name}…"));
                self.spawn_task(async move {
                    let result =
                        nmc_archive::pack(&registry, &paths, &output, PackOptions::new(format))
                            .await;
                    done(result.map(|count| format!("Packed {count} entries into {name}")), None)
                });
            }
            purpose @ (InputPurpose::PackPassword { .. }
            | InputPurpose::PackPasswordAgain { .. }
            | InputPurpose::PackAdvanced { .. }
            | InputPurpose::ArchivePassword(_)) => {
                self.submit_archive_input(purpose, dialog.field.text())
            }
            InputPurpose::SearchName => {
                let mask = if text.is_empty() { "*".to_owned() } else { text };
                self.ask(
                    "Search",
                    "Containing text (re:REGEX for a regular expression; empty: any file):",
                    "",
                    InputPurpose::SearchContent { mask },
                );
            }
            InputPurpose::SearchContent { mask } => self.ask(
                "Search",
                "Size, age, archives (>10M <1G newer:7d older:2w archives; empty: any):",
                "",
                InputPurpose::SearchFilters { mask, content: text },
            ),
            InputPurpose::SearchFilters { mask, content } => {
                self.start_search(&mask, &content, &text)
            }
            InputPurpose::NameSearch => self.save_search_as(&text),
            InputPurpose::ExportTo { format, selection_only } => {
                self.export_listing(format, selection_only, &text)
            }
            InputPurpose::Attributes(paths) => {
                let change = match nmc_ops::parse_change(&text, std::time::SystemTime::now()) {
                    Ok(change) => change,
                    Err(e) => return self.set_error(e),
                };
                self.spawn_task(async move {
                    let result = nmc_ops::apply_change(&registry, &paths, &change).await;
                    done(result.map(|n| format!("Changed {n} item(s)")), None)
                });
            }
            InputPurpose::ExportSettings => {
                let file = expand_home(&text);
                match ExportBundle::new(&self.context.config).write(&file) {
                    Ok(()) => self.set_status(format!("Settings exported to {}", file.display())),
                    Err(e) => self.set_error(e.to_string()),
                }
            }
            InputPurpose::ImportSettings => match ExportBundle::read(&expand_home(&text)) {
                Ok(bundle) => {
                    let items = vec![
                        "Replace all settings".to_owned(),
                        "Merge: add bookmarks, connections and key overrides".to_owned(),
                    ];
                    self.overlay = Some(Overlay::Choose(ChooseDialog::new(
                        "Import settings",
                        items,
                        ChoosePurpose::ImportMode(Box::new(bundle)),
                    )));
                }
                Err(e) => self.set_error(e.to_string()),
            },
            InputPurpose::SettingValue { field, .. } => self.setting_typed(field, &text),
            InputPurpose::SendSettings(_) => self.send_settings(&text),
            InputPurpose::FetchSettings(_) => self.fetch_settings(&text),
            InputPurpose::AiScript => self.write_script_with_ai(text),
            InputPurpose::InstallServe => self.install_serve(&text),
            InputPurpose::InstallLinger(install) => self.start_install_with(&install, Some(text)),
        }
    }

    /// Shows the new names `rule` gives `paths` and asks to rename them;
    /// `explanation` (from Claude) and the rule are shown first. When a new
    /// name belongs to another file, the preview marks it and nothing can
    /// be renamed.
    fn confirm_renames(
        &mut self,
        paths: Vec<VfsPath>,
        rule: &RenameRule,
        explanation: Option<String>,
    ) {
        let names = match preview_renames(&paths, rule) {
            Ok(names) => names,
            Err(e) => return self.set_error(e.to_string()),
        };
        let mut lines = Vec::new();
        if let Some(explanation) = explanation {
            lines.push(explanation);
            lines.push(format!("Template: {}", rule.template));
            if !rule.find.is_empty() {
                let kind = if rule.regex { "Regex" } else { "Find" };
                lines.push(format!("{kind}: {}  →  {}", rule.find, rule.replace));
            }
        }
        self.confirm_new_names(paths, names, lines);
    }

    /// Shows what renaming `paths` to `names` would do and asks to go ahead
    /// (or says which names are taken). `lines` come first.
    pub(super) fn confirm_new_names(
        &mut self,
        paths: Vec<VfsPath>,
        names: Vec<String>,
        mut lines: Vec<String>,
    ) {
        let changed = paths.iter().zip(&names).filter(|(p, n)| p.file_name() != Some(n)).count();
        if changed == 0 {
            return self.set_error("Rename: no name would change");
        }
        let registry = self.context.registry.clone();
        self.spawn_task(async move {
            let conflicts = match rename_conflicts(&registry, &paths, &names).await {
                Ok(conflicts) => conflicts,
                Err(e) => return done(Err(e), None),
            };
            let title = "Multi-rename".to_owned();
            if !conflicts.is_empty() {
                lines.push(format!(
                    "{} new names belong to other files, so nothing is renamed:",
                    conflicts.len()
                ));
                lines.extend(rename_lines(&paths, &names, &conflicts, PREVIEW_ROWS));
                return AppEvent::Show(Box::new(Overlay::Message(MessageDialog {
                    title,
                    lines,
                    then: None,
                    then_choose: None,
                    back_to_settings: None,
                    logo: false,
                    busy: false,
                })));
            }
            lines.push(format!("Rename {changed} items?"));
            lines.extend(rename_lines(&paths, &names, &[], PREVIEW_ROWS));
            AppEvent::Show(Box::new(Overlay::Confirm(ConfirmDialog {
                title,
                lines,
                purpose: ConfirmPurpose::MultiRename { paths, names },
            })))
        });
    }

    /// Removes what the user saved, after the question, and opens its list
    /// again.
    fn forget(&mut self, what: Forget) {
        match what {
            Forget::Bookmark(index) => {
                if index < self.context.config.bookmarks.len() {
                    self.context.config.bookmarks.remove(index);
                    self.save_settings();
                }
                self.open_bookmarks();
            }
            Forget::Search(index) => {
                if index < self.context.config.searches.len() {
                    self.context.config.searches.remove(index);
                    self.save_settings();
                }
                self.open_saved_searches();
            }
            Forget::Connection(name, uri) => {
                self.context.config.connections.retain(|c| !(c.name == name && c.uri == uri));
                self.save_settings();
                self.perform(Action::Drives);
            }
            Forget::Basket => {
                self.basket.clear();
                self.set_status("The basket is empty");
            }
        }
    }

    fn confirmed(&mut self, purpose: ConfirmPurpose) {
        let registry = self.context.registry.clone();
        match purpose {
            ConfirmPurpose::Delete { paths, trash } => {
                self.workspace.active_mut().clear_selection();
                self.basket_forget(&paths);
                self.spawn_task(async move {
                    let count = paths.len();
                    let options = DeleteOptions { use_trash: trash, observer: None };
                    let result =
                        delete_paths(&registry, &paths, options).await.and_then(|report| {
                            match report.failed.first() {
                                Some((_, message)) => {
                                    Err(nmc_vfs::VfsError::Other(message.clone()))
                                }
                                None => Ok(()),
                            }
                        });
                    let verb = if trash { "Moved to trash" } else { "Deleted" };
                    let noun = if count == 1 { "item" } else { "items" };
                    done(result.map(|()| format!("{verb}: {count} {noun}")), None)
                });
            }
            ConfirmPurpose::Forget(what) => self.forget(what),
            ConfirmPurpose::RunPlugin(choice) => self.run_plugin_choice(choice),
            ConfirmPurpose::UninstallPlugin(name) => self.uninstall_plugin(&name),
            ConfirmPurpose::AddPluginRepo(repository) => self.add_plugin_repo(repository),
            ConfirmPurpose::InstallServe(text) => self.start_install(&text),
            ConfirmPurpose::OfferInstall(host) => {
                let mut dialog = InputDialog::new(
                    "Install nmc serve",
                    crate::install::prompt(),
                    &host,
                    InputPurpose::InstallServe,
                );
                dialog.preview = crate::install::hints(&host);
                self.overlay = Some(Overlay::Input(dialog));
            }
            ConfirmPurpose::MultiRename { paths, names } => {
                self.workspace.active_mut().clear_selection();
                self.spawn_task(async move {
                    let count = paths.len();
                    let result = apply_renames(&registry, &paths, &names).await;
                    let pairs = paths
                        .iter()
                        .zip(&names)
                        .map(|(path, name)| (renamed(path, name), path.clone()))
                        .filter(|(now, before)| now != before)
                        .collect();
                    let step = UndoStep::Renamed(pairs);
                    undoable(step, done(result.map(|()| format!("Renamed {count} items")), None))
                });
            }
        }
    }

    fn chosen(&mut self, dialog: ChooseDialog) {
        let here = self.workspace.active().location().clone();
        let cursor = dialog.cursor;
        match dialog.purpose {
            ChoosePurpose::Bookmarks(rows) => match rows.get(cursor) {
                Some(JumpRow::Bookmark(index)) => {
                    if let Some(bookmark) = self.context.config.bookmarks.get(*index).cloned() {
                        self.open_bookmark(bookmark);
                    }
                }
                Some(JumpRow::Recent(uri)) => self.open_location(uri.clone()),
                Some(JumpRow::Header) | None => {}
            },
            ChoosePurpose::BookmarkTarget(targets) => {
                if let Some(target) = targets.get(cursor) {
                    self.open_in_other(target);
                }
            }
            ChoosePurpose::Drives(drives) => match drives.get(cursor) {
                Some(drive) => match resolve_location(&drive.location, &here) {
                    Ok(path) => self.open_connection(&drive.location, path, false),
                    Err(e) => self.set_error(e),
                },
                None => self.ask_address("New connection", "", InputPurpose::NewConnection),
            },
            ChoosePurpose::SearchResults(paths) => {
                if let Some(cancel) = self.search_cancel.take() {
                    cancel.cancel();
                }
                if let Some(path) = paths.get(cursor) {
                    let request = self.workspace.active_mut().reveal(path);
                    self.load_opt(request);
                }
            }
            ChoosePurpose::PluginStore(rows) => self.plugin_store_chosen(&rows, cursor),
            ChoosePurpose::PluginRepos(rows) => self.plugin_repo_chosen(&rows, cursor),
            ChoosePurpose::Plugin(mut choices) => {
                if cursor < choices.len() {
                    self.plugin_chosen(choices.swap_remove(cursor));
                }
            }
            purpose @ ChoosePurpose::ConnectionLogin { .. } => self.chosen_login(purpose, cursor),
            purpose @ (ChoosePurpose::PackFormat { .. } | ChoosePurpose::PackLevel { .. }) => {
                self.chosen_pack(purpose, cursor)
            }
            purpose @ (ChoosePurpose::CopyPath(_)
            | ChoosePurpose::ExportFormat { .. }
            | ChoosePurpose::LinkKind(_)
            | ChoosePurpose::SavedSearches
            | ChoosePurpose::Basket(_)
            | ChoosePurpose::SyncMode
            | ChoosePurpose::Similar(_)) => self.chosen_feature(purpose, cursor),
            purpose @ (ChoosePurpose::Duplicates(_) | ChoosePurpose::Disks(_)) => {
                self.chosen_disk_tool(purpose, cursor);
            }
            // Enter is handled by its own key.
            ChoosePurpose::DiskUsage { .. } => {}
            ChoosePurpose::Font { field, screen, names } => {
                if let Some(name) = names.get(cursor) {
                    self.setting_typed(field, name);
                }
                self.overlay = Some(Overlay::Settings(*screen));
            }
            ChoosePurpose::ImportMode(bundle) => {
                let mode = if cursor == 0 { ImportMode::Replace } else { ImportMode::Merge };
                self.context.config = bundle.apply(&self.context.config, mode);
                self.apply_settings();
                if self.save_settings() {
                    self.set_status("Settings imported");
                }
            }
        }
    }

    fn start_search(&mut self, mask: &str, content: &str, filters: &str) {
        let criteria = match crate::searches::criteria(mask, content, filters, SystemTime::now()) {
            Ok(criteria) => criteria,
            Err(e) => return self.set_error(e),
        };
        let search = nmc_config::SavedSearch {
            name: String::new(),
            mask: mask.to_owned(),
            content: content.to_owned(),
            filters: filters.to_owned(),
        };
        self.search_with(criteria, search);
    }

    /// Searches the folder of the active panel; `search` is remembered so it
    /// can be saved.
    fn search_with(&mut self, criteria: SearchCriteria, search: nmc_config::SavedSearch) {
        self.last_search = Some(search);
        let root = self.workspace.active().location().clone();
        let cancel = CancellationToken::new();
        self.search_cancel = Some(cancel.clone());
        let mut dialog = ChooseDialog::new(
            &format!("Search in {}", display_location(&root)),
            Vec::new(),
            ChoosePurpose::SearchResults(Vec::new()),
        );
        dialog.footer = format!("searching…{}{}", crate::hints::SEP, self.basket_hint());
        self.overlay = Some(Overlay::Choose(dialog));
        let registry = self.context.registry.clone();
        let events = self.events.clone();
        self.spawn_task(async move {
            let provider = match registry.provider_for(&root).await {
                Ok(provider) => provider,
                Err(e) => return AppEvent::SearchDone(Some(e.to_string())),
            };
            let opener = Arc::new(nmc_app::RegistryArchives(registry.clone()));
            let mut hits = nmc_index::search_in_archives(provider, root, criteria, cancel, opener);
            let mut error = None;
            while let Some(hit) = hits.recv().await {
                match hit {
                    Ok(hit) => {
                        let _ = events.send(AppEvent::SearchHit(hit));
                    }
                    // Unreadable folders do not stop the search.
                    Err(e) => error = Some(e.to_string()),
                }
            }
            AppEvent::SearchDone(error)
        });
    }

    fn view_current(&mut self) {
        let panel = self.workspace.active();
        let Some(entry) = panel.current_entry().filter(|_| panel.current_path().is_some()) else {
            return;
        };
        let path = panel.location().join(&entry.name);
        if entry.is_dir() {
            return self.folder_size(path);
        }
        self.overlay =
            Some(Overlay::Viewer(Viewer::new(path.clone(), self.context.config.viewer.wrap)));
        self.load_file(path, false);
    }

    fn load_file(&mut self, path: VfsPath, quick: bool) {
        let registry = self.context.registry.clone();
        let paths = self.context.paths.clone();
        let name = path.file_name().unwrap_or_default().to_owned();
        // Quick view reads the start of a file, or a whole image or
        // previewed document.
        let limit = if let Some(limit) = crate::preview::read_limit(&name) {
            limit
        } else if quick && !viewer::is_image_name(&name) {
            QUICK_VIEW_LIMIT
        } else {
            VIEW_LIMIT
        };
        self.spawn_task(async move {
            let loaded = viewer::load(&registry, &path, limit).await.map_err(|e| e.to_string());
            let facts = match registry.provider_for(&path).await {
                Ok(provider) => {
                    provider.stat(&path).await.ok().map(|e| crate::preview::Facts::of(&e))
                }
                Err(_) => None,
            }
            .unwrap_or_default();
            // An archive shows its files, not its bytes; a plugin's previewer
            // shows what the plugin makes of the file.
            let listing = match &loaded {
                Ok(_) => match plugin_views::preview_text(registry.clone(), &paths, &path).await {
                    Some(Ok(text)) => Some(nmc_preview::markdown::parse(&text)),
                    Some(Err(e)) => Some(vec![
                        nmc_preview::rich::Block::heading(1, "The previewer failed"),
                        nmc_preview::rich::Block::paragraph(e),
                    ]),
                    None => crate::preview::archive_listing(&registry, &path, &facts).await,
                },
                Err(_) => None,
            };
            let result = match loaded {
                Ok((data, truncated)) => tokio::task::spawn_blocking(move || {
                    let data = Arc::new(data);
                    let prepared = match listing {
                        Some(blocks) => crate::preview::Prepared::Rich(blocks),
                        None => crate::preview::prepare(&name, &data, truncated, &facts)?,
                    };
                    Ok((data, truncated, prepared))
                })
                .await
                .unwrap_or_else(|e| Err(format!("the preview failed: {e}"))),
                Err(e) => Err(e),
            };
            AppEvent::Loaded { path, quick, result }
        });
    }

    /// The viewer or quick view showing `path`, if it still does.
    fn viewer_for(&mut self, path: &VfsPath, quick: bool) -> Option<&mut Viewer> {
        let viewer = if quick {
            match &mut self.quick_view {
                Some(Some(viewer)) => Some(viewer),
                _ => None,
            }
        } else {
            match &mut self.overlay {
                Some(Overlay::Viewer(viewer)) => Some(viewer),
                _ => None,
            }
        };
        viewer.filter(|v| &v.path == path)
    }

    /// Renders a picture for the viewer in the background (and its PNG for
    /// windows; plain pictures of this disk need none, the window reads
    /// the file itself).
    fn render_picture(&mut self, path: VfsPath, quick: bool, source: crate::preview::Source) {
        let Some(generation) = self.viewer_for(&path, quick).map(|v| v.generation) else { return };
        let local_picture =
            matches!(source, crate::preview::Source::Picture(_)) && path.to_local_path().is_some();
        let png = self.graphical && !local_picture;
        if matches!(source, crate::preview::Source::Picture(_)) && !png {
            return;
        }
        let size =
            if quick { crate::preview::QUICK_RENDER_SIZE } else { crate::preview::RENDER_SIZE };
        self.spawn_task(async move {
            let result = tokio::task::spawn_blocking(move || {
                let picture = crate::preview::render(&source, size)?;
                let file = if png { Some(crate::preview::write_png(&picture)?) } else { None };
                Ok((picture, file))
            })
            .await
            .unwrap_or_else(|e| Err(format!("drawing failed: {e}")));
            AppEvent::Rendered { path, quick, generation, result }
        });
    }

    fn folder_size(&mut self, path: VfsPath) {
        let registry = self.context.registry.clone();
        self.set_status("Counting…");
        self.spawn_task(async move {
            use futures::StreamExt;
            let provider = match registry.provider_for(&path).await {
                Ok(provider) => provider,
                Err(e) => return done(Err(e), None),
            };
            let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
            let mut items = nmc_vfs::walk(provider.as_ref(), &path, None);
            while let Some(item) = items.next().await {
                if let Ok(item) = item {
                    match item.entry.kind {
                        EntryKind::Directory => dirs += 1,
                        _ => {
                            files += 1;
                            bytes += item.entry.size;
                        }
                    }
                }
            }
            let name = path.file_name().unwrap_or("/");
            AppEvent::TaskDone {
                message: format!(
                    "{name}: {} in {files} files, {dirs} folders",
                    nmc_panel::format::human_size(bytes)
                ),
                error: false,
                reload: false,
                focus: None,
            }
        });
    }

    fn edit_current(&mut self) {
        let Some(path) = self.workspace.active().current_path() else { return };
        if self.workspace.active().current_entry().is_some_and(|e| e.is_dir()) {
            return;
        }
        match path.to_local_path().filter(|_| path.scheme() == "file") {
            Some(local) => {
                self.external = Some(External {
                    command: self.editor_command(&local),
                    reload_settings: false,
                    rename: None,
                });
            }
            None => self.set_error("Only local files can be edited; use F3 to view"),
        }
    }

    /// The editor command line for `file`: the configured editor, then
    /// `$VISUAL`, `$EDITOR` (in a terminal only), and finally a platform
    /// default.
    fn editor_command(&self, file: &Path) -> Vec<String> {
        let configured = self.context.config.general.editor.trim().to_owned();
        let from_environment = if self.graphical {
            Vec::new()
        } else {
            vec![
                std::env::var("VISUAL").unwrap_or_default(),
                std::env::var("EDITOR").unwrap_or_default(),
            ]
        };
        let editor = std::iter::once(configured)
            .chain(from_environment)
            .find(|e| !e.trim().is_empty())
            .unwrap_or_else(|| default_editor(self.graphical).into());
        let mut command: Vec<String> = editor.split_whitespace().map(str::to_owned).collect();
        command.push(file.display().to_string());
        command
    }

    /// Called by the main loop after an external program ended.
    pub fn after_external(&mut self, external: &External, result: Result<(), String>) {
        if let Some((file, paths)) = &external.rename {
            match result {
                Ok(()) => self.finish_bulk_rename(file, paths.clone()),
                Err(e) => {
                    let _ = std::fs::remove_file(file);
                    self.set_error(format!("{}: {e}", external.command[0]));
                }
            }
            return;
        }
        if let Err(e) = result {
            self.set_error(format!("{}: {e}", external.command[0]));
        }
        if external.reload_settings {
            match nmc_config::Config::load(&self.context.paths.config_file()) {
                Ok(config) => {
                    self.context.config = config;
                    self.apply_settings();
                    if self.status.is_none() {
                        self.set_status("Settings reloaded");
                    }
                }
                Err(e) => self.set_error(e.to_string()),
            }
        }
        self.reload_panels(None);
    }

    fn apply_key_overrides(&mut self) {
        let (keymap, warnings) = Keymap::with_overrides(&self.context.config.keys);
        self.context.keymap = keymap;
        if !warnings.is_empty() {
            self.set_error(warnings.join("; "));
        }
    }

    /// Saves the settings; reports and returns `false` on failure.
    fn save_settings(&mut self) -> bool {
        match self.context.save_config() {
            Ok(()) => {
                // Connections may have been added, changed or deleted: what was
                // remembered about their servers follows.
                nmc_app::apply_endpoint_limits(&self.manager, &self.context.config);
                true
            }
            Err(e) => {
                self.set_error(e.to_string());
                false
            }
        }
    }

    fn unpack_current(&mut self) {
        let Some(archive) = self.workspace.active().current_path() else { return };
        self.unpack_archive(archive);
    }

    /// Queues the copy of everything in `archive` to the other panel. An
    /// encrypted archive is first opened and one file read: when that needs
    /// a password, the user is asked for it (the copy would only fail with
    /// "permission denied" in the queue) and the unpack starts again.
    pub(super) fn unpack_archive(&mut self, archive: VfsPath) {
        let Some(root) = archive_root(&archive) else {
            return self.set_error("Not an archive");
        };
        let destination = self.workspace.inactive().location().clone();
        let registry = self.context.registry.clone();
        let manager = self.manager.clone();
        let options = self.context.job_options_to(None, Some(&destination));
        self.spawn_task(async move {
            let result = async {
                let provider = registry.provider_for(&root).await?;
                let listing = collect_listing(provider.as_ref(), &root).await?;
                probe_member(provider.as_ref(), &root).await?;
                let members: Vec<VfsPath> = listing.iter().map(|e| root.join(&e.name)).collect();
                let spec = JobSpec {
                    kind: JobKind::Copy,
                    sources: members,
                    destination: Some(destination),
                    options,
                    priority: 0,
                };
                let title = spec.title();
                manager.submit(spec).map_err(|e| nmc_vfs::VfsError::Other(e.to_string()))?;
                Ok(format!("Queued: {title}"))
            }
            .await;
            if result.is_err() && nmc_archive::needs_password(&archive) {
                return AppEvent::NeedPassword(archive);
            }
            AppEvent::TaskDone {
                error: result.is_err(),
                message: result.unwrap_or_else(|e: nmc_vfs::VfsError| e.to_string()),
                reload: false,
                focus: None,
            }
        });
    }

    fn show_properties(&mut self) {
        let panel = self.workspace.active();
        let Some(path) = panel.current_path() else { return };
        let Some(entry) = panel.current_entry().cloned() else { return };
        let kind = match entry.kind {
            EntryKind::File => "file",
            EntryKind::Directory => "folder",
            EntryKind::Symlink => "symbolic link",
            EntryKind::Other => "special file",
        };
        let mut lines = vec![
            format!("Name:        {}", entry.name),
            format!("Location:    {}", display_location(panel.location())),
            format!("Type:        {kind}"),
        ];
        if !entry.is_dir() {
            lines.push(format!(
                "Size:        {} ({} bytes)",
                nmc_panel::format::human_size(entry.size),
                entry.size
            ));
        }
        if let Some(modified) = entry.modified {
            lines.push(format!("Modified:    {}", nmc_panel::format::local_time(modified)));
        }
        if let Some(target) = &entry.symlink_target {
            lines.push(format!("Points to:   {target}"));
        }
        let mut suggestion = "now".to_owned();
        if let Some(mode) = entry.permissions {
            lines.push(format!(
                "Permissions: {} ({:o})",
                nmc_panel::format::permissions(mode),
                mode & 0o7777
            ));
            suggestion = format!("mode={:o}", mode & 0o7777);
        }
        if let Some(flags) = entry.os_flags {
            let names = nmc_vfs::os_flags::describe(flags);
            let shown = if names.is_empty() { "none".to_owned() } else { names.join(", ") };
            lines.push(format!("Attributes:  {shown}"));
        }
        let then = Some(InputDialog::new(
            "Dates and permissions",
            &format!("Change {} (date, time, now, mode=644, -r):", entry.name),
            &suggestion,
            InputPurpose::Attributes(vec![path.clone()]),
        ));
        self.overlay = Some(Overlay::Message(MessageDialog {
            title: "Properties".into(),
            lines,
            then,
            then_choose: None,
            back_to_settings: None,
            logo: false,
            busy: false,
        }));
    }

    fn checksum_current(&mut self) {
        let panel = self.workspace.active();
        let Some(path) = panel.current_path() else { return };
        if panel.current_entry().is_some_and(|e| e.is_dir()) {
            return self.set_error("Checksums are computed for files");
        }
        let registry = self.context.registry.clone();
        self.set_status("Computing checksums…");
        self.spawn_task(async move {
            let result = async {
                let provider = registry.provider_for(&path).await?;
                let mut lines = vec![format!("{}", path.file_name().unwrap_or(""))];
                for (name, algorithm) in
                    [("BLAKE3", HashAlgorithm::Blake3), ("SHA-256", HashAlgorithm::Sha256)]
                {
                    let digest = nmc_ops::checksum(provider.as_ref(), &path, algorithm).await?;
                    lines.push(format!("{name:<8} {digest}"));
                }
                Ok::<_, nmc_vfs::VfsError>(lines)
            }
            .await;
            match result {
                Ok(lines) => AppEvent::Show(Box::new(Overlay::Message(MessageDialog {
                    title: "Checksums".into(),
                    lines,
                    then: None,
                    then_choose: None,
                    back_to_settings: None,
                    logo: false,
                    busy: false,
                }))),
                Err(e) => done(Err(e), None),
            }
        });
    }

    /// Compare (view only): two files under the cursors are compared by
    /// content; otherwise the two folders are compared by name, size and
    /// date, and the differences are selected in both panels.
    fn compare(&mut self) {
        let a = self.workspace.active();
        let b = self.workspace.inactive();
        let file = |panel: &Panel| {
            panel.current_entry().filter(|e| !e.is_dir()).and_then(|_| panel.current_path())
        };
        if let (Some(left), Some(right)) = (file(a), file(b)) {
            let registry = self.context.registry.clone();
            self.set_status("Comparing…");
            return self.spawn_task(async move {
                match diff_files(&registry, &left, &right).await {
                    Ok(Some(viewer)) => AppEvent::Show(Box::new(Overlay::Viewer(viewer))),
                    Ok(None) | Err(_) => {
                        let result = compare_files(&registry, &left, &right).await;
                        done(result, None)
                    }
                }
            });
        }
        let (only_a, only_b, different) = compare_listings(a, b);
        let active = self.workspace.active_side();
        let summary = format!(
            "{} only here, {} only there, {} different",
            only_a.len() + different.0.len(),
            only_b.len() + different.1.len(),
            different.0.len() + different.1.len()
        );
        for (side, names) in
            [(active, [only_a, different.0]), (active.other(), [only_b, different.1])]
        {
            let panel = self.workspace.side_mut(side).panel_mut();
            panel.clear_selection();
            for name in names.iter().flatten() {
                panel.set_selected(name, true);
            }
        }
        self.compare_marks = true;
        self.set_status(format!("Compared folders: {summary} (selected, Esc clears)"));
    }

    /// Shift+arrows, Shift+PgUp/PgDn and Shift+Home/End scroll the quick
    /// view (or turn the pages of a PDF in it); the plain keys keep moving
    /// the cursor of the panel. Returns whether the key was used.
    fn quick_view_key(&mut self, press: &KeyPress) -> bool {
        let chord = press.chord;
        if !chord.shift || chord.ctrl || chord.alt {
            return false;
        }
        let key = chord.key;
        if !matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home | Key::End)
        {
            return false;
        }
        let page = self.page.max(1);
        let Some(Some(viewer)) = &mut self.quick_view else { return false };
        if let Some(source) = viewer_picture_key(viewer, Some(key), None) {
            let path = viewer.path.clone();
            self.render_picture(path, true, source);
            return true;
        }
        let ipage = page as isize;
        match key {
            Key::Up => viewer.scroll_by(-1, page),
            Key::Down => viewer.scroll_by(1, page),
            Key::PageUp => viewer.scroll_by(-ipage, page),
            Key::PageDown => viewer.scroll_by(ipage, page),
            Key::Home => viewer.scroll = 0,
            _ => viewer.scroll_by(isize::MAX, page),
        }
        true
    }

    /// Keeps the quick view in sync with the cursor.
    fn update_quick_view(&mut self) {
        let Some(current) = &self.quick_view else { return };
        let panel = self.workspace.active();
        let wanted =
            panel.current_path().filter(|_| panel.current_entry().is_some_and(|e| !e.is_dir()));
        if current.as_ref().map(|v| &v.path) == wanted.as_ref() {
            return;
        }
        match wanted {
            Some(path) => {
                self.quick_view = Some(Some(Viewer::new(path.clone(), true)));
                self.load_file(path, true);
            }
            None => self.quick_view = Some(None),
        }
    }
}

/// Keys that change a PDF page or the view of a model. Returns what to
/// render when the key did something.
fn viewer_picture_key(
    viewer: &mut Viewer,
    key: Option<Key>,
    text: Option<char>,
) -> Option<crate::preview::Source> {
    if viewer.hex {
        return None;
    }
    if viewer.pdf.is_some() && viewer.zoom.factor <= 1.0 {
        return match (key, text) {
            (Some(Key::PageDown | Key::Space | Key::Right | Key::Down), _) => viewer.turn_page(1),
            (Some(Key::PageUp | Key::Left | Key::Up), _) => viewer.turn_page(-1),
            (Some(Key::Home), _) => viewer.turn_page(isize::MIN / 2),
            (Some(Key::End), _) => viewer.turn_page(isize::MAX / 2),
            _ => None,
        };
    }
    if viewer.pdf.is_some() {
        return match key {
            Some(Key::PageDown | Key::Space) => viewer.turn_page(1),
            Some(Key::PageUp) => viewer.turn_page(-1),
            _ => None,
        };
    }
    if viewer.model.is_some() {
        return match (key, text) {
            (Some(Key::Left), _) => viewer.turn_model(-15.0, 0.0, 0),
            (Some(Key::Right), _) => viewer.turn_model(15.0, 0.0, 0),
            (Some(Key::Up), _) => viewer.turn_model(0.0, 15.0, 0),
            (Some(Key::Down), _) => viewer.turn_model(0.0, -15.0, 0),
            (_, Some('+' | '=')) => viewer.turn_model(0.0, 0.0, 1),
            (_, Some('-')) => viewer.turn_model(0.0, 0.0, -1),
            (Some(Key::Home), _) | (_, Some('0')) => viewer.reset_model(),
            _ => None,
        };
    }
    None
}

/// Zoom and pan keys of pictures and PDF pages: `+`, `-`, `0` (fit) and,
/// when zoomed in, the arrows. Returns whether the key was used.
fn viewer_zoom_key(viewer: &mut Viewer, key: Option<Key>, text: Option<char>) -> bool {
    if viewer.model.is_some() {
        return false;
    }
    let zoomed = viewer.zoom.factor > 1.0;
    match (key, text) {
        (_, Some('+' | '=')) => viewer.zoom.change(1),
        (_, Some('-')) => viewer.zoom.change(-1),
        (_, Some('0')) => viewer.zoom = crate::preview::Zoom::default(),
        (Some(Key::Left), _) if zoomed => viewer.zoom.pan(-1, 0),
        (Some(Key::Right), _) if zoomed => viewer.zoom.pan(1, 0),
        (Some(Key::Up), _) if zoomed => viewer.zoom.pan(0, -1),
        (Some(Key::Down), _) if zoomed => viewer.zoom.pan(0, 1),
        _ => return false,
    }
    true
}

/// Reads a byte of the first file found in `folder` (looking into folders
/// too, a few levels down): this is where a wrong or missing password of an
/// archive shows, before a copy is queued.
async fn probe_member(
    provider: &dyn nmc_vfs::Provider,
    folder: &VfsPath,
) -> Result<(), nmc_vfs::VfsError> {
    use tokio::io::AsyncReadExt;
    let mut pending = vec![(folder.clone(), 0)];
    while let Some((dir, depth)) = pending.pop() {
        for entry in collect_listing(provider, &dir).await? {
            let path = dir.join(&entry.name);
            if entry.is_dir() {
                if depth < 4 {
                    pending.push((path, depth + 1));
                }
            } else if entry.kind == EntryKind::File && entry.size > 0 {
                let mut stream = provider.read(&path, 0).await?;
                let mut byte = [0u8; 1];
                stream
                    .read(&mut byte)
                    .await
                    .map_err(|e| nmc_vfs::VfsError::Other(e.to_string()))?;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The result of a background task as a [`AppEvent::TaskDone`] that
/// reloads the panels.
/// How many new names the multi-rename confirmation lists.
const PREVIEW_ROWS: usize = 12;

/// `old → new` lines for the names that change, at most `rows` of them;
/// names that belong to another file (`conflicts`) come first, marked.
fn rename_lines(
    paths: &[VfsPath],
    names: &[String],
    conflicts: &[VfsPath],
    rows: usize,
) -> Vec<String> {
    let mut changed: Vec<(bool, String)> = paths
        .iter()
        .zip(names)
        .filter(|(path, name)| path.file_name() != Some(name.as_str()))
        .map(|(path, name)| {
            let taken = conflicts.contains(&renamed(path, name));
            let mark = if taken { "  (already exists)" } else { "" };
            (!taken, format!("  {} → {name}{mark}", path.file_name().unwrap_or("")))
        })
        .collect();
    changed.sort_by_key(|(not_taken, _)| *not_taken);
    let more = changed.len().saturating_sub(rows);
    let mut lines: Vec<String> = changed.into_iter().take(rows).map(|(_, line)| line).collect();
    if more > 0 {
        lines.push(format!("  … and {more} more"));
    }
    lines
}

/// The live preview of the Ctrl+M dialog: the first new names `template`
/// gives `paths`, or why it cannot be used.
fn template_preview(paths: &[VfsPath], template: &str) -> Vec<String> {
    let rule = RenameRule { template: template.to_owned(), ..RenameRule::default() };
    match preview_renames(paths, &rule) {
        Ok(names) => {
            let lines = rename_lines(paths, &names, &[], 5);
            if lines.is_empty() { vec!["  (no name changes)".into()] } else { lines }
        }
        Err(e) => vec![format!("  Cannot rename: {e}")],
    }
}

/// Marks the result of a task as something Ctrl+Z can take back.
fn undoable(step: UndoStep, event: AppEvent) -> AppEvent {
    AppEvent::Undoable(step, Box::new(event))
}

fn done(result: nmc_vfs::Result<String>, focus: Option<String>) -> AppEvent {
    match result {
        Ok(message) => AppEvent::TaskDone { message, error: false, reload: true, focus },
        Err(e) => {
            AppEvent::TaskDone { message: e.to_string(), error: true, reload: true, focus: None }
        }
    }
}

/// Creates `relative` (which may contain `/`) below `base`.
async fn make_dirs(
    registry: &nmc_vfs::ProviderRegistry,
    base: &VfsPath,
    relative: &str,
) -> nmc_vfs::Result<()> {
    let provider = registry.provider_for(base).await?;
    let mut path = base.clone();
    let parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
    for (i, part) in parts.iter().enumerate() {
        path = path.join(part);
        let last = i + 1 == parts.len();
        if !last && provider.stat(&path).await.is_ok() {
            continue;
        }
        provider.create_dir(&path).await?;
    }
    Ok(())
}

/// Compares two files by size, then by BLAKE3 hash.
/// The side-by-side comparison of two text files, or `None` when they are
/// not both (small enough) text.
async fn diff_files(
    registry: &nmc_vfs::ProviderRegistry,
    a: &VfsPath,
    b: &VfsPath,
) -> Result<Option<Viewer>, String> {
    const LIMIT: u64 = 4 << 20;
    let (data_a, cut_a) = viewer::load(registry, a, LIMIT).await.map_err(|e| e.to_string())?;
    let (data_b, cut_b) = viewer::load(registry, b, LIMIT).await.map_err(|e| e.to_string())?;
    if cut_a || cut_b || !viewer::looks_like_text(&data_a) || !viewer::looks_like_text(&data_b) {
        return Ok(None);
    }
    let (Ok(text_a), Ok(text_b)) = (String::from_utf8(data_a), String::from_utf8(data_b)) else {
        return Ok(None);
    };
    let name = |p: &VfsPath| p.file_name().unwrap_or_default().to_owned();
    Ok(crate::diff::Diff::new(&name(a), &text_a, &name(b), &text_b)
        .map(|diff| Viewer::for_diff(a.clone(), diff)))
}

async fn compare_files(
    registry: &nmc_vfs::ProviderRegistry,
    a: &VfsPath,
    b: &VfsPath,
) -> nmc_vfs::Result<String> {
    let provider_a = registry.provider_for(a).await?;
    let provider_b = registry.provider_for(b).await?;
    let (stat_a, stat_b) = (provider_a.stat(a).await?, provider_b.stat(b).await?);
    if stat_a.size != stat_b.size {
        return Ok(format!("Files differ: {} vs {} bytes", stat_a.size, stat_b.size));
    }
    let hash_a = nmc_ops::checksum(provider_a.as_ref(), a, HashAlgorithm::Blake3).await?;
    let hash_b = nmc_ops::checksum(provider_b.as_ref(), b, HashAlgorithm::Blake3).await?;
    Ok(if hash_a == hash_b {
        "Files are identical".into()
    } else {
        format!("Files differ (same size, {} bytes)", stat_a.size)
    })
}

/// Names only in `a`, only in `b`, and (newer or different in a, in b).
type Differences = (Vec<String>, Vec<String>, (Vec<String>, Vec<String>));

/// Compares the listings of two panels. A file present on both sides counts
/// as different when size or modification time differ; the newer copy is
/// marked (both when only the size differs).
fn compare_listings(a: &Panel, b: &Panel) -> Differences {
    use std::collections::BTreeMap;
    let rows = |panel: &Panel| -> BTreeMap<String, nmc_vfs::Entry> {
        panel
            .entries()
            .iter()
            .filter(|e| e.name != nmc_panel::PARENT)
            .map(|e| (e.name.clone(), e.clone()))
            .collect()
    };
    let (left, right) = (rows(a), rows(b));
    let only_a = left.keys().filter(|n| !right.contains_key(*n)).cloned().collect();
    let only_b = right.keys().filter(|n| !left.contains_key(*n)).cloned().collect();
    let mut newer = (Vec::new(), Vec::new());
    for (name, x) in &left {
        let Some(y) = right.get(name) else { continue };
        if x.is_dir() || y.is_dir() {
            continue;
        }
        // Times are compared to the second: many file systems store less.
        let seconds = |e: &nmc_vfs::Entry| {
            e.modified
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        };
        let (tx, ty) = (seconds(x), seconds(y));
        if tx == ty && x.size == y.size {
            continue;
        }
        match tx.cmp(&ty) {
            std::cmp::Ordering::Greater => newer.0.push(name.clone()),
            std::cmp::Ordering::Less => newer.1.push(name.clone()),
            std::cmp::Ordering::Equal => {
                newer.0.push(name.clone());
                newer.1.push(name.clone());
            }
        }
    }
    (only_a, only_b, newer)
}

fn home_file(name: &str) -> String {
    directories::UserDirs::new()
        .map(|dirs| dirs.home_dir().join(name).display().to_string())
        .unwrap_or_else(|| name.to_owned())
}

/// Expands a leading `~/` in a local file name.
fn expand_home(text: &str) -> PathBuf {
    match text.strip_prefix("~/") {
        Some(rest) => directories::UserDirs::new()
            .map(|dirs| dirs.home_dir().join(rest))
            .unwrap_or_else(|| PathBuf::from(text)),
        None => PathBuf::from(text),
    }
}

/// The editor used when none is configured. A terminal gets `vi`; a desktop
/// window gets the system's text editor, waiting for it where the platform
/// allows (`open -W` on macOS).
fn default_editor(graphical: bool) -> &'static str {
    if cfg!(windows) {
        "notepad"
    } else if !graphical {
        "vi"
    } else if cfg!(target_os = "macos") {
        "open -W -t"
    } else {
        "xdg-open"
    }
}

/// The question before a list entry is removed (see [`Forget`]).
fn forget_question(what: Forget, question: String) -> Overlay {
    Overlay::Confirm(ConfirmDialog {
        title: "Remove".into(),
        lines: vec![question],
        purpose: ConfirmPurpose::Forget(what),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn home_is_written_as_a_tilde() {
        let home = Path::new("/home/ada");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            abbreviate_home(&home.join("Documents"), Some(home)),
            format!("~{sep}Documents")
        );
        assert_eq!(abbreviate_home(home, Some(home)), "~");
        assert_eq!(abbreviate_home(Path::new("/home/adam/x"), Some(home)), "/home/adam/x");
        assert_eq!(abbreviate_home(Path::new("/etc"), Some(Path::new("/"))), "/etc", "not for /");
        assert_eq!(abbreviate_home(Path::new("/etc"), None), "/etc");
    }
}
