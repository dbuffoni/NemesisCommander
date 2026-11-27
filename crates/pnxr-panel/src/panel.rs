//! A single directory view.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use pnxr_index::{Mask, SortKey, SortSpec, sort_entries};
use pnxr_vfs::{Entry, EntryKind, ProviderRegistry, Result, VfsPath, collect_listing};

/// Ask the front end to list `path` and give the result back with
/// [`Panel::apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadRequest {
    /// Identifies the request; stale results are ignored.
    pub id: u64,
    /// Directory to list.
    pub path: VfsPath,
    /// List every file below `path` (the branch, or flat, view) instead of
    /// the folder itself.
    pub branch: bool,
}

/// The answer to a [`LoadRequest`].
#[derive(Debug)]
pub struct Listing {
    /// The request this answers.
    pub id: u64,
    /// Directory listed.
    pub path: VfsPath,
    /// The entries are every file below `path` (see [`LoadRequest::branch`]).
    pub branch: bool,
    /// A branch listing stopped at [`BRANCH_LIMIT`] files.
    pub truncated: bool,
    /// Entries or the error.
    pub result: Result<Vec<Entry>>,
}

/// The most files a branch (flat) view lists; deeper trees are cut there.
pub const BRANCH_LIMIT: usize = 100_000;

/// Lists a directory for a [`LoadRequest`].
pub async fn load(registry: &ProviderRegistry, request: LoadRequest) -> Listing {
    let mut truncated = false;
    let result = async {
        let provider = registry.provider_for(&request.path).await?;
        if request.branch {
            let (entries, cut) = collect_branch(provider.as_ref(), &request.path).await?;
            truncated = cut;
            Ok(entries)
        } else {
            collect_listing(provider.as_ref(), &request.path).await
        }
    }
    .await;
    Listing { id: request.id, path: request.path, branch: request.branch, truncated, result }
}

/// Every file below `root`, named by its path relative to `root`
/// (`sub/deeper/file.txt`), and whether the walk stopped at
/// [`BRANCH_LIMIT`]. Folders are not listed, only walked; a folder that
/// cannot be read is skipped. Symbolic links to folders are listed but not
/// followed, so a link cycle cannot loop.
async fn collect_branch(
    provider: &dyn pnxr_vfs::Provider,
    root: &VfsPath,
) -> Result<(Vec<Entry>, bool)> {
    let mut files = Vec::new();
    let mut pending = vec![(root.clone(), String::new())];
    let mut first = true;
    while let Some((folder, prefix)) = pending.pop() {
        let listing = match collect_listing(provider, &folder).await {
            Ok(listing) => listing,
            // The folder asked for must be readable; deeper ones may not be.
            Err(e) if first => return Err(e),
            Err(_) => continue,
        };
        first = false;
        for mut entry in listing {
            if entry.kind == EntryKind::Directory {
                pending.push((folder.join(&entry.name), format!("{prefix}{}/", entry.name)));
                continue;
            }
            entry.name = format!("{prefix}{}", entry.name);
            files.push(entry);
            if files.len() >= BRANCH_LIMIT {
                return Ok((files, true));
            }
        }
    }
    Ok((files, false))
}

/// Totals of the current selection, for the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectionSummary {
    /// Selected files.
    pub files: usize,
    /// Selected directories.
    pub dirs: usize,
    /// Sum of selected file sizes.
    pub bytes: u64,
}

/// State of a selection that Shift+arrows are extending.
#[derive(Debug, Clone)]
struct Extending {
    anchor: usize,
    before: HashSet<String>,
    cursor: usize,
}

