use super::*;

impl Waku {
    pub(super) fn finish_streaming_assistant(&mut self, session_id: Uuid) {
        if let Some(session) = self.state.session_mut(session_id) {
            finish_streaming_messages(session);
        }
    }

    pub(super) fn append_text_delta(
        &mut self,
        session_id: Uuid,
        runtime: &mut SessionRuntime,
        delta: String,
    ) {
        let previous_phase = runtime.stream_phase;
        if previous_phase == Some(StreamPhase::Reasoning) {
            self.complete_reasoning_activity(session_id);
        }
        let continuing = previous_phase == Some(StreamPhase::Text);
        append_text_delta_to_session(&mut self.state.sessions, session_id, continuing, delta);
        self.state.mark_session_dirty(session_id);
        runtime.stream_phase = Some(StreamPhase::Text);
    }

    fn complete_reasoning_activity(&mut self, session_id: Uuid) {
        let Some(session) = self.state.session_mut(session_id) else {
            return;
        };
        let reasoning = session
            .transcript_blocks
            .iter_mut()
            .rev()
            .flat_map(|block| block.activities.iter_mut().rev())
            .find(|activity| activity.reasoning.is_some() && !activity.complete);
        if let Some(reasoning) = reasoning {
            reasoning.complete = true;
            session.updated_at = unix_time();
        }
    }

    pub(super) fn append_reasoning_delta(
        &mut self,
        session_id: Uuid,
        runtime: &mut SessionRuntime,
        delta: String,
    ) {
        let previous_phase = runtime.stream_phase;
        let continuing = previous_phase == Some(StreamPhase::Reasoning);
        if !continuing {
            runtime.pending_reasoning_newlines = 0;
            if delta.trim().is_empty() {
                return;
            }
            self.finish_streaming_assistant(session_id);
        }
        let now = unix_time_millis();
        if let Some(session) = self.state.session_mut(session_id) {
            if continuing
                && let Some(reasoning) = session
                    .transcript_blocks
                    .last_mut()
                    .and_then(|block| block.activities.last_mut())
                    .and_then(|activity| activity.reasoning.as_mut())
            {
                reasoning.content.push_str(&delta);
                reasoning.finished_at_ms = now;
            } else {
                push_transcript_activity(
                    session,
                    ActivityItem::from_reasoning(
                        ReasoningBlock {
                            // A folded batch can open with a collapsed
                            // paragraph break left over from a previous
                            // block's trailing newline run; a fresh block
                            // never starts on a break.
                            content: delta.trim_start_matches(['\n', '\r']).to_owned(),
                            started_at_ms: now,
                            finished_at_ms: now,
                        },
                        false,
                    ),
                    matches!(
                        previous_phase,
                        Some(StreamPhase::Reasoning | StreamPhase::Activity)
                    ),
                );
            }
            session.updated_at = unix_time();
        }
        runtime.stream_phase = Some(StreamPhase::Reasoning);
    }

    pub(super) fn update_activity(
        &mut self,
        session_id: Uuid,
        runtime: &mut SessionRuntime,
        item: ActivityItem,
    ) {
        let previous_phase = runtime.stream_phase;
        let continuing_work = matches!(
            previous_phase,
            Some(StreamPhase::Reasoning | StreamPhase::Activity)
        );
        let item = match self.state.session_mut(session_id) {
            Some(session) => match update_transcript_activity(session, item) {
                Ok((activity_id, replaces_changes)) => {
                    if replaces_changes {
                        // The rows this activity's diff was built from are
                        // gone; an expanded card rebuilds from the new ones.
                        self.activity_diffs.borrow_mut().remove(&activity_id);
                    }
                    // Progress on an already-anchored row is not a content
                    // boundary: providers interleave status heartbeats between
                    // token-level text chunks, and closing the streaming
                    // message here splices one provider message into
                    // fragments. Streaming phases keep their course.
                    if !matches!(
                        previous_phase,
                        Some(StreamPhase::Text | StreamPhase::Reasoning)
                    ) {
                        runtime.stream_phase = Some(StreamPhase::Activity);
                    }
                    return;
                }
                Err(item) => item,
            },
            None => item,
        };

        // A brand-new row is a real boundary: the provider moved on to its
        // next step, so any streaming text or reasoning ends here.
        if previous_phase == Some(StreamPhase::Text) {
            self.finish_streaming_assistant(session_id);
        }
        if previous_phase == Some(StreamPhase::Reasoning) {
            self.complete_reasoning_activity(session_id);
        }
        if let Some(session) = self.state.session_mut(session_id) {
            push_transcript_activity(session, item, continuing_work);
            session.updated_at = unix_time();
        }
        runtime.stream_phase = Some(StreamPhase::Activity);
    }

    pub(super) fn complete_turn_blocks(&mut self, session_id: Uuid) {
        if let Some(session) = self.state.session_mut(session_id) {
            complete_transcript_activities(session);
        }
    }

    pub(super) fn turn_has_assistant_message(&self, session_id: Uuid) -> bool {
        self.state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .is_some_and(|session| {
                let Some(turn_id) = session.active_turn_id() else {
                    return false;
                };
                session.messages.iter().any(|message| {
                    message.role == MessageRole::Assistant && message.turn_id == Some(turn_id)
                })
            })
    }

    /// The banner title for a session's OS notification: its display title,
    /// with the untitled placeholder swapped for the localized "New task".
    fn task_notification_title(&self, session_id: Uuid) -> Option<String> {
        let session = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;
        let title = session.display_title();
        Some(if title == AgentSession::DEFAULT_TITLE {
            tr!("session.new_task")
        } else {
            title.to_owned()
        })
    }

    /// An OS banner for a session blocked on the user — a permission or a
    /// question — gated by its own setting and the same "app in the
    /// background" rule as the finished-turn banner. The per-session tag
    /// replaces any earlier banner for the same task.
    fn notify_waiting_input(&self, session_id: Uuid, body: String, cx: &mut Context<Self>) {
        if !self.state.notify_waiting_input || cx.active_window().is_some() {
            return;
        }
        let Some(title) = self.task_notification_title(session_id) else {
            return;
        };
        crate::platform::show_task_notification(
            &task_notification_tag(session_id),
            &title,
            &body,
            cx,
        );
    }

