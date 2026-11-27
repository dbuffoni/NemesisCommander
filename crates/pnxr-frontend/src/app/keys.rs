//! Keys: what a key press does in each window and in the panels.

use super::*;

impl App {
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
    pub(super) fn remember_step(&mut self, before: Option<Overlay>) {
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
        let (ctrl, alt, shift) = self.held;
        (ctrl || self.bar_row == 2, alt, shift || self.bar_row == 1)
    }

    /// The function key bar shows its next row: plain, Shift, Ctrl, plain...
    pub fn next_bar_row(&mut self) {
        self.bar_row = (self.bar_row + 1) % 3;
    }

    /// The row of the function key bar chosen with its key (0, 1 or 2).
    pub fn bar_row(&self) -> u8 {
        self.bar_row
    }

    pub(super) fn normal_key(&mut self, press: KeyPress) {
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

    pub(super) fn conflict_key(&mut self, press: KeyPress) {
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
    pub(super) fn overlay_key(&mut self, overlay: Overlay, press: KeyPress) -> Option<Overlay> {
        let key = press.plain();
        let is_esc = key == Some(Key::Esc);
        match overlay {
            Overlay::Help => None,
            Overlay::Commands(palette) => self.palette_key(palette, press),
            Overlay::Input(mut dialog) => {
                if let InputPurpose::PropsEdit { row, state } = &dialog.purpose {
                    let (row, state) = (*row, (**state).clone());
                    if is_esc {
                        self.overlay = Some(Overlay::Choose(Self::properties_dialog(state, 0)));
                        return self.overlay.take();
                    }
                    if key == Some(Key::Enter) {
                        let text = dialog.field.text().to_owned();
                        self.properties_typed(row, state, &text);
                        return self.overlay.take();
                    }
                }
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
                if matches!(
                    dialog.purpose,
                    InputPurpose::SelectMask | InputPurpose::UnselectMask | InputPurpose::KeepMask
                ) {
                    if key == Some(Key::F(2)) {
                        let next = match dialog.purpose {
                            InputPurpose::SelectMask => InputPurpose::UnselectMask,
                            InputPurpose::UnselectMask => InputPurpose::KeepMask,
                            _ => InputPurpose::SelectMask,
                        };
                        let text = dialog.field.text().to_owned();
                        self.ask_mask(next, Some(&text));
                        return self.overlay.take();
                    }
                    if press.chord.ctrl && press.chord.key == Key::Char('o') {
                        let rows = crate::masks::PRESETS
                            .iter()
                            .map(|(name, mask)| format!("{name:<28} {mask}"))
                            .collect();
                        return Some(Overlay::Choose(ChooseDialog::new(
                            "Ready-made masks",
                            rows,
                            ChoosePurpose::MaskPresets(dialog.purpose.clone()),
                        )));
                    }
                    if key == Some(Key::F(1)) {
                        let mut help =
                            MessageDialog::new("Select by mask", crate::masks::help_lines());
                        help.then = Some(dialog);
                        help.back = true;
                        return Some(Overlay::Message(help));
                    }
                }
                if let InputPurpose::MultiRename(paths) = &dialog.purpose {
                    if press.chord.ctrl && press.chord.key == Key::Char('o') {
                        let rows = crate::renames::PRESETS
                            .iter()
                            .map(|(name, rule)| format!("{name:<36} {rule}"))
                            .collect();
                        return Some(Overlay::Choose(ChooseDialog::new(
                            "Ready-made rename rules",
                            rows,
                            ChoosePurpose::RenamePresets(paths.clone()),
                        )));
                    }
                    if key == Some(Key::F(1)) {
                        let mut help =
                            MessageDialog::new("Multi-rename", crate::renames::help_lines());
                        help.then = Some(dialog);
                        help.back = true;
                        return Some(Overlay::Message(help));
                    }
                }
                if key == Some(Key::F(1))
                    && let Some((title, text)) = crate::help::input_help(&dialog.purpose)
                {
                    let mut help = MessageDialog::new(&title, text);
                    help.then = Some(dialog);
                    help.back = true;
                    return Some(Overlay::Message(help));
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
                            dialog.preview =
                                crate::install::hints(dialog.field.text(), &*self.context.remotes)
                        }
                        InputPurpose::QuickFilter => {
                            self.workspace.active_mut().set_filter(dialog.field.text())
                        }
                        InputPurpose::SelectMask
                        | InputPurpose::UnselectMask
                        | InputPurpose::KeepMask => {
                            dialog.preview = crate::masks::preview(
                                self.workspace.active().entries(),
                                dialog.field.text(),
                            )
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
                if key == Some(Key::F(1))
                    && let Some((title, text)) =
                        crate::help::choose_help(&dialog.purpose, dialog.cursor)
                {
                    let mut help = MessageDialog::new(&title, text);
                    help.then_choose = Some(Box::new(dialog));
                    help.back = true;
                    return Some(Overlay::Message(help));
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
                        if let ChoosePurpose::Font { screen, .. }
                        | ChoosePurpose::Editor { screen, .. } = dialog.purpose
                        {
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
}