/// One directory view. See the crate documentation for the I/O model.
#[derive(Debug, Clone)]
pub struct Panel {
    location: VfsPath,
    /// Everything the provider returned, sorted.
    all: Vec<Entry>,
    /// Entries of `all` that pass the hidden/quick filters, plus a leading
    /// `..` entry when there is a parent.
    visible: Vec<Entry>,
    cursor: usize,
    selected: HashSet<String>,
    /// Selected names that the other panel does not have (a folder
    /// comparison); they are shown in another colour.
    missing: HashSet<String>,
    /// A selection being extended with Shift+arrows: where it started, the
    /// selection from before it, and where the cursor was left.
    extending: Option<Extending>,
    sort: SortSpec,
    show_hidden: bool,
    filter: String,
    back: Vec<VfsPath>,
    forward: Vec<VfsPath>,
    /// Name to put the cursor on once the next listing arrives.
    focus_after_load: Option<String>,
    pending: Option<u64>,
    loading_error: Option<String>,
    /// The listing is the branch (flat) view: every file below `location`.
    branch: bool,
    /// The branch listing was cut at [`BRANCH_LIMIT`].
    truncated: bool,
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// `true` if any part of a relative path (`a/.cache/x`) starts with a dot.
fn is_hidden_path(name: &str) -> bool {
    name.split('/').any(|part| part.starts_with('.') && part != "..")
}

static HIDE_DS_STORE: AtomicBool = AtomicBool::new(false);

/// Hide macOS `.DS_Store` files from every panel, even when hidden files are
/// shown.
pub fn set_hide_ds_store(hide: bool) {
    HIDE_DS_STORE.store(hide, Ordering::Relaxed);
}

/// `true` for an entry the panel hides unless hidden files are shown: a name
/// starting with a dot, or the Windows hidden/system attribute.
fn is_hidden_entry(entry: &Entry) -> bool {
    use pnxr_vfs::os_flags;
    is_hidden_path(&entry.name)
        || entry.os_flags.is_some_and(|f| f & (os_flags::HIDDEN | os_flags::SYSTEM) != 0)
}

fn is_ds_store(name: &str) -> bool {
    name.rsplit('/').next() == Some(".DS_Store")
}

/// Name of the synthetic "parent directory" row.
pub const PARENT: &str = "..";

impl Panel {
    /// A panel showing `location`. Call [`Panel::reload`] to get the first
    /// listing.
    pub fn new(location: VfsPath, sort: SortSpec, show_hidden: bool) -> Self {
        Panel {
            location,
            all: Vec::new(),
            visible: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            missing: HashSet::new(),
            extending: None,
            sort,
            show_hidden,
            filter: String::new(),
            back: Vec::new(),
            forward: Vec::new(),
            focus_after_load: None,
            pending: None,
            loading_error: None,
            branch: false,
            truncated: false,
        }
    }

    // ------------------------------------------------------------ loading

    fn request(&mut self, path: VfsPath) -> LoadRequest {
        // Ids are unique in the whole process, so a front end can offer a
        // listing to every panel and exactly the one that asked takes it.
        let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        self.pending = Some(id);
        LoadRequest { id, path, branch: false }
    }

    /// Lists the current location again, keeping cursor and selection.
    pub fn reload(&mut self) -> LoadRequest {
        self.focus_after_load = self.current_entry().map(|e| e.name.clone());
        let mut request = self.request(self.location.clone());
        request.branch = self.branch;
        request
    }

    /// Switches between the folder and the branch (flat) view: every file
    /// below it in one list, named by its path inside the folder.
    pub fn toggle_branch(&mut self) -> LoadRequest {
        let on = !self.branch;
        self.focus_after_load = None;
        let mut request = self.request(self.location.clone());
        request.branch = on;
        request
    }

    /// `true` while the panel shows the branch (flat) view.
    pub fn is_branch(&self) -> bool {
        self.branch
    }

    /// `true` when the branch view stopped at [`BRANCH_LIMIT`] files.
    pub fn branch_truncated(&self) -> bool {
        self.branch && self.truncated
    }

    /// Lists the current location again and puts the cursor on `name` (for
    /// example a folder that was just created).
    pub fn reload_focus(&mut self, name: &str) -> LoadRequest {
        let request = self.request(self.location.clone());
        self.focus_after_load = Some(name.to_owned());
        request
    }

    /// Opens the folder containing `path` with the cursor on it (used for
    /// search results). `None` when `path` has no parent.
    pub fn reveal(&mut self, path: &VfsPath) -> Option<LoadRequest> {
        let parent = path.parent()?;
        let name = path.file_name()?.to_owned();
        let request = self.navigate(parent);
        self.focus_after_load = Some(name);
        Some(request)
    }

    /// Goes to `path`, recording history.
    pub fn navigate(&mut self, path: VfsPath) -> LoadRequest {
        if path != self.location {
            self.back.push(self.location.clone());
            self.forward.clear();
        }
        self.go(path, None)
    }

    fn go(&mut self, path: VfsPath, focus: Option<String>) -> LoadRequest {
        self.focus_after_load = focus;
        self.request(path)
    }

    /// `true` while a listing is outstanding.
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Id of the outstanding listing request, if any.
    pub fn pending_request(&self) -> Option<u64> {
        self.pending
    }

