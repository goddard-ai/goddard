//! Antigravity (`agy`) terminal-backed sessions.
//!
//! An Antigravity task has no driver, no daemon runtime, and no transcript:
//! the surface is a PTY running the CLI's own TUI, and the sidebar status
//! comes from the CLI's own `conversation_summaries.db`, polled off-thread.
//! The session record itself rides the same persistence and sidebar as
//! every other task.

use super::runtime::prepare_submission;
use super::*;
use crate::agy;

/// How often the summaries db is read while any Antigravity session has a
/// live terminal or an unresolved conversation id. The poll is one batched
/// query, so a cheaper cadence buys responsiveness for free.
pub(super) const AGY_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// How long a deselected, idle Antigravity terminal survives before its
/// process is released. A visible terminal is never killed; reselecting
/// the task respawns the TUI against `--conversation`.
const AGY_TERMINAL_IDLE_GRACE: Duration = Duration::from_secs(10 * 60);

/// What one background poll learned about the Antigravity sessions.
pub(super) struct AgyPollUpdate {
    /// Session id → the conversation id discovered for it, when the session
    /// had no cursor yet.
    pub discovered: HashMap<Uuid, String>,
    /// Conversation id → its summaries row, for sessions that carry a
    /// cursor already.
    pub summaries: HashMap<String, agy::ConversationSummary>,
}

/// The mapped session state a summaries row implies, in the sidebar's own
/// vocabulary: foreground work is `Working`, detached work the TUI has
/// finished waiting on is `Background`, and everything else is `Idle`.
fn agy_session_status(summary: &agy::ConversationSummary) -> SessionStatus {
    if summary.is_running() {
        SessionStatus::Working
    } else if !summary.killed && summary.not_fully_idle {
        SessionStatus::Background
    } else {
        SessionStatus::Idle
    }
}