    pub(super) fn accepts_turn_output(&mut self, session_id: Uuid) -> bool {
        // The turn begins at submission accept, before its prompt has reached
        // any provider. While preparation is still running, a reused runtime
        // could only be draining leftovers of a settled turn — output landing
        // in the new turn then would attribute stale text to it.
        if self.submission_preparations.contains(&session_id) {
            return false;
        }
        self.state
            .session_mut(session_id)
            .is_some_and(session_accepts_turn_output)
    }

    /// Returns whether the runtime should remain attached after this event.
    ///
    /// `allow_queue_drain` is false when the caller is flushing buffered
    /// events for a turn the user just stopped: a settling event must not
    /// start queued follow-ups then, because the user asked to stop, not to
    /// continue.
    pub(super) fn handle_driver_event(
        &mut self,
        session_id: Uuid,
        runtime: &mut SessionRuntime,
        event: DriverEvent,
        allow_queue_drain: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        runtime.last_active_at = Instant::now();
        // Keyed errors render in the client's locale, then flow through the
        // same handling as any opaque provider error.
        let event = match event {
            DriverEvent::LocalizedError { i18n, .. } => DriverEvent::Error(i18n.render()),
            event => event,
        };
        // The daemon owning this runtime restarted; its provider process is
        // gone. The exit arm below settles the session, but a resumable turn
        // gets one labeled continuation on a fresh runtime first.
        let runtime_lost = matches!(event, DriverEvent::RuntimeLost);
        match event {
            // Unreachable: normalized into `Error` above so it renders in the
            // client's locale before any dispatch runs.
            DriverEvent::LocalizedError { .. } => {}
            DriverEvent::RuntimeEventCursorAdvanced(cursor) => {
                if let Some(session) = self.state.session_mut(session_id) {
                    session.runtime_event_cursor = Some(cursor);
                }
            }
            DriverEvent::Connected { provider_cursor } => {
                runtime.last_driver_error = None;
                runtime.last_background_refresh_at = Instant::now();
                runtime.driver.refresh_background_work();
                if let Some(session) = self.state.session_mut(session_id) {
                    if let Some(ProviderResumeCursor::Claude {
                        resume_at: Some(message_id),
                        ..
                    }) = &provider_cursor
                    {
                        session.mark_active_turn_provider_resume_at(message_id.clone());
                    }
                    session.provider_cursor = provider_cursor;
                    if session.status == SessionStatus::Connecting {
                        session.status = SessionStatus::Working;
                    }
                }
            }
            DriverEvent::AgentPresetSelected(agent_preset) => {
                if let Some(session) = self.state.session_mut(session_id) {
                    session.agent_preset = agent_preset;
                }
            }
            DriverEvent::AutoTitleUpdated(title) => {
                let finished_turn = self.state.session_mut(session_id).and_then(|session| {
                    if !session.set_auto_title(title) || session.active_turn_id().is_some() {
                        return None;
                    }
                    session
                        .turns
                        .iter()
                        .rev()
                        .find(|turn| turn.status == TurnStatus::Completed)
                        .map(|turn| turn.id)
                });
                if let Some(turn_id) = finished_turn {
                    self.check_session_title_quality(session_id, Some(turn_id), None, cx);
                }
            }
            DriverEvent::AvailableCommands(names) => {
                if let Some(session) = self
                    .state
                    .session_mut(session_id)
                    .filter(|session| session.available_commands != names)
                {
                    session.available_commands = names;
                    // The drain has no `Context`; the frame loop rebuilds the
                    // drawn index when it sees this.
                    self.composer_sources_stale = true;
                }
            }
            DriverEvent::PromptSubmitted {
                message,
                turn_id,
                message_id,
                sent_by_task,
                hidden,
            } => {
                // A prompt reached this runtime: another client's submission,
                // or the echo of this one. The session decides whether that
                // is news; a mirrored turn is marked for the next save so the
                // projection this client persists carries the prompt whose
                // reply it is about to stream.
                if let Some(session) = self.state.session_mut(session_id)
                    && session.adopt_submitted_prompt(
                        &message,
                        turn_id,
                        message_id,
                        sent_by_task,
                        hidden,
                    )
                {
                    self.state.mark_session_dirty(session_id);
                }
            }
            DriverEvent::QueuedMessagesChanged { messages } => {
                // The daemon owns the agent-sourced slice of the follow-up
                // queue — a parked prompt appeared, delivered, or was
                // cancelled. Composer-queued entries pass through untouched.
                if let Some(session) = self.state.session_mut(session_id)
                    && session.merge_agent_queued(messages)
                {
                    self.state.mark_session_dirty(session_id);
                }
            }
            DriverEvent::TurnStarted => {
                runtime.last_driver_error = None;
                if let Some(session) = self.state.session_mut(session_id) {
                    if session.active_turn_id().is_some() {
                        // Covers submissions and the optimistic pursuit turn
                        // a `/goal` began: the provider's start confirms it.
                        session.mark_active_turn_provider_started();
                        session.status = SessionStatus::Working;
                    } else if matches!(
                        session.provider,
                        ProviderKind::Codex
                            | ProviderKind::Claude
                            | ProviderKind::OpenCode2
                            | ProviderKind::Muse
                    ) {
                        // Some providers start turns on their own: Codex goal
                        // continuation pursues an active goal whenever the
                        // thread is idle, Claude Code re-enters the model
                        // once a backgrounded command, subagent or monitor
                        // settles, and a Muse resume can attach mid-turn —
                        // or with a pending approval — before any local
                        // submission. Give the turn a transcript home — there
                        // is no user message for it — so its work streams in
                        // instead of being dropped.
                        session.begin_provider_turn();
                        session.mark_active_turn_provider_started();
                        session.status = SessionStatus::Working;
                        self.state.mark_session_dirty(session_id);
                    }
                }
            }
            DriverEvent::TurnParked => {
                // The reply ended while detached work the provider will wake
                // the session for is still running. The turn stays open for
                // that wake; only its streaming state settles, and the session
                // shows the wait instead of a finish. Parking never banners —
                // the wake's own settle (or an input request) is the
                // notify-worthy event.
                if self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .and_then(AgentSession::active_turn_id)
                    .is_none()
                {
                    return true;
                }
                self.settle_foreground_work(session_id, BackgroundWorkStatus::Completed);
                let previous_kinds = self.snapshot_selected_transcript_rows(session_id);
                self.finish_streaming_assistant(session_id);
                self.complete_turn_blocks(session_id);
                runtime.stream_phase = None;
                if let Some(session) = self.state.session_mut(session_id) {
                    session.status = SessionStatus::Background;
                    session.updated_at = unix_time();
                }
                if let Some(previous_kinds) = previous_kinds.as_deref() {
                    self.splice_active_transcript_rows_after_visibility_change(previous_kinds);
                }
            }
            DriverEvent::TextDelta(delta) => {
                if self.accepts_turn_output(session_id) {
                    self.append_text_delta(session_id, runtime, delta);
                }
            }
            DriverEvent::ReasoningDelta(delta) => {
                if self.accepts_turn_output(session_id) {
                    self.append_reasoning_delta(session_id, runtime, delta);
                }
            }
            DriverEvent::Activity {
                id,
                kind,
                title,
                detail,
                complete,
            } => {
                if self.accepts_turn_output(session_id) {
                    let refresh_branch = should_refresh_branch_after_activity(kind, complete)
                        && self.state.selected_session == Some(session_id);
                    let item = ActivityItem::new(id, kind, title, detail, complete);
                    self.observe_foreground_command_activity(session_id, &item);
                    self.note_phase_activity(session_id, &item, cx);
                    self.update_activity(session_id, runtime, item);
                    if refresh_branch {
                        self.refresh_selected_branch_snapshot(cx);
                    }
                }
            }
            DriverEvent::RichActivity(item) => {
                if self.accepts_turn_output(session_id) {
                    let refresh_branch =
                        should_refresh_branch_after_activity(item.kind, item.complete)
                            && self.state.selected_session == Some(session_id);
                    self.observe_foreground_command_activity(session_id, &item);
                    self.note_phase_activity(session_id, &item, cx);
                    self.update_activity(session_id, runtime, item);
                    if refresh_branch {
                        self.refresh_selected_branch_snapshot(cx);
                    }
                }
            }
            DriverEvent::BackgroundWork(event) => {
                // Background work is session state, not turn output. It must
                // survive a settled or rewound turn and therefore bypasses
                // `accepts_turn_output` deliberately.
                self.handle_background_work_event(session_id, event);
            }
            DriverEvent::ProjectMap(status) => {
                // `Sent` doubles as the transcript artifact: the row carries
                // the exact text the provider received. Status otherwise just
                // moves the composer chip.
                if let crate::model::ProjectMapStatus::Sent {
                    mapped_files,
                    estimated_tokens,
                    text,
                    ..
                } = &status
                    && self.accepts_turn_output(session_id)
                {
                    let item = ActivityItem::new(
                        Some("goddard-project-map".to_owned()),
                        ActivityKind::ProjectMap,
                        tr!(
                            "project_map.artifact_title",
                            files = *mapped_files,
                            tokens = *estimated_tokens
                        ),
                        Some(text.clone()),
                        true,
                    );
                    self.update_activity(session_id, runtime, item);
                }
                runtime.project_map = Some(status);
            }
            DriverEvent::SandboxSetup(status) => {
                // Launch progress is transient: `Ready` (or a fresh runtime)
                // clears it so the working indicator resumes its own label.
                runtime.sandbox_setup = match status {
                    crate::model::SandboxSetupStatus::Ready => None,
                    status => Some(status),
                };
            }
            DriverEvent::Permission {
                request_id,
                title,
                title_i18n,
                detail,
                detail_i18n,
                options,
            } => {
                if self.accepts_turn_output(session_id) {
                    let already_allowed =
                        waku_protocol::computer_use::ComputerApprovalId::decode(&request_id)
                            .and_then(|approval| approval.app_grant())
                            .is_some_and(|grant| {
                                self.state
                                    .computer_use_allowed_apps
                                    .iter()
                                    .any(|saved| saved.verified && saved.key() == grant.key())
                            });
                    if already_allowed && self.state.computer_use_enabled {
                        runtime.driver.respond(request_id, "task".into());
                    } else {
                        runtime.pending_permission = Some(PendingPermission {
                            request_id,
                            title,
                            title_i18n,
                            detail,
                            detail_i18n,
                            options,
                        });
                        if let Some(session) = self.state.session_mut(session_id) {
                            session.status = SessionStatus::Waiting;
                        }
                        self.notify_waiting_input(
                            session_id,
                            tr!("session.waiting_for_approval"),
                            cx,
                        );
                    }
                }
            }
            DriverEvent::UserInputRequested {
                request_id,
                questions,
            } => {
                if self.accepts_turn_output(session_id) && !questions.is_empty() {
                    runtime.pending_user_input = Some(PendingUserInput::new(request_id, questions));
                    if self.state.selected_session == Some(session_id) {
                        self.user_input_answer
                            .update(cx, |input, cx| input.clear(cx));
                    }
                    if let Some(session) = self.state.session_mut(session_id) {
                        session.status = SessionStatus::Waiting;
                    }
                    self.notify_waiting_input(session_id, tr!("session.waiting_for_answer"), cx);
                }
            }
            DriverEvent::ComputerUseUpdated(state) => {
                if self.accepts_turn_output(session_id) {
                    Self::upsert_computer_use_preview(session_id, runtime, state, cx);
                }
            }
            DriverEvent::SteerAccepted {
                message,
                sent_by_task,
                hidden,
            } => {
                // An accepted steer folds into the running turn; one that
                // lands after the turn ended — after Stop, say — would
                // otherwise append a loose user message to the settled
                // session.
                let accepts = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .is_some_and(session_accepts_steer_result);
                if !accepts {
                    return true;
                }
                if hidden {
                    // The daemon's context steer — provider-facing text, so
                    // it lands in the turn's record without a transcript row
                    // and never entered the composer's pending steers.
                    if let Some(session) = self.state.session_mut(session_id) {
                        session.push_hidden_user_message(message.clone());
                        // A project-move steer acknowledged while the turn
                        // was parked reached the provider — retire the
                        // pending notice so the next prompt does not repeat
                        // it. One acknowledged while the provider was still
                        // generating can sit in a volatile buffer and vanish
                        // with the turn, so the flag stays and the next
                        // prompt re-warns.
                        if session.status == SessionStatus::Background
                            && session.workspace_move.as_ref().is_some_and(|mv| {
                                AgentSession::workspace_move_notice_text(
                                    &mv.from,
                                    &mv.to,
                                    mv.project_switch,
                                    mv.cross_repo,
                                ) == message
                            })
                        {
                            session.workspace_move = None;
                        }
                        self.state.mark_session_dirty(session_id);
                    }
                    return true;
                }
                let submission = runtime
                    .pending_steers
                    .iter()
                    .position(|submission| submission.prompt == message)
                    .and_then(|index| runtime.pending_steers.remove(index))
                    // Providers normally echo the exact transport text, but a
                    // normalized echo still acknowledges the oldest pending
                    // steer. Preserve its attachment presentation metadata.
                    .or_else(|| runtime.pending_steers.pop_front())
                    .unwrap_or_else(|| ComposerSubmission::plain(message.clone()));
                // The provider folded the message into the live turn. Append
                // it to the same turn so the transcript mirrors the provider
                // conversation (no new turn boundary). Landing behind whatever
                // was still streaming closes that segment, so settle it first:
                // a group collapsing behind the new message must read "Ran",
                // not keep claiming aborted work is running for the rest of
                // the turn, and the next delta opens a fresh part on this side
                // of the boundary.
                let sent_message_id = if let Some(session) = self.state.session_mut(session_id) {
                    settle_stream_segment(session);
                    let message_id = session.push_user_message_with_presentation(
                        message,
                        submission.display_content,
                        submission.attachments,
                        submission.message_atoms,
                        sent_by_task,
                    );
                    session.updated_at = unix_time();
                    Some(message_id)
                } else {
                    None
                };
                if let Some(message_id) = sent_message_id {
                    self.record_sent_annotations(session_id, message_id, &submission.annotations);
                }
                runtime.stream_phase = None;
            }
            DriverEvent::SteerRejected {
                message,
                reason,
                reason_i18n,
                hidden,
            } => {
                // A hidden context steer is the daemon's own delivery — it
                // retries on the next prompt, so no toast and no queue entry.
                if hidden {
                    return true;
                }
                let reason = reason_i18n.map(|i18n| i18n.render()).unwrap_or(reason);
                let mut submission = runtime
                    .pending_steers
                    .iter()
                    .position(|submission| submission.prompt == message)
                    .and_then(|index| runtime.pending_steers.remove(index))
                    .or_else(|| runtime.pending_steers.pop_front())
                    .unwrap_or_else(|| ComposerSubmission::plain(message));
                let (busy, settled_cleanly) = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .map(|session| {
                        let settled_cleanly = session
                            .turns
                            .last()
                            .is_some_and(|turn| turn.status == TurnStatus::Completed);
                        (session.is_busy(), settled_cleanly)
                    })
                    .unwrap_or((false, false));
                if busy {
                    self.enqueue_follow_up_submission(session_id, submission, cx);
                    if self.state.selected_session == Some(session_id) {
                        self.show_toast(tr!(
                            "session.steer_rejected",
                            error = compact_driver_error(&reason)
                        ));
                    }
                } else if settled_cleanly {
                    // The turn settled before the steer arrived; run the
                    // message as a fresh turn instead of losing it. Submission
                    // is deferred through the queue-drain pass because this
                    // session's runtime is detached from the map while its
                    // events are handled — an inline submit would spawn a
                    // second driver process only to have it clobbered when the
                    // drain re-inserts the detached runtime.
                    if let Some(session) = self.state.session_mut(session_id) {
                        let annotations = std::mem::take(&mut submission.annotations);
                        let queued = submission.into_queued_message();
                        if !annotations.is_empty() {
                            self.queued_annotations.insert(queued.id, annotations);
                        }
                        session.queued_messages.insert(0, queued);
                    }
                    if allow_queue_drain {
                        self.pending_queue_drains.push(session_id);
                    }
                } else {
                    // The user stopped the turn (or the provider died) before
                    // the steer landed. Keep the message visible and
                    // user-controlled instead of auto-running it.
                    self.enqueue_follow_up_submission(session_id, submission, cx);
                }
            }
            DriverEvent::PlanUsageUpdated(usage) => {
                if let Some(provider) = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .map(|session| session.provider)
                {
                    // Codex's rolling `rateLimits/updated` snapshot never
                    // carries the reset-credit bank; keep the last HTTP
                    // read's count rather than blanking the row.
                    let mut usage = usage;
                    if usage.reset_credits.is_none()
                        && let Some(credits) = self
                            .plan_usage
                            .get(&provider)
                            .and_then(|plan| plan.reset_credits.clone())
                    {
                        usage.reset_credits = Some(credits);
                    }
                    self.plan_usage.insert(provider, usage);
                }
            }
            DriverEvent::GoalUpdated(goal) => {
                if self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .and_then(|session| session.thread_goal.as_ref())
                    .is_some_and(|goal| goal.managed_id.is_some())
                {
                    return true;
                }
                // Conversation meta like usage: it applies regardless of turn
                // state, and `None` means the provider cleared the goal.
                if goal.is_some() {
                    self.goal_observed_at.insert(session_id, Instant::now());
                } else {
                    self.goal_observed_at.remove(&session_id);
                }
                if let Some(session) = self.state.session_mut(session_id) {
                    if let Some(goal) = &goal
                        && session.messages.is_empty()
                    {
                        // A goal-first task is named after its objective
                        // until the provider reports a better title.
                        session.set_title_from_prompt(&goal.objective);
                    }
                    if session.thread_goal != goal {
                        session.thread_goal = goal;
                        self.state.mark_session_dirty(session_id);
                    }
                }
            }
            DriverEvent::UsageUpdated {
                context_tokens,
                context_window,
            } => {
                // Meta about the conversation, not turn output: it applies
                // even while a rewound or cancelled turn's tail drains.
                if let Some(session) = self.state.session_mut(session_id) {
                    let usage = session.context_usage.get_or_insert(ContextUsage::default());
                    if let Some(tokens) = context_tokens {
                        usage.tokens = tokens;
                    }
                    if let Some(window) = context_window {
                        usage.window = Some(window);
                    }
                    self.state.mark_session_dirty(session_id);
                }
            }
            DriverEvent::TurnFinished {
                success,
                summary,
                summary_i18n,
            } => {
                // The cancelled turn's settle is what frees a driver parked
                // inside its in-flight prompt: follow-ups queued behind the
                // drain can finally reach it. This clears before the
                // finished-turn early return — that turn was already closed
                // by the cancel itself.
                if allow_queue_drain && self.cancel_drains.remove(&session_id).is_some() {
                    self.pending_queue_drains.push(session_id);
                }
                self.settle_foreground_work(
                    session_id,
                    if success {
                        BackgroundWorkStatus::Completed
                    } else {
                        BackgroundWorkStatus::Failed
                    },
                );
                let previous_kinds = self.snapshot_selected_transcript_rows(session_id);
                runtime.last_driver_error = None;
                // A settled turn moved the account's rate-limit needles; ask
                // that provider's plan meter to refresh once its backoff
                // allows.
                if let Some(provider) = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .map(|session| session.provider)
                    .filter(|provider| usage_meter::PLAN_USAGE_PROVIDERS.contains(provider))
                {
                    self.plan_usage_stale.insert(provider);
                }
                let finished_turn_id = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .and_then(AgentSession::active_turn_id);
                if finished_turn_id.is_none() {
                    return true;
                }
                let task_notification = (self.state.notify_turn_finished
                    && cx.active_window().is_none())
                .then(|| {
                    self.task_notification_title(session_id).map(|title| {
                        let body = if success {
                            tr!("session.turn_completed")
                        } else {
                            tr!("session.stopped")
                        };
                        (title, body)
                    })
                })
                .flatten();
                self.finish_streaming_assistant(session_id);
                self.complete_turn_blocks(session_id);
                runtime.stream_phase = None;
                let needs_fallback = !self.turn_has_assistant_message(session_id);
                if let Some(session) = self.state.session_mut(session_id) {
                    session.status = if success {
                        SessionStatus::Idle
                    } else {
                        SessionStatus::Failed
                    };
                    if needs_fallback {
                        let kind = if success {
                            TranscriptNoticeStatus::Completed
                        } else {
                            match summary_i18n.as_ref().map(|i18n| i18n.key.as_str()) {
                                Some("session.agent_ran_out_of_context") => {
                                    TranscriptNoticeStatus::OutOfContext
                                }
                                Some("session.agent_declined_turn") => {
                                    TranscriptNoticeStatus::Declined
                                }
                                Some("session.agent_stopped_reason") => {
                                    TranscriptNoticeStatus::StoppedWithReason
                                }
                                Some(_) => TranscriptNoticeStatus::Error,
                                None if summary.is_some() => TranscriptNoticeStatus::Error,
                                None => TranscriptNoticeStatus::StoppedBeforeResponse,
                            }
                        };
                        session.push_notice_message(
                            MessageRole::Assistant,
                            summary_i18n
                                .as_ref()
                                .map(|i18n| i18n.render())
                                .or_else(|| summary.clone())
                                .unwrap_or_else(|| {
                                    if success {
                                        tr!("session.turn_completed")
                                    } else {
                                        tr!("session.stopped_before_response")
                                    }
                                }),
                            TranscriptNotice::Status { kind },
                        );
                    }
                }
                self.finish_active_turn_with_analytics(
                    session_id,
                    if success {
                        TurnStatus::Completed
                    } else {
                        TurnStatus::Failed
                    },
                    if success {
                        crate::analytics::TurnOutcome::Completed
                    } else {
                        crate::analytics::TurnOutcome::Failed
                    },
                );
                runtime.pending_permission = None;
                runtime.pending_user_input = None;
                runtime.pending_computer_approval = None;
                runtime.driver.cancel_computer_use();
                // The agent may have edited files or switched branches, so the
                // cached view of the workspace is no longer trustworthy. This
                // handler has no `Context`, so the drain loop acts on the flag.
                if self.state.selected_session == Some(session_id) {
                    self.workspace_queries_stale = true;
                }
                // A settled turn is when an agent's commits land — including
                // commits it made on the user's behalf — so the sidebar's git
                // rows re-scan and this session's checkout answers drop even
                // while another session is selected.
                self.invalidate_session_workspace_queries(session_id);
                runtime.computer_use_previews.clear();
                runtime.driver.refresh_background_work();
                self.capture_latest_turn_checkpoint_for(session_id);
                // A completed turn clears the restart-resume bound: only
                // consecutive resume losses should ever stop auto-resuming.
                if success {
                    self.runtime_auto_resumes.remove(&session_id);
                }
                // A natural end is the only finish the status-marker eval
                // scores; failed and interrupted turns keep their own status.
                if success {
                    self.note_turn_finished_for_status_markers(
                        session_id,
                        finished_turn_id,
                        summary.clone(),
                        cx,
                    );
                    self.note_turn_finished_for_action_predictions(
                        session_id,
                        finished_turn_id,
                        summary.clone(),
                        cx,
                    );
                    self.note_turn_finished_for_phase_eval(
                        session_id,
                        finished_turn_id,
                        summary.clone(),
                        cx,
                    );
                    self.check_session_title_quality(
                        session_id,
                        finished_turn_id,
                        summary.clone(),
                        cx,
                    );
                }
                if let Some(turn_id) = finished_turn_id {
                    self.note_managed_goal_settle(session_id, turn_id, success, cx);
                }
                if allow_queue_drain && success {
                    // Start the next queued follow-up once the runtime has
                    // been re-inserted so the same process is reused.
                    self.pending_queue_drains.push(session_id);
                }
                if let Some(previous_kinds) = previous_kinds.as_deref() {
                    self.splice_active_transcript_rows_after_visibility_change(previous_kinds);
                }
                // The selected session's finish is already on screen; only a
                // turn settling out of view gets the sound. A queued follow-up
                // means the task keeps working, so that settle stays quiet.
                let finished = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id);
                if self.state.completion_sound_enabled
                    && self.state.selected_session != Some(session_id)
                    && finished.is_some_and(|session| session.queued_messages.is_empty())
                {
                    // A starred project's finish plays its own sound unless
                    // the user disabled the override.
                    let starred = finished.is_some_and(|session| {
                        self.state
                            .projects
                            .iter()
                            .any(|project| project.id == session.project_id && project.starred)
                    });
                    let sound = if starred && self.state.starred_completion_sound {
                        waku_client::persistence::CompletionSound::Crystal
                    } else {
                        self.state.completion_sound
                    };
                    crate::platform::play_completion_sound(
                        sound,
                        self.state.completion_sound_volume,
                    );
                }
                // The briefing's clip builds while the task is still
                // unread, so landing on it plays instantly rather than
                // waiting on both gateway calls.
                self.prefetch_voice_brief(session_id, cx);
                if let Some((title, body)) = task_notification {
                    crate::platform::show_task_notification(
                        &task_notification_tag(session_id),
                        &title,
                        &body,
                        cx,
                    );
                }
                // A project switch that landed mid-turn retires the driver
                // now that the turn settled: the next turn spawns in the
                // new project root instead of reusing a process still
                // rooted in the old one.
                if self.project_switch_reset_pending.remove(&session_id) {
                    runtime.driver.cancel();
                    runtime.driver.close();
                    self.mark_background_work_lost(session_id);
                    return false;
                }
            }
            DriverEvent::Error(error) => {
                let error = compact_driver_error(&error);
                runtime.last_driver_error = Some(error.clone());
                if self.state.selected_session == Some(session_id) {
                    self.show_toast(error.clone());
                }
                // An optimistic pursuit turn has no submission to fail with.
                // Unwind it so the error cannot strand a spinner; if the
                // pursuit does start later, its own start report recreates
                // the turn.
                self.unwind_unconfirmed_pursuit_turn(session_id);
                let has_active_turn = self
                    .state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .and_then(AgentSession::active_turn_id)
                    .is_some();
                let should_append = has_active_turn
                    && !self.turn_has_assistant_message(session_id)
                    && self
                        .state
                        .sessions
                        .iter()
                        .find(|session| session.id == session_id)
                        .is_some_and(|session| session.status != SessionStatus::Working);
                if let Some(session) = self.state.session_mut(session_id)
                    && has_active_turn
                {
                    if session.status != SessionStatus::Working {
                        session.status = SessionStatus::Failed;
                    }
                    if should_append {
                        session.push_notice_message(
                            MessageRole::Assistant,
                            error,
                            TranscriptNotice::Status {
                                kind: TranscriptNoticeStatus::Error,
                            },
                        );
                    }
                }
            }
            DriverEvent::RuntimeLost | DriverEvent::ProcessExited => {
                // The parked driver died mid-drain: queued follow-ups go to
                // the fresh runtime the next submission spawns.
                if allow_queue_drain && self.cancel_drains.remove(&session_id).is_some() {
                    self.pending_queue_drains.push(session_id);
                }
                // The runtime a pending project-switch reset would drop is
                // already gone.
                self.project_switch_reset_pending.remove(&session_id);
                self.mark_background_work_lost(session_id);
                let previous_kinds = self.snapshot_selected_transcript_rows(session_id);
                self.finish_streaming_assistant(session_id);
                self.complete_turn_blocks(session_id);
                runtime.stream_phase = None;
                runtime.pending_permission = None;
                runtime.pending_user_input = None;
                runtime.pending_computer_approval = None;
                runtime.driver.cancel_computer_use();
                runtime.computer_use_previews.clear();
                if runtime_lost {
                    // A resumable turn moves onto a fresh runtime; the dead
                    // handle still leaves the map via `false` below.
                    if self.resume_lost_runtime(session_id, cx) {
                        if let Some(previous_kinds) = previous_kinds.as_deref() {
                            self.splice_active_transcript_rows_after_visibility_change(
                                previous_kinds,
                            );
                        }
                        return false;
                    }
                    // Not resumable — report the daemon restart, not a bare
                    // provider exit, as the turn's failure.
                    runtime.last_driver_error = Some(
                        "the Goddard daemon restarted and this turn could not be reattached"
                            .to_owned(),
                    );
                }
                let needs_fallback = !self.turn_has_assistant_message(session_id);
                let failure_kind = if runtime.last_driver_error.is_some() {
                    TranscriptNoticeStatus::Error
                } else {
                    TranscriptNoticeStatus::Exited
                };
                let failure_message = runtime
                    .last_driver_error
                    .take()
                    .unwrap_or_else(|| tr!("session.codex_exited_before_response"));
                let should_finish_turn = if let Some(session) = self.state.session_mut(session_id)
                    && session.status.is_busy()
                {
                    session.status = SessionStatus::Failed;
                    session.updated_at = unix_time();
                    if needs_fallback {
                        session.push_notice_message(
                            MessageRole::Assistant,
                            failure_message,
                            TranscriptNotice::Status { kind: failure_kind },
                        );
                    }
                    true
                } else {
                    false
                };
                let finished_turn = should_finish_turn
                    && self
                        .finish_active_turn_with_analytics(
                            session_id,
                            TurnStatus::Failed,
                            crate::analytics::TurnOutcome::ProcessExited,
                        )
                        .is_some();
                if finished_turn {
                    self.capture_latest_turn_checkpoint_for(session_id);
                    self.invalidate_session_workspace_queries(session_id);
                }
                if let Some(previous_kinds) = previous_kinds.as_deref() {
                    self.splice_active_transcript_rows_after_visibility_change(previous_kinds);
                }
                return false;
            }
        }
        true
    }

    fn upsert_computer_use_preview(
        session_id: Uuid,
        runtime: &mut SessionRuntime,
        state: ComputerUseState,
        cx: &mut Context<Self>,
    ) {
        if !state.visible {
            return;
        }
        let Some(window_id) = state.target.as_ref().map(|target| target.window_id) else {
            return;
        };
        let mut preview = if let Some(index) =
            runtime.computer_use_previews.iter().position(|preview| {
                preview
                    .target
                    .as_ref()
                    .is_some_and(|target| target.window_id == window_id)
            }) {
            if !runtime.computer_use_previews[index].visible {
                return;
            }
            runtime.computer_use_previews.remove(index)
        } else {
            ComputerUsePreview {
                target: None,
                phase: state.phase,
                visible: state.visible,
                frames: Default::default(),
                decode_task: None,
            }
        };
        preview.target = state.target;
        preview.phase = state.phase;
        preview.visible = state.visible;
        if let Some(image_url) = state.image_url {
            let generation = preview.frames.begin();
            // Dropping the prior task also prevents a dismissed/recreated
            // window or replaced runtime from receiving its stale completion.
            preview.decode_task = None;
            let renderer = cx.svg_renderer();
            let current_source = preview.frames.current.as_ref().map(|frame| frame.source_id);
            let decode = cx.background_executor().spawn(async move {
                crate::computer_use::decode_preview_image_url(&image_url, renderer, current_source)
                    .ok()
                    .flatten()
            });
            preview.decode_task = Some(cx.spawn(async move |this, cx| {
                let image = decode.await;
                let _ = this.update(cx, |this, cx| {
                    let Some(preview) = this.runtimes.get_mut(&session_id).and_then(|runtime| {
                        runtime.computer_use_previews.iter_mut().find(|preview| {
                            preview
                                .target
                                .as_ref()
                                .is_some_and(|target| target.window_id == window_id)
                        })
                    }) else {
                        return;
                    };
                    let image = image.map(|(source_id, image)| {
                        crate::computer_use::PreviewImage::new(source_id, image, cx)
                    });
                    if preview.frames.complete(generation, image) {
                        cx.notify();
                    }
                });
            }));
        }
        runtime.computer_use_previews.push(preview);
    }
}