    /// The error of the last listing, if it failed.
    pub fn error(&self) -> Option<&str> {
        self.loading_error.as_deref()
    }

    /// Applies a listing. Returns `false` (and changes nothing) if it is the
    /// answer to an outdated request.
    pub fn apply(&mut self, listing: Listing) -> bool {
        if self.pending != Some(listing.id) {
            return false;
        }
        self.pending = None;
        match listing.result {
            Ok(entries) => {
                if listing.path != self.location || listing.branch != self.branch {
                    self.selected.clear();
                    self.missing.clear();
                    self.extending = None;
                    self.filter.clear();
                    self.cursor = 0;
                }
                self.location = listing.path;
                self.branch = listing.branch;
                self.truncated = listing.truncated;
                self.loading_error = None;
                self.all = entries;
                sort_entries(&mut self.all, self.sort);
                let still_there: HashSet<&str> = self.all.iter().map(|e| e.name.as_str()).collect();
                self.selected.retain(|name| still_there.contains(name.as_str()));
                let focus = self
                    .focus_after_load
                    .take()
                    .or_else(|| self.current_entry().map(|e| e.name.clone()));
                self.rebuild(focus.as_deref());
            }
            Err(e) => {
                // Stay where we were; the front end shows the error.
                self.focus_after_load = None;
                self.loading_error = Some(e.to_string());
            }
        }
        true
    }

    /// Recomputes the visible rows and puts the cursor on `focus` if given.
    fn rebuild(&mut self, focus: Option<&str>) {
        let filter = Mask::parse(&quick_filter_mask(&self.filter)).unwrap_or_default();
        let mut visible = Vec::with_capacity(self.all.len() + 1);
        // In the branch view `..` always leaves the view.
        if self.branch || self.location.parent().is_some() {
            visible.push(Entry::new(PARENT, EntryKind::Directory));
        }
        visible.extend(
            self.all
                .iter()
                .filter(|e| !(HIDE_DS_STORE.load(Ordering::Relaxed) && is_ds_store(&e.name)))
                .filter(|e| self.show_hidden || !is_hidden_entry(e))
                .filter(|e| filter.matches(&e.name))
                .cloned(),
        );
        self.visible = visible;
        if let Some(name) = focus
            && let Some(i) = self.visible.iter().position(|e| e.name == name)
        {
            self.cursor = i;
        }
        self.cursor = self.cursor.min(self.visible.len().saturating_sub(1));
    }

    // --------------------------------------------------------- navigation

    /// The directory shown.
    pub fn location(&self) -> &VfsPath {
        &self.location
    }

    /// Rows to display, `..` first when there is a parent.
    pub fn entries(&self) -> &[Entry] {
        &self.visible
    }

    /// Index of the cursor row.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The entry under the cursor.
    pub fn current_entry(&self) -> Option<&Entry> {
        self.visible.get(self.cursor)
    }

    /// Full path of the entry under the cursor (`None` on `..`).
    pub fn current_path(&self) -> Option<VfsPath> {
        self.current_entry().filter(|e| e.name != PARENT).map(|e| self.location.join(&e.name))
    }