impl Waku {
    /// Submit a draft Antigravity task's first prompt.
    ///
    /// This is the terminal-backed counterpart of the driver path: the
    /// prompt is recorded so the task persists and titles itself, the
    /// workspace materializes through the same preparation the driver path
    /// uses, and the finished preparation spawns `agy -i` as the session's
    /// terminal instead of asking the daemon for a runtime.
    pub(super) fn submit_agy_submission(
        &mut self,
        session_id: Uuid,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        let selected = self.state.selected_session == Some(session_id);
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        // A started session's TUI owns every follow-up — the composer is
        // already hidden for it, so a submission reaching this path has
        // nowhere valid to go.
        if session.has_started() {
            return;
        }
        // The surface is a local PTY; a remote daemon's workspace paths do
        // not exist on this machine.
        if self.daemon.is_remote() {
            if selected {
                self.restore_composer_submission(submission, cx);
                self.show_toast(tr!(
                    "errors.provider_terminal_sessions_local_only",
                    provider = ProviderKind::Antigravity.display_name()
                ));
            }
            cx.notify();
            return;
        }
        let project_id = session.project_id;
        let workspace = session.workspace.clone();
        let next_turn_count = session.turns.len() + 1;
        let Some(project) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            if selected {
                self.restore_composer_submission(submission, cx);
                self.show_toast(tr!("errors.prepare_task_project_not_found"));
            }
            cx.notify();
            return;
        };
        // The task's message and busy state land before any Git work begins,
        // the same rule the driver path follows; a failed preparation
        // unwinds them together.
        let prompt = submission.prompt.clone();
        let mut incognito = false;
        let message_id = if let Some(session) = self.state.session_mut(session_id) {
            incognito = session.incognito;
            if !submission.hidden {
                session.set_title_from_prompt(&submission.human_prompt());
            }
            let message_id = session.push_message(MessageRole::User, &submission.human_prompt());
            session.status = SessionStatus::Connecting;
            session.updated_at = unix_time();
            Some(message_id)
        } else {
            None
        };
        self.submission_preparations.insert(session_id);
        let workspace_client = waku_client::WorkspaceClient::new(self.daemon.client());
        let sync_default_branch = self.state.new_worktree_sync_default_branch;
        let sync_branches = self.state.new_worktree_sync_branches.clone();
        cx.spawn(async move |waku, cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    prepare_submission(
                        workspace_client,
                        project,
                        workspace,
                        None,
                        None,
                        session_id,
                        next_turn_count,
                        sync_default_branch,
                        sync_branches,
                        incognito,
                    )
                })
                .await;
            let _ = waku.update(cx, move |waku, cx| {
                waku.finish_agy_submission(
                    session_id, submission, prompt, message_id, prepared, cx,
                );
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_agy_submission(
        &mut self,
        session_id: Uuid,
        submission: ComposerSubmission,
        prompt: String,
        message_id: Option<Uuid>,
        prepared: anyhow::Result<PreparedSubmission>,
        cx: &mut Context<Self>,
    ) {
        if !self.submission_preparations.remove(&session_id) {
            return;
        }
        let selected = self.state.selected_session == Some(session_id);
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.drain_pending_workspace_cleanups(cx);
                if let Some(session) = self.state.session_mut(session_id) {
                    if session.status == SessionStatus::Connecting {
                        if let Some(message_id) = message_id {
                            session.messages.retain(|message| message.id != message_id);
                        }
                        session.status = SessionStatus::Idle;
                    }
                }
                if selected {
                    self.restore_composer_submission(submission, cx);
                    self.show_toast(tr!("errors.create_worktree", error = error));
                }
                cx.notify();
                return;
            }
        };
        let PreparedSubmission {
            workspace,
            checkpoint_warning,
            lfs_warning,
            worktree_restored,
            driver: _,
            route_decision: _,
            turn_effort: _,
        } = prepared;
        let workspace_changed = self.state.session_mut(session_id).is_some_and(|session| {
            let changed = session.workspace != workspace;
            session.workspace = workspace;
            changed
        });
        if selected && (workspace_changed || worktree_restored) {
            self.invalidate_workspace_queries(cx);
            self.ensure_right_panel_terminals(cx);
        }
        if selected && worktree_restored {
            self.show_toast(tr!("session.worktree_recreated"));
        }
        if selected && let Some(warning) = checkpoint_warning {
            self.show_toast(warning);
        }
        if selected && let Some(warning) = lfs_warning {
            self.show_toast(warning);
        }
        let prompt = self.resolve_provider_submission(ProviderKind::Antigravity, &prompt);
        let spawned = self.spawn_agy_terminal(session_id, AgyLaunchKind::Prompt(prompt), cx);
        if let Some(session) = self.state.session_mut(session_id) {
            // A launch that never produced a terminal is a failed start, not
            // work in flight — the relaunch affordance is the retry path.
            session.status = if spawned {
                SessionStatus::Working
            } else {
                SessionStatus::Failed
            };
            session.updated_at = unix_time();
        }
        self.save();
        cx.notify();
    }

    /// Whether the Antigravity session's TUI is on screen right now.
    pub(super) fn agy_session_visible(&self, session_id: Uuid) -> bool {
        self.state.selected_session == Some(session_id)
    }

    /// The terminal entity for an Antigravity session's TUI, if the process
    /// is up.
    pub(super) fn agy_terminal(&self, session_id: Uuid) -> Option<Entity<TerminalView>> {
        self.agy_terminals.get(&session_id).cloned()
    }

    /// Bring the session's TUI up if it is down: fresh sessions get `agy -i`
    /// through the submit path, while selecting a started session respawns
    /// against `--conversation` (or a bare TUI when no cursor was captured).
    /// No-op for non-Antigravity sessions and while a terminal is alive.
    pub(super) fn ensure_agy_terminal(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        if self.agy_terminals.contains_key(&session_id) || self.daemon.is_remote() {
            return;
        }
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if session.provider != ProviderKind::Antigravity || !session.has_started() {
            return;
        }
        let launch = match session.provider_cursor.as_ref() {
            Some(ProviderResumeCursor::Antigravity { conversation_id }) => {
                AgyLaunchKind::Resume(conversation_id.clone())
            }
            _ => AgyLaunchKind::Fresh,
        };
        self.spawn_agy_terminal(session_id, launch, cx);
    }

    /// Spawn the session's TUI process and track the terminal. The spawn
    /// timestamp bounds conversation-id discovery: a summaries row older
    /// than it cannot be the process that just started. Returns whether a
    /// terminal was created.
    fn spawn_agy_terminal(
        &mut self,
        session_id: Uuid,
        kind: AgyLaunchKind,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned()
        else {
            return false;
        };
        let Some(cwd) = self
            .workspace_path_for_session(&session)
            .map(Path::to_path_buf)
        else {
            self.show_toast(tr!("errors.prepare_task_project_not_found"));
            return false;
        };
        let SessionOptions {
            model,
            reasoning_effort,
            ..
        } = self.session_options(&session);
        let Some(binary) = self
            .probes
            .iter()
            .find(|probe| probe.provider == ProviderKind::Antigravity)
            .and_then(|probe| probe.path.clone())
        else {
            // Provider detection resolves on a background thread, so the
            // startup respawn can legitimately run before the `agy` probe
            // lands. Defer instead of failing: the poll tick retries once
            // detection completes, and only an absent binary toasts.
            if self.provider_detection_checked_at.is_none() {
                self.agy_pending_spawns.insert(session_id);
            } else {
                self.show_toast(tr!(
                    "errors.provider_not_found",
                    provider = ProviderKind::Antigravity.display_name()
                ));
            }
            return false;
        };
        let launch = match &kind {
            AgyLaunchKind::Prompt(prompt) => agy::AgyLaunch::initial_prompt(
                prompt,
                model.as_deref(),
                reasoning_effort.as_deref(),
            ),
            AgyLaunchKind::Resume(conversation_id) => agy::AgyLaunch::resume(
                conversation_id,
                model.as_deref(),
                reasoning_effort.as_deref(),
            ),
            AgyLaunchKind::Fresh => {
                agy::AgyLaunch::fresh(model.as_deref(), reasoning_effort.as_deref())
            }
        };
        let view = cx.new(|cx| {
            TerminalView::with_launch(
                cwd,
                TerminalLaunch::Program {
                    program: binary,
                    args: launch.args,
                },
                cx,
            )
        });
        cx.subscribe(&view, move |this, _view, event: &TerminalViewEvent, cx| {
            match event {
                // The TUI exiting is the session's own lifecycle — `/quit`,
                // a crash, or the PTY dying. Dropping the view keeps the
                // task; selecting it again respawns the surface.
                TerminalViewEvent::Exited => {
                    if let Some(session) = this.state.session_mut(session_id)
                        && session.status.is_busy()
                    {
                        session.status = SessionStatus::Idle;
                        session.updated_at = unix_time();
                    }
                    this.agy_terminals.remove(&session_id);
                    this.save();
                    cx.notify();
                }
                TerminalViewEvent::CommandFinished(_)
                | TerminalViewEvent::LocalhostUrl(_)
                | TerminalViewEvent::ActivityChanged
                | TerminalViewEvent::GenerateCommand { .. } => {}
            }
        })
        .detach();
        if self.agy_session_visible(session_id) {
            self.agy_last_visible.insert(session_id, Instant::now());
            let focus = view.read(cx).focus_handle(cx);
            let window_handle = self.window_handle;
            let _ = window_handle.update(cx, move |_, window, cx| {
                window.focus(&focus, cx);
            });
        }
        self.agy_terminals.insert(session_id, view);
        self.agy_spawned_at.insert(session_id, unix_time());
        true
    }

    /// The session's main surface: its live TUI terminal, or a relaunch
    /// affordance once the process has exited. Renders only for a selected,
    /// started Antigravity session — drafts keep the ordinary composer.
    pub(super) fn render_agy_surface(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(session_id) = self.state.selected_session else {
            return div().into_any_element();
        };
        let Some(terminal) = self.agy_terminals.get(&session_id).cloned() else {
            return div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(sp(13.0))
                        .text_color(theme.text_tertiary)
                        .child(tr_cow!("session.agy_ended")),
                )
                .child(
                    div()
                        .id("agy-relaunch")
                        .px(px(14.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .bg(theme.raised)
                        .border_1()
                        .border_color(theme.border)
                        .cursor_pointer()
                        .text_size(sp(12.5))
                        .text_color(theme.text)
                        .hover(|style| style.border_color(theme.border_strong))
                        .child(tr_cow!("session.agy_relaunch"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.ensure_agy_terminal(session_id, cx);
                                if let Some(terminal) = this.agy_terminal(session_id) {
                                    let focus = terminal.read(cx).focus_handle(cx);
                                    let window_handle = this.window_handle;
                                    let _ = window_handle.update(cx, move |_, window, cx| {
                                        window.focus(&focus, cx);
                                    });
                                }
                                cx.notify();
                            }),
                        ),
                )
                .into_any_element();
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

    /// Free a deselected Antigravity terminal that has been idle past the
    /// grace window — the TUI is reattachable through `--conversation`, so
    /// an idle process is pure cost. `reap_idle_sessions` calls this on its
    /// sweep.
    pub(super) fn reap_idle_agy_terminals(&mut self) {
        let now = Instant::now();
        let stale = self
            .agy_terminals
            .keys()
            .filter(|session_id| {
                !self.agy_session_visible(**session_id)
                    && self
                        .state
                        .sessions
                        .iter()
                        .find(|session| session.id == **session_id)
                        .is_some_and(|session| {
                            matches!(session.status, SessionStatus::Idle | SessionStatus::Failed)
                        })
                    && self
                        .agy_last_visible
                        .get(*session_id)
                        .is_none_or(|last| now.duration_since(*last) >= AGY_TERMINAL_IDLE_GRACE)
            })
            .copied()
            .collect::<Vec<_>>();
        for session_id in stale {
            self.agy_terminals.remove(&session_id);
        }
    }

    /// Retry TUI spawns that ran while provider detection was still in
    /// flight. Once detection settles, a still-missing `agy` is reported
    /// once and the session left for a manual relaunch.
    fn retry_pending_agy_spawns(&mut self, cx: &mut Context<Self>) {
        if self.agy_pending_spawns.is_empty() {
            return;
        }
        let detected = self.provider_detection_checked_at.is_some();
        let installed = self
            .probes
            .iter()
            .any(|probe| probe.provider == ProviderKind::Antigravity && probe.path.is_some());
        if !installed {
            if detected {
                self.agy_pending_spawns.clear();
                self.show_toast(tr!(
                    "errors.provider_not_found",
                    provider = ProviderKind::Antigravity.display_name()
                ));
            }
            return;
        }
        let pending: Vec<Uuid> = self.agy_pending_spawns.drain().collect();
        for session_id in pending {
            self.ensure_agy_terminal(session_id, cx);
        }
    }

    /// The Antigravity sessions a poll should cover: live terminals that
    /// still need their conversation id, and every session carrying a
    /// cursor, whose status row the sidebar mirrors.
    pub(super) fn maybe_poll_agy_sessions(&mut self, cx: &mut Context<Self>) {
        self.retry_pending_agy_spawns(cx);
        if self.agy_poll_pending {
            return;
        }
        let mut queries: Vec<AgyPollQuery> = Vec::new();
        for session in &self.state.sessions {
            if session.provider != ProviderKind::Antigravity || !session.has_started() {
                continue;
            }
            let cursor_id = match session.provider_cursor.as_ref() {
                Some(ProviderResumeCursor::Antigravity { conversation_id }) => {
                    Some(conversation_id.clone())
                }
                _ => None,
            };
            if cursor_id.is_none()
                && !self.agy_terminals.contains_key(&session.id)
                && !session.status.is_busy()
            {
                // No cursor, no live TUI, nothing claiming work — no row
                // could exist to correct the session's state.
                continue;
            }
            queries.push(AgyPollQuery {
                session_id: session.id,
                cwd: self
                    .workspace_path_for_session(session)
                    .map(Path::to_path_buf),
                // `updated_at` approximates spawn time for a session whose
                // cursor is still undiscovered; the exact spawn timestamp is
                // preferred while the terminal lives.
                seen_at: self
                    .agy_spawned_at
                    .get(&session.id)
                    .copied()
                    .unwrap_or(session.updated_at),
                cursor_id,
            });
        }
        if queries.is_empty() {
            return;
        }
        self.agy_poll_pending = true;
        let tx = self.agy_poll_tx.clone();
        let event_wake = self.event_wake_tx.clone();
        cx.background_executor()
            .spawn(async move {
                let mut update = AgyPollUpdate {
                    discovered: HashMap::new(),
                    summaries: HashMap::new(),
                };
                for query in &queries {
                    let Some(id) = query.cursor_id.clone().or_else(|| {
                        let cwd = query.cwd.as_deref()?;
                        agy::conversation_id_for_cwd(cwd, query.seen_at)
                            .or_else(|| agy::newest_conversation_for_cwd(cwd, query.seen_at))
                    }) else {
                        continue;
                    };
                    if query.cursor_id.is_none() {
                        update.discovered.insert(query.session_id, id.clone());
                    }
                }
                let ids: Vec<String> = queries
                    .iter()
                    .filter_map(|query| {
                        query
                            .cursor_id
                            .clone()
                            .or_else(|| update.discovered.get(&query.session_id).cloned())
                    })
                    .collect();
                update.summaries = agy::conversation_summaries(&ids);
                if tx.send(update).is_ok() {
                    signal_event_pump(&event_wake);
                }
            })
            .detach();
    }

    /// Apply the latest poll: capture newly discovered conversation ids and
    /// mirror status/title rows onto the session records. Runs inside the
    /// event pump drain like every other background result.
    pub(super) fn drain_agy_poll_events(&mut self) -> bool {
        let mut changed = false;
        let mut dirty = false;
        while let Ok(update) = self.agy_poll_events.try_recv() {
            self.agy_poll_pending = false;
            for (session_id, conversation_id) in update.discovered {
                let stale =
                    self.state.sessions.iter().any(|session| {
                        session.id == session_id && session.provider_cursor.is_some()
                    });
                if stale {
                    continue;
                }
                if let Some(session) = self.state.session_mut(session_id) {
                    session.provider_cursor =
                        Some(ProviderResumeCursor::Antigravity { conversation_id });
                    session.updated_at = unix_time();
                    dirty = true;
                }
            }
            let updates = self
                .state
                .sessions
                .iter()
                .filter(|session| session.provider == ProviderKind::Antigravity)
                .filter_map(|session| {
                    let Some(ProviderResumeCursor::Antigravity { conversation_id }) =
                        session.provider_cursor.as_ref()
                    else {
                        return None;
                    };
                    let summary = update.summaries.get(conversation_id)?;
                    let status = agy_session_status(summary);
                    let title = (!summary.title.is_empty()
                        && session.auto_title.as_deref() != Some(&summary.title))
                    .then(|| summary.title.clone());
                    (session.status != status || title.is_some())
                        .then(|| (session.id, status, title))
                })
                .collect::<Vec<_>>();
            for (session_id, status, title) in updates {
                if let Some(session) = self.state.session_mut(session_id) {
                    session.status = status;
                    session.updated_at = unix_time();
                    if let Some(title) = title {
                        session.auto_title = Some(title);
                    }
                    dirty = true;
                }
            }
        }
        changed |= dirty;
        if dirty {
            self.save();
        }
        changed
    }
}

/// What a spawned Antigravity terminal should run.
enum AgyLaunchKind {
    /// `agy -i "<prompt>"` — the first prompt of a new conversation.
    Prompt(String),
    /// `agy --conversation <id>` — reopen a captured conversation.
    Resume(String),
    /// A bare TUI, when no conversation id was ever captured.
    Fresh,
}

struct AgyPollQuery {
    session_id: Uuid,
    cwd: Option<PathBuf>,
    /// Earliest `last_modified_time` a summary row may carry and still be
    /// this spawn's conversation.
    seen_at: u64,
    cursor_id: Option<String>,
}
