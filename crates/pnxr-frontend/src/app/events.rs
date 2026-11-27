//! Background work: tasks, their events and what each one changes.

use super::*;

impl App {
    // ----------------------------------------------------- background work

    pub(super) fn load(&self, request: LoadRequest) {
        let registry = self.context.registry.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            let listing = pnxr_panel::load(&registry, request).await;
            let _ = events.send(AppEvent::Listing(listing));
        });
    }

    pub(super) fn load_opt(&self, request: Option<LoadRequest>) {
        if let Some(request) = request {
            self.load(request);
        }
    }

    /// The active panel could not open an `scs://` server that is not
    /// there (nothing answers, or no certificates yet): offers to install
    /// `pnxr serve` instead of leaving the user with a bare error.
    pub(super) fn offer_install_for(&mut self, location: &VfsPath, error: &str) {
        if location.scheme() != "scs" || self.overlay.is_some() {
            return;
        }
        let lower = error.to_lowercase();
        let missing =
            ["refused", "no answer", "timed out", "no certificates", "handshake", "certificate"]
                .iter()
                .any(|word| lower.contains(word));
        let Ok(host) = self.context.remotes.serve_host(location.authority()) else { return };
        if !missing || !self.install_offered.insert(host.clone()) {
            return;
        }
        self.offer_install(&host, error);
    }

    /// Uses `host` for terminals, Git and disks (tests put a fake).
    pub fn set_host(&mut self, host: Arc<dyn crate::ports::HostTools>) {
        self.host = host;
    }

    /// Runs `work` in the background; its result event ends the task.
    pub(super) fn spawn_task(&mut self, work: impl Future<Output = AppEvent> + Send + 'static) {
        self.tasks += 1;
        let events = self.events.clone();
        tokio::spawn(async move {
            let _ = events.send(work.await);
        });
    }

    /// Frees the archives no tab shows any more (a panel left them). Not
    /// while transfers run: they may be reading from one.
    pub(super) fn release_archives(&self) {
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

    pub(super) fn reload_panels(&mut self, focus: Option<&str>) {
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

    pub(super) fn refresh_jobs(&mut self) {
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

    pub(super) fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(Status { text: text.into(), error: false });
    }

    pub(super) fn set_error(&mut self, text: impl Into<String>) {
        self.status = Some(Status { text: text.into(), error: true });
    }

    /// Applies the result of background work.
    pub fn handle_event(&mut self, event: AppEvent) {
        self.apply_event(event);
        self.ask_archive_password();
    }

    pub(super) fn apply_event(&mut self, event: AppEvent) {
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
                            blocks.clone().map(|b| (path.clone(), FolderNote::new(b)));
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
                                panel.error().is_some_and(pnxr_ops::is_connection_error);
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
                        if pnxr_ops::is_connection_error(&error) {
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
                if !error && message.starts_with("Installed ") && message.contains(" It asks to") {
                    // A plugin was installed: its default keys come into effect.
                    self.apply_key_overrides();
                }
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
                            back: false,
                            back_to_settings: None,
                            logo: false,
                            busy: false,
                            fixed: false,
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
}
