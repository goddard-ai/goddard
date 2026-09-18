use super::*;

fn retain_runtime_after_cancel(provider: ProviderKind) -> bool {
    // Codex's app-server owns the Computer Use process tree, and Amp offers no
    // interrupt on its stream — stopping it means ending the process. Both
    // resume their native thread on the next prompt.
    !matches!(provider, ProviderKind::Codex | ProviderKind::Amp)
}

fn new_task_runtime_mode(current: Option<&AgentSession>, remembered: RuntimeMode) -> RuntimeMode {
    current
        .map(|session| session.runtime_mode)
        .unwrap_or(remembered)
}

fn new_task_sandboxed(current: Option<&AgentSession>, remembered: bool) -> bool {
    current
        .map(|session| session.sandboxed)
        .unwrap_or(remembered)
}

/// The text an unclaimed keystroke should send to the composer, if any:
/// printable characters typed without command-level modifiers. `key_char`
/// carries the layout-resolved character, so Option digraphs and shifted
/// letters arrive as the text a field would have received. Keys whose
/// character is a control character — Tab's `\t`, Enter's `\n` — keep their
/// focus-navigation and activation meanings instead, which also keeps a
/// stray Enter from submitting an unseen draft.
pub(super) fn type_to_focus_text(keystroke: &gpui::Keystroke) -> Option<&str> {
    let modifiers = keystroke.modifiers;
    // Ctrl+Alt passes because it is AltGr on Windows: text composition,
    // not a shortcut chord. Bound chords never reach this listener anyway.
    if (modifiers.control && !modifiers.alt) || modifiers.platform || modifiers.function {
        return None;
    }
    keystroke
        .key_char
        .as_deref()
        .filter(|text| text.chars().all(|character| !character.is_control()))
}

/// Contexts that tell a root-level key listener the keystroke is spoken for:
/// a text surface — or a surface that consumes keystrokes itself — already
/// holds focus. "TextInput" covers every field in the app; the rest are
/// focused panes whose typing is not the composer's to take.
const TYPING_OWNED_CONTEXTS: &[&str] = &[
    "TextInput",
    "Terminal",
    "Browser",
    "BrowserAddress",
    "WakuMenu",
    "CommandPalette",
    "TaskSwitcher",
    "ProjectSwitcher",
    "FindBar",
    "FileEditorPane",
];

/// The topmost unread target in the sidebar — shared by
/// GoToNextUnreadCompletion (⌘D / ctrl-backtick), the unseen-completion
/// bell, and the session-departure fallbacks. "Unread" is the
/// unseen-completion set plus a task blocked on its user — a pending
/// permission or question cannot make progress until someone answers.
/// Sessions with queued prompts are about to be busy again, so they are
/// skipped, and the on-screen or pending-activation session is never a
/// candidate.
///
/// Sidebar order is the importance order — pinned tasks sort to the top of
/// the sidebar and lead automatically — and landing on a session clears its
/// stamp, so repeated presses drain the queue top-down.
pub(super) fn next_unread_completion(
    sessions: &[AgentSession],
    unseen_completions: &HashMap<Uuid, u64>,
    rows: &[sidebar::SidebarRow],
    selected_session: Option<Uuid>,
    pending_activation: Option<Uuid>,
) -> Option<Uuid> {
    let by_id = sessions
        .iter()
        .map(|session| (session.id, session))
        .collect::<HashMap<_, _>>();
    sidebar::next_sidebar_session_in_rows(rows, 0, |session_id| {
        Some(session_id) != selected_session
            && Some(session_id) != pending_activation
            && by_id.get(&session_id).is_some_and(|session| {
                session.has_started()
                    && session.archived_at.is_none()
                    && session.queued_messages.is_empty()
                    && (session.status == SessionStatus::Waiting
                        || unseen_completions.contains_key(&session_id))
            })
    })
}

/// The next non-busy session at-or-below `start_row` in the sidebar's
/// displayed order, wrapping to the top — the shared walk behind the idle
/// rotation and ⌘⇧D's park-and-move-down jump. The selected or
/// pending-activation session is never a candidate.
pub(super) fn next_non_busy_session(
    sessions: &[AgentSession],
    rows: &[sidebar::SidebarRow],
    selected_session: Option<Uuid>,
    pending_activation: Option<Uuid>,
    start_row: usize,
) -> Option<Uuid> {
    let by_id = sessions
        .iter()
        .map(|session| (session.id, session))
        .collect::<HashMap<_, _>>();
    sidebar::next_sidebar_session_in_rows(rows, start_row, |session_id| {
        Some(session_id) != selected_session
            && Some(session_id) != pending_activation
            && by_id
                .get(&session_id)
                .is_some_and(|session| !session.is_busy())
    })
}

/// The drained-queue landing for the unread jumps: when nothing is unread a
/// press cycles through the non-busy sessions instead of dead-ending on the
/// New task page. A selected session that is itself in the rotation
/// continues the walk below its row — wrapping to the top — while anything
/// else enters at the topmost non-busy row: sidebar order is the importance
/// order, so a drained queue restarts at the most important task.
pub(super) fn next_idle_session(
    sessions: &[AgentSession],
    rows: &[sidebar::SidebarRow],
    selected_session: Option<Uuid>,
    pending_activation: Option<Uuid>,
) -> Option<Uuid> {
    let start = selected_session
        .filter(|session_id| {
            sessions
                .iter()
                .any(|session| session.id == *session_id && !session.is_busy())
        })
        .and_then(|session_id| sidebar::sidebar_session_row_index(rows, session_id))
        .map_or(0, |index| index + 1);
    next_non_busy_session(sessions, rows, selected_session, pending_activation, start)
}

impl Waku {
    pub(crate) fn open_task_from_notification(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.select_session(session_id, cx);
    }

    pub(super) fn select_project(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        self.state.selected_project = Some(project_id);
        self.create_session_for(project_id, self.state.last_provider, cx);
    }

    pub(super) fn select_session(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.request_session_activation(session_id, SessionActivationTransition::Visit, cx);
    }