/// Foreground output is stronger evidence of a started provider turn than a
/// replayed lifecycle cursor. Repair both pieces of transient state here so a
/// runtime attachment that missed `TurnStarted` cannot leave Cmd-Enter
/// permanently falling back to the follow-up queue while output is visible.
pub(super) fn session_accepts_turn_output(session: &mut AgentSession) -> bool {
    if session.active_turn_id().is_none() || !session.status.is_busy() {
        return false;
    }
    session.mark_active_turn_provider_started();
    if session.status == SessionStatus::Connecting {
        session.status = SessionStatus::Working;
    }
    true
}

/// A steer can only fold into a turn that is still running; after it ends —
/// completed, interrupted, or stopped — a late acceptance is a straggler
/// that must not inject a message into the settled session.
pub(super) fn session_accepts_steer_result(session: &AgentSession) -> bool {
    session.active_turn_id().is_some()
}

/// A completed edit or shell command is the earliest provider-neutral point at
/// which its filesystem effects are stable enough to re-read. The actual Git
/// work remains behind the branch cache's background fetch.
pub(super) fn should_refresh_branch_after_activity(
    kind: crate::model::ActivityKind,
    complete: bool,
) -> bool {
    complete
        && matches!(
            kind,
            crate::model::ActivityKind::Command | crate::model::ActivityKind::FileChange
        )
}

