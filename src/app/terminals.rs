use super::*;

/// One entry in the sidebar's Terminals group. `session` scopes the
/// terminal to the task whose right-panel surfaces own it; `None` is a
/// global terminal that exists only in the group.
pub(super) struct TerminalRecord {
    pub session: Option<Uuid>,
    /// Pinned terminals keep a sidebar row while the group is collapsed.
    pub pinned: bool,
    /// Where the PTY starts — `None` resolves to the owning session's
    /// workspace at spawn time, so the terminal follows the workspace
    /// when it moves; `Some` pins the terminal to a chosen directory.
    pub working_directory: Option<PathBuf>,
    /// When the terminal opened (unix seconds) — the row's "…ago" label
    /// until the view reports a command's start.
    pub opened_at: u64,
    /// A sidebar rename — wins over the view's OSC-set title and is
    /// applied to the view when it spawns.
    pub custom_title: Option<String>,
}

/// A terminal row is two lines — title and status over location and time —
/// the same card rhythm a session row uses.
const SIDEBAR_TERMINAL_CARD_HEIGHT: f32 = 51.0;
const SIDEBAR_TERMINAL_ROW_GAP: f32 = 1.0;
pub(super) const SIDEBAR_TERMINAL_ROW_HEIGHT: f32 =
    SIDEBAR_TERMINAL_CARD_HEIGHT + SIDEBAR_TERMINAL_ROW_GAP;

/// The nearest enclosing repository's root — `.git` may be a file in a
/// linked worktree, so existence rather than `is_dir` is the test.
pub(super) fn nearest_repo_root(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Character budget for a row's location label before ancestors fold
/// under `…/` — beyond it a layout-level tail clip would saw through a
/// component's middle.
const SIDEBAR_TERMINAL_DETAIL_MAX_CHARS: usize = 32;

/// Shorten a slash-separated label by popping whole ancestors, never
/// cutting through a component: `~/a/b/c` becomes `…/b/c`, then `…/c`.
/// With `keep_first`, the leading component — a repository name — stays
/// put and the popped middle folds as `repo/…/leaf` instead.
fn truncate_path_ancestors(path: &str, max_chars: usize, keep_first: bool) -> String {
    if path.chars().count() <= max_chars {
        return path.to_owned();
    }
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.len() <= 1 {
        return path.to_owned();
    }
    if keep_first {
        // The leading component anchors; the `…` stands for the middle
        // components between it and the leaf, so it needs both to exist.
        for dropped in 2..components.len() {
            let candidate = format!("{}/…/{}", components[0], components[dropped..].join("/"));
            if candidate.chars().count() <= max_chars {
                return candidate;
            }
        }
        return if components.len() > 2 {
            format!("{}/…/{}", components[0], components[components.len() - 1])
        } else {
            path.to_owned()
        };
    }
    for dropped in 1..components.len() {
        let candidate = format!("…/{}", components[dropped..].join("/"));
        if candidate.chars().count() <= max_chars {
            return candidate;
        }
    }
    format!("…/{}", components[components.len() - 1])
}

/// The group's flat listing in sidebar order: pinned terminals lead,
/// with `terminal_order`'s creation order kept inside each partition.
/// Stays a lazy scan — the sidebar fingerprint reads it every frame.
fn sidebar_terminal_order<'a>(
    terminal_order: &'a [Uuid],
    records: &'a HashMap<Uuid, TerminalRecord>,
) -> impl Iterator<Item = Uuid> + 'a {
    let partition = move |pinned: bool| {
        terminal_order.iter().copied().filter(move |id| {
            records
                .get(id)
                .is_some_and(|record| record.pinned == pinned)
        })
    };
    partition(true).chain(partition(false))
}