    /// Moves the cursor by `delta` rows, clamped.
    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.visible.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, last.max(0)) as usize;
    }

    /// Moves the cursor by `delta` rows (clamped) and selects the rows from
    /// where the movement started to the cursor, Shift+arrows style:
    /// moving back unselects again, and what was selected before stays.
    /// Any other selection command ends the range.
    pub fn extend_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let mut state = match self.extending.take() {
            Some(s) if s.cursor == self.cursor => s,
            _ => Extending {
                anchor: self.cursor,
                before: self.selected.clone(),
                cursor: self.cursor,
            },
        };
        self.move_cursor(delta);
        state.cursor = self.cursor;
        let last = self.visible.len() - 1;
        let anchor = state.anchor.min(last);
        let (from, to) = (anchor.min(self.cursor), anchor.max(self.cursor));
        self.selected = state.before.clone();
        self.selected.extend(
            self.visible[from..=to].iter().filter(|e| e.name != PARENT).map(|e| e.name.clone()),
        );
        self.extending = Some(state);
    }

    /// Puts the cursor on a row index (clamped).
    pub fn set_cursor(&mut self, index: usize) {
        self.cursor = index.min(self.visible.len().saturating_sub(1));
    }

    /// Puts the cursor on the entry called `name`, if visible.
    pub fn focus(&mut self, name: &str) {
        if let Some(i) = self.visible.iter().position(|e| e.name == name) {
            self.cursor = i;
        }
    }

    /// Enters the entry under the cursor. Returns a request for directories
    /// and archives, `None` for regular files (the front end opens those).
    pub fn enter(&mut self) -> Option<LoadRequest> {
        let entry = self.current_entry()?.clone();
        if entry.name == PARENT {
            return self.parent();
        }
        let path = self.location.join(&entry.name);
        if entry.is_dir() {
            return Some(self.navigate(path));
        }
        pnxr_archive::archive_root(&path).map(|root| self.navigate(root))
    }

    /// Goes to the parent directory, with the cursor on where we came from.
    /// At an archive root this leaves the archive. In the branch view it
    /// leaves the view instead and shows the folder itself.
    pub fn parent(&mut self) -> Option<LoadRequest> {
        if self.branch {
            return Some(self.go(self.location.clone(), None));
        }
        let parent = self.location.parent()?;
        let came_from = if self.location.is_root() {
            self.location.container().and_then(|c| c.file_name()).map(str::to_owned)
        } else {
            self.location.file_name().map(str::to_owned)
        };
        self.back.push(self.location.clone());
        self.forward.clear();
        Some(self.go(parent, came_from))
    }

    /// History: previous location.
    pub fn back(&mut self) -> Option<LoadRequest> {
        let previous = self.back.pop()?;
        self.forward.push(self.location.clone());
        Some(self.go(previous, None))
    }

    /// History: next location.
    pub fn forward(&mut self) -> Option<LoadRequest> {
        let next = self.forward.pop()?;
        self.back.push(self.location.clone());
        Some(self.go(next, None))
    }

    // ---------------------------------------------------------- selection

    /// Toggles selection of the cursor row and moves down (`..` is never
    /// selectable).
    pub fn toggle_current(&mut self) {
        self.extending = None;
        if let Some(entry) = self.current_entry().filter(|e| e.name != PARENT) {
            let name = entry.name.clone();
            if !self.selected.remove(&name) {
                self.selected.insert(name);
            }
        }
        self.move_cursor(1);
    }

    /// Selects or unselects the entry called `name` (ignored for `..` and
    /// names that are not listed).
    pub fn set_selected(&mut self, name: &str, select: bool) {
        self.extending = None;
        if name == PARENT || !self.all.iter().any(|e| e.name == name) {
            return;
        }
        if select {
            self.selected.insert(name.to_owned());
        } else {
            self.selected.remove(name);
        }
    }

    /// Selects (or unselects) every visible entry matching `mask`.
    /// Directories are only matched when `include_dirs` is set.
    pub fn select_mask(&mut self, mask: &Mask, select: bool, include_dirs: bool) {
        self.extending = None;
        for entry in &self.visible {
            if entry.name == PARENT
                || (entry.is_dir() && !include_dirs)
                || !mask.matches(&entry.name)
            {
                continue;
            }
            if select {
                self.selected.insert(entry.name.clone());
            } else {
                self.selected.remove(&entry.name);
            }
        }
    }

    /// Selects every visible entry.
    pub fn select_all(&mut self) {
        self.extending = None;
        self.selected
            .extend(self.visible.iter().filter(|e| e.name != PARENT).map(|e| e.name.clone()));
    }

    /// Inverts the selection of visible files (directories are left alone,
    /// as in classic commanders).
    pub fn invert_selection(&mut self) {
        self.extending = None;
        for entry in self.visible.iter().filter(|e| e.name != PARENT && !e.is_dir()) {
            if !self.selected.remove(&entry.name) {
                self.selected.insert(entry.name.clone());
            }
        }
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self) {
        self.extending = None;
        self.selected.clear();
        self.missing.clear();
    }

    /// Selects `name` and marks it as missing from the other panel (a
    /// folder comparison): it is shown in light red, not as a plain
    /// selection.
    pub fn set_missing(&mut self, name: &str) {
        self.set_selected(name, true);
        if self.selected.contains(name) {
            self.missing.insert(name.to_owned());
        }
    }

    /// `true` if `name` is selected as missing from the other panel.
    pub fn is_missing(&self, name: &str) -> bool {
        self.missing.contains(name) && self.selected.contains(name)
    }

    /// `true` if `name` is selected.
    pub fn is_selected(&self, name: &str) -> bool {
        self.selected.contains(name)
    }

    /// What an operation should act on: the selection, or the cursor entry
    /// when nothing is selected. Paths are in display order.
    pub fn targets(&self) -> Vec<VfsPath> {
        if self.selected.is_empty() {
            return self.current_path().into_iter().collect();
        }
        self.visible
            .iter()
            .filter(|e| self.selected.contains(&e.name))
            .map(|e| self.location.join(&e.name))
            .collect()
    }

    /// Totals of the selection.
    pub fn selection_summary(&self) -> SelectionSummary {
        let mut summary = SelectionSummary::default();
        for entry in self.visible.iter().filter(|e| self.selected.contains(&e.name)) {
            if entry.is_dir() {
                summary.dirs += 1;
            } else {
                summary.files += 1;
                summary.bytes += entry.size;
            }
        }
        summary
    }

    // ------------------------------------------------------------ display

    /// The sort in effect.
    pub fn sort(&self) -> SortSpec {
        self.sort
    }

    /// Sorts by `key`; choosing the current key again flips the direction.
    pub fn set_sort(&mut self, key: SortKey) {
        self.sort = if self.sort.key == key {
            SortSpec { key, descending: !self.sort.descending }
        } else {
            SortSpec { key, descending: false }
        };
        let focus = self.current_entry().map(|e| e.name.clone());
        sort_entries(&mut self.all, self.sort);
        self.rebuild(focus.as_deref());
    }

    /// Whether dot files are shown.
    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// Shows or hides dot files.
    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
        let focus = self.current_entry().map(|e| e.name.clone());
        self.rebuild(focus.as_deref());
    }

    /// The quick filter text.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Sets the quick filter. Plain text matches anywhere in the name
    /// (case-insensitive); text with `*` or `?` is a mask.
    pub fn set_filter(&mut self, text: &str) {
        self.filter = text.to_owned();
        let focus = self.current_entry().map(|e| e.name.clone());
        self.rebuild(focus.as_deref());
    }
}