fn finish_streaming_messages(session: &mut AgentSession) {
    for message in &mut session.messages {
        if message.role == MessageRole::Assistant && message.streaming {
            message.streaming = false;
        }
    }
}

fn complete_transcript_activities(session: &mut AgentSession) {
    for block in &mut session.transcript_blocks {
        for activity in &mut block.activities {
            activity.complete = true;
        }
    }
}

/// Close the stream segment a mid-turn message boundary cuts off: open text
/// stops streaming and in-flight thinking or tool work counts as finished.
/// A provider that kept a call alive reconciles it — the next update for its
/// source id writes `complete` again.
pub(super) fn settle_stream_segment(session: &mut AgentSession) {
    finish_streaming_messages(session);
    complete_transcript_activities(session);
}

/// Write `item` into the activity row it reports on. `Ok((activity_id,
/// replaces_changes))` when an existing row matched — progress on work the
/// transcript already shows — or `Err(item)` to hand the item back for a
/// fresh row.
pub(super) fn update_transcript_activity(
    session: &mut AgentSession,
    item: ActivityItem,
) -> Result<(Uuid, bool), ActivityItem> {
    for block in session.transcript_blocks.iter_mut().rev() {
        let matching = block.activities.iter_mut().rev().find(|activity| {
            item.source_id
                .as_ref()
                .is_some_and(|id| activity.source_id.as_ref() == Some(id))
                || (item.source_id.is_none() && activity.title == item.title && !activity.complete)
        });
        if let Some(activity) = matching {
            let has_arguments = item.arguments.is_some();
            let replaces_changes = !item.file_changes.is_empty();
            let activity_id = activity.id;
            activity.kind = item.kind;
            activity.title = item.title;
            if item.tool_name.is_some() {
                activity.tool_name = item.tool_name;
            }
            if item.mcp_server.is_some() {
                activity.mcp_server = item.mcp_server;
            }
            activity.complete = item.complete;
            activity.failed = item.failed;
            if item.detail.is_some() {
                activity.detail = item.detail;
            }
            if item.arguments.is_some() {
                activity.arguments = item.arguments;
            }
            if item.output.is_some() {
                activity.output = item.output;
            }
            if !item.image_urls.is_empty() {
                activity.image_urls = item.image_urls;
            }
            if !item.file_changes.is_empty() {
                activity.file_changes = item.file_changes;
            }
            if item.display_target.is_some() && (activity.display_target.is_none() || has_arguments)
            {
                activity.display_target = item.display_target;
            }
            if item.display_description.is_some()
                && (activity.display_description.is_none() || has_arguments)
            {
                activity.display_description = item.display_description;
            }
            if item.reasoning.is_some() {
                activity.reasoning = item.reasoning;
            }
            session.updated_at = unix_time();
            return Ok((activity_id, replaces_changes));
        }
    }
    Err(item)
}