impl Waku {
    /// Terminal ids in sidebar order — pinned rows lead the group.
    pub(super) fn sidebar_terminal_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        sidebar_terminal_order(&self.terminal_order, &self.terminal_records)
    }

    /// The directory a terminal spawns into — the record's own directory,
    /// or the owning session's workspace when the record tracks it.
    pub(super) fn terminal_spawn_directory(&self, record: &TerminalRecord) -> Option<PathBuf> {
        record.working_directory.clone().or_else(|| {
            record
                .session
                .and_then(|session_id| {
                    self.state
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                })
                .and_then(|session| self.workspace_path_for_session(session))
                .map(Path::to_path_buf)
        })
    }

    /// The directory a row reports — the live PTY cwd once the view is up,
    /// the recorded spawn directory otherwise.
    pub(super) fn terminal_cwd(&self, terminal_id: Uuid, cx: &App) -> Option<PathBuf> {
        self.right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).working_directory().to_path_buf())
            .or_else(|| {
                self.terminal_records
                    .get(&terminal_id)
                    .and_then(|record| self.terminal_spawn_directory(record))
            })
    }

    /// Resolve every terminal's nearest repository root in one background
    /// pass. Rows read only the store — the ancestor walk is a filesystem
    /// probe, and a miss just means "not known yet".
    pub(super) fn ensure_sidebar_terminal_repo_roots(&self, cx: &mut Context<Self>) {
        let mut fingerprint = 0x7e0e_5a1d_1e0c_a710;
        let mut directories = HashSet::new();
        for terminal_id in self.sidebar_terminal_ids() {
            fingerprint = mix_uuid(fingerprint, terminal_id);
            if let Some(cwd) = self.terminal_cwd(terminal_id, cx) {
                fingerprint = mix_str(fingerprint, &cwd.to_string_lossy());
                directories.insert(cwd);
            }
        }
        if self.sidebar_terminal_repo_scan_fingerprint.get() == Some(fingerprint) {
            return;
        }
        self.sidebar_terminal_repo_scan_fingerprint
            .set(Some(fingerprint));
        let generation = self
            .sidebar_terminal_repo_scan_generation
            .get()
            .wrapping_add(1);
        self.sidebar_terminal_repo_scan_generation.set(generation);

        if directories.is_empty() {
            self.sidebar_terminal_repo_roots.borrow_mut().clear();
            return;
        }
        cx.spawn(async move |waku, cx| {
            let roots = cx
                .background_executor()
                .spawn(async move {
                    directories
                        .into_iter()
                        .map(|cwd| (cwd.clone(), nearest_repo_root(&cwd)))
                        .collect::<HashMap<_, _>>()
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if waku.sidebar_terminal_repo_scan_generation.get() != generation {
                    return;
                }
                *waku.sidebar_terminal_repo_roots.borrow_mut() = roots;
                cx.notify();
            });
        })
        .detach();
    }

    /// The next "…ago" boundary any terminal row crosses, in seconds from
    /// `now` — the same cadence contract `next_time_label_change` keeps
    /// for session rows, so the shared wake re-renders when a label flips.
    pub(super) fn next_terminal_time_label_change(&self, now: u64, cx: &App) -> Option<u64> {
        let mut next: Option<u64> = None;
        for terminal_id in self.sidebar_terminal_ids() {
            let Some(record) = self.terminal_records.get(&terminal_id) else {
                continue;
            };
            let started_at = self
                .right_panel_terminals
                .get(&terminal_id)
                .and_then(|view| view.read(cx).last_command_started_at())
                .unwrap_or(record.opened_at);
            let elapsed = now.saturating_sub(started_at);
            let step = match elapsed {
                0..=3_599 => 60,
                3_600..=86_399 => 3_600,
                _ => 86_400,
            };
            let remaining = (step - elapsed % step).max(1);
            next = Some(next.map_or(remaining, |next| next.min(remaining)));
        }
        next
    }

    /// Register a terminal surface with the group. Called wherever a
    /// terminal surface is created; records carry what the surface lists
    /// cannot — creation order, pin state, and the spawn directory.
    pub(super) fn register_terminal(
        &mut self,
        terminal_id: Uuid,
        session: Option<Uuid>,
        working_directory: Option<PathBuf>,
    ) {
        if self.terminal_records.contains_key(&terminal_id) {
            return;
        }
        self.terminal_records.insert(
            terminal_id,
            TerminalRecord {
                session,
                pinned: false,
                working_directory,
                opened_at: unix_time(),
                custom_title: None,
            },
        );
        self.terminal_order.push(terminal_id);
        self.sidebar_rows_fingerprint.set(None);
    }

    /// Drop every trace of a terminal: the view entity, its launch state,
    /// the group record, and any selection pointing at it.
    pub(super) fn drop_terminal(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        // A terminal filling the main area hands the view to a neighbor —
        // the row listed before it, else the one after — found before the
        // order entry disappears. No neighbor means the new task page.
        let listed = self.sidebar_terminal_ids().collect::<Vec<_>>();
        let successor = (self.selected_terminal == Some(terminal_id))
            .then(|| {
                let index = listed.iter().position(|id| *id == terminal_id)?;
                listed[..index]
                    .iter()
                    .rev()
                    .chain(listed[index + 1..].iter())
                    .copied()
                    .next()
            })
            .flatten();
        self.right_panel_terminals.remove(&terminal_id);
        self.right_panel_terminal_commands.remove(&terminal_id);
        self.custom_command_runs.remove(&terminal_id);
        self.terminal_records.remove(&terminal_id);
        self.terminal_order.retain(|id| *id != terminal_id);
        self.unseen_terminal_completions.remove(&terminal_id);
        self.session_navigation.remove_terminal(terminal_id);
        if self.terminal_rename == Some(terminal_id) {
            self.terminal_rename = None;
        }
        if self
            .terminal_close_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.terminal_id == terminal_id)
        {
            self.terminal_close_dialog = None;
        }
        if self.selected_terminal == Some(terminal_id) {
            self.selected_terminal = None;
            // The dead terminal's strip transfers to Bare rather than
            // parking under a key nothing can select again — its panel
            // terminals and browsers are live utilities, not the shell's.
            self.right_panel_live_owner = RightPanelOwner::Bare;
            self.right_panel_states
                .remove(&RightPanelOwner::Terminal(terminal_id));
            self.sync_right_panel_owner(cx);
        }
        if self.last_visible_terminal == Some(terminal_id) {
            self.last_visible_terminal = None;
        }
        if self.terminal_order.is_empty() {
            // Closing the last terminal folds the group — expanding an
            // empty group is how a new global terminal is made, so a row of
            // nothing adds nothing.
            self.set_sidebar_group_collapsed(SidebarGroup::Terminals, true, cx);
        }
        self.sidebar_rows_fingerprint.set(None);
        if let Some(successor) = successor
            && let Some(focus) = self.activate_terminal_state(successor, true, cx)
        {
            // Close paths reach here without a `Window`; focus goes
            // through the stored handle like `commit_go_to_line`.
            let window_handle = self.window_handle;
            let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
        }
    }

    /// Move a session-scoped terminal out of every session strip and into
    /// the Terminals group, keeping its PTY. Called when the workspace it
    /// tracked moved: the running shell can't follow, and respawning would
    /// kill whatever the user ran — or re-run a launch line — so the record
    /// drops its session, pins the live directory, and the surface leaves
    /// every strip that borrowed it.
    pub(super) fn detach_terminal_to_group(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        let Some(record) = self.terminal_records.get_mut(&terminal_id) else {
            return;
        };
        record.session = None;
        if record.working_directory.is_none() {
            record.working_directory = self
                .right_panel_terminals
                .get(&terminal_id)
                .map(|terminal| terminal.read(cx).working_directory().to_path_buf());
        }
        // The live strip is the terminal's own when it fills the main area —
        // its tab belongs there; anywhere else it sat on a session's loan.
        if self.right_panel_live_owner != RightPanelOwner::Terminal(terminal_id)
            && let Some(index) = self
                .right_panel_surfaces
                .iter()
                .position(|surface| surface.terminal_id() == Some(terminal_id))
        {
            self.right_panel_surfaces.remove(index);
            self.right_panel_active_surface = if self.right_panel_surfaces.is_empty() {
                None
            } else {
                Some(match self.right_panel_active_surface {
                    Some(active) if active > index => active - 1,
                    Some(active) if active == index => index.saturating_sub(1),
                    Some(active) => active.min(self.right_panel_surfaces.len() - 1),
                    None => 0,
                })
            };
            if let Some(active) = self.right_panel_active_surface {
                self.reveal_right_panel_tab(active);
                self.request_active_terminal_focus();
                self.request_active_browser_focus();
            } else {
                self.right_panel_pending_tab_reveal = None;
                self.right_panel_pending_terminal_focus = None;
                self.right_panel_pending_browser_focus = None;
                self.set_right_panel_visible(false, cx);
            }
        }
        if self.right_panel_last_focused_terminal == Some(terminal_id) {
            self.right_panel_last_focused_terminal = None;
        }
        for (owner, state) in self.right_panel_states.iter_mut() {
            if matches!(owner, RightPanelOwner::Terminal(id) if *id == terminal_id) {
                continue;
            }
            let Some(index) = state
                .surfaces
                .iter()
                .position(|surface| surface.terminal_id() == Some(terminal_id))
            else {
                continue;
            };
            state.surfaces.remove(index);
            state.active_surface = if state.surfaces.is_empty() {
                None
            } else {
                Some(match state.active_surface {
                    Some(active) if active > index => active - 1,
                    Some(active) if active == index => index.saturating_sub(1),
                    Some(active) => active.min(state.surfaces.len() - 1),
                    None => 0,
                })
            };
            if state.last_focused_terminal == Some(terminal_id) {
                state.last_focused_terminal = None;
            }
        }
        self.sidebar_rows_fingerprint.set(None);
        // Reveal the terminal's new home — under a folded group the row
        // hides and the vanished tab reads as a kill after all.
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
        self.show_toast(tr!("terminal.moved_to_group"));
        cx.notify();
    }

    /// Spawn the PTY-backed view for a terminal id. Shared by the right
    /// panel's lazy ensure and the Terminals group's direct creation.
    pub(super) fn spawn_terminal_entity(
        &mut self,
        terminal_id: Uuid,
        working_directory: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let command = self.right_panel_terminal_commands.get(&terminal_id);
        let launch = self
            .right_panel_terminal_programs
            .get(&terminal_id)
            .map(|(program, args)| TerminalLaunch::Program {
                program: program.clone(),
                args: args.clone(),
            })
            .or_else(|| command.cloned().map(TerminalLaunch::CustomCommand))
            .unwrap_or(TerminalLaunch::Shell);
        let close_on_exit = command.is_some_and(|command| command.close_on_success);
        let view = cx.new(|cx| TerminalView::with_launch(working_directory.clone(), launch, cx));
        cx.subscribe(&view, move |this, view, event: &TerminalViewEvent, cx| {
            match event {
                // The PTY exiting retires whatever surface held it — a
                // right-panel tab whose shell or program ended (ctrl+d,
                // `exit`, a signal), a terminal filling the main area,
                // or a close-on-success command whose launch line's
                // `&& exit` already reported through CommandFinished.
                TerminalViewEvent::Exited => {
                    // A sign-in tab's exit resolves the auth gate: probe the
                    // provider home and resubmit whatever it was holding.
                    this.sandbox_sign_in_terminal_exited(terminal_id, cx);
                    this.close_terminal_view_surface(&view, cx);
                }
                TerminalViewEvent::CommandFinished(code) => {
                    // A finished command may have changed the
                    // checkout, so drop the cached snapshot; the
                    // next read refetches.
                    this.refresh_selected_branch_snapshot(cx);
                    // Hand-typed commands journal here; custom-command
                    // terminals already journaled at launch.
                    if !this
                        .right_panel_terminal_commands
                        .contains_key(&terminal_id)
                    {
                        let session = this
                            .terminal_records
                            .get(&terminal_id)
                            .and_then(|record| record.session)
                            .or(this.state.selected_session);
                        this.record_action(session, action_predictions::JournalAction::TerminalRun);
                    }
                    this.custom_command_finished(terminal_id, *code, cx);
                    // A clean exit off-screen earns the row an unread dot;
                    // the terminal the user is watching needs none.
                    if *code == Some(0)
                        && !this.terminal_is_active_surface(terminal_id)
                        && this.unseen_terminal_completions.insert(terminal_id)
                    {
                        cx.notify();
                    }
                    // A command launched as plain typed input leaves its
                    // shell running after a clean exit, so the report
                    // itself is the close signal the sourced line's
                    // `&& exit` provides on the fallback path.
                    if close_on_exit && *code == Some(0) {
                        this.close_terminal_view_surface(&view, cx);
                    }
                }
                TerminalViewEvent::LocalhostUrl(url) => {
                    this.on_localhost_url_detected(&view, url.clone(), cx);
                }
                // Command status or cwd changed — the sidebar row reads
                // both straight off the view; the cwd fingerprint makes
                // the repo scan re-run on its own. A `cd` in the terminal
                // on screen also re-roots the panel's files.
                TerminalViewEvent::ActivityChanged => {
                    if this.selected_terminal == Some(terminal_id)
                        && this.sync_right_panel_files_root(cx)
                    {
                        this.refresh_right_panel_working_tree(cx);
                    }
                    cx.notify()
                }
                TerminalViewEvent::GenerateCommand {
                    generation,
                    request,
                    scrollback,
                    cwd,
                    shell,
                } => this.generate_terminal_command(
                    terminal_id,
                    &view,
                    *generation,
                    request.clone(),
                    scrollback.clone(),
                    cwd.clone(),
                    shell.clone(),
                    cx,
                ),
            }
        })
        .detach();
        if command.is_some() {
            // The view's 24ms poll notifies on every PTY dirty flag;
            // each one is a chance to refresh the toast's output tail.
            cx.observe(&view, move |this, _, cx| {
                this.refresh_command_run_tail(terminal_id, cx);
            })
            .detach();
        }
        // A rename that landed before the view existed applies on spawn.
        if let Some(title) = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.custom_title.clone())
        {
            view.update(cx, |view, cx| view.set_custom_title(Some(title), cx));
        }
        self.right_panel_terminals.insert(terminal_id, view);
    }

    /// The command bar's daemon round-trip: resolve the owning session's
    /// provider — a global terminal falls back to the selected one — and
    /// run the same one-shot agent invocation the commit dialog uses. The
    /// answer lands back on the view's bar, guarded by its generation.
    fn generate_terminal_command(
        &mut self,
        terminal_id: Uuid,
        view: &Entity<TerminalView>,
        generation: u64,
        request: String,
        scrollback: String,
        cwd: PathBuf,
        shell: String,
        cx: &mut Context<Self>,
    ) {
        let session = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.session)
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
            .or_else(|| self.selected_session());
        let invocation = session.and_then(|session| {
            Some(crate::git_commit::AgentInvocation {
                provider: session.provider,
                binary: self.provider_probe(session.provider)?.path.clone()?,
                model: self.model_for_session(session).map(str::to_owned),
                reasoning_effort: session.reasoning_effort.clone(),
            })
        });
        let Some(invocation) = invocation else {
            view.update(cx, |view, cx| {
                view.apply_command_generation(
                    generation,
                    SharedString::default(),
                    Err(tr!("terminal_command.agent_unavailable")),
                    cx,
                );
            });
            return;
        };
        let provider = SharedString::new_static(invocation.provider.display_name());
        let workspace_client = self.workspace_client_for_path(&cwd);
        let view = view.downgrade();
        cx.spawn(async move |_, cx| {
            let Some(workspace_client) = workspace_client else {
                let _ = view.update(cx, |view, cx| {
                    view.apply_command_generation(
                        generation,
                        provider,
                        Err(tr!("errors.daemon_disconnected")),
                        cx,
                    );
                });
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    match workspace_client.request(
                        waku_client::WorkspaceOperation::GenerateTerminalCommand {
                            cwd,
                            request,
                            scrollback: Some(scrollback).filter(|text| !text.trim().is_empty()),
                            shell: Some(shell).filter(|name| !name.is_empty()),
                            invocation,
                        },
                    ) {
                        Ok(waku_client::WorkspaceResult::TerminalCommand { command }) => {
                            Ok(command)
                        }
                        Ok(_) => {
                            Err("the daemon returned an invalid terminal command response"
                                .to_owned())
                        }
                        Err(error) => Err(error.to_string()),
                    }
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.apply_command_generation(generation, provider, result, cx)
            });
        })
        .detach();
    }

    /// Create a terminal rooted at `working_directory`. With `session` set
    /// it joins that task's right-panel surfaces; `None` makes it global —
    /// listed in the Terminals group but in no tab strip. A `command`
    /// replaces the plain shell launch: the PTY sources its script and the
    /// tab wears its icon.
    pub(super) fn create_terminal(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
        command: Option<CustomCommand>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        self.create_terminal_with_launch(
            working_directory,
            session,
            command.map(TerminalLaunch::CustomCommand),
            cx,
        )
    }

    /// A terminal tab running a bare program rather than a shell or a
    /// custom-command script — the sandbox sign-in `shuru run` argv lands
    /// here.
    pub(super) fn create_program_terminal(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
        program: PathBuf,
        args: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        self.create_terminal_with_launch(
            working_directory,
            session,
            Some(TerminalLaunch::Program { program, args }),
            cx,
        )
    }

    fn create_terminal_with_launch(
        &mut self,
        working_directory: PathBuf,
        session: Option<Uuid>,
        launch: Option<TerminalLaunch>,
        cx: &mut Context<Self>,
    ) -> Option<Uuid> {
        // A desktop PTY can only open on a local working directory.
        if self.is_remote_path(&working_directory) {
            return None;
        }
        let session = session.filter(|session_id| {
            self.state
                .sessions
                .iter()
                .any(|session| session.id == *session_id)
        });
        // A terminal created at its owning session's workspace tracks the
        // workspace (`None` resolves to it at spawn); any other directory
        // is the caller's deliberate choice and the record keeps it.
        let workspace_bound = session
            .and_then(|session_id| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
            })
            .and_then(|session| self.workspace_path_for_session(session))
            .is_some_and(|workspace| workspace == working_directory.as_path());
        let terminal_id = Uuid::new_v4();
        self.register_terminal(
            terminal_id,
            session,
            (!workspace_bound).then(|| working_directory.clone()),
        );
        let kind = match &launch {
            Some(TerminalLaunch::CustomCommand(command)) => {
                self.right_panel_terminal_commands
                    .insert(terminal_id, command.clone());
                "command"
            }
            Some(TerminalLaunch::Program { program, args }) => {
                self.right_panel_terminal_programs
                    .insert(terminal_id, (program.clone(), args.clone()));
                "command"
            }
            Some(TerminalLaunch::Shell) | None if session.is_some() => "session",
            _ => "global",
        };
        if let Some(session_id) = session {
            let surface = RightPanelSurface::Terminal(terminal_id);
            if self.right_panel_live_owner == RightPanelOwner::Session(session_id) {
                // Push directly rather than add_right_panel_surface: the tab
                // joins the strip without activating or stealing focus.
                self.right_panel_surfaces.push(surface);
            } else {
                self.right_panel_states
                    .entry(RightPanelOwner::Session(session_id))
                    .or_insert_with(|| RightPanelSessionState::empty(false))
                    .surfaces
                    .push(surface);
            }
        }
        self.spawn_terminal_entity(terminal_id, working_directory, cx);
        self.analytics
            .track(crate::analytics::Event::TerminalOpened { kind });
        cx.notify();
        Some(terminal_id)
    }

    /// Run a transcript code block in a terminal: the fence tag picks the
    /// interpreter, the block's own session roots it at its workspace — a
    /// side chat's strip is its parent's, the surface the user is looking
    /// at. The run rides the custom-command launch, so the shell stays
    /// open with the output when the script finishes.
    pub(super) fn run_markdown_code_block(
        &mut self,
        session_id: Option<Uuid>,
        language: &str,
        code: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = md::render::code_run_script(language, code) else {
            return;
        };
        let session = session_id
            .or(self.state.selected_session)
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id));
        // The strip the tab joins: the session itself, or a side chat's
        // parent — a side-chat surface only renders inside it. Any other
        // attach point falls through to the full-width terminal.
        let attach = session.map(|session| session.side_chat_of.unwrap_or(session.id));
        let working_directory = session
            .and_then(|session| self.workspace_path_for_session(session))
            .map(std::path::Path::to_path_buf);
        let Some(working_directory) =
            working_directory.filter(|directory| !self.is_remote_path(directory))
        else {
            self.show_toast(tr!("code_block.no_local_workspace"));
            return;
        };
        let mut command = CustomCommand::new(run.script);
        command.name = Some(language.to_owned());
        command.icon = CustomCommandIcon::Terminal;
        command.shell = run.shell.map(str::to_owned);
        let Some(terminal_id) = self.create_terminal(working_directory, attach, Some(command), cx)
        else {
            return;
        };
        self.record_action(session_id, action_predictions::JournalAction::TerminalRun);
        if let Some(index) = self
            .right_panel_surfaces
            .iter()
            .rposition(|surface| surface.terminal_id() == Some(terminal_id))
        {
            // The tab landed in the live strip — select it, scroll it
            // into view, and give it the pending focus slot like ⌘T's
            // fresh terminal.
            self.right_panel_active_surface = Some(index);
            self.reveal_right_panel_tab(index);
            self.request_active_terminal_focus();
            self.set_right_panel_visible(true, cx);
        } else {
            self.select_terminal(terminal_id, window, cx);
        }
        cx.notify();
    }

    /// Whether the terminal is the surface on screen — the full-width
    /// selection, or the visible right panel's active tab. An inactive tab
    /// or a background session's surface counts as unseen even when its
    /// stored strip still points at it.
    pub(super) fn terminal_is_active_surface(&self, terminal_id: Uuid) -> bool {
        self.selected_terminal == Some(terminal_id)
            || (self.right_panel_visible
                && self
                    .active_right_panel_surface()
                    .and_then(RightPanelSurface::terminal_id)
                    == Some(terminal_id))
    }

    /// Swap a sidebar terminal row's title for the shared rename field,
    /// seeded with the terminal's current title.
    fn begin_terminal_rename(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.terminal_records.contains_key(&terminal_id) {
            return;
        }
        let title = self
            .terminal_records
            .get(&terminal_id)
            .and_then(|record| record.custom_title.clone())
            .or_else(|| {
                self.right_panel_terminals
                    .get(&terminal_id)
                    .map(|terminal| terminal.read(cx).title().to_owned())
                    .filter(|title| !title.is_empty())
            })
            .unwrap_or_else(|| tr!("right_panel.terminal"));

        self.session_rename = None;
        self.terminal_rename = Some(terminal_id);
        self.session_rename_input.update(cx, |input, cx| {
            input.set_content(title, cx);
            input.select_all_text(cx);
        });
        let focus = self.session_rename_input.read(cx).focus();
        window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        cx.notify();
    }

    pub(super) fn commit_terminal_rename(&mut self, cx: &mut Context<Self>) {
        let Some(terminal_id) = self.terminal_rename.take() else {
            return;
        };
        let title = self
            .session_rename_input
            .read(cx)
            .content()
            .trim()
            .to_owned();
        if !title.is_empty() {
            if let Some(record) = self.terminal_records.get_mut(&terminal_id) {
                record.custom_title = Some(title.clone());
            }
            if let Some(terminal) = self.right_panel_terminals.get(&terminal_id) {
                terminal.update(cx, |terminal, cx| {
                    terminal.set_custom_title(Some(title), cx);
                });
            }
        }
        cx.notify();
    }

    fn cancel_terminal_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_rename.take().is_none() {
            return;
        }
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Show a terminal full-width in the main area. Entering terminal mode
    /// parks the session selection — composer draft, transcript scroll, and
    /// right-panel state are stored exactly as a session switch stores them.
    pub(super) fn select_terminal(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_terminal(terminal_id, true, window, cx);
    }

    /// `record_visit` is false only for back/forward history, which already
    /// moved the stacks before landing here.
    pub(super) fn activate_terminal(
        &mut self,
        terminal_id: Uuid,
        record_visit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focus) = self.activate_terminal_state(terminal_id, record_visit, cx) {
            window.focus(&focus, cx);
        }
    }

    /// The state half of activation — spawn the view if needed, record the
    /// visit, move selection — returning the terminal's focus handle so a
    /// caller with or without a `Window` can aim it.
    fn activate_terminal_state(
        &mut self,
        terminal_id: Uuid,
        record_visit: bool,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        let record = self.terminal_records.get(&terminal_id)?;
        if !self.right_panel_terminals.contains_key(&terminal_id) {
            let working_directory = self.terminal_spawn_directory(record);
            let Some(working_directory) =
                working_directory.filter(|directory| !self.is_remote_path(directory))
            else {
                return None;
            };
            self.spawn_terminal_entity(terminal_id, working_directory, cx);
        }
        if record_visit {
            self.session_navigation.visit(
                self.navigation_location(),
                NavigationLocation::Terminal(terminal_id),
            );
        }
        if self.state.selected_session.is_some() {
            self.capture_and_save_current_composer_draft(cx);
            self.store_transcript_scroll_position();
            self.state.selected_session = None;
        }
        self.pending_session_activation = None;
        // A terminal claims the main area too: open pages fold, keeping
        // their state for the next visit.
        self.projects_page = None;
        self.drafts_page = false;
        self.automations_page = false;
        self.automations_detail = None;
        self.notifications.open = false;
        self.selected_terminal = Some(terminal_id);
        self.last_visible_terminal = Some(terminal_id);
        self.unseen_terminal_completions.remove(&terminal_id);
        // Whatever owned the strip — a session, a page, or another
        // terminal — parks; this terminal's own strip comes back.
        self.sync_right_panel_owner(cx);
        // The terminal may have `cd`'d while it was off screen, or its
        // strip may have come back rooted somewhere else.
        if self.sync_right_panel_files_root(cx) {
            self.refresh_right_panel_working_tree(cx);
        }
        let focus = self
            .right_panel_terminals
            .get(&terminal_id)
            .map(|terminal| terminal.read(cx).focus_handle(cx));
        self.save();
        cx.notify();
        focus
    }

    /// Fold the group open and land on the last terminal that was on
    /// screen — the newest one if none has been shown yet, or a fresh
    /// global terminal in ~ when the group is empty.
    pub(super) fn expand_terminals_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
        let target = self
            .last_visible_terminal
            .filter(|id| self.terminal_records.contains_key(id))
            .or_else(|| {
                self.terminal_order
                    .iter()
                    .rev()
                    .copied()
                    .find(|id| self.terminal_records.contains_key(id))
            });
        let terminal_id = match target {
            Some(terminal_id) => Some(terminal_id),
            None => dirs::home_dir().and_then(|home| self.create_terminal(home, None, None, cx)),
        };
        if let Some(terminal_id) = terminal_id {
            self.select_terminal(terminal_id, window, cx);
        }
    }

    /// Folding the group while a terminal holds the main area also leaves
    /// terminal mode: the selection returns to wherever the history came
    /// from — the previous chat, a page, or an earlier terminal. With no
    /// back target the terminal keeps the column; the fold alone changes.
    pub(super) fn collapse_terminals_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, true, cx);
        if self.selected_terminal.is_none() {
            return;
        }
        let Some(current) = self.navigation_location() else {
            return;
        };
        Self::prune_navigation_stack(
            &self.state.projects,
            self.state.projects_page_enabled,
            self.state.automations_enabled,
            &mut self.session_navigation.back,
        );
        // Switching terminals stacks each one on the history; fold them
        // all away so closing lands on the chat behind them, not the
        // previous terminal.
        while matches!(
            self.session_navigation.back_target(),
            Some(NavigationLocation::Terminal(_))
        ) {
            self.session_navigation.back.pop();
        }
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
            Some(NavigationLocation::DraftsPage) => {
                let _ = self.session_navigation.go_back(current);
                self.show_drafts_page(window, cx);
            }
            Some(NavigationLocation::AutomationsPage) => {
                let _ = self.session_navigation.go_back(current);
                self.show_automations_page(window, cx);
            }
            None => {}
        }
    }

    /// `secondary-t` — always a fresh terminal, rooted where the user is.
    /// A focused right-panel terminal keeps the chord in the panel: the new
    /// terminal joins its strip in the same directory. Everywhere else the
    /// terminal lands in the Terminals group — rooted in the selected
    /// terminal's directory, the selected session's workspace, or ~ when
    /// the main area shows neither.
    pub(super) fn new_terminal_action(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        if let Some(terminal_id) = self.focused_right_panel_terminal(window, cx)
            && let Some(session_id) = self.state.selected_session
            && let Some(working_directory) = self.terminal_cwd(terminal_id, cx)
            && let Some(new_terminal_id) =
                self.create_terminal(working_directory, Some(session_id), None, cx)
            && let Some(index) = self
                .right_panel_surfaces
                .iter()
                .rposition(|surface| surface.terminal_id() == Some(new_terminal_id))
        {
            // create_terminal appended the tab without activating it —
            // select it in the strip and hand it the pending focus slot.
            self.right_panel_active_surface = Some(index);
            self.reveal_right_panel_tab(index);
            self.request_active_terminal_focus();
            cx.notify();
            return;
        }
        // Expand the group so the new row — and the selection — is visible.
        self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
        // The terminal on screen seeds the spawn directory: the full-width
        // selection in terminal mode, or the visible right panel's active
        // tab while its session is selected.
        let source_terminal = self.selected_terminal.or_else(|| {
            if self.right_panel_visible {
                self.active_right_panel_surface()
                    .and_then(RightPanelSurface::terminal_id)
            } else {
                None
            }
        });
        let (working_directory, session) = if let Some(terminal_id) = source_terminal {
            (
                self.terminal_cwd(terminal_id, cx),
                self.terminal_records
                    .get(&terminal_id)
                    .and_then(|record| record.session),
            )
        } else if let Some(session) = self.selected_session() {
            (
                self.workspace_path_for_session(session)
                    .map(std::path::Path::to_path_buf),
                None,
            )
        } else {
            (dirs::home_dir(), None)
        };
        if let Some(working_directory) = working_directory
            && let Some(terminal_id) = self.create_terminal(working_directory, session, None, cx)
        {
            self.select_terminal(terminal_id, window, cx);
        }
        cx.notify();
    }

    /// `secondary-shift-t` — the Terminals chord. It opens the group on
    /// the last-shown terminal — spawning a global one in ~ when the
    /// group is empty — or, while a terminal holds the main area, cycles
    /// to the next one in sidebar order, wrapping past the end. A lone
    /// terminal has nowhere to cycle, so the chord folds back to where
    /// it took over instead.
    pub(super) fn toggle_terminals_action(
        &mut self,
        _: &ToggleTerminals,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        if let Some(terminal_id) = self.selected_terminal {
            // The next row after the selected one, wrapping — the
            // selected id itself sits out of the chained slices, so a
            // lone terminal finds nothing.
            let listed = self.sidebar_terminal_ids().collect::<Vec<_>>();
            let next = listed
                .iter()
                .position(|id| *id == terminal_id)
                .and_then(|index| {
                    listed[index + 1..]
                        .iter()
                        .chain(listed[..index].iter())
                        .copied()
                        .next()
                });
            if let Some(next) = next {
                self.set_sidebar_group_collapsed(SidebarGroup::Terminals, false, cx);
                self.select_terminal(next, window, cx);
            } else {
                self.collapse_terminals_group(window, cx);
            }
        } else {
            self.expand_terminals_group(window, cx);
        }
        cx.notify();
    }

    /// ⌘⌥P on a terminal row: pinned terminals keep a sidebar row while the
    /// group is collapsed. Returns the new pinned state for the chord's
    /// toast — `None` when the terminal no longer exists.
    pub(super) fn toggle_terminal_pin(
        &mut self,
        terminal_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let Some(record) = self.terminal_records.get_mut(&terminal_id) else {
            return None;
        };
        record.pinned = !record.pinned;
        self.sidebar_rows_fingerprint.set(None);
        cx.notify();
        Some(record.pinned)
    }

    /// Close a terminal wherever it lives — the active session's tab
    /// strip, a background session's stored surfaces, or the global list.
    pub(super) fn close_terminal(&mut self, terminal_id: Uuid, cx: &mut Context<Self>) {
        let filled_main_area = self.selected_terminal == Some(terminal_id);
        // A main-area terminal opened from another surface is a temporary
        // visit. Consume its back entry before dropping the terminal, so
        // closing the tab or exiting the shell returns to where it came from.
        let history_target = if filled_main_area {
            Self::prune_navigation_stack(
                &self.state.projects,
                self.state.projects_page_enabled,
                self.state.automations_enabled,
                &mut self.session_navigation.back,
            );
            let current = NavigationLocation::Terminal(terminal_id);
            self.session_navigation
                .back_target()
                .and_then(|target| self.session_navigation.go_back(current).map(|_| target))
        } else {
            None
        };
        let terminal_focus = filled_main_area
            .then(|| {
                self.right_panel_terminals
                    .get(&terminal_id)
                    .map(|terminal| terminal.read(cx).focus_handle(cx))
            })
            .flatten();
        if let Some(index) = self
            .right_panel_surfaces
            .iter()
            .position(|surface| surface.terminal_id() == Some(terminal_id))
        {
            self.close_right_panel_surface(index, cx);
        } else {
            for state in self.right_panel_states.values_mut() {
                let Some(index) = state
                    .surfaces
                    .iter()
                    .position(|surface| surface.terminal_id() == Some(terminal_id))
                else {
                    continue;
                };
                state.surfaces.remove(index);
                state.active_surface = state.active_surface.and_then(|active| {
                    (!state.surfaces.is_empty()).then(|| match active.cmp(&index) {
                        std::cmp::Ordering::Greater => active - 1,
                        std::cmp::Ordering::Equal => index.saturating_sub(1),
                        std::cmp::Ordering::Less => active.min(state.surfaces.len() - 1),
                    })
                });
                break;
            }
            self.drop_terminal(terminal_id, cx);
        }
        // A terminal that filled the main area parked the session selection
        // on its way in, so with no successor taking over the main area
        // falls to a session-less new task page: the composer mounts but
        // its pickers stay disabled, and window focus stays on the dead
        // view. Land on the new-task draft the way ⌘N does. This runs
        // after the strip settles — selecting a session re-syncs the panel
        // owner, which swaps `right_panel_surfaces` out from under any
        // caller still holding an index into it.
        if filled_main_area
            && history_target.is_none()
            && self.selected_terminal.is_none()
            && self.state.selected_session.is_none()
        {
            let current_project = self
                .state
                .selected_project
                .and_then(|id| self.state.projects.iter().find(|project| project.id == id))
                .map(|project| (project.id, project.is_projectless()));
            match current_project {
                Some((_, true)) | None => self.create_projectless_session(cx),
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
            }
            // Settings covers the workspace and owns the keyboard for its
            // visit; the composer field is not in the dispatch tree there.
            if self.settings_page.is_none() {
                let focus = self.composer_focus(cx);
                let window_handle = self.window_handle;
                let _ = window_handle.update(cx, move |_, window, cx| {
                    // Take the keyboard back only from the dead view — a
                    // field or overlay focused in the meantime keeps it.
                    if window
                        .focused(cx)
                        .is_none_or(|focused| terminal_focus.as_ref() == Some(&focused))
                    {
                        window.focus(&focus, cx);
                    }
                });
            }
        }
        if let Some(target) = history_target {
            let window_handle = self.window_handle;
            let waku = cx.entity();
            cx.defer(move |cx| {
                let _ = window_handle.update(cx, move |_, window, cx| {
                    let _ = waku.update(cx, |this, cx| match target {
                        NavigationLocation::Task(session_id) => this.request_session_activation(
                            session_id,
                            SessionActivationTransition::Visit,
                            cx,
                        ),
                        NavigationLocation::Terminal(terminal_id) => {
                            this.activate_terminal(terminal_id, false, window, cx);
                        }
                        NavigationLocation::ProjectsPage(project_id) => {
                            this.show_projects_page(project_id, window, cx);
                        }
                        NavigationLocation::DraftsPage => this.show_drafts_page(window, cx),
                        NavigationLocation::AutomationsPage => {
                            this.show_automations_page(window, cx);
                        }
                    });
                });
            });
        }
        cx.notify();
    }

    /// ⌘W with a terminal filling the main area: an idle shell dies
    /// outright, a shell mid-command earns a confirmation first — the
    /// same `command_running` signal the sidebar spinner reads.
    pub(super) fn close_main_terminal(
        &mut self,
        terminal_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let busy = self
            .right_panel_terminals
            .get(&terminal_id)
            .is_some_and(|terminal| terminal.read(cx).command_running());
        if busy {
            let focus = self.open_terminal_close_dialog(terminal_id, cx);
            window.focus(&focus, cx);
        } else {
            self.close_terminal(terminal_id, cx);
        }
    }

    /// The selected terminal rendered full-width in the main area,
    /// standing in for the transcript while terminal mode is active.
    pub(super) fn render_main_terminal(
        &self,
        terminal_id: Uuid,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(terminal) = self.right_panel_terminals.get(&terminal_id).cloned() else {
            return div().into_any_element();
        };
        if (terminal.read(cx).panel_width() - width).abs() > 0.5 {
            terminal.update(cx, |terminal, _| terminal.set_panel_width(width));
        }
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(terminal)
            .into_any_element()
    }

    /// One terminal row in the Terminals group — also what a pinned
    /// terminal contributes to the collapsed group.
    pub(super) fn render_sidebar_terminal_item(
        &self,
        terminal_id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(record) = self.terminal_records.get(&terminal_id) else {
            return div().into_any_element();
        };
        let pinned = record.pinned;
        let terminal = self.right_panel_terminals.get(&terminal_id);
        let title = record
            .custom_title
            .clone()
            .or_else(|| {
                terminal
                    .map(|terminal| single_line_label(terminal.read(cx).title()))
                    .filter(|title| !title.is_empty())
            })
            .unwrap_or_else(|| tr!("right_panel.terminal"));
        let cwd = self.terminal_cwd(terminal_id, cx);
        // The title line's trailing slot is the command's status: spinning
        // while one runs, a failure mark or — for a clean exit the user
        // has not seen — an unread dot after, empty when the shell reports
        // nothing.
        let status_icon = terminal.and_then(|terminal| {
            let terminal = terminal.read(cx);
            if terminal.command_running() {
                Some(motion::spin_slow(icon(
                    "icons/loader-circle.svg",
                    12.0,
                    theme.text_tertiary,
                )))
            } else {
                match terminal.last_command_exit() {
                    Some(0)
                        if self.unseen_terminal_completions.contains(&terminal_id)
                            && !self.terminal_is_active_surface(terminal_id) =>
                    {
                        Some(
                            div()
                                .size(px(7.0))
                                .rounded_full()
                                .bg(theme.info)
                                .into_any_element(),
                        )
                    }
                    Some(0) => None,
                    Some(_) => {
                        Some(icon("icons/x-bold.svg", 12.0, theme.danger).into_any_element())
                    }
                    None => None,
                }
            }
        });
        let shell_name = terminal
            .map(|terminal| terminal.read(cx).shell_name().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                crate::command_env::default_terminal_shell()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned()
            });
        // The trailing label is the terminal's location, named by its
        // nearest repository the way a session row names its project —
        // "waku" at the root, "waku/src/app" deeper in. Outside any
        // repository the shell's name stands in under a shell glyph
        // rather than a folder, with the cwd trailing it.
        let repo = cwd.as_deref().and_then(|cwd| {
            self.sidebar_terminal_repo_roots
                .borrow()
                .get(cwd)
                .cloned()
                .flatten()
                .map(|root| (cwd, root))
        });
        let (detail_icon, detail) = match repo {
            Some((cwd, root)) => {
                let name = root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let label = cwd
                    .strip_prefix(&root)
                    .ok()
                    .filter(|rel| !rel.as_os_str().is_empty())
                    .map(|rel| format!("{name}/{}", rel.display()))
                    .unwrap_or(name);
                // The repo name anchors the label — the folded middle
                // comes from the working directory's ancestors.
                (
                    "icons/folder.svg",
                    truncate_path_ancestors(&label, SIDEBAR_TERMINAL_DETAIL_MAX_CHARS, true),
                )
            }
            None => (
                "icons/terminal-square.svg",
                cwd.as_deref()
                    .map(|cwd| {
                        let cwd =
                            settings::abbreviate_home_path(cwd, self.home_directory.as_deref());
                        let budget = SIDEBAR_TERMINAL_DETAIL_MAX_CHARS
                            .saturating_sub(shell_name.chars().count() + 1);
                        let cwd = truncate_path_ancestors(&cwd, budget, false);
                        format!("{shell_name} {cwd}")
                    })
                    .unwrap_or_else(|| shell_name.clone()),
            ),
        };
        // "…ago" marks the latest command's start while one runs; before
        // the first command it is the terminal's own open time.
        let started_at = terminal
            .and_then(|terminal| terminal.read(cx).last_command_started_at())
            .unwrap_or(record.opened_at);
        let time_label = format_time_ago(unix_time().saturating_sub(started_at));
        let selected = self.selected_terminal == Some(terminal_id);
        let renaming = self.terminal_rename == Some(terminal_id);
        let menu = self.menu_handle(format!("terminal-{terminal_id}"), cx);
        let row_focus = menu.trigger_focus_handle().clone();
        let keyboard_menu = menu.clone();
        let waku = cx.entity().downgrade();
        let group_name = SharedString::from(format!("terminal-row-{terminal_id}"));
        let close_focus = self
            .sidebar_terminal_close_focuses
            .borrow_mut()
            .entry(terminal_id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let pin_focus = self
            .sidebar_terminal_pin_focuses
            .borrow_mut()
            .entry(terminal_id)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        // The pin control shares the close control's reveal: zero-width
        // until the row is hovered or the button takes keyboard focus.
        let pin_button = div()
            .id(SharedString::from(format!("terminal-pin-{terminal_id}")))
            .track_focus(&pin_focus)
            .tab_index(0)
            .flex_none()
            .w_0()
            .h(px(18.0))
            .overflow_hidden()
            .rounded(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(0.0)
            .group_hover(group_name.clone(), |style| style.w(px(20.0)).opacity(1.0))
            .focus_visible(|style| style.w(px(20.0)).opacity(1.0).bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text_with_action(
                if pinned {
                    tr!("session.unpin")
                } else {
                    tr!("session.pin")
                },
                &ToggleSessionPin,
            ))
            .child(icon(
                if pinned {
                    "icons/pin-filled.svg"
                } else {
                    "icons/pin.svg"
                },
                12.0,
                theme.text_secondary,
            ))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                let _ = this.toggle_terminal_pin(terminal_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    let _ = this.toggle_terminal_pin(terminal_id, cx);
                    cx.stop_propagation();
                }
            }));
        // The close control borrows the status slot: it stays zero-width
        // until the row is hovered or the button takes keyboard focus.
        let close_button = div()
            .id(SharedString::from(format!("terminal-close-{terminal_id}")))
            .track_focus(&close_focus)
            .tab_index(0)
            .flex_none()
            .w_0()
            .h(px(18.0))
            .overflow_hidden()
            .rounded(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(0.0)
            .group_hover(group_name.clone(), |style| style.w(px(20.0)).opacity(1.0))
            .focus_visible(|style| style.w(px(20.0)).opacity(1.0).bg(theme.focus_highlight()))
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .tooltip(Tooltip::text(tr!("common.close")))
            .child(icon("icons/trash.svg", 12.0, theme.text_secondary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.close_terminal(terminal_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.close_terminal(terminal_id, cx);
                    cx.stop_propagation();
                }
            }));
        let row = div()
            .id(SharedString::from(format!(
                "sidebar-terminal-{terminal_id}"
            )))
            .group(group_name.clone())
            .w_full()
            .min_w_0()
            .h(px(SIDEBAR_TERMINAL_CARD_HEIGHT))
            .pl(px(8.0))
            .pr(px(8.0))
            .py(px(7.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .rounded(px(9.0))
            .cursor_default()
            .when(selected, |element| {
                element.bg(theme.sidebar_item_background)
            })
            .hover(|element| element.bg(theme.sidebar_item_background))
            .active(|element| element.bg(theme.sidebar_item_background))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .overflow_hidden()
                    .line_height(sp(18.0))
                    .child(if renaming {
                        div()
                            .id(SharedString::from(format!(
                                "terminal-rename-field-{terminal_id}"
                            )))
                            .key_context(sidebar::SESSION_RENAME_PARENT_CONTEXT)
                            .on_action(cx.listener(|this, _: &CancelSessionRename, window, cx| {
                                this.cancel_terminal_rename(window, cx);
                            }))
                            .h(px(18.0))
                            .flex_1()
                            .min_w_0()
                            .px(px(4.0))
                            .rounded(px(4.0))
                            .border(hairline())
                            .border_color(theme.accent)
                            .bg(theme.inset)
                            .flex()
                            .items_center()
                            .text_size(sp(13.5))
                            .text_color(theme.text)
                            .child(self.session_rename_input.clone())
                    } else {
                        div()
                            .id(SharedString::from(format!("terminal-title-{terminal_id}")))
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(sp(13.5))
                            .text_color(theme.text)
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    if event.click_count() == 2 {
                                        this.begin_terminal_rename(terminal_id, window, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(SharedString::from(title))
                    })
                    .when_some(status_icon, |element, status_icon| {
                        element.child(
                            div()
                                .flex_none()
                                .size(px(12.0))
                                // The zero-width pin/close pair still
                                // claims its two flex gaps; pulling the slot
                                // right by that amount keeps the indicator's
                                // right edge flush with the timestamp below.
                                .mr(px(-12.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .group_hover(group_name.clone(), |style| style.invisible())
                                .child(status_icon),
                        )
                    })
                    .child(pin_button)
                    .child(close_button),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .text_size(sp(13.0))
                    .line_height(sp(15.0))
                    .child(icon(detail_icon, 12.5, theme.text_tertiary))
                    // The char budget folds ancestors first; this clip is
                    // only the last resort for a single oversized leaf.
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(detail)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(12.5))
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(time_label)),
                    ),
            )
            .when(!renaming, |element| {
                element
                    .track_focus(&row_focus)
                    .tab_index(0)
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let key = event.keystroke.key.as_str();
                        if matches!(key, "enter" | "space") {
                            this.select_terminal(terminal_id, window, cx);
                            cx.stop_propagation();
                        } else if key == "f10" && event.keystroke.modifiers.shift {
                            keyboard_menu.open_context_menu(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_terminal(terminal_id, window, cx);
                    }))
            });

        let row = if renaming {
            div()
                .w_full()
                .child(row)
                .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                    if this.terminal_rename == Some(terminal_id) {
                        this.commit_terminal_rename(cx);
                    }
                }))
                .into_any_element()
        } else {
            context_menu(
                div().w_full().child(row),
                SharedString::from(format!("terminal-menu-{terminal_id}")),
                &menu,
                move |cx| {
                    let pinned = waku
                        .update(cx, |waku, _| {
                            waku.terminal_records
                                .get(&terminal_id)
                                .is_some_and(|record| record.pinned)
                        })
                        .unwrap_or(false);
                    let rename_waku = waku.clone();
                    let pin_waku = waku.clone();
                    let close_waku = waku.clone();
                    vec![
                        MenuItem::new(tr!("common.rename"), move |window, cx| {
                            let _ = rename_waku.update(cx, |waku, cx| {
                                waku.begin_terminal_rename(terminal_id, window, cx);
                            });
                        })
                        .icon("icons/pencil.svg"),
                        MenuItem::new(
                            if pinned {
                                tr!("session.unpin")
                            } else {
                                tr!("session.pin")
                            },
                            move |_, cx| {
                                let _ = pin_waku.update(cx, |waku, cx| {
                                    let _ = waku.toggle_terminal_pin(terminal_id, cx);
                                });
                            },
                        )
                        .shortcut_action(&ToggleSessionPin)
                        .icon(if pinned {
                            "icons/pin-off.svg"
                        } else {
                            "icons/pin.svg"
                        }),
                        MenuItem::Separator,
                        MenuItem::new(tr!("common.close"), move |_, cx| {
                            let _ = close_waku.update(cx, |waku, cx| {
                                waku.close_terminal(terminal_id, cx);
                            });
                        })
                        .icon("icons/x.svg"),
                    ]
                },
            )
        };

        div()
            .w_full()
            .pb(px(SIDEBAR_TERMINAL_ROW_GAP))
            .child(row)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_terminal_order_leads_with_pinned() {
        let ids = (1..=4u128).map(Uuid::from_u128).collect::<Vec<_>>();
        let record = |pinned| TerminalRecord {
            session: None,
            pinned,
            working_directory: None,
            opened_at: 0,
            custom_title: None,
        };
        let mut records = HashMap::new();
        records.insert(ids[0], record(false));
        records.insert(ids[1], record(true));
        records.insert(ids[2], record(false));
        records.insert(ids[3], record(true));

        // Each partition keeps the flat list's creation order.
        let listed = sidebar_terminal_order(&ids, &records).collect::<Vec<_>>();
        assert_eq!(listed, vec![ids[1], ids[3], ids[0], ids[2]]);

        // Order entries without records drop out of the listing.
        records.remove(&ids[3]);
        let listed = sidebar_terminal_order(&ids, &records).collect::<Vec<_>>();
        assert_eq!(listed, vec![ids[1], ids[0], ids[2]]);
    }

    #[test]
    fn ancestor_truncation_leaves_short_paths_alone() {
        assert_eq!(truncate_path_ancestors("~/src", 32, false), "~/src");
        assert_eq!(truncate_path_ancestors("waku", 4, true), "waku");
    }

    #[test]
    fn ancestor_truncation_pops_whole_components() {
        assert_eq!(
            truncate_path_ancestors("~/very/long/ancestor/path", 12, false),
            "…/path"
        );
        assert_eq!(
            truncate_path_ancestors("~/very/long/ancestor/path", 16, false),
            "…/ancestor/path"
        );
    }

    #[test]
    fn ancestor_truncation_keeps_the_repo_name_anchored() {
        assert_eq!(
            truncate_path_ancestors("repo/alpha/beta/gamma", 14, true),
            "repo/…/gamma"
        );
        assert_eq!(
            truncate_path_ancestors("repo/alpha/beta/gamma", 18, true),
            "repo/…/beta/gamma"
        );
        // Two components have no ancestors to pop — nothing is faked.
        assert_eq!(truncate_path_ancestors("repo/leaf", 4, true), "repo/leaf");
    }

    #[test]
    fn ancestor_truncation_never_cuts_through_a_component() {
        let truncated = truncate_path_ancestors("~/a/averyverylongleafname", 8, false);
        assert_eq!(truncated, "…/averyverylongleafname");
        assert!(
            truncated.ends_with("averyverylongleafname"),
            "the leaf stays whole even past the budget: {truncated}"
        );
        // A lone component has no ancestors to pop.
        assert_eq!(
            truncate_path_ancestors("averyverylongname", 8, false),
            "averyverylongname"
        );
    }
}