    fn request_session_activation(
        &mut self,
        session_id: Uuid,
        transition: SessionActivationTransition,
        cx: &mut Context<Self>,
    ) {
        if !self
            .state
            .sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            return;
        }
        // Archived tasks stay reachable through explicit activation paths like
        // notification clicks; opening one is intent to bring it back.
        if self
            .state
            .sessions
            .iter()
            .any(|session| session.id == session_id && session.archived_at.is_some())
        {
            self.unarchive_session(session_id, false, cx);
        }
        // Selecting a chat folds the Terminals group; the terminal keeps its
        // last-visible memory for the next expand.
        if self
            .sidebar_collapsed_groups
            .insert(SidebarGroup::Terminals)
        {
            self.sidebar_rows_fingerprint.set(None);
        }
        self.reveal_sidebar_session(session_id);
        let needs_hydration = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .is_some_and(|session| !session.detail_loaded);
        if needs_hydration {
            self.pending_session_activation = Some(PendingSessionActivation {
                session_id,
                transition,
            });
            // Keep the current transcript visible until the daemon returns the
            // target session, but acknowledge the click immediately in the
            // sidebar instead of making the UI appear unresponsive.
            cx.notify();
            self.ensure_session_loaded(session_id, cx);
            return;
        }
        self.pending_session_activation = None;
        self.finish_session_activation(session_id, transition, cx);
    }

    fn finish_session_activation(
        &mut self,
        session_id: Uuid,
        transition: SessionActivationTransition,
        cx: &mut Context<Self>,
    ) {
        match transition {
            SessionActivationTransition::Visit => self.session_navigation.visit(
                self.navigation_location(),
                NavigationLocation::Task(session_id),
            ),
            SessionActivationTransition::Back { from } => {
                if self.navigation_location() != Some(from)
                    || self.session_navigation.back_target()
                        != Some(NavigationLocation::Task(session_id))
                {
                    return;
                }
                let _ = self.session_navigation.go_back(from);
            }
            SessionActivationTransition::Forward { from } => {
                if self.navigation_location() != Some(from)
                    || self.session_navigation.forward_target()
                        != Some(NavigationLocation::Task(session_id))
                {
                    return;
                }
                let _ = self.session_navigation.go_forward(from);
            }
        }
        self.activate_session(session_id, transition, cx);
    }

    /// Loads a session's transcript if startup only fetched its list columns.
    ///
    /// The SQLite query and daemon round trip both stay off the UI thread. The
    /// current selection stays rendered until the requested session is whole.
    pub(super) fn ensure_session_loaded(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let needs_hydration = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .is_some_and(|session| !session.detail_loaded);
        if !needs_hydration || !self.session_hydrations.insert(session_id) {
            return;
        }
        let Some(daemon) = self.daemon_for_session(session_id) else {
            // Offline remote host: drop the mark so hydration retries when
            // the supervisor registers and the row is re-selected.
            self.session_hydrations.remove(&session_id);
            return;
        };
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match waku_client::persistence::hydrate_session(&daemon, session_id)? {
                        Some(session) => Ok(session),
                        None => {
                            anyhow::bail!("the task no longer exists")
                        }
                    }
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                waku.session_hydrations.remove(&session_id);
                match result {
                    Ok(session) => {
                        let replaced = if let Some(existing) = waku
                            .state
                            .sessions
                            .iter_mut()
                            .find(|existing| existing.id == session_id)
                        {
                            *existing = session;
                            true
                        } else {
                            false
                        };
                        let pending = waku
                            .pending_session_activation
                            .filter(|pending| pending.session_id == session_id);
                        if pending.is_some() {
                            waku.pending_session_activation = None;
                        }
                        if replaced && let Some(pending) = pending {
                            waku.finish_session_activation(session_id, pending.transition, cx);
                        } else if waku.state.selected_session == Some(session_id) {
                            waku.reset_visible_state();
                            waku.reset_transcript_rows(waku.transcript_row_count());
                            if !waku.transcript_is_scrolled.get() {
                                waku.apply_transcript_landing(
                                    SessionActivationTransition::Visit,
                                    cx,
                                );
                            }
                            waku.refresh_composer_sources(cx);
                        }
                    }
                    Err(error) => {
                        if waku
                            .pending_session_activation
                            .is_some_and(|pending| pending.session_id == session_id)
                        {
                            waku.pending_session_activation = None;
                        }
                        waku.show_toast(tr!("errors.open_session", error = error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn activate_session(
        &mut self,
        session_id: Uuid,
        transition: SessionActivationTransition,
        cx: &mut Context<Self>,
    ) {
        let session_changed = self.state.selected_session != Some(session_id);
        if session_changed {
            self.capture_and_save_current_composer_draft(cx);
            self.store_selected_right_panel_state();
            self.store_transcript_scroll_position();
        }
        self.state.selected_session = Some(session_id);
        // Session selection and terminal selection are mutually exclusive —
        // the transcript takes the main area back from the terminal.
        self.selected_terminal = None;
        self.state.unseen_completions.remove(&session_id);
        self.task_switcher.record_access(session_id);
        // Picking a task hands the main area back to the transcript; the
        // Projects page keeps its per-project state for the next visit.
        self.projects_page = None;
        if let Some((
            project_id,
            provider,
            runtime_mode,
            sandboxed,
            model,
            reasoning_effort,
            service_tier,
            context_window,
        )) = self.selected_session().map(|session| {
            (
                session.project_id,
                session.provider,
                session.runtime_mode,
                session.sandboxed,
                session.model.clone(),
                session.reasoning_effort.clone(),
                session.service_tier.clone(),
                session.context_window.clone(),
            )
        }) {
            self.state.selected_project = Some(project_id);
            self.state.last_provider = provider;
            self.state.last_runtime_mode = runtime_mode;
            self.state.last_sandboxed = sandboxed;
            self.state.last_model = model;
            self.state.last_reasoning_effort = reasoning_effort;
            self.state.last_service_tier = service_tier;
            self.state.last_context_window = context_window;
        }
        if self
            .selected_session()
            .is_some_and(|session| !session.has_started())
        {
            self.session_navigation.remember_new_task(session_id);
        }
        if session_changed {
            self.restore_selected_composer_draft(cx);
            self.sync_user_input_answer(cx);
            let panel_state = RightPanelSessionState::take_or_closed(
                &mut self.right_panel_session_states,
                session_id,
            );
            self.restore_right_panel_state(panel_state, cx);
            self.restore_missing_worktree(session_id, cx);
            // An open Git panel follows the newly selected session's checkout.
            self.sync_git_panel_workspace(cx);
        } else {
            self.ensure_right_panel_terminals(cx);
        }
        self.reset_visible_state();
        if session_changed {
            // Each materialized worktree has its own cache entry. A task that
            // finished while another session was selected could otherwise
            // retain the clean snapshot captured before its agent made edits.
            self.refresh_selected_branch_snapshot(cx);
        }
        self.refresh_composer_sources(cx);
        self.reset_transcript_rows(self.transcript_row_count());
        self.apply_transcript_landing(transition, cx);
        self.save();
        if self
            .selected_session()
            .is_some_and(AgentSession::has_started)
        {
            self.start_runtime_attachment(session_id, cx);
        }
        cx.notify();
    }

    /// Mirror runtime-only UI state — back/forward history, scroll positions,
    /// open panels — into `self.state` so the next `save` carries it to disk.
    /// The dev watcher's relaunch can SIGTERM the app past its quit hooks, so
    /// state survives only when a routine save already holds it.
    pub(super) fn capture_ui_state(&mut self) {
        self.store_transcript_scroll_position();
        self.state.navigation_back = self
            .session_navigation
            .back
            .iter()
            .filter_map(|location| persisted_location(*location))
            .collect();
        self.state.navigation_forward = self
            .session_navigation
            .forward
            .iter()
            .filter_map(|location| persisted_location(*location))
            .collect();
        self.state.transcript_scroll_positions = self
            .transcript_scroll_positions
            .iter()
            .map(|(session_id, offset)| (*session_id, persisted_list_offset(*offset)))
            .collect();
        let sidebar_top = self.sidebar_list_state.logical_scroll_top();
        self.state.sidebar_scroll = (sidebar_top.item_ix > 0
            || sidebar_top.offset_in_item > Pixels::ZERO)
            .then(|| persisted_list_offset(sidebar_top));
        self.state.projects_page = self.projects_page;
        self.state.settings_page = self.settings_page.map(persisted_settings_page);
        self.state.fullscreen_surface =
            self.fullscreen_surface
                .clone()
                .and_then(|(surface, detail)| {
                    persisted_panel_surface(&surface)
                        .map(|surface| PersistedFullscreenSurface { surface, detail })
                });
        let mut panels: HashMap<Uuid, PersistedRightPanelState> = self
            .right_panel_session_states
            .iter()
            .map(|(session_id, state)| {
                (
                    *session_id,
                    persist_right_panel_state(
                        state.visible,
                        &state.surfaces,
                        state.active_surface,
                        &state.expanded_paths,
                        &state.files_selected_path,
                        state.file_tree_width,
                        state.diff_selected_file,
                        &state.diff_expanded_paths,
                        state.diff_source,
                    ),
                )
            })
            .collect();
        if let Some(session_id) = self.state.selected_session {
            panels.insert(
                session_id,
                persist_right_panel_state(
                    self.right_panel_visible,
                    &self.right_panel_surfaces,
                    self.right_panel_active_surface,
                    &self.right_panel_expanded_paths,
                    &self.right_panel_files_selected_path,
                    self.right_panel_file_tree_width,
                    self.right_panel_diff_selected_file,
                    &self.right_panel_diff_expanded_paths,
                    self.right_panel_diff_source,
                ),
            );
        }
        self.state.right_panel_sessions = panels;
    }

    /// Rehydrate the UI state persisted across the last quit — history,
    /// scroll positions, panel tabs — once the entity exists. Entries naming
    /// sessions or projects that vanished while the app was down are dropped.
    pub(super) fn restore_ui_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task_exists = |id: &Uuid| {
            self.state
                .sessions
                .iter()
                .any(|session| session.id == *id && session.has_started())
        };
        let project_exists = |id: &Uuid| {
            self.state
                .projects
                .iter()
                .any(|project| project.id == *id && !project.is_projectless())
        };
        let location = |entry: &PersistedNavigationLocation| match *entry {
            PersistedNavigationLocation::Task(id) => {
                task_exists(&id).then_some(NavigationLocation::Task(id))
            }
            PersistedNavigationLocation::ProjectsPage(id) => {
                project_exists(&id).then_some(NavigationLocation::ProjectsPage(id))
            }
        };
        self.session_navigation.back = self
            .state
            .navigation_back
            .iter()
            .filter_map(|entry| location(entry))
            .collect();
        self.session_navigation.forward = self
            .state
            .navigation_forward
            .iter()
            .filter_map(|entry| location(entry))
            .collect();
        // Resolved while the read-only closures are still in scope; the
        // mutable work below happens after their borrows end.
        let projects_page = self.state.projects_page.filter(|id| project_exists(id));
        self.transcript_scroll_positions = self
            .state
            .transcript_scroll_positions
            .iter()
            .filter(|(id, _)| task_exists(id))
            .map(|(id, offset)| (*id, list_offset_from_persisted(*offset)))
            .collect();
        self.startup_scroll_restores = self.transcript_scroll_positions.keys().copied().collect();
        self.pending_sidebar_scroll
            .set(self.state.sidebar_scroll.map(list_offset_from_persisted));
        self.right_panel_session_states = self
            .state
            .right_panel_sessions
            .iter()
            .filter(|(id, _)| task_exists(id))
            .map(|(id, state)| (*id, right_panel_state_from_persisted(state)))
            .collect();
        if let Some(session_id) = self.state.selected_session {
            let panel_state = RightPanelSessionState::take_or_closed(
                &mut self.right_panel_session_states,
                session_id,
            );
            self.restore_right_panel_state(panel_state, cx);
            if let Some(offset) = self.transcript_scroll_positions.get(&session_id).copied() {
                let landing = TranscriptLanding::Position(offset);
                // The runtime attach that lands after this resets the rows
                // again — `transcript_landing` re-applies the position there.
                self.transcript_landing = Some((session_id, landing));
                self.scroll_to_transcript_landing(landing, cx);
            }
        }
        // After `restore_right_panel_state`, which clears any fullscreen — a
        // surface swap starts docked — so a persisted one is reinstated here.
        self.fullscreen_surface = self.state.fullscreen_surface.clone().map(|surface| {
            (
                panel_surface_from_persisted(&surface.surface),
                surface.detail,
            )
        });
        if let Some(project_id) = projects_page {
            self.show_projects_page(project_id, window, cx);
        }
        if let Some(page) = self.state.settings_page {
            // The open path, so Usage's scan and the Skills catalog kick off
            // the same way a click on their page would.
            self.open_settings_page(settings_page_from_persisted(page), window, cx);
            self.automatic_updates_enabled = cx
                .try_global::<crate::updater::UpdaterState>()
                .and_then(|updater| updater.0.as_ref())
                .is_some_and(|updater| updater.automatically_checks_for_updates());
        }
    }

    /// Park the departing session's scroll position for back/forward history.
    /// Runs while `selected_session` still points at the session being left.
    pub(super) fn store_transcript_scroll_position(&mut self) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let rows = self.active_transcript_rows();
        let offset = rows.logical_scroll_top();
        // A bottom-aligned list reports a past-the-end index while parked on
        // the tail; there is nothing to restore — the tail is the default.
        if offset.item_ix < rows.item_count() {
            self.transcript_scroll_positions.insert(session_id, offset);
        } else {
            self.transcript_scroll_positions.remove(&session_id);
        }
    }

    /// Drops cached answers about the workspace on disk.
    ///
    /// These queries cache to keep `git` and directory walks out of frames, but
    /// nothing tells us when the working tree changes underneath. Rather than
    /// expire on a timer, they are dropped at the moments the answer plausibly
    /// moved — coming back to the window, or a turn finishing.
    pub(super) fn invalidate_workspace_queries(&mut self, cx: &mut Context<Self>) {
        let Some(workspace_path) = self
            .selected_workspace_path()
            .map(std::path::Path::to_path_buf)
        else {
            return;
        };
        self.branch_snapshots.invalidate(&workspace_path);
        self.sidebar_branch_scan_fingerprint.set(None);
        self.sidebar_branch_scan_generation
            .set(self.sidebar_branch_scan_generation.get().wrapping_add(1));
        self.refresh_workspace_surfaces(cx);
        self.invalidate_composer_sources(cx);
        // The panel's working-tree snapshot moved with the same moments.
        if self.git_panel.is_some() {
            self.refresh_git_panel(cx);
        }
    }

    pub(super) fn create_session_for(
        &mut self,
        project_id: Uuid,
        provider: ProviderKind,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft_id) = self
            .state
            .sessions
            .iter()
            .find(|session| session.project_id == project_id && !session.has_started())
            .map(|session| session.id)
        {
            self.select_session(draft_id, cx);
            return;
        }
        // A task opened from the current task carries its working access mode.
        // `last_runtime_mode` covers launch and the few creation paths without
        // a selected source task.
        let runtime_mode =
            new_task_runtime_mode(self.selected_session(), self.state.last_runtime_mode);
        let sandboxed = new_task_sandboxed(self.selected_session(), self.state.last_sandboxed);
        let mut session = self.state.new_session(project_id, provider);
        session.runtime_mode = runtime_mode;
        session.sandboxed = sandboxed;
        let id = session.id;
        self.daemons
            .claim_session(id, self.daemons.project_owner(project_id));
        self.state.push_session(session);
        self.select_session(id, cx);
    }

    /// The session a workspace choice from the composer lands on, resolving
    /// the workspace subject: the selection normally, the armed card under
    /// Big Picture, or — untargeted — the destination project's unstarted
    /// draft, materialized on first choice so the pick has somewhere to
    /// live. That draft is the same session the submit path's
    /// `create_session_for` finds and sends on.
    pub(super) fn ensure_workspace_subject_session(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        let (session_id, project_id) = self.workspace_subject();
        if session_id.is_some() || !self.big_picture.is_open() {
            return session_id;
        }
        let project_id = project_id?;
        let runtime_mode =
            new_task_runtime_mode(self.selected_session(), self.state.last_runtime_mode);
        let mut session = self.state.new_session(project_id, self.state.last_provider);
        session.runtime_mode = runtime_mode;
        let session_id = session.id;
        self.state.push_session(session);
        self.save();
        cx.notify();
        Some(session_id)
    }

    /// The Projects page docks the same composer a chat does; its workspace,
    /// branch, and model controls target the project's draft — the unstarted
    /// task New Task would reuse. Activation runs without the visit
    /// `select_session` records: the page itself is the location history
    /// captured.
    pub(super) fn bind_projects_page_draft(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        let source = self.composer_draft_key();
        let draft_id = self
            .state
            .sessions
            .iter()
            .find(|session| session.project_id == project_id && !session.has_started())
            .map(|session| session.id)
            .unwrap_or_else(|| {
                let runtime_mode =
                    new_task_runtime_mode(self.selected_session(), self.state.last_runtime_mode);
                let sandboxed =
                    new_task_sandboxed(self.selected_session(), self.state.last_sandboxed);
                let mut session = self.state.new_session(project_id, self.state.last_provider);
                session.runtime_mode = runtime_mode;
                session.sandboxed = sandboxed;
                let id = session.id;
                self.state.push_session(session);
                id
            });
        self.activate_session(draft_id, SessionActivationTransition::Visit, cx);
        // A draft typed against the page's previous project follows the
        // switch into the new project's empty slot — the composer's project
        // picker hands text off the same way.
        if let Some(crate::persistence::ComposerDraftKey::NewSession(_)) = source {
            self.move_composer_draft_after_project_change(source, cx);
        }
        // A remotely synced draft may still be waiting on its detail fetch.
        self.ensure_session_loaded(draft_id, cx);
    }

    pub(super) fn select_workspace(&mut self, workspace: SessionWorkspace, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        self.select_workspace_for(session_id, workspace, cx);
    }

    /// `select_workspace` against an explicit session — the workspace subject
    /// under Big Picture is not necessarily the session selected underneath.
    pub(super) fn select_workspace_for(
        &mut self,
        session_id: Uuid,
        workspace: SessionWorkspace,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if session.has_started() || session.is_busy() {
            return;
        }
        let project_id = session.project_id;
        // Reopening "New worktree" restores the base branch last used for
        // this project; the branch selector is how it changes from there.
        let workspace = match workspace {
            SessionWorkspace::NewWorktree { base_branch: None } => SessionWorkspace::NewWorktree {
                base_branch: self.state.remembered_base_branch(project_id),
            },
            workspace => workspace,
        };
        let abandoned_worktree = match &session.workspace {
            SessionWorkspace::Worktree { path, .. } if workspace != session.workspace => {
                Some(path.clone())
            }
            _ => None,
        };
        let changed = session.workspace != workspace;
        self.state.remember_workspace(project_id, &workspace);
        if changed && let Some(session) = self.state.session_mut(session_id) {
            session.workspace = workspace;
        }
        self.save();
        if changed {
            // A materialized worktree the draft walked away from frees its
            // checkout on disk; Git refuses to remove a dirty worktree.
            if let Some(path) = abandoned_worktree {
                self.remove_draft_worktree(path, cx);
            }
            // An open terminal keeps the old workspace's cwd; respawn it
            // where the draft now points. Only the visible session's
            // surfaces can be open — a Big Picture subject's are not.
            if self.state.selected_session == Some(session_id) {
                self.ensure_right_panel_terminals(cx);
            }
            cx.notify();
        }
    }

    /// Primary modifier + Shift + T: flip the draft between the local checkout
    /// and a new worktree — the same two rows the "Work in" menu offers. The
    /// guards mirror its disabled states; `select_workspace` restores the
    /// remembered base branch.
    pub(super) fn toggle_workspace_action(
        &mut self,
        _: &ToggleWorkspace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.is_some() {
            return;
        }
        let (subject_session_id, subject_project_id) = self.workspace_subject();
        let session = subject_session_id.and_then(|session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
        });
        if session.is_some_and(|session| session.has_started() || session.is_busy()) {
            return;
        }
        let Some(project_id) = session
            .map(|session| session.project_id)
            .or(subject_project_id)
        else {
            return;
        };
        if self
            .state
            .projects
            .iter()
            .any(|project| project.id == project_id && project.is_projectless())
        {
            return;
        }
        let next = if session
            .map(|session| session.workspace.is_local())
            .unwrap_or_else(|| {
                matches!(
                    self.state.workspace_for_new_session(project_id),
                    SessionWorkspace::Local
                )
            }) {
            SessionWorkspace::NewWorktree { base_branch: None }
        } else {
            SessionWorkspace::Local
        };
        let Some(session_id) = self.ensure_workspace_subject_session(cx) else {
            return;
        };
        self.select_workspace_for(session_id, next, cx);
    }

    pub(super) fn remove_session(
        &mut self,
        session_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.response_fork_preparations.contains_key(&session_id) {
            self.show_toast(tr!("session.response_fork_in_progress"));
            cx.notify();
            return;
        }
        let Some(index) = self
            .state
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        else {
            return;
        };
        let project_id = self.state.sessions[index].project_id;
        let composer_draft_key =
            crate::persistence::ComposerDraftKey::for_session(&self.state.sessions[index]);
        let projectless = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .is_some_and(Project::is_projectless);
        // Temporary projects share the projectless lifecycle: the catalog
        // entry exists for its tasks and dies with the last one.
        let temporary = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .is_some_and(|project| project.temporary);
        // Checkpoint refs live in the repository's shared namespace, so
        // delete them from the project checkout — the task's worktree may
        // already be gone, and a missing cwd would silently leave the
        // refs behind.
        let project_path = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
            .or_else(|| {
                self.state.sessions[index]
                    .workspace
                    .path()
                    .map(std::path::Path::to_path_buf)
            });
        // A draft's eagerly created worktree dies with it. Once a session has
        // started the worktree may hold the agent's work and stays on disk.
        let draft_worktree = match &self.state.sessions[index].workspace {
            SessionWorkspace::Worktree { path, .. }
                if !self.state.sessions[index].has_started() =>
            {
                Some(path.clone())
            }
            _ => None,
        };
        let was_selected = self.state.selected_session == Some(session_id);
        self.submission_preparations.remove(&session_id);
        self.goal_runtime_starts.remove(&session_id);
        self.pending_goal_operations.remove(&session_id);
        self.goal_observed_at.remove(&session_id);
        self.state.unseen_completions.remove(&session_id);
        self.pending_workspace_cleanups.remove(&session_id);
        self.reset_session_runtime(session_id);
        self.background_work.remove(&session_id);
        self.remove_right_panel_session_state(session_id, cx);
        self.remove_composer_draft(composer_draft_key, cx);
        self.state.sessions.remove(index);
        if let Err(error) = self.store.remove_session(session_id) {
            self.show_toast(tr!("errors.save_local_state", error = error));
        }
        if self
            .pending_session_activation
            .is_some_and(|pending| pending.session_id == session_id)
        {
            self.pending_session_activation = None;
        }
        self.session_navigation.remove(session_id);
        self.task_switcher.remove(session_id);
        self.project_switcher.session_removed(session_id);
        self.transcript_scroll_positions.remove(&session_id);
        if self
            .transcript_landing
            .is_some_and(|(landing_session, _)| landing_session == session_id)
        {
            self.transcript_landing = None;
        }
        let project_still_used = self
            .state
            .sessions
            .iter()
            .any(|session| session.project_id == project_id);
        if (projectless || temporary) && !project_still_used {
            self.remove_composer_draft(
                crate::persistence::ComposerDraftKey::NewSession(project_id),
                cx,
            );
            self.state
                .projects
                .retain(|project| project.id != project_id);
            if self.state.selected_project == Some(project_id) {
                self.state.selected_project = None;
            }
        }
        if let Some(project_path) = project_path
            && let Some(workspace) = self.workspace_client_for_path(&project_path)
        {
            cx.background_executor()
                .spawn(async move {
                    let _ = workspace.request(waku_client::WorkspaceOperation::DeleteSessionRefs {
                        cwd: project_path,
                        session_id,
                    });
                })
                .detach();
        }
        if let Some(path) = draft_worktree {
            self.remove_draft_worktree(path, cx);
        }
        self.invalidate_checkpoint_refs();

        if was_selected {
            self.select_session_fallback(project_id, projectless || temporary, window, cx);
        } else {
            self.save();
            cx.notify();
        }

        // Only now is the row gone, so the sweep can see which blobs are
        // genuinely unreferenced. It reads the database and walks the blob
        // directory, so it stays off the UI thread.
        let sweep = self.store.blob_sweep();
        cx.background_executor()
            .spawn(async move { sweep() })
            .detach();
    }

    /// Moves selection after the viewed task departs: the topmost unread
    /// session like GoToNextUnreadCompletion, then the top of the idle
    /// rotation, then the project's New task composer when nothing navigable
    /// remains.
    fn select_session_fallback(
        &mut self,
        project_id: Uuid,
        projectless: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.selected_session = None;
        self.settings_page = None;
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let pending = self
            .pending_session_activation
            .map(|pending| pending.session_id);
        if let Some(session_id) = next_unread_completion(
            &self.state.sessions,
            &self.state.unseen_completions,
            &rows,
            self.state.selected_session,
            pending,
        )
        .or_else(|| next_idle_session(&self.state.sessions, &rows, None, pending))
        {
            self.request_session_activation(session_id, SessionActivationTransition::Visit, cx);
        } else {
            self.compose_new_task(project_id, projectless, window, cx);
        }
    }

    /// Opens the project's New task composer — the drained-queue landing for
    /// departures with nowhere left to navigate.
    fn compose_new_task(
        &mut self,
        project_id: Uuid,
        projectless: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if projectless {
            self.create_projectless_session(cx);
        } else {
            self.create_session_for(project_id, self.state.last_provider, cx);
        }
        let focus_handle = self.composer_focus(cx);
        window.focus(&focus_handle, cx);
    }

    /// Hides a task from the sidebar and search without deleting it.
    ///
    /// A task with a turn still running gets a confirmation first — archiving
    /// stops the turn — and so does a worktree still holding uncommitted or
    /// unpushed work — inspected on the background executor — so the user
    /// sees what the archive snapshot is about to carry. A settled local
    /// checkout skips it: its git state is the user's own and archive
    /// leaves it untouched. Inspection
    /// failures archive anyway: the snapshot ref keeps the state regardless.
    ///
    /// An active turn is stopped first — a hidden session must not keep
    /// working. A worktree-based task is then snapshotted into its archive
    /// ref and its worktree removed, so archived chats stop costing a full
    /// checkout of disk; a projectless task's workspace zips into
    /// `~/.goddard/archives` and removes the directory the same way. The
    /// daemon purges archives once they outlive the retention window.
    /// Terminals that ran inside the directory are closed once the removal
    /// lands.
    /// `landing_row` is the sidebar position the session's row occupied, so a
    /// [`ArchiveNavigation::NextSession`] landing can hand selection to the
    /// neighbor that slid into its slot. `None` when the row is not on screen.
    pub(super) fn archive_session(
        &mut self,
        session_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hold_sidebar_peek();
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .filter(|session| session.has_started() && session.archived_at.is_none())
        else {
            return;
        };
        let landing_row = sidebar::sidebar_session_row_index(
            &self.sidebar_rows_cached(Local::now().date_naive()),
            session_id,
        );
        let busy = session.is_busy();
        // Only a worktree gets a preview: archiving snapshots its checkout
        // into the archive ref and removes the directory. A local checkout
        // is the user's own git state — archive never touches it, so dirty
        // files and unpushed commits are nothing to warn about. An active
        // turn is still worth confirming: archiving stops it.
        let Some(workspace) = (match &session.workspace {
            SessionWorkspace::Worktree { path, .. } => Some(path.clone()),
            _ => None,
        }) else {
            if busy && self.archive_dialog.is_none() {
                let focus = self.open_archive_dialog(
                    session_id,
                    crate::git_commit::ArchivePreview::default(),
                    true,
                    landing_row,
                    cx,
                );
                // Like the other deferred surfaces, focus lands two frames
                // after the modal joins the dispatch tree.
                window.on_next_frame(move |window, _| {
                    window.on_next_frame(move |window, cx| window.focus(&focus, cx));
                });
            } else if !busy {
                self.finish_archive_session(session_id, landing_row, window, cx);
            }
            return;
        };
        if self.archive_dialog.is_some() || !self.archive_preview_pending.insert(session_id) {
            return;
        }
        let Some(workspace_client) = self.workspace_client_for_session(session_id) else {
            self.archive_preview_pending.remove(&session_id);
            self.finish_archive_session(session_id, landing_row, window, cx);
            return;
        };
        let window_handle = window.window_handle();
        cx.spawn(async move |waku, cx| {
            let preview = cx
                .background_executor()
                .spawn(async move {
                    match workspace_client.request(
                        waku_client::WorkspaceOperation::InspectArchivePreview { cwd: workspace },
                    ) {
                        Ok(waku_client::WorkspaceResult::ArchivePreview { preview }) => preview,
                        _ => None,
                    }
                })
                .await;
            let finish = waku
                .update(cx, |waku, cx| {
                    waku.archive_preview_pending.remove(&session_id);
                    // The turn may have settled while the preview was being
                    // inspected — warn only about what is still true now.
                    let busy = waku
                        .state
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                        .is_some_and(|session| session.is_busy());
                    let preview = preview.unwrap_or_default();
                    if busy || !preview.files.is_empty() || !preview.unpushed_commits.is_empty() {
                        let focus = waku.open_archive_dialog(
                            session_id,
                            preview,
                            busy,
                            landing_row,
                            cx,
                        );
                        Some(focus)
                    } else {
                        None
                    }
                })
                .unwrap_or(None);
            let _ = window_handle.update(cx, move |_, window, cx| {
                match finish {
                    Some(focus) => {
                        // Like the other deferred surfaces, focus lands two
                        // frames after the modal joins the dispatch tree.
                        window.on_next_frame(move |window, _| {
                            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
                        });
                    }
                    None => {
                        let _ = waku.update(cx, |waku, cx| {
                            waku.finish_archive_session(session_id, landing_row, window, cx)
                        });
                    }
                }
            });
        })
        .detach();
    }

    /// Hides the task outright — the point every archive path reaches once
    /// the checkout proved clean or the user confirmed.
    ///
    /// Where selection moves is the `archive_navigation` setting's call;
    /// `landing_row` is the sidebar position the departed row occupied, so a
    /// [`ArchiveNavigation::NextSession`] landing can pick the neighbor that
    /// slid into its slot.
    pub(super) fn finish_archive_session(
        &mut self,
        session_id: Uuid,
        landing_row: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((project_id, is_busy)) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .filter(|session| session.has_started() && session.archived_at.is_none())
            .map(|session| (session.project_id, session.is_busy()))
        else {
            return;
        };
        if is_busy {
            self.cancel_session_turn(session_id, cx);
        }
        let projectless = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .is_some_and(Project::is_projectless);
        let was_selected = self.state.selected_session == Some(session_id);
        if self
            .pending_session_activation
            .is_some_and(|pending| pending.session_id == session_id)
        {
            self.pending_session_activation = None;
        }
        self.session_navigation.remove(session_id);
        self.task_switcher.remove(session_id);
        self.project_switcher.session_removed(session_id);
        self.transcript_scroll_positions.remove(&session_id);
        if self
            .transcript_landing
            .is_some_and(|(landing_session, _)| landing_session == session_id)
        {
            self.transcript_landing = None;
        }
        let now = unix_time();
        if let Some(session) = self.state.session_mut(session_id) {
            session.archived_at = Some(now);
            // Archiving is a mutation: bumping `updated_at` keeps merge
            // precedence honest so a stale client save cannot resurrect or
            // clobber the flag.
            session.updated_at = now;
        }
        self.queue_archived_workspace_cleanup(session_id, cx);
        if was_selected {
            self.state.selected_session = None;
            self.settings_page = None;
            match self.state.archive_navigation {
                ArchiveNavigation::NextSession => {
                    // The row that followed the departed one now sits at its
                    // index; without a position the scan enters at the top.
                    let next = self.next_sidebar_session_from_row(landing_row.unwrap_or(0));
                    if let Some(next_id) = next {
                        self.request_session_activation(
                            next_id,
                            SessionActivationTransition::Visit,
                            cx,
                        );
                    } else {
                        self.compose_new_task(project_id, projectless, window, cx);
                    }
                }
                ArchiveNavigation::NewTask => {
                    self.compose_new_task(project_id, projectless, window, cx);
                }
                ArchiveNavigation::NextUnread => {
                    self.select_session_fallback(project_id, projectless, window, cx);
                }
            }
        } else {
            self.save();
            cx.notify();
        }
    }

    /// Returns an archived task to the sidebar and search.
    ///
    /// `announce` raises the "Task unarchived" toast with a "View now" jump;
    /// activation-driven unarchives skip it because the task is already
    /// opening.
    pub(super) fn unarchive_session(
        &mut self,
        session_id: Uuid,
        announce: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if session.archived_at.is_none() {
            return;
        }
        let now = unix_time();
        if let Some(session) = self.state.session_mut(session_id) {
            session.archived_at = None;
            session.updated_at = now;
        }
        // An unarchived session keeps its worktree: a queued cleanup must
        // not fire after the task is back.
        self.pending_workspace_cleanups.remove(&session_id);
        // If cleanup already removed the worktree, bring it back now —
        // waiting for the next prompt's restore leaves terminals and file
        // surfaces pointing at a directory that does not exist. A
        // projectless workspace unzips its archive the same way.
        self.restore_missing_worktree(session_id, cx);
        self.restore_archived_projectless_workspace(session_id, cx);
        self.save();
        if announce {
            self.show_unarchived_toast(session_id);
        }
        cx.notify();
    }

    pub(super) fn archive_session_action(
        &mut self,
        _: &ArchiveSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session_id) = self.composer_session_id() {
            self.archive_session(session_id, window, cx);
        }
    }

    /// Moves a task into or out of the sidebar's top Pinned group.
    ///
    /// Like archiving, the toggle is a mutation: bumping `updated_at` keeps
    /// merge precedence honest so a stale client save cannot resurrect or
    /// clobber the flag.
    pub(super) fn toggle_session_pin(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.hold_sidebar_peek();
        let Some(pinned) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .filter(|session| session.has_started() && session.archived_at.is_none())
            .map(|session| session.pinned_at.is_some())
        else {
            return;
        };
        let now = unix_time();
        if let Some(session) = self.state.session_mut(session_id) {
            session.pinned_at = if pinned { None } else { Some(now) };
            session.updated_at = now;
        }
        self.save();
        cx.notify();
    }

    /// Lift a received-file session out of quarantine: from here the daemon
    /// accepts prompts for it and the agent may touch the transfer's files.
    /// Same mutation-then-save shape as pin/archive.
    pub(super) fn trust_transfer_session(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let now = unix_time();
        if let Some(session) = self.state.session_mut(session_id) {
            if !session.quarantined {
                return;
            }
            session.quarantined = false;
            session.updated_at = now;
        }
        self.save();
        cx.notify();
    }

    pub(super) fn toggle_session_pin_action(
        &mut self,
        _: &ToggleSessionPin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The chord pins whichever surface it plausibly means: the
        // full-width terminal while one owns the main area, the right-panel
        // terminal while it holds focus, the selected task otherwise.
        if self.big_picture.is_open() {
            if let Some(session_id) = self.composer_session_id() {
                self.toggle_session_pin(session_id, cx);
            }
            return;
        }
        if let Some(terminal_id) = self.selected_terminal {
            self.toggle_terminal_pin(terminal_id, cx);
            return;
        }
        let focused_terminal = self
            .active_right_panel_surface()
            .and_then(RightPanelSurface::terminal_id)
            .filter(|terminal_id| {
                self.right_panel_terminals
                    .get(terminal_id)
                    .is_some_and(|terminal| terminal.read(cx).focus_handle(cx).is_focused(window))
            });
        if let Some(terminal_id) = focused_terminal {
            self.toggle_terminal_pin(terminal_id, cx);
            return;
        }
        if let Some(session_id) = self.state.selected_session {
            self.toggle_session_pin(session_id, cx);
        }
    }

    pub(super) fn new_session_action(
        &mut self,
        _: &NewSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // In Big Picture a new task is the untargeted composer: peel an armed
        // card off and drop the caret in the field.
        if self.big_picture.is_open() {
            self.set_big_picture_target(None, cx);
            let focus_handle = self.composer_focus(cx);
            window.focus(&focus_handle, cx);
            cx.notify();
            return;
        }
        self.settings_page = None;
        let current_project = self
            .selected_project()
            .map(|project| (project.id, project.is_projectless()));
        match current_project {
            Some((_, true)) => self.create_projectless_session(cx),
            Some((project_id, false)) => {
                if let Some(session_id) = self
                    .session_navigation
                    .remembered_new_task(&self.state.sessions, project_id)
                {
                    self.select_session(session_id, cx);
                } else {
                    self.create_session_for(project_id, self.state.last_provider, cx);
                }
            }
            None => self.create_projectless_session(cx),
        }
        let focus_handle = self.composer_focus(cx);
        window.focus(&focus_handle, cx);
    }

    /// "New task in same worktree": hand the selected task's materialized
    /// worktree to the project's next draft. The palette only offers this
    /// while the selected task has one; the lookup repeats here because the
    /// clicked item is a snapshot.
    pub(super) fn new_task_in_same_worktree(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        let Some((project_id, workspace)) =
            self.selected_session()
                .and_then(|session| match &session.workspace {
                    workspace @ SessionWorkspace::Worktree { .. } if session.has_started() => {
                        Some((session.project_id, workspace.clone()))
                    }
                    _ => None,
                })
        else {
            return;
        };
        self.bind_new_draft_to_worktree(project_id, workspace, window, cx);
    }

    /// "New task in…": a task draft in the picked directory. An existing
    /// project at that path is reused; anything else becomes a temporary
    /// project — in the catalog while it has tasks, swept once none do.
    pub(super) fn create_task_in_directory(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        let project_id = match self
            .state
            .projects
            .iter()
            .find(|project| project.path == path)
        {
            Some(project) => project.id,
            None => {
                let mut project = Project::from_path(path);
                project.temporary = true;
                project.bookmark = crate::bookmarks::create(&project.path);
                let project_id = project.id;
                self.state.projects.push(project);
                project_id
            }
        };
        self.create_session_for(project_id, self.state.last_provider, cx);
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    /// The project's new-task draft — an unstarted one it already had, or a
    /// fresh session — bound to a materialized worktree, with the composer
    /// focused. The draft owns the checkout from there.
    pub(super) fn bind_new_draft_to_worktree(
        &mut self,
        project_id: Uuid,
        workspace: SessionWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_session_for(project_id, self.state.last_provider, cx);
        if let Some(session_id) = self.state.selected_session
            && let Some(session) = self.state.session_mut(session_id)
            && !session.has_started()
        {
            session.workspace = workspace;
            self.save();
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    pub(super) fn new_project_action(
        &mut self,
        _: &NewProject,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_project(cx);
    }

    pub(super) fn open_settings_action(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = Some(SettingsPage::General);
        self.settings_scroll.set_offset(gpui::Point::default());
        // Sparkle owns this value and its consent prompt can flip it outside
        // the settings UI, so re-mirror it each time settings opens.
        self.automatic_updates_enabled = cx
            .try_global::<crate::updater::UpdaterState>()
            .and_then(|updater| updater.0.as_ref())
            .is_some_and(|updater| updater.automatically_checks_for_updates());
        // Warm the Usage page's transcript scan while the user is still on
        // General, so clicking Usage lands on data instead of a spinner.
        self.ensure_usage_history(false, cx);
        window.focus(&self.settings_focus, cx);
        cx.notify();
    }

    pub(super) fn toggle_sidebar_action(
        &mut self,
        _: &ToggleSidebar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_sidebar_visible(!self.sidebar_visible, cx);
    }

    pub(super) fn toggle_right_panel_action(
        &mut self,
        _: &ToggleRightPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_right_panel_visible(!self.right_panel_visible, cx);
    }

    pub(super) fn toggle_fps_counter_action(
        &mut self,
        _: &ToggleFpsCounter,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fps_counter_visible = !self.fps_counter_visible;
        cx.notify();
    }

    pub(super) fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.sidebar_visible == visible {
            return;
        }
        self.sidebar_visible = visible;
        self.sidebar_slide = self.begin_panel_slide(self.sidebar_rendered_width, cx);
        self.persist_panel_layout();
        cx.notify();
    }

    /// A toggle's slide, starting from the width the panel currently occupies
    /// so an interrupted one reverses from where its edge actually is.
    /// Reduce-motion gets `None`: the panel simply appears at its new width,
    /// and no frames are scheduled for it.
    pub(super) fn begin_panel_slide(&self, from: f32, cx: &App) -> Option<motion::WidthTween> {
        (!cx.reduce_motion()).then(|| motion::WidthTween::new(from))
    }

    pub(super) fn set_right_panel_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if visible {
            self.request_active_terminal_focus();
            // The Git panel shares this slot: opening the right panel
            // dismisses it without touching its width or slide.
            self.close_git_panel_state();
        } else {
            self.right_panel_pending_terminal_focus = None;
        }
        if self.right_panel_visible == visible {
            return;
        }
        self.right_panel_visible = visible;
        self.right_panel_slide = self.begin_panel_slide(self.right_panel_rendered_width, cx);
        if visible {
            self.analytics
                .track(crate::analytics::Event::RightPanelOpened);
        }
        self.persist_panel_layout();
        cx.notify();
    }

    pub(super) fn persist_panel_layout(&mut self) {
        self.state.sidebar_visible = self.sidebar_visible;
        self.state.right_panel_visible = self.right_panel_visible;
        self.state.git_panel_visible = self.git_panel_visible;
        self.state.sidebar_width = self.sidebar_width;
        self.state.right_panel_width = self.right_panel_width;
        self.state.git_panel_top_height = self.git_panel_top_height;
        self.save();
    }

    /// Mirror the live window frame into persisted state; disk waits for the
    /// app-quit save (any other `save` carries the frame along for free).
    /// macOS reports a zoomed window as `Windowed` with screen-filling bounds,
    /// so while maximized (and while fullscreen) the last floating frame is
    /// kept as the restore size — and the display it was captured on — and
    /// only the flag advances.
    pub(super) fn capture_window_state(&mut self, window: &Window, cx: &App) {
        // Bounds also change when the OS relocates the window — a monitor
        // unplugged, a display asleep. Those moves are not the user's; like
        // Zed, only capture while the window is the active one.
        if !window.is_window_active() {
            return;
        }
        let previous = self.state.window_state;
        let display = window.display(cx).and_then(|display| display.uuid().ok());
        self.state.window_state = Some(match window.window_bounds() {
            WindowBounds::Fullscreen(restore) => {
                previous.unwrap_or_else(|| persisted_window_state(restore, false, display))
            }
            WindowBounds::Maximized(restore) => persisted_window_state(restore, true, display),
            WindowBounds::Windowed(bounds) if window.is_maximized() => PersistedWindowState {
                maximized: true,
                ..previous.unwrap_or_else(|| persisted_window_state(bounds, true, display))
            },
            WindowBounds::Windowed(bounds) => persisted_window_state(bounds, false, display),
        });
    }

    /// The width each panel lays its content out at. A panel mid-slide counts
    /// as on screen and keeps its full width here: the slide narrows the
    /// container that clips it, so nothing inside reflows on the way out.
    /// What the panel actually occupies this frame is
    /// [`Waku::sidebar_rendered_width`] / [`Waku::right_panel_rendered_width`].
    pub(super) fn effective_panel_widths(&self, window: &Window) -> (f32, f32) {
        fitted_panel_widths(
            f32::from(window.viewport_size().width),
            self.sidebar_visible || self.sidebar_slide.is_some(),
            self.right_panel_visible || self.git_panel_visible || self.right_panel_slide.is_some(),
            self.sidebar_width,
            self.right_panel_width,
        )
    }

    pub(super) fn begin_panel_resize(
        &mut self,
        target: PanelResizeTarget,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (sidebar_width, right_panel_width) = self.effective_panel_widths(window);
        // A drag tracks the pointer directly; whatever slide was still
        // finishing would fight it for the same edge.
        let start_size = match target {
            PanelResizeTarget::Sidebar => {
                self.sidebar_slide = None;
                self.sidebar_width = sidebar_width;
                crate::platform::set_sidebar_material_width(window, sidebar_width);
                sidebar_width
            }
            PanelResizeTarget::RightPanel => {
                self.right_panel_slide = None;
                self.right_panel_width = right_panel_width;
                right_panel_width
            }
            PanelResizeTarget::FileTree => {
                let width =
                    fitted_file_tree_width(right_panel_width, self.right_panel_file_tree_width);
                self.right_panel_file_tree_width = width;
                width
            }
            PanelResizeTarget::GitPanelTop => {
                let height = fitted_git_panel_top_height(
                    f32::from(window.viewport_size().height),
                    self.git_panel_top_height,
                );
                self.git_panel_top_height = height;
                height
            }
        };
        self.panel_resize_drag = Some(PanelResizeDrag {
            target,
            start_mouse_x: f32::from(event.position.x),
            start_mouse_y: f32::from(event.position.y),
            start_size,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn resize_panel_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.panel_resize_drag else {
            return;
        };
        let viewport_width = f32::from(window.viewport_size().width);
        let (sidebar_width, right_panel_width) = self.effective_panel_widths(window);
        let delta = f32::from(event.position.x) - drag.start_mouse_x;
        match drag.target {
            PanelResizeTarget::Sidebar => {
                let maximum = SIDEBAR_MAX_WIDTH
                    .min(viewport_width - MAIN_PANEL_MIN_WIDTH - right_panel_width)
                    .max(SIDEBAR_MIN_WIDTH);
                let width = (drag.start_size + delta).clamp(SIDEBAR_MIN_WIDTH, maximum);
                if (self.sidebar_width - width).abs() < 0.5 {
                    return;
                }
                self.sidebar_width = width;
                crate::platform::set_sidebar_material_width(window, width);
            }
            PanelResizeTarget::RightPanel => {
                let maximum = RIGHT_PANEL_MAX_WIDTH
                    .min(viewport_width - MAIN_PANEL_MIN_WIDTH - sidebar_width)
                    .max(RIGHT_PANEL_MIN_WIDTH);
                let width = (drag.start_size - delta).clamp(RIGHT_PANEL_MIN_WIDTH, maximum);
                if (self.right_panel_width - width).abs() < 0.5 {
                    return;
                }
                self.right_panel_width = width;
            }
            PanelResizeTarget::FileTree => {
                let maximum = FILE_TREE_MAX_WIDTH
                    .min(right_panel_width - FILE_EDITOR_MIN_WIDTH)
                    .max(FILE_TREE_MIN_WIDTH);
                let width = (drag.start_size - delta).clamp(FILE_TREE_MIN_WIDTH, maximum);
                if (self.right_panel_file_tree_width - width).abs() < 0.5 {
                    return;
                }
                self.right_panel_file_tree_width = width;
            }
            PanelResizeTarget::GitPanelTop => {
                let delta = f32::from(event.position.y) - drag.start_mouse_y;
                let height = fitted_git_panel_top_height(
                    f32::from(window.viewport_size().height),
                    drag.start_size + delta,
                );
                if (self.git_panel_top_height - height).abs() < 0.5 {
                    return;
                }
                self.git_panel_top_height = height;
            }
        }
        cx.notify();
    }

    pub(super) fn finish_panel_resize(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left
            && let Some(drag) = self.panel_resize_drag.take()
        {
            if drag.target != PanelResizeTarget::FileTree {
                self.persist_panel_layout();
            }
            cx.notify();
        }
    }

    /// Where the main column's back/forward history currently sits — the
    /// Projects page while it claims the column, then the selected task's
    /// transcript, then the full-width terminal that parked it.
    pub(super) fn navigation_location(&self) -> Option<NavigationLocation> {
        if let Some(project_id) = self.projects_page {
            Some(NavigationLocation::ProjectsPage(project_id))
        } else if let Some(session_id) = self.state.selected_session {
            Some(NavigationLocation::Task(session_id))
        } else {
            self.selected_terminal.map(NavigationLocation::Terminal)
        }
    }

    /// Drop page targets whose project is gone — or whose experiment is off;
    /// a stale entry would leave a live-looking button that does nothing.
    fn prune_navigation_stack(
        projects: &[Project],
        projects_page_enabled: bool,
        stack: &mut Vec<NavigationLocation>,
    ) {
        while let Some(NavigationLocation::ProjectsPage(project_id)) = stack.last() {
            if projects_page_enabled
                && projects
                    .iter()
                    .any(|project| project.id == *project_id && !project.is_projectless())
            {
                break;
            }
            stack.pop();
        }
    }

    pub(super) fn navigate_back_action(
        &mut self,
        _: &NavigateBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture.is_open() {
            return;
        }
        if self.settings_page.take().is_some() {
            let focus_handle = self.composer_focus(cx);
            window.focus(&focus_handle, cx);
            cx.notify();
            return;
        }

        Self::prune_navigation_stack(
            &self.state.projects,
            self.state.projects_page_enabled,
            &mut self.session_navigation.back,
        );
        let Some(current) = self.navigation_location() else {
            return;
        };
        match self.session_navigation.back_target() {
            Some(NavigationLocation::Task(target)) => {
                self.request_session_activation(
                    target,
                    SessionActivationTransition::Back { from: current },
                    cx,
                );
            }
            Some(NavigationLocation::Terminal(target)) => {
                let _ = self.session_navigation.go_back(current);
                self.activate_terminal(target, false, window, cx);
            }
            Some(NavigationLocation::ProjectsPage(project_id)) => {
                let _ = self.session_navigation.go_back(current);
                self.show_projects_page(project_id, window, cx);
            }
            None => {}
        }
    }

    pub(super) fn navigate_forward_action(
        &mut self,
        _: &NavigateForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture.is_open() {
            return;
        }
        if self.settings_page.is_some() {
            return;
        }

        Self::prune_navigation_stack(
            &self.state.projects,
            self.state.projects_page_enabled,
            &mut self.session_navigation.forward,
        );
        let Some(current) = self.navigation_location() else {
            return;
        };
        match self.session_navigation.forward_target() {
            Some(NavigationLocation::Task(target)) => {
                self.request_session_activation(
                    target,
                    SessionActivationTransition::Forward { from: current },
                    cx,
                );
            }
            Some(NavigationLocation::Terminal(target)) => {
                let _ = self.session_navigation.go_forward(current);
                self.activate_terminal(target, false, window, cx);
            }
            Some(NavigationLocation::ProjectsPage(project_id)) => {
                let _ = self.session_navigation.go_forward(current);
                self.show_projects_page(project_id, window, cx);
            }
            None => {}
        }
    }

    /// ⌘D / ctrl-backtick: the topmost unread completion — sidebar order is
    /// the importance order — then a drained queue cycles into the idle
    /// rotation, and only a list with nothing navigable lands on New task.
    /// Stamps clear on activation, so repeated presses drain top-down.
    pub(super) fn go_to_next_unread_completion_action(
        &mut self,
        _: &GoToNextUnreadCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let selected = self.state.selected_session;
        let pending = self
            .pending_session_activation
            .map(|pending| pending.session_id);
        let target = next_unread_completion(
            &self.state.sessions,
            &self.state.unseen_completions,
            &rows,
            selected,
            pending,
        )
        .or_else(|| next_idle_session(&self.state.sessions, &rows, selected, pending));
        match target {
            Some(target) => self.go_to_unread_target(target, window, cx),
            None => self.new_session_action(&NewSession, window, cx),
        }
    }

    /// Shared landing for the unread-jump actions. With the overlay up the
    /// jump arms the card when it is on the grid and exits to the task when
    /// it is not — either way it never rewrites the selection invisibly
    /// behind the scrim.
    fn go_to_unread_target(&mut self, target: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        if self.big_picture.is_open() {
            if self.big_picture_card_visible(target) {
                self.arm_big_picture_card(target, cx);
            } else {
                self.close_big_picture(window, cx);
                self.select_session(target, cx);
            }
            return;
        }
        self.settings_page = None;
        self.request_session_activation(target, SessionActivationTransition::Visit, cx);
    }

    /// Context-menu "Mark as unread": the task rejoins the unseen-completion
    /// set — sidebar dot, GoToNextUnreadCompletion candidate — on demand,
    /// where `mark_unseen_turn_settled` only stamps off-screen finishes.
    /// Stamping now orders it newest.
    pub(super) fn mark_session_unread(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if !self
            .state
            .sessions
            .iter()
            .any(|session| session.id == session_id && session.has_started())
        {
            return;
        }
        self.state
            .unseen_completions
            .insert(session_id, unix_time());
        self.save();
        cx.notify();
    }

    /// Command palette "Mark all tasks as read": drains the unseen-completion
    /// set in one pass — the same stamps selection clears one row at a time.
    pub(super) fn mark_all_sessions_read(&mut self, cx: &mut Context<Self>) {
        if self.state.unseen_completions.is_empty() {
            return;
        }
        self.state.unseen_completions.clear();
        self.save();
        cx.notify();
    }

    /// ⌘⇧D: mark the viewed task unread — it stays a GoToNextUnreadCompletion
    /// candidate for a later ⌘D — then move to the next non-busy session
    /// below its row, wrapping to the top. The jump is positional rather
    /// than next-unread: the command is a sweep down the sidebar, and a
    /// topmost-unread jump would bounce between the top two stamped tasks on
    /// repeated presses.
    pub(super) fn mark_unread_and_go_to_next_idle_action(
        &mut self,
        _: &MarkUnreadAndGoToNextIdle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session_id) = self.composer_session_id() {
            self.mark_session_unread(session_id, cx);
        }
        let rows = self.sidebar_rows_cached(Local::now().date_naive());
        let selected = self.state.selected_session;
        let pending = self
            .pending_session_activation
            .map(|pending| pending.session_id);
        let start = selected
            .and_then(|session_id| sidebar::sidebar_session_row_index(&rows, session_id))
            .map_or(0, |index| index + 1);
        match next_non_busy_session(&self.state.sessions, &rows, selected, pending, start) {
            Some(target) => self.go_to_unread_target(target, window, cx),
            None => self.new_session_action(&NewSession, window, cx),
        }
    }

    /// ⌘⌥U: the sidebar's "Mark as Unread" on the viewed task — the same
    /// rejoin-the-unseen-set stamp as ⌘⇧D, without leaving the session.
    pub(super) fn mark_session_unread_action(
        &mut self,
        _: &MarkSessionUnread,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session_id) = self.composer_session_id() {
            self.mark_session_unread(session_id, cx);
        }
    }

    pub(super) fn navigation_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.button {
            MouseButton::Navigate(NavigationDirection::Back) => {
                cx.stop_propagation();
                self.navigate_back_action(&NavigateBack, window, cx);
            }
            MouseButton::Navigate(NavigationDirection::Forward) => {
                cx.stop_propagation();
                self.navigate_forward_action(&NavigateForward, window, cx);
            }
            _ => {}
        }
    }

    pub(super) fn focus_composer_action(
        &mut self,
        _: &FocusComposer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        let focus_handle = self.composer_focus(cx);
        window.focus(&focus_handle, cx);
        cx.notify();
    }

    /// Type-to-focus: a printable keystroke no binding or focused element
    /// claimed routes to the composer — typing with a session on screen
    /// means "write a prompt". The character is spliced in manually rather
    /// than left for the platform's `insertText`: the platform input handler
    /// was bound to whatever element held focus when the frame was painted,
    /// so after a mid-dispatch focus move it would land in a stale field (or
    /// nowhere). Stopping propagation keeps that delivery from arriving at
    /// all.
    pub(super) fn type_to_focus_composer(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = type_to_focus_text(&event.keystroke) else {
            return;
        };
        // The composer only exists once a project is on screen; settings
        // replaces the workspace root wholesale, a selected terminal owns the
        // main area's keystrokes, and the Projects page's own filter and
        // composer own theirs.
        if self.selected_project().is_none()
            || self.settings_page.is_some()
            || self.selected_terminal.is_some()
            || self.projects_page.is_some()
        {
            return;
        }
        // An overlay owns the keyboard while it is up — including one whose
        // focus has not landed yet, which the context check cannot see.
        if self.command_palette.is_open()
            || self.task_switcher.is_open()
            || self.project_switcher.is_open()
            || self.commit_dialog.is_some()
            || self.archive_dialog.is_some()
            || self.shortcuts_dialog.is_some()
            || self.goal_dialog.is_some()
            || self.image_preview.is_some()
            || self.menus.borrow().values().any(|menu| menu.is_open())
        {
            return;
        }
        if window.context_stack().iter().any(|context| {
            TYPING_OWNED_CONTEXTS
                .iter()
                .any(|owned| context.contains(owned))
        }) {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        let text = text.to_owned();
        self.composer
            .update(cx, |composer, cx| composer.insert_text(&text, cx));
        cx.stop_propagation();
    }

    /// Enter outside the composer. The field's own binding claims it while
    /// the composer is focused, and a focused control that activates on Enter
    /// — a transcript button, a rail item — stops it earlier in the bubble.
    /// What arrives here is a keystroke nobody wanted, so when the submit
    /// affordance is the stopped-turn Continue, Enter fires it exactly like
    /// the play button. A draft keeps Enter dead: the affordance would be
    /// Send, and submitting a draft the user may not be looking at is the
    /// one thing this keystroke must not do.
    pub(super) fn enter_to_continue(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "enter" || event.keystroke.modifiers != Modifiers::none() {
            return;
        }
        // The same reach as type-to-focus: a surface that owns its keys — or
        // an overlay whose focus has not landed yet — is not the composer's
        // to take over. Big Picture and an open message edit own Enter for
        // their own layers too.
        if self.selected_project().is_none()
            || self.settings_page.is_some()
            || self.selected_terminal.is_some()
            || self.projects_page.is_some()
            || self.big_picture.is_open()
            || self.message_edit.is_some()
        {
            return;
        }
        if self.command_palette.is_open()
            || self.task_switcher.is_open()
            || self.project_switcher.is_open()
            || self.commit_dialog.is_some()
            || self.archive_dialog.is_some()
            || self.shortcuts_dialog.is_some()
            || self.goal_dialog.is_some()
            || self.image_preview.is_some()
            || self.menus.borrow().values().any(|menu| menu.is_open())
        {
            return;
        }
        if window.context_stack().iter().any(|context| {
            TYPING_OWNED_CONTEXTS
                .iter()
                .any(|owned| context.contains(owned))
        }) {
            return;
        }
        let session = self.composer_session();
        let preparing = session.is_some_and(|session| {
            self.submission_preparations.contains(&session.id)
                || self.response_fork_preparations.contains_key(&session.id)
        });
        let has_draft = !self.composer.read(cx).content(cx).trim().is_empty()
            || !self.composer_attachments.is_empty()
            || !self.composer_pasted_blocks.is_empty()
            || !self
                .transcript_selection
                .annotations
                .borrow()
                .items
                .is_empty();
        if composer::composer_submit_action(session, preparing, has_draft)
            != composer::ComposerSubmitAction::Continue
        {
            return;
        }
        self.continue_interrupted_session(cx);
        cx.stop_propagation();
    }

    /// The directory the session's agent runs in — its worktree once one is
    /// materialized, the project checkout until then.
    pub(super) fn copy_session_working_directory(&self, session_id: Uuid, cx: &mut App) {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        let Some(path) = self.workspace_path_for_session(session) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(
            path.to_string_lossy().into_owned(),
        ));
    }

    pub(super) fn copy_working_directory_action(
        &mut self,
        _: &CopyWorkingDirectory,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session_id) = self.composer_session_id() {
            self.copy_session_working_directory(session_id, cx);
        }
    }

    pub(super) fn cancel_turn_action(
        &mut self,
        action: &CancelTurn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Bare Escape never reaches here — the overlay's Dismiss binding is
        // deeper in the context stack — but ⌥Escape does, and it must stop
        // the armed card's turn, never the session idling underneath.
        if self.big_picture.is_open() {
            self.cancel_turn(cx);
            return;
        }
        // The switcher focus lands after its deferred overlay is painted.
        // Route the root Escape action here too so an immediate press always
        // cancels the provisional selection instead of reaching the session.
        if self.task_switcher.is_open() {
            self.cancel_task_switcher(window, cx);
            return;
        }
        if self.project_switcher.is_open() {
            self.cancel_project_switcher(window, cx);
            return;
        }
        if self.settings_page.take().is_some() {
            let focus_handle = self.composer_focus(cx);
            window.focus(&focus_handle, cx);
            cx.notify();
            return;
        }
        if self.message_edit.is_some() {
            self.cancel_message_edit(window, cx);
            return;
        }
        // ⌥Escape is a deliberate chord, so it stops on a single press
        // instead of arming the second-press confirmation bare Escape needs.
        if action.immediate {
            self.cancel_turn(cx);
            return;
        }
        let Some(target) = self.selected_escape_stop_target() else {
            self.cancel_turn(cx);
            return;
        };
        match self.escape_stop_confirmation.press(target, Instant::now()) {
            EscapeStopPress::Stop => self.cancel_turn(cx),
            EscapeStopPress::Arm(arm) => {
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(ESCAPE_STOP_CONFIRMATION_TIMEOUT)
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this.escape_stop_confirmation.expire(arm) {
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
    }

    fn selected_escape_stop_target(&self) -> Option<EscapeStopTarget> {
        let session = self.selected_session()?;
        (!self.submission_preparations.contains(&session.id) && session.status.is_busy())
            .then(|| EscapeStopTarget::for_session(session))
    }

    pub(super) fn reset_visible_state(&mut self) {
        self.activities_expanded.clear();
        self.expanded_activity_items.clear();
        self.expanded_turns.clear();
        self.expanded_changed_files.clear();
        self.changed_files_diff_hover = None;
        self.changed_files_diffs.clear();
        self.changed_files_diff_generation = self.changed_files_diff_generation.wrapping_add(1);
        self.transcript_control_focuses.borrow_mut().clear();
        self.user_message_viewports.borrow_mut().clear();
        self.expanded_user_messages.clear();
        self.user_message_expand_focuses.borrow_mut().clear();
        self.hovered_response_row = None;
        // Selection belongs to the session being left.
        self.transcript_selection.selection.borrow_mut().clear();
        self.transcript_selection.registry.borrow_mut().clear();
        *self.transcript_selection.hovered_commit.borrow_mut() = None;
        self.transcript_commit_hover = None;
        self.transcript_commit_details.clear();
        self.transcript_commit_press = None;
        // Annotations are session-scoped too, but survive a round trip: park
        // the departing set under its session id, then load the arriving
        // session's parked set (usually none). `annotation_session` tracks who
        // owns the live set because `selected_session` already points at the
        // new session by the time this runs.
        {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            if let Some(owner) = self.annotation_session.take() {
                self.transcript_annotations
                    .insert(owner, std::mem::take(&mut annotations.items));
            }
            annotations.hovered = None;
            annotations.hovered_ref = None;
            annotations.editing = None;
            self.annotation_session = self.state.selected_session;
            if let Some(id) = self.state.selected_session {
                annotations.items = self.transcript_annotations.remove(&id).unwrap_or_default();
            }
        }
        self.annotation_editor = None;
        self.annotation_hover = None;
        self.annotation_ref_hover = None;
        self.annotation_press = None;
        self.transcript_annotations
            .retain(|id, _| self.state.sessions.iter().any(|session| session.id == *id));
        self.sent_annotations
            .retain(|id, _| self.state.sessions.iter().any(|session| session.id == *id));
        self.queued_annotations.retain(|id, _| {
            self.state.sessions.iter().any(|session| {
                session
                    .queued_messages
                    .iter()
                    .any(|message| message.id == *id)
            })
        });
        self.reset_transcript_search_for_session();
        let (streaming_messages, live_reasoning) = self.selected_session().map_or_else(
            || (Vec::new(), Vec::new()),
            |session| {
                let messages = session
                    .messages
                    .iter()
                    .filter(|message| message.role == MessageRole::Assistant && message.streaming)
                    .map(|message| message.id)
                    .collect();
                let reasoning = session
                    .transcript_blocks
                    .iter()
                    .flat_map(|block| &block.activities)
                    .filter(|activity| activity.reasoning.is_some() && !activity.complete)
                    .map(|activity| activity.id)
                    .collect();
                (messages, reasoning)
            },
        );
        // Parsed messages are keyed by message id, which is unique across
        // sessions, so they stay cached — switching back to a recent session
        // then costs no re-parse. Bounded so a long-running window cannot grow
        // without limit.
        let mut message_markdown = self.message_markdown.borrow_mut();
        let cached_bytes: usize = message_markdown
            .values()
            .map(md::render::MarkdownView::source_len)
            .sum();
        if cached_bytes > MAX_CACHED_MESSAGE_SOURCE_BYTES {
            message_markdown.clear();
        }
        for id in streaming_messages {
            message_markdown
                .entry(id)
                .or_insert_with(MarkdownView::new)
                .seed_streaming_baseline();
        }
        drop(message_markdown);
        // Block parses are keyed by position within the session, so they would
        // be read as another session's blocks.
        let mut activity_markdown = self.activity_markdown.borrow_mut();
        activity_markdown.clear();
        for id in live_reasoning {
            activity_markdown.insert(id, MarkdownView::seeded());
        }
        drop(activity_markdown);
        self.reasoning_window_starts.borrow_mut().clear();
        self.activity_scroll_viewports.borrow_mut().clear();
        self.menus.borrow_mut().clear();
        self.message_edit = None;
        self.hide_toast();
        self.navigation_rail_reset_generation
            .set(self.navigation_rail_reset_generation.get().wrapping_add(1));
        self.transcript_anchor.set(None);
        self.transcript_anchor_end_space.set(Pixels::ZERO);
        self.transcript_anchor_following.set(false);
    }

    pub(super) fn reset_session_runtime(&mut self, session_id: Uuid) {
        if let Some(runtime) = self.runtimes.remove(&session_id) {
            runtime.driver.cancel();
            runtime.driver.close();
            self.mark_background_work_lost(session_id);
        }
    }

    fn remember_selected_model_traits(&mut self) {
        let Some((provider, model, reasoning_effort, service_tier, context_window)) =
            self.composer_session().and_then(|session| {
                Some((
                    session.provider,
                    self.model_for_session(session)?.to_owned(),
                    session.reasoning_effort.clone(),
                    session.service_tier.clone(),
                    session.context_window.clone(),
                ))
            })
        else {
            return;
        };
        self.state.remember_model_traits(
            provider,
            &model,
            reasoning_effort,
            service_tier,
            context_window,
        );
    }

    /// The session's effective model+effort+fast selection, in picker-row
    /// terms: the catalog model id (a suffix-encoded Cursor id resolves to
    /// its base), the effort a turn would run at, and whether the fast tier
    /// is on. `None` when no model can be named at all.
    pub(super) fn session_model_combo(
        &self,
        session: &AgentSession,
    ) -> Option<(String, Option<String>, bool)> {
        let model_id = self.catalog_model_id_for_session(session)?.to_owned();
        let metadata = self.model_metadata_for_session(session);
        // Mirror the traits chip's suffix decode: Cursor packs the choice
        // into the model id, so the suffix fills what the session fields
        // leave unset.
        let (suffix_effort, suffix_tier) = (session.provider == ProviderKind::Cursor)
            .then(|| self.model_for_session(session))
            .flatten()
            .and_then(|requested| {
                self.provider_probe(session.provider).and_then(|probe| {
                    crate::model_catalog::cursor_catalog_model(&probe.models, requested)
                })
            })
            .map(|matched| {
                (
                    crate::model_catalog::cursor_suffix_reasoning_effort(
                        &matched.suffix,
                        &matched.model.reasoning_efforts,
                    ),
                    crate::model_catalog::cursor_suffix_service_tier(
                        &matched.suffix,
                        &matched.model.service_tiers,
                    ),
                )
            })
            .unwrap_or_default();
        let effort = session
            .reasoning_effort
            .as_deref()
            .filter(|selected| {
                metadata.is_some_and(|model| {
                    model
                        .reasoning_efforts
                        .iter()
                        .any(|option| option.id == *selected)
                })
            })
            .or(suffix_effort.as_deref())
            .map(str::to_owned)
            .or_else(|| {
                if super::composer::supports_reasoning_default_reset(session.provider) {
                    None
                } else {
                    metadata.and_then(|model| {
                        model.default_reasoning_effort.clone().or_else(|| {
                            model
                                .reasoning_efforts
                                .first()
                                .map(|option| option.id.clone())
                        })
                    })
                }
            });
        let tier = session
            .service_tier
            .as_deref()
            .filter(|selected| {
                *selected == "default"
                    || metadata.is_some_and(|model| {
                        model
                            .service_tiers
                            .iter()
                            .any(|option| option.id == *selected)
                    })
            })
            .or(suffix_tier.as_deref())
            .or_else(|| metadata.and_then(|model| model.default_service_tier.as_deref()))
            .unwrap_or("default");
        Some((model_id, effort, tier == "fast"))
    }

    /// Applies a picker row: the model plus the exact effort and fast-tier
    /// choice the row names, rather than the model's remembered traits.
    pub(super) fn choose_model(
        &mut self,
        provider: ProviderKind,
        model: String,
        effort: Option<String>,
        fast: bool,
        cx: &mut Context<Self>,
    ) {
        let service_tier = fast.then(|| "fast".to_owned());
        // Picking a concrete model exits an Auto draft even when provider and
        // model happen to match the draft's last-used carryover.
        let Some((session_id, provider_changed, was_routed)) = self
            .composer_session()
            .filter(|session| {
                session.can_choose_model(provider)
                    && (session.auto_route
                        || session.provider != provider
                        || session.model.as_deref() != Some(model.as_str())
                        || session.reasoning_effort != effort
                        || session.service_tier != service_tier)
            })
            .map(|session| {
                (
                    session.id,
                    session.provider != provider,
                    session.route_decision.is_some(),
                )
            })
        else {
            return;
        };

        self.remember_selected_model_traits();
        // Effort and tier come from the row; the context window stays a
        // per-model memory like before.
        let (_, _, context_window) = self.state.model_traits_for(provider, &model);
        if let Some(session) = self.composer_session_mut() {
            session.provider = provider;
            session.model = Some(model.clone());
            session.auto_route = false;
            session.route_decision = None;
            if provider_changed {
                session.agent_preset = None;
            }
            session.reasoning_effort.clone_from(&effort);
            session.service_tier.clone_from(&service_tier);
            session.context_window.clone_from(&context_window);
            self.state.last_provider = provider;
            self.state.last_model = Some(model.clone());
            self.state.last_reasoning_effort.clone_from(&effort);
            self.state.last_service_tier = service_tier;
            self.state.last_context_window = context_window;
            // A different provider is a different binary and protocol; only a
            // model change within one provider can be applied in session.
            if provider_changed {
                self.reset_session_runtime(session_id);
                // A different provider is also a different command registry.
                self.refresh_composer_sources(cx);
            } else {
                self.apply_session_options(session_id, cx);
            }
            // The picked combo becomes the model's remembered traits, so a
            // later plain pick of the same model lands back on it.
            self.remember_selected_model_traits();
            if was_routed {
                self.record_route_override(session_id, provider, Some(model), cx);
            }
            self.save();
            cx.notify();
        }
    }

    /// Pick the Auto row: the draft's provider/model stay as the last-used
    /// hint, and the first submission's route call resolves what actually
    /// runs. Only drafts reach here — a started session's picker does not
    /// offer the row.
    pub(super) fn choose_auto_route(&mut self, cx: &mut Context<Self>) {
        if !self.auto_route_available() {
            return;
        }
        let Some(session) = self.composer_session_mut() else {
            return;
        };
        if session.auto_route {
            return;
        }
        session.auto_route = true;
        session.updated_at = unix_time();
        self.save();
        cx.notify();
    }

    /// Primary modifier + /: toggle the composer's model picker as if its chip were clicked.
    pub(super) fn toggle_model_picker_action(
        &mut self,
        _: &ToggleModelPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.is_some() {
            return;
        }
        if !self
            .composer_session()
            .is_some_and(|session| session.can_choose_model(session.provider))
        {
            return;
        }
        self.defer_menu_toggle(
            MODEL_PICKER_MENU_ID,
            crate::ui::menu::toggle_popover,
            window,
            cx,
        );
    }

    /// Primary modifier + Shift + B: toggle the branch picker as if its chip
    /// were clicked — a worktree draft's base branch, or a checkout's current
    /// branch. The guards mirror the selector's disabled states.
    pub(super) fn toggle_branch_picker_action(
        &mut self,
        _: &ToggleBranchPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.is_some() {
            return;
        }
        let Some(session) = self.composer_session() else {
            return;
        };
        if session.is_busy() || self.branch_operation_pending {
            return;
        }
        if self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id)
            .filter(|project| !project.is_projectless())
            .is_none()
        {
            return;
        }
        let Some(workspace_path) = self.workspace_path_for_session(session) else {
            return;
        };
        let workspace_path = workspace_path.to_path_buf();
        if self
            .branch_snapshot_for_workspace(&workspace_path, cx)
            .is_none()
        {
            return;
        }
        self.defer_menu_toggle(
            BRANCH_PICKER_MENU_ID,
            crate::ui::menu::toggle_popover,
            window,
            cx,
        );
    }

    /// Primary modifier + .: toggle the composer's runtime-mode menu as if
    /// its chip were clicked.
    pub(super) fn toggle_runtime_mode_picker_action(
        &mut self,
        _: &ToggleRuntimeModePicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.is_some() {
            return;
        }
        self.defer_menu_toggle(
            RUNTIME_MODE_MENU_ID,
            crate::ui::menu::toggle_dropdown,
            window,
            cx,
        );
    }

    /// A keyboard toggle produces no mouse-down for another open menu's
    /// dismiss-on-down-out to see, so close the rest here. The pickers' toggle
    /// observers update this entity, so the toggle itself has to run after
    /// this listener releases it.
    fn defer_menu_toggle(
        &self,
        menu_id: &'static str,
        toggle: fn(&ContextMenuHandle, MenuAlign, &mut Window, &mut App),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menus = self.menus.borrow();
        let Some(handle) = menus.get(menu_id).cloned() else {
            return;
        };
        let other_open: Vec<_> = menus
            .iter()
            .filter(|(id, other)| id.as_ref() != menu_id && other.is_open())
            .map(|(_, other)| other.clone())
            .collect();
        drop(menus);
        window.defer(cx, move |window, cx| {
            for menu in other_open {
                menu.close(window, cx);
            }
            toggle(&handle, MenuAlign::AboveLeft, window, cx);
        });
    }

    /// ⌘⌥1–⌘⌥9 applies the nth starred selection to the composer session —
    /// a draft or an idle session, and only while its provider is one the
    /// session may still run.
    pub(super) fn select_favorite_model_action(
        &mut self,
        action: &SelectFavoriteModel,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.is_some() {
            return;
        }
        let Some(favorite) = self.state.favorite_models.get(action.index).cloned() else {
            return;
        };
        let Some(session) = self.composer_session() else {
            return;
        };
        if !session.can_choose_model(favorite.provider) {
            return;
        }
        // A switched-off provider's favorite stays reachable only for the
        // session already locked to it — same rule the picker's rows follow.
        let locked = !session.messages.is_empty() && session.provider == favorite.provider;
        if !locked && !self.provider_enabled(favorite.provider) {
            return;
        }
        // A favorite stored before rows were combos carries no effort; it
        // claims the model's default-effort row in the list, so the chord
        // applies that same effort rather than leaving the field unset.
        let effort = favorite.effort.clone().or_else(|| {
            self.provider_probe(favorite.provider)
                .and_then(|probe| probe.model(&favorite.model))
                .and_then(|model| {
                    model.default_reasoning_effort.clone().or_else(|| {
                        model
                            .reasoning_efforts
                            .first()
                            .map(|option| option.id.clone())
                    })
                })
        });
        self.choose_model(favorite.provider, favorite.model, effort, favorite.fast, cx);
    }

    /// ⌘E steps the composer session's reasoning effort through the current
    /// model's ladder, wrapping at the top. Providers that can return to a
    /// base `default` variant include the unset step in the cycle.
    pub(super) fn cycle_reasoning_effort_action(
        &mut self,
        _: &CycleReasoningEffort,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_page.is_some() {
            return;
        }
        let Some((steps, current)) = self.composer_session().and_then(|session| {
            let model = self.model_metadata_for_session(session)?;
            if model.reasoning_efforts.is_empty() {
                return None;
            }
            let current = self
                .session_model_combo(session)
                .and_then(|(_, effort, _)| effort);
            let mut steps: Vec<Option<String>> = Vec::new();
            if super::composer::supports_reasoning_default_reset(session.provider) {
                steps.push(None);
            }
            steps.extend(
                model
                    .reasoning_efforts
                    .iter()
                    .map(|option| Some(option.id.clone())),
            );
            Some((steps, current))
        }) else {
            return;
        };
        let position = steps
            .iter()
            .position(|step| *step == current)
            // An unset or unlisted effort takes the ladder's first step.
            .unwrap_or_else(|| steps.len().saturating_sub(1));
        match steps[(position + 1) % steps.len()].clone() {
            Some(effort) => self.set_reasoning_effort(effort, cx),
            None => self.clear_reasoning_effort(cx),
        }
    }

    pub(super) fn toggle_favorite_model(
        &mut self,
        provider: ProviderKind,
        model: String,
        effort: Option<String>,
        fast: bool,
        cx: &mut Context<Self>,
    ) {
        let default_effort = self
            .provider_probe(provider)
            .and_then(|probe| probe.model(&model))
            .and_then(|model| {
                model.default_reasoning_effort.clone().or_else(|| {
                    model
                        .reasoning_efforts
                        .first()
                        .map(|option| option.id.clone())
                })
            });
        if let Some(index) = self.state.favorite_models.iter().position(|favorite| {
            super::composer::favorite_matches_row(
                favorite,
                provider,
                &model,
                effort.as_deref(),
                fast,
                default_effort.as_deref(),
            )
        }) {
            self.state.favorite_models.remove(index);
        } else {
            self.state.favorite_models.push(FavoriteModel {
                provider,
                model,
                effort,
                fast,
            });
        }
        self.save();
        cx.notify();
    }

    /// Drag-reorder inside the picker's favorites section: the dropped entry
    /// takes the target row's slot.
    pub(super) fn move_favorite_model(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.state.favorite_models.len() {
            return;
        }
        let favorite = self.state.favorite_models.remove(from);
        self.state
            .favorite_models
            .insert(to.min(self.state.favorite_models.len()), favorite);
        self.save();
        cx.notify();
    }

    pub(super) fn set_runtime_mode(&mut self, mode: RuntimeMode, cx: &mut Context<Self>) {
        let Some((session_id, session_changed)) = self
            .composer_session()
            .map(|session| (session.id, session.runtime_mode != mode))
        else {
            return;
        };
        let remembered_changed = self.state.last_runtime_mode != mode;
        if session_changed {
            self.composer_session_mut()
                .expect("composer session still exists")
                .runtime_mode = mode;
            self.apply_session_options(session_id, cx);
        }
        if session_changed || remembered_changed {
            self.state.last_runtime_mode = mode;
            self.save();
            cx.notify();
        }
    }

    /// The environment is fixed when the session boots; once a task has
    /// started it can only report where it runs, not move.
    pub(super) fn set_sandboxed(&mut self, sandboxed: bool, cx: &mut Context<Self>) {
        let Some(session_changed) = self
            .composer_session()
            .filter(|session| !session.has_started())
            .map(|session| session.sandboxed != sandboxed)
        else {
            return;
        };
        let remembered_changed = self.state.last_sandboxed != sandboxed;
        if session_changed {
            self.composer_session_mut()
                .expect("composer session still exists")
                .sandboxed = sandboxed;
        }
        if session_changed || remembered_changed {
            self.state.last_sandboxed = sandboxed;
            self.save();
            cx.notify();
        }
    }

    pub(super) fn set_reasoning_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        if let Some(session) = self.composer_session_mut()
            && session.reasoning_effort.as_deref() != Some(effort.as_str())
        {
            let session_id = session.id;
            session.reasoning_effort = Some(effort.clone());
            self.state.last_reasoning_effort = Some(effort);
            self.remember_selected_model_traits();
            self.apply_session_options(session_id, cx);
            self.save();
            cx.notify();
        }
    }

    /// Clears an explicitly chosen OpenCode variant back to the base model's
    /// `default` selection.
    pub(super) fn clear_reasoning_effort(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.composer_session_mut()
            && super::composer::supports_reasoning_default_reset(session.provider)
            && session.reasoning_effort.is_some()
        {
            let session_id = session.id;
            session.reasoning_effort = None;
            self.state.last_reasoning_effort = None;
            self.remember_selected_model_traits();
            self.apply_session_options(session_id, cx);
            self.save();
            cx.notify();
        }
    }

    pub(super) fn set_service_tier(&mut self, tier: String, cx: &mut Context<Self>) {
        if let Some(session) = self.composer_session_mut()
            && session.service_tier.as_deref() != Some(tier.as_str())
        {
            let session_id = session.id;
            session.service_tier = Some(tier.clone());
            self.state.last_service_tier = Some(tier);
            self.remember_selected_model_traits();
            self.apply_session_options(session_id, cx);
            self.save();
            cx.notify();
        }
    }

    pub(super) fn set_context_window(&mut self, window: String, cx: &mut Context<Self>) {
        if let Some(session) = self.composer_session_mut()
            && session.context_window.as_deref() != Some(window.as_str())
        {
            let session_id = session.id;
            session.context_window = Some(window.clone());
            self.state.last_context_window = Some(window);
            self.remember_selected_model_traits();
            self.apply_session_options(session_id, cx);
            self.save();
            cx.notify();
        }
    }

    pub(super) fn set_agent_preset(&mut self, agent_preset: String, cx: &mut Context<Self>) {
        let selectable = self
            .provider_probe(ProviderKind::DeepSeek)
            .is_some_and(|probe| {
                probe
                    .agent_presets
                    .iter()
                    .any(|preset| preset.id == agent_preset)
            });
        if !selectable {
            return;
        }
        if let Some(session) = self.composer_session_mut()
            && session.provider == ProviderKind::DeepSeek
            && !session.has_started()
            && !session.is_busy()
            && session.agent_preset.as_deref() != Some(agent_preset.as_str())
        {
            let session_id = session.id;
            session.agent_preset = Some(agent_preset);
            // A provider cursor makes a session started, so this is normally a
            // no-op. It also closes the narrow race where a blank runtime was
            // prepared but had not reported its native session yet.
            self.reset_session_runtime(session_id);
            self.save();
            cx.notify();
        }
    }

    pub(super) fn cancel_turn(&mut self, cx: &mut Context<Self>) {
        self.escape_stop_confirmation.clear();
        let Some(session_id) = self.composer_session_id() else {
            return;
        };
        self.cancel_session_turn(session_id, cx);
    }

    pub(super) fn cancel_session_turn(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.escape_stop_confirmation.clear();
        // Worktree/checkpoint preparation has no safe interrupt contract. The
        // composer deliberately shows a spinner rather than Stop until the
        // provider runtime exists, and the keyboard action follows the same
        // boundary.
        if self.submission_preparations.contains(&session_id) {
            return;
        }
        let retain_runtime = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .is_some_and(|session| retain_runtime_after_cancel(session.provider))
            || self.session_has_live_detached_work(session_id);
        // Goal operations queued behind a starting runtime would set the
        // objective after this stop and begin pursuing it; the user asked to
        // stop, so they leave with the turn.
        self.pending_goal_operations.remove(&session_id);
        let mut runtime = self.runtimes.remove(&session_id);
        if let Some(runtime) = runtime.as_ref() {
            runtime.driver.cancel();
            if retain_runtime {
                // A detached process keeps Codex's app-server resident, but
                // Computer Use descendants still belong to the cancelled turn.
                runtime.driver.cancel_computer_use();
            }
        }
        // Do not leave already-received text in the smoothing queue: once the
        // message is marked complete, a later delta would otherwise create a
        // second assistant bubble. Show the received portion immediately.
        // Buffered turn-completion events also must not start queued
        // follow-ups: the user asked to stop, not to continue.
        let mut keep_runtime = true;
        if let Some(runtime) = runtime.as_mut() {
            Self::collect_runtime_events(runtime);
            while let Some(event) = runtime.pending_events.pop_front() {
                keep_runtime &= self.handle_driver_event(session_id, runtime, event, false, cx);
                if !keep_runtime {
                    break;
                }
            }
        }
        self.pending_queue_drains.retain(|id| *id != session_id);
        let has_active_turn = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(AgentSession::active_turn_id)
            .is_some();
        let previous_kinds = has_active_turn
            .then(|| self.snapshot_selected_transcript_rows(session_id))
            .flatten();
        self.finish_streaming_assistant(session_id);
        self.complete_turn_blocks(session_id);
        self.settle_foreground_work(session_id, BackgroundWorkStatus::Stopped);
        if let Some(runtime) = runtime.as_mut() {
            runtime.stream_phase = None;
            runtime.pending_permission = None;
            runtime.pending_user_input = None;
            runtime.pending_computer_approval = None;
            runtime.computer_use_previews.clear();
        }
        if has_active_turn {
            let needs_fallback = !self.turn_has_assistant_message(session_id);
            if let Some(session) = self.state.session_mut(session_id) {
                session.status = SessionStatus::Idle;
                if needs_fallback {
                    session.push_message(MessageRole::Assistant, tr!("session.stopped"));
                }
            }
            self.finish_active_turn_with_analytics(
                session_id,
                TurnStatus::Interrupted,
                crate::analytics::TurnOutcome::Cancelled,
            );
        }
        if has_active_turn {
            self.capture_latest_turn_checkpoint_for(session_id);
            self.start_pending_checkpoint_captures(cx);
        }
        if let Some(previous_kinds) = previous_kinds.as_deref() {
            self.splice_active_transcript_rows_after_visibility_change(previous_kinds);
        }
        // A provider runtime owns its Goddard JavaScript REPL and Computer Use
        // descendants. Normally Stop closes that process tree and the next
        // prompt resumes the same provider thread with a fresh runtime. A
        // detached process or subagent is the exception: its provider must
        // remain resident so Goddard can keep observing and stopping it.
        if retain_runtime && keep_runtime {
            if let Some(runtime) = runtime.take() {
                self.runtimes.insert(session_id, runtime);
            }
        } else if let Some(runtime) = runtime {
            runtime.driver.close();
        }
        self.remeasure_transcript_tail();
        self.save();
        cx.notify();
    }

    pub(super) fn respond_permission(
        &mut self,
        request_id: String,
        option_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let provider = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.provider.id());
        let decision = if let Some(runtime) = self.runtimes.get_mut(&session_id) {
            let decision = runtime
                .pending_permission
                .as_ref()
                .and_then(|permission| {
                    permission
                        .options
                        .iter()
                        .find(|option| option.id == option_id)
                })
                .map_or(
                    "other",
                    |option| if option.allow { "allow" } else { "deny" },
                );
            runtime.driver.respond(request_id, option_id);
            runtime.pending_permission = None;
            Some(decision)
        } else {
            None
        };
        if let (Some(provider), Some(decision)) = (provider, decision) {
            self.analytics
                .track(crate::analytics::Event::PermissionResponded {
                    provider,
                    kind: "provider",
                    decision,
                });
        }
        if let Some(session) = self.selected_session_mut() {
            session.status = SessionStatus::Working;
        }
        cx.notify();
    }

    pub(super) fn sync_user_input_answer(&mut self, cx: &mut Context<Self>) {
        let answer = self
            .selected_runtime()
            .and_then(|runtime| runtime.pending_user_input.as_ref())
            .and_then(|pending| {
                pending
                    .current_question()
                    .map(|question| (pending, question))
            })
            .and_then(|(pending, question)| pending.custom_answers.get(&question.id))
            .cloned()
            .unwrap_or_default();
        self.user_input_answer
            .update(cx, |input, cx| input.set_content(answer, cx));
    }

    pub(super) fn update_user_input_custom_answer(
        &mut self,
        answer: impl AsRef<str>,
        cx: &mut Context<Self>,
    ) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let Some(pending) = self
            .runtimes
            .get_mut(&session_id)
            .and_then(|runtime| runtime.pending_user_input.as_mut())
        else {
            return;
        };
        let Some(question_id) = pending
            .current_question()
            .map(|question| question.id.clone())
        else {
            return;
        };
        let answer = answer.as_ref().to_owned();
        if answer.trim().is_empty() {
            pending.custom_answers.remove(&question_id);
        } else {
            pending.custom_answers.insert(question_id.clone(), answer);
            pending.selections.remove(&question_id);
        }
        cx.notify();
    }

    pub(super) fn submit_user_input_custom_answer(
        &mut self,
        answer: String,
        cx: &mut Context<Self>,
    ) {
        if answer.trim().is_empty() {
            return;
        }
        self.update_user_input_custom_answer(answer, cx);
        self.advance_user_input(cx);
    }

    pub(super) fn select_user_input_option(&mut self, label: String, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let Some(pending) = self
            .runtimes
            .get_mut(&session_id)
            .and_then(|runtime| runtime.pending_user_input.as_mut())
        else {
            return;
        };
        let Some((question_id, multi_select)) = pending
            .current_question()
            .map(|question| (question.id.clone(), question.multi_select))
        else {
            return;
        };
        let selected = pending.selections.entry(question_id.clone()).or_default();
        if multi_select {
            if let Some(index) = selected.iter().position(|answer| answer == &label) {
                selected.remove(index);
            } else {
                selected.push(label);
            }
        } else {
            selected.clear();
            selected.push(label);
        }
        if selected.is_empty() {
            pending.selections.remove(&question_id);
        }
        pending.custom_answers.remove(&question_id);
        self.user_input_answer
            .update(cx, |input, cx| input.clear(cx));
        cx.notify();
    }

    pub(super) fn previous_user_input(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let Some(pending) = self
            .runtimes
            .get_mut(&session_id)
            .and_then(|runtime| runtime.pending_user_input.as_mut())
        else {
            return;
        };
        if pending.question_index == 0 {
            return;
        }
        pending.question_index -= 1;
        self.sync_user_input_answer(cx);
        cx.notify();
    }

    pub(super) fn advance_user_input(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let should_submit = {
            let Some(pending) = self
                .runtimes
                .get_mut(&session_id)
                .and_then(|runtime| runtime.pending_user_input.as_mut())
            else {
                return;
            };
            let Some(question) = pending.current_question() else {
                return;
            };
            let answered = pending
                .custom_answers
                .get(&question.id)
                .is_some_and(|answer| !answer.trim().is_empty())
                || pending
                    .selections
                    .get(&question.id)
                    .is_some_and(|answers| !answers.is_empty());
            if !answered {
                return;
            }
            if pending.question_index + 1 < pending.questions.len() {
                pending.question_index += 1;
                false
            } else {
                true
            }
        };

        if should_submit {
            let Some(runtime) = self.runtimes.get_mut(&session_id) else {
                return;
            };
            let Some(pending) = runtime.pending_user_input.take() else {
                return;
            };
            let answers = pending.answers();
            runtime
                .driver
                .respond_user_input(pending.request_id, answers);
            if let Some(session) = self.state.session_mut(session_id) {
                session.status = SessionStatus::Working;
            }
            self.user_input_answer
                .update(cx, |input, cx| input.clear(cx));
        } else {
            self.sync_user_input_answer(cx);
        }
        cx.notify();
    }

    /// Settle the pending questions with the typed clarification — the
    /// provider reads it as "let me explain" and re-decides rather than
    /// recording an answer. Only offered where the transport supports
    /// user-input actions; the button itself gates on non-empty text.
    pub(super) fn clarify_user_input(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let Some(runtime) = self.runtimes.get_mut(&session_id) else {
            return;
        };
        let Some(content) = runtime
            .pending_user_input
            .as_ref()
            .and_then(|pending| {
                pending
                    .current_question()
                    .and_then(|question| pending.custom_answers.get(&question.id))
            })
            .cloned()
            .filter(|content| !content.trim().is_empty())
        else {
            return;
        };
        let Some(pending) = runtime.pending_user_input.take() else {
            return;
        };
        runtime
            .driver
            .clarify_user_input(pending.request_id, content);
        if let Some(session) = self.state.session_mut(session_id) {
            session.status = SessionStatus::Working;
        }
        self.user_input_answer
            .update(cx, |input, cx| input.clear(cx));
        cx.notify();
    }

    /// Dismiss the pending questions without answering. The provider stops
    /// waiting on them; the card is gone either way, so a transport that
    /// ignores the command leaves no dead UI.
    pub(super) fn dismiss_user_input(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let Some(runtime) = self.runtimes.get_mut(&session_id) else {
            return;
        };
        let Some(pending) = runtime.pending_user_input.take() else {
            return;
        };
        runtime.driver.cancel_user_input(pending.request_id);
        if let Some(session) = self.state.session_mut(session_id) {
            session.status = SessionStatus::Working;
        }
        self.user_input_answer
            .update(cx, |input, cx| input.clear(cx));
        cx.notify();
    }

    pub(super) fn respond_computer_permission(
        &mut self,
        decision: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        let provider = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map_or("unknown", |session| session.provider.id());
        let Some(mut runtime) = self.runtimes.remove(&session_id) else {
            return;
        };
        let Some(pending) = runtime.pending_computer_approval.take() else {
            self.runtimes.insert(session_id, runtime);
            return;
        };

        if decision == "deny" {
            runtime.driver.reject_computer_tool(
                pending.request,
                "The user denied control of this app.".into(),
            );
        } else {
            let key = pending.target.grant_key();
            runtime.computer_session_grants.insert(key);
            if decision == "always" && pending.target.persistable() {
                let grant = crate::computer_use::ComputerAppGrant {
                    bundle_id: pending.target.bundle_id.clone(),
                    app_name: pending.target.app_name.clone(),
                };
                if !self
                    .state
                    .computer_use_allowed_apps
                    .iter()
                    .any(|existing| existing.key() == grant.key())
                {
                    self.state.computer_use_allowed_apps.push(grant);
                    self.save();
                }
            }
            runtime.driver.run_computer_tool(pending.request);
        }
        if let Some(session) = self.state.session_mut(session_id) {
            session.status = SessionStatus::Working;
        }
        self.analytics
            .track(crate::analytics::Event::PermissionResponded {
                provider,
                kind: "computer_use",
                decision: match decision {
                    "deny" => "deny",
                    "always" => "allow_always",
                    "task" => "allow_task",
                    _ => "other",
                },
            });
        self.runtimes.insert(session_id, runtime);
        cx.notify();
    }

    pub(super) fn bring_computer_use_to_front(&mut self, window_id: u64, cx: &mut Context<Self>) {
        if let Some(runtime) = self
            .state
            .selected_session
            .and_then(|session_id| self.runtimes.get_mut(&session_id))
            && let Some(index) = runtime.computer_use_previews.iter().position(|preview| {
                preview
                    .target
                    .as_ref()
                    .is_some_and(|target| target.window_id == window_id)
            })
        {
            let preview = runtime.computer_use_previews.remove(index);
            runtime.computer_use_previews.push(preview);
        }
        cx.notify();
    }

    pub(super) fn dismiss_computer_use(&mut self, window_id: u64, cx: &mut Context<Self>) {
        if let Some(runtime) = self
            .state
            .selected_session
            .and_then(|session_id| self.runtimes.get_mut(&session_id))
        {
            if let Some(preview) = runtime.computer_use_previews.iter_mut().find(|preview| {
                preview
                    .target
                    .as_ref()
                    .is_some_and(|target| target.window_id == window_id)
            }) {
                // Keep the hidden entry until the turn ends so the next
                // screenshot cannot reopen a preview the user just closed.
                preview.visible = false;
                preview.decode_task = None;
                preview.frames = Default::default();
            }
        }
        cx.notify();
    }

    pub(super) fn add_project(&mut self, cx: &mut Context<Self>) {
        if self.daemon.is_remote() {
            self.show_toast(tr!("errors.remote_project_picker"));
            cx.notify();
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr!("project.add_project").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    if let Some(existing) = this.state.projects.iter().find(|p| p.path == path) {
                        this.select_project(existing.id, cx);
                        return;
                    }
                    let mut project = Project::from_path(path);
                    project.bookmark = crate::bookmarks::create(&project.path);
                    let project_id = project.id;
                    this.state.projects.push(project);
                    this.analytics.track(crate::analytics::Event::ProjectAdded);
                    this.create_session_for(project_id, this.state.last_provider, cx);
                });
            }
        })
        .detach();
    }

    pub(super) fn create_projectless_session(&mut self, cx: &mut Context<Self>) {
        if let Some(draft_id) = self
            .state
            .sessions
            .iter()
            .find(|session| {
                !session.has_started()
                    && self.state.projects.iter().any(|project| {
                        project.id == session.project_id
                            && project.is_projectless()
                            && !crate::projectless::is_legacy_root_path(&project.path)
                            && !self.is_remote_project(project.id)
                    })
            })
            .map(|session| session.id)
        {
            self.select_session(draft_id, cx);
            return;
        }

        // Projectless workspaces stay local: the projectless root is a path
        // under `~/.waku` on the creating host, and "no project" should not
        // pick a host at random.
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace.request(
                        waku_client::WorkspaceOperation::CreateProjectlessWorkspace {
                            prompt: None,
                        },
                    )? {
                        waku_client::WorkspaceResult::ProjectlessWorkspace { cwd } => Ok(cwd),
                        _ => anyhow::bail!("the daemon returned an invalid projectless response"),
                    }
                })
                .await;
            let _ = waku.update(cx, |waku, cx| match result {
                Ok(cwd) => {
                    let mut project = Project::from_path(cwd);
                    project.name = Project::PROJECTLESS_NAME.to_owned();
                    let project_id = project.id;
                    waku.state.projects.push(project);
                    waku.create_session_for(project_id, waku.state.last_provider, cx);
                    // A "no project" pick from the overlay's composer
                    // retargets its new-task destination to the provisioned
                    // workspace, carrying the typed draft across.
                    if waku.big_picture.is_open()
                        && waku.big_picture.target().is_none()
                        && waku.big_picture.new_task_project != Some(project_id)
                    {
                        let source = waku.big_picture.draft_key;
                        waku.big_picture.new_task_project = Some(project_id);
                        waku.sync_big_picture_draft(cx);
                        waku.move_composer_draft_after_project_change(source, cx);
                    }
                    // A Big Picture new-task submit stashes its prompt while
                    // the workspace is provisioned; the fresh draft is now
                    // selected, so it can land.
                    waku.drain_big_picture_pending_submission(cx);
                }
                Err(error) => {
                    waku.show_toast(tr!("errors.create_projectless_task", error = error));
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_carries_the_current_tasks_access_mode() {
        let mut current = AgentSession::new(Uuid::new_v4(), ProviderKind::OpenCode);
        current.runtime_mode = RuntimeMode::Ask;

        assert_eq!(
            new_task_runtime_mode(Some(&current), RuntimeMode::FullAccess),
            RuntimeMode::Ask
        );
        assert_eq!(
            new_task_runtime_mode(None, RuntimeMode::AutoAcceptEdits),
            RuntimeMode::AutoAcceptEdits
        );
    }

    #[test]
    fn new_task_carries_the_current_tasks_environment() {
        let mut current = AgentSession::new(Uuid::new_v4(), ProviderKind::OpenCode);
        current.sandboxed = true;

        assert!(new_task_sandboxed(Some(&current), false));
        assert!(!new_task_sandboxed(None, false));
        assert!(new_task_sandboxed(None, true));
    }

    #[test]
    fn new_task_navigation_reuses_a_draft_from_the_current_project() {
        let project_id = Uuid::new_v4();
        let draft = AgentSession::new(project_id, ProviderKind::Codex);
        let mut started = AgentSession::new(project_id, ProviderKind::Claude);
        started.begin_turn("Existing task");
        let mut navigation = SessionNavigation::default();

        navigation.remember_new_task(draft.id);
        navigation.visit(
            Some(NavigationLocation::Task(draft.id)),
            NavigationLocation::Task(started.id),
        );

        assert_eq!(
            navigation.remembered_new_task(&[draft.clone(), started], project_id),
            Some(draft.id)
        );
    }

    #[test]
    fn new_task_navigation_does_not_reopen_a_draft_from_another_project() {
        let draft = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let current_project_id = Uuid::new_v4();
        let mut navigation = SessionNavigation::default();

        navigation.remember_new_task(draft.id);

        assert_eq!(
            navigation.remembered_new_task(&[draft], current_project_id),
            None
        );
    }

    #[test]
    fn new_task_navigation_does_not_reopen_a_started_or_removed_draft() {
        let project_id = Uuid::new_v4();
        let mut draft = AgentSession::new(project_id, ProviderKind::Codex);
        let mut navigation = SessionNavigation::default();
        navigation.remember_new_task(draft.id);

        draft.begin_turn("Start it");
        assert_eq!(
            navigation.remembered_new_task(&[draft.clone()], project_id),
            None
        );

        navigation.remove(draft.id);
        assert_eq!(navigation.new_task, None);
    }

    #[test]
    fn stopping_releases_the_runtimes_that_cannot_be_interrupted_in_place() {
        // Codex owns a Computer Use process tree; Amp has no stream interrupt.
        assert!(!retain_runtime_after_cancel(ProviderKind::Codex));
        assert!(!retain_runtime_after_cancel(ProviderKind::Amp));
        for provider in ProviderKind::ALL {
            if !matches!(provider, ProviderKind::Codex | ProviderKind::Amp) {
                assert!(retain_runtime_after_cancel(provider));
            }
        }
    }
}