pub(super) fn push_transcript_activity(
    session: &mut AgentSession,
    item: ActivityItem,
    continuing_work: bool,
) {
    let after_message = session.messages.len();
    let turn_id = session.active_turn_id();
    if continuing_work
        && let Some(block) = session.transcript_blocks.last_mut()
        && block.after_message == after_message
        && block.turn_id == turn_id
    {
        block.activities.push(item);
    } else {
        session.transcript_blocks.push(TranscriptBlock {
            after_message,
            turn_id,
            activities: vec![item],
        });
    }
}

pub(super) fn stream_delta_kind(event: &DriverEvent) -> Option<StreamDeltaKind> {
    match event {
        DriverEvent::TextDelta(_) => Some(StreamDeltaKind::Text),
        DriverEvent::ReasoningDelta(_) => Some(StreamDeltaKind::Reasoning),
        _ => None,
    }
}

pub(super) fn stream_delta_text(event: &DriverEvent, kind: StreamDeltaKind) -> Option<&str> {
    match (kind, event) {
        (StreamDeltaKind::Text, DriverEvent::TextDelta(text))
        | (StreamDeltaKind::Reasoning, DriverEvent::ReasoningDelta(text)) => Some(text),
        _ => None,
    }
}

pub(super) fn compact_driver_error(error: &str) -> String {
    const MAX_LINES: usize = 6;
    const MAX_CHARS: usize = 800;

    let lines = error.lines().collect::<Vec<_>>();
    let mut compact = lines
        .iter()
        .take(MAX_LINES)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if lines.len() > MAX_LINES {
        compact.push_str("\n…");
    }
    if compact.chars().count() > MAX_CHARS {
        compact = compact.chars().take(MAX_CHARS - 1).collect();
        compact.push('…');
    }
    compact
}