/// Turns quick-filter text into a mask: `rep` → `*rep*`, `*.rs` stays.
fn quick_filter_mask(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() || text.starts_with("re:") || text.contains(['*', '?', ';', '!', '[']) {
        text.to_owned()
    } else {
        format!("*{text}*")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnxr_providers::memory::MemoryProvider;
    use std::sync::Arc;

    fn registry() -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        pnxr_archive::register_archive_providers(&mut registry);
        let m = MemoryProvider::new();
        m.put_file("/home/a.txt", "1");
        m.put_file("/home/B.txt", "22");
        m.put_file("/home/.hidden", "");
        m.put_file("/home/docs/readme.md", "333");
        m.put_file("/home/file10.log", "");
        m.put_file("/home/file2.log", "");
        registry.insert(&p("/"), Arc::new(m));
        registry
    }

    fn p(path: &str) -> VfsPath {
        VfsPath::parse(&format!("mem://m{path}")).unwrap()
    }

    async fn run(panel: &mut Panel, registry: &ProviderRegistry, request: Option<LoadRequest>) {
        let listing = load(registry, request.expect("a request")).await;
        assert!(panel.apply(listing));
    }

    fn names(panel: &Panel) -> Vec<&str> {
        panel.entries().iter().map(|e| e.name.as_str()).collect()
    }

    async fn home() -> (Panel, ProviderRegistry) {
        let registry = registry();
        let mut panel = Panel::new(p("/home"), SortSpec::default(), false);
        let request = panel.reload();
        run(&mut panel, &registry, Some(request)).await;
        (panel, registry)
    }

    #[tokio::test]
    async fn reveal_and_reload_focus_place_the_cursor() {
        let (mut panel, registry) = home().await;
        let request = panel.reveal(&p("/home/docs/readme.md"));
        run(&mut panel, &registry, request).await;
        assert_eq!(panel.location(), &p("/home/docs"));
        assert_eq!(panel.current_entry().unwrap().name, "readme.md");

        let request = panel.parent();
        run(&mut panel, &registry, request).await;
        let request = panel.reload_focus("file2.log");
        run(&mut panel, &registry, Some(request)).await;
        assert_eq!(panel.current_entry().unwrap().name, "file2.log");
    }

    #[tokio::test]
    async fn set_selected_ignores_unknown_names() {
        let (mut panel, _) = home().await;
        panel.set_selected("a.txt", true);
        panel.set_selected("nope", true);
        panel.set_selected("..", true);
        assert_eq!(panel.selection_summary().files, 1);
        panel.set_selected("a.txt", false);
        assert!(panel.targets().len() <= 1);
        assert!(!panel.is_selected("a.txt"));
    }

    #[tokio::test]
    async fn shift_arrows_extend_and_shrink_the_selection() {
        let (mut panel, _) = home().await;
        // Rows: `..`, docs, a.txt, B.txt, file2.log, file10.log.
        panel.set_cursor(0);
        panel.set_selected("file10.log", true);
        panel.extend_selection(1);
        panel.extend_selection(1);
        panel.extend_selection(1);
        assert_eq!(panel.cursor(), 3);
        // The anchor is `..`, which is never selected; the rest is.
        assert!(panel.is_selected("docs") && panel.is_selected("a.txt"));
        assert!(panel.is_selected("file10.log"), "earlier selection stays");
        panel.extend_selection(-1);
        assert!(panel.is_selected("a.txt") && panel.is_selected("docs"));
        panel.extend_selection(-2);
        assert!(!panel.is_selected("docs") && panel.is_selected("file10.log"));
        // A move without Shift starts a new range from the new place.
        panel.set_cursor(2);
        panel.extend_selection(isize::MAX / 2);
        assert!(panel.is_selected("file2.log") && panel.is_selected("a.txt"));
        panel.clear_selection();
        panel.extend_selection(-1);
        assert_eq!(panel.selection_summary().files + panel.selection_summary().dirs, 2);
    }

    #[tokio::test]
    async fn listing_is_sorted_with_parent_row_and_no_hidden() {
        let (panel, _) = home().await;
        assert_eq!(names(&panel), ["..", "docs", "a.txt", "B.txt", "file2.log", "file10.log"]);
    }

    #[tokio::test]
    async fn enter_and_parent_keep_the_cursor_meaningful() {
        let (mut panel, registry) = home().await;
        panel.focus("docs");
        let request = panel.enter();
        run(&mut panel, &registry, request).await;
        assert_eq!(panel.location(), &p("/home/docs"));
        assert_eq!(names(&panel), ["..", "readme.md"]);
        let request = panel.parent();
        run(&mut panel, &registry, request).await;
        assert_eq!(
            panel.current_entry().unwrap().name,
            "docs",
            "cursor returns to where we came from"
        );
        // Enter on ".." is the same as parent.
        panel.set_cursor(0);
        let request = panel.enter();
        run(&mut panel, &registry, request).await;
        assert_eq!(panel.location(), &p("/"));
        assert_eq!(panel.current_entry().unwrap().name, "home");
    }

    #[tokio::test]
    async fn the_branch_view_lists_every_file_below() {
        let registry = registry();
        let m = MemoryProvider::new();
        m.put_file("/t/top.txt", "1");
        m.put_file("/t/sub/deep/inner.txt", "22");
        m.put_file("/t/sub/.secret/x", "3");
        m.put_file("/t/sub/side.md", "4");
        registry.insert(&p("/t"), Arc::new(m));
        let mut panel = Panel::new(p("/t"), SortSpec::default(), false);
        let request = panel.reload();
        run(&mut panel, &registry, Some(request)).await;
        assert!(!panel.is_branch());

        let request = panel.toggle_branch();
        run(&mut panel, &registry, Some(request)).await;
        assert!(panel.is_branch());
        // Files only, named by their path in the folder, hidden ones left out.
        assert_eq!(names(&panel), ["..", "sub/deep/inner.txt", "sub/side.md", "top.txt"]);
        // The names resolve to real paths, so operations work on them.
        panel.focus("sub/deep/inner.txt");
        assert_eq!(panel.targets(), [p("/t/sub/deep/inner.txt")]);

        // A reload keeps the view; the parent row leaves it.
        let request = panel.reload();
        run(&mut panel, &registry, Some(request)).await;
        assert!(panel.is_branch());
        panel.set_cursor(0);
        let request = panel.enter();
        run(&mut panel, &registry, request).await;
        assert!(!panel.is_branch());
        assert_eq!(panel.location(), &p("/t"));
        assert_eq!(names(&panel), ["..", "sub", "top.txt"]);
    }

    #[tokio::test]
    async fn the_branch_view_shows_hidden_files_on_request() {
        let (mut panel, registry) = home().await;
        panel.set_show_hidden(true);
        let request = panel.toggle_branch();
        run(&mut panel, &registry, Some(request)).await;
        assert!(names(&panel).contains(&".hidden"));
        assert!(names(&panel).contains(&"docs/readme.md"));
    }

    #[tokio::test]
    async fn enter_on_a_plain_file_is_for_the_front_end() {
        let (mut panel, _) = home().await;
        panel.focus("a.txt");
        assert!(panel.enter().is_none());
    }

    #[tokio::test]
    async fn history_back_and_forward() {
        let (mut panel, registry) = home().await;
        let request = panel.navigate(p("/home/docs"));
        run(&mut panel, &registry, Some(request)).await;
        let request = panel.back();
        run(&mut panel, &registry, request).await;
        assert_eq!(panel.location(), &p("/home"));
        let request = panel.forward();
        run(&mut panel, &registry, request).await;
        assert_eq!(panel.location(), &p("/home/docs"));
        assert!(panel.forward().is_none());
    }

    #[tokio::test]
    async fn stale_listings_are_ignored() {
        let (mut panel, registry) = home().await;
        let slow = panel.navigate(p("/home/docs"));
        let fast = panel.navigate(p("/"));
        let slow_listing = load(&registry, slow).await;
        run(&mut panel, &registry, Some(fast)).await;
        assert!(!panel.apply(slow_listing));
        assert_eq!(panel.location(), &p("/"));
    }

    #[tokio::test]
    async fn failed_listing_keeps_the_old_view() {
        let (mut panel, registry) = home().await;
        let request = panel.navigate(p("/missing"));
        run(&mut panel, &registry, Some(request)).await;
        assert_eq!(panel.location(), &p("/home"));
        assert!(panel.error().unwrap().contains("not found"));
        assert_eq!(panel.entries().len(), 6);
    }

    #[tokio::test]
    async fn selection_targets_and_summary() {
        let (mut panel, _) = home().await;
        assert!(panel.targets().is_empty(), "cursor on .. selects nothing");
        panel.focus("a.txt");
        assert_eq!(panel.targets(), [p("/home/a.txt")]);
        panel.toggle_current(); // a.txt, cursor moves to B.txt
        panel.toggle_current(); // B.txt
        assert_eq!(panel.targets(), [p("/home/a.txt"), p("/home/B.txt")]);
        assert_eq!(panel.selection_summary(), SelectionSummary { files: 2, dirs: 0, bytes: 3 });
        panel.select_mask(&Mask::parse("*.log").unwrap(), true, false);
        assert_eq!(panel.targets().len(), 4);
        panel.invert_selection();
        assert!(panel.targets().is_empty() || panel.targets() == [panel.current_path().unwrap()]);
        panel.select_all();
        assert_eq!(panel.selection_summary().dirs, 1);
        panel.clear_selection();
        assert!(!panel.is_selected("docs"));
    }

    #[tokio::test]
    async fn filter_sort_and_hidden() {
        let (mut panel, _) = home().await;
        panel.set_filter("file");
        assert_eq!(names(&panel), ["..", "file2.log", "file10.log"]);
        panel.set_filter("*.TXT");
        assert_eq!(names(&panel), ["..", "a.txt", "B.txt"]);
        panel.set_filter("");
        panel.set_show_hidden(true);
        assert!(names(&panel).contains(&".hidden"));
        panel.set_sort(SortKey::Size);
        panel.set_sort(SortKey::Size); // second time: descending
        assert!(panel.sort().descending);
        assert_eq!(names(&panel)[2], "B.txt", "largest file first after dirs");
    }

    #[tokio::test]
    async fn reload_keeps_cursor_and_drops_vanished_selection() {
        let registry = registry();
        let mem = registry.provider_for(&p("/")).await.unwrap();
        let mut panel = Panel::new(p("/home"), SortSpec::default(), false);
        let request = panel.reload();
        run(&mut panel, &registry, Some(request)).await;
        panel.focus("B.txt");
        panel.toggle_current();
        panel.focus("file2.log");
        mem.remove_file(&p("/home/B.txt")).await.unwrap();
        let request = panel.reload();
        run(&mut panel, &registry, Some(request)).await;
        assert_eq!(panel.current_entry().unwrap().name, "file2.log");
        assert!(panel.targets() == [p("/home/file2.log")], "vanished selection is dropped");
    }
}