/// Coalesce every adjacent delta of one kind while retaining provider order.
/// Runtime cursors are acknowledgements rather than visible boundaries, so the
/// newest cursor follows the combined delta. The full text enters layout in
/// this pass; Markdown's paint-only veil provides the progressive dissolve.
pub(super) fn pop_stream_batch(
    events: &mut VecDeque<DriverEvent>,
    kind: StreamDeltaKind,
    pending_reasoning_newlines: &mut usize,
) -> Option<DriverEvent> {
    let mut chunk = String::new();
    let mut latest_cursor = None;
    loop {
        match events.front() {
            Some(DriverEvent::RuntimeEventCursorAdvanced(_)) => {
                latest_cursor = events.pop_front();
            }
            Some(event) if stream_delta_text(event, kind).is_some() => {
                let event = events.pop_front()?;
                match (kind, event) {
                    (StreamDeltaKind::Text, DriverEvent::TextDelta(text)) => {
                        chunk.push_str(&text);
                    }
                    (StreamDeltaKind::Reasoning, DriverEvent::ReasoningDelta(text)) => {
                        push_reasoning_delta(&mut chunk, pending_reasoning_newlines, &text);
                    }
                    _ => unreachable!("the stream kind was checked before removing the event"),
                }
            }
            _ => break,
        }
    }
    if let Some(cursor) = latest_cursor {
        events.push_front(cursor);
    }
    match kind {
        StreamDeltaKind::Text => Some(DriverEvent::TextDelta(chunk)),
        StreamDeltaKind::Reasoning => Some(DriverEvent::ReasoningDelta(chunk)),
    }
}

/// Newline count when a reasoning delta is nothing but line breaks, e.g. the
/// `"\n"` chunks GLM via OpenRouter interleaves between every text chunk.
/// Mixed deltas keep their interior newlines untouched.
fn reasoning_delta_newlines(delta: &str) -> Option<usize> {
    delta
        .bytes()
        .all(|byte| matches!(byte, b'\n' | b'\r'))
        .then(|| delta.bytes().filter(|byte| *byte == b'\n').count())
}

/// Fold one reasoning delta into a batch's chunk. Newline-only deltas are
/// buffered rather than appended — a lone one is dropped and a run collapses
/// to a single paragraph break once real text resumes — so providers that
/// emit line-delimited reasoning don't render one token per line. A run left
/// pending when the batch ends carries into the next one.
pub(super) fn push_reasoning_delta(
    content: &mut String,
    pending_newlines: &mut usize,
    delta: &str,
) {
    if let Some(newlines) = reasoning_delta_newlines(delta) {
        *pending_newlines += newlines;
        return;
    }
    if *pending_newlines >= 2 {
        content.push_str("\n\n");
    }
    *pending_newlines = 0;
    content.push_str(delta);
}

pub(super) fn append_text_delta_to_session(
    sessions: &mut [AgentSession],
    session_id: Uuid,
    continuing: bool,
    delta: String,
) {
    let Some(session) = sessions.iter_mut().find(|session| session.id == session_id) else {
        return;
    };
    if !continuing {
        for message in &mut session.messages {
            if message.role == MessageRole::Assistant && message.streaming {
                message.streaming = false;
            }
        }
    }
    let existing = if continuing {
        session
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.role == MessageRole::Assistant && message.streaming)
    } else {
        // An interleaved activity is not a boundary in the provider's own
        // message: it dispatches tool calls wherever a sentence happens to
        // be, and its transcript keeps the text as one. Rejoin the last
        // assistant text when it clearly ended mid-thought and the delta
        // reads as its continuation — a completed sentence still opens a
        // fresh row.
        let turn_id = session.active_turn_id();
        session.messages.last_mut().filter(|message| {
            message.role == MessageRole::Assistant
                && turn_id.is_some_and(|id| message.turn_id == Some(id))
                && message.notice.is_none()
                && !message.hidden
                && text_delta_rejoins(&message.content, &delta)
        })
    };
    if let Some(message) = existing {
        message.content.push_str(&delta);
        message.streaming = true;
    } else {
        let mut message = session
            .active_turn_id()
            .map(|turn_id| Message::new_for_turn(MessageRole::Assistant, delta.clone(), turn_id))
            .unwrap_or_else(|| Message::new(MessageRole::Assistant, delta));
        message.streaming = true;
        session.messages.push(message);
    }
    session.updated_at = unix_time();
}

/// Whether a text delta resumes a message a mid-stream boundary cut off.
/// Providers emit token-granular chunks and dispatch tool calls wherever a
/// sentence happens to be, so a split lands mid-thought while their own
/// transcript keeps the text as one message. `previous` must read as
/// unfinished — no terminal punctuation — and `delta` must not open a fresh
/// sentence.
fn text_delta_rejoins(previous: &str, delta: &str) -> bool {
    let ends_mid_thought = previous
        .trim_end()
        .chars()
        .next_back()
        .is_some_and(|c| !matches!(c, '.' | '!' | '?' | ':' | '…'));
    let opens_fresh = delta.chars().next().is_some_and(char::is_uppercase);
    ends_mid_thought && !opens_fresh
}
