use anyhow::ensure;

use super::*;

impl WakuBackend {
    pub(super) fn switch_session_provider(
        &self,
        session_id: Uuid,
        request: waku_protocol::ProviderSwitchRequest,
        events: &EventSink,
    ) -> anyhow::Result<AgentSession> {
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let _owner = lifecycle.lock();
        ensure!(
            self.boss.employee_including_retired(session_id).is_none(),
            "employee provider changes are handled by the Boss"
        );
        let mut state = self.task_state.lock();
        let index = state
            .sessions
            .iter()
            .position(|session| session.id == session_id)
            .context("the task is unavailable")?;
        let existing = &mut state.sessions[index];
        self.task_store.hydrate(existing)?;
        ensure!(!existing.is_busy(), "the task is busy");
        ensure!(existing.archived_at.is_none(), "the task is archived");
        ensure!(
            existing.provider == request.source
                && existing.history_generation == request.history_generation
                && existing.transcript_boundary() == request.boundary,
            "the task changed while the provider handoff was prepared"
        );
        let target = request.configuration.provider;
        ensure!(
            target != request.source,
            "the task already uses this provider"
        );
        let mut switched = existing.clone();
        let suspended = &mut switched.suspended_provider_sessions;
        let target_entry = suspended.iter().find(|entry| entry.provider == target);
        if let Some(cursor) = &request.resume_cursor {
            ensure!(
                cursor.provider() == target
                    && target_entry.is_some_and(|entry| &entry.cursor == cursor),
                "the target provider's resume cursor changed"
            );
        }
        let restarted = target_entry.is_some() && request.resume_cursor.is_none();
        suspended.retain(|entry| entry.provider != target);
        if let Some(cursor) = switched.provider_cursor.take()
            && cursor.provider() != target
        {
            let provider = cursor.provider();
            suspended.retain(|entry| entry.provider != provider);
            suspended.push(crate::model::SuspendedProviderSession {
                provider,
                cursor,
                boundary: request.boundary,
            });
        }
        switched.preserve_title_for_provider_switch();
        waku_protocol::task_metadata::TaskMetadataEdit::Configuration {
            configuration: request.configuration,
            auto_route: false,
            route_decision: None,
        }
        .apply(&mut switched);
        switched.provider_cursor = request.resume_cursor;
        switched.provider_session_id = None;
        switched.pending_model_switch = None;
        switched.agent_preset = None;
        switched.available_commands.clear();
        switched.context_usage = None;
        switched.runtime_event_cursor = None;
        switched.pending_provider_context = Some(request.handoff);
        for message in &mut switched.queued_messages {
            if !message.is_agent_owned() {
                message.configuration = None;
            }
        }
        let from = request.source.display_name();
        let to = target.display_name();
        switched.push_notice_message(
            crate::model::MessageRole::System,
            if restarted {
                tr!(
                    "transcript.provider_switched_restarted",
                    from = from,
                    to = to
                )
            } else {
                tr!("transcript.provider_switched", from = from, to = to)
            },
            crate::model::TranscriptNotice::ProviderSwitched {
                from: request.source,
                to: target,
                restarted,
            },
        );
        switched.updated_at = switched.updated_at.max(crate::model::unix_time());
        switched.metadata_revision = switched
            .metadata_revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .context("task metadata revision exhausted")?;

        // A surviving provider host needs a confirmed stop before it can
        // admit a different provider under the same task.
        if self.task_store.stable_link(session_id)?.is_some() {
            let driver = self
                .sessions
                .lock()
                .get(&session_id)
                .filter(|entry| entry.stable.is_some())
                .map(|entry| entry.driver.clone())
                .context("the surviving provider runtime must be attached before switching")?;
            driver.delete_provider_session();
            ensure!(
                self.task_store.stable_link(session_id)?.is_none(),
                "the provider runtime did not confirm it stopped"
            );
        }

        let previous = std::mem::replace(existing, switched);
        state.mark_session_dirty(session_id);
        if let Err(error) = self.task_store.save(&mut state) {
            state.sessions[index] = previous;
            return Err(error).context("could not save the provider switch");
        }
        let switched = &state.sessions[index];
        let mut reply = switched.list_projection();
        reply.provider_cursor.clone_from(&switched.provider_cursor);
        reply
            .suspended_provider_sessions
            .clone_from(&switched.suspended_provider_sessions);
        reply
            .pending_provider_context
            .clone_from(&switched.pending_provider_context);
        reply.messages.extend(switched.messages.last().cloned());
        drop(state);

        let removed = self.remove_runtime(session_id, None, false);
        if let Some(entry) = &removed {
            let sink = events.for_session(session_id, entry.runtime_id);
            sink.notify_runtime_ended();
            sink.end_session_runtime();
        }
        drop_detached(removed);
        Ok(reply)
    }

    /// Fork a response using only daemon-host state.
    ///
    /// A browser must never reconstruct or persist this operation itself:
    /// provider-native sessions, checkpoint refs, and the task database all
    /// belong to the daemon and may be on another machine.
    pub(super) fn fork_session_from_response(
        &self,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<(AgentSession, Option<String>)> {
        let (source, cwd, fork_title) = {
            let mut state = self.task_state.lock();
            let source_index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("the source task is unavailable"))?;
            self.task_store
                .hydrate(&mut state.sessions[source_index])
                .context("could not load the source task")?;
            let source = state.sessions[source_index].clone();
            let project = state
                .projects
                .iter()
                .find(|project| project.id == source.project_id)
                .ok_or_else(|| anyhow!("the source task project is unavailable"))?;
            let cwd = source.workspace.path().unwrap_or(&project.path).to_owned();
            let fork_title = next_response_fork_title(
                source.display_title(),
                state
                    .sessions
                    .iter()
                    .filter(|session| session.project_id == source.project_id)
                    .map(AgentSession::display_title),
            );
            (source, cwd, fork_title)
        };

        validate_response_fork(&source, turn_count)?;
        let provider_turn_count = source
            .turns
            .iter()
            .take(turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let turns_to_remove = source.provider_turns_after(turn_count);
        let (provider_cursor, message_ids) = self.fork_provider_response(
            &source,
            &cwd,
            &fork_title,
            turn_count,
            provider_turn_count,
            turns_to_remove,
        )?;
        let mut forked = source
            .fork_through_turn(turn_count, provider_cursor, &fork_title)
            .ok_or_else(|| anyhow!("the selected response cannot be copied"))?;
        if !message_ids.is_empty() {
            for turn in &mut forked.turns {
                if let Some(message_id) = turn.provider_resume_at.as_mut()
                    && let Some(remapped) = message_ids.get(message_id)
                {
                    *message_id = remapped.clone();
                }
            }
        }

        let fork_id = forked.id;
        for turn in &mut forked.turns {
            if let Some(checkpoint) = turn.checkpoint.as_mut() {
                checkpoint.git_ref =
                    crate::checkpoint::checkpoint_ref(fork_id, checkpoint.turn_count);
            }
        }
        let checkpoint_warning =
            crate::checkpoint::copy_session_refs(&cwd, source.id, fork_id, turn_count)
                .err()
                .map(|error| error.to_string());

        let pinned = self.sessions.lock().keys().copied().collect();
        let mut state = self.task_state.lock();
        state.push_session(forked.clone());
        if let Err(error) = self.task_store.save(&mut state) {
            state.sessions.retain(|session| session.id != fork_id);
            let _ = crate::checkpoint::delete_all_session_refs(&cwd, fork_id);
            return Err(error).context("could not save the forked task");
        }
        trim_resident_transcripts(&mut state, &pinned);
        Ok((forked, checkpoint_warning))
    }

    /// Restore the daemon-host worktree, provider conversation, and stored
    /// transcript to immediately before one user turn.
    pub(super) fn rewind_session_to_message(
        &self,
        session_id: Uuid,
        turn_count: usize,
        runtime_id: Uuid,
        cancellation: &crate::server::RuntimeCancellation,
    ) -> anyhow::Result<(AgentSession, Option<String>)> {
        let lifecycle = self.runtime_lifecycles.session(session_id);
        let _owner = lifecycle.lock();
        let (source, cwd) = {
            let mut state = self.task_state.lock();
            let source_index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("the task is unavailable"))?;
            self.task_store
                .hydrate(&mut state.sessions[source_index])
                .context("could not load the task")?;
            let source = state.sessions[source_index].clone();
            let project = state
                .projects
                .iter()
                .find(|project| project.id == source.project_id)
                .ok_or_else(|| anyhow!("the task project is unavailable"))?;
            let cwd = source.workspace.path().unwrap_or(&project.path).to_owned();
            (source, cwd)
        };
        validate_message_rewind(&source, turn_count)?;
        let next_history_generation = source
            .history_generation
            .checked_add(1)
            .filter(|generation| *generation <= i64::MAX as u64)
            .ok_or_else(|| anyhow!("task history generation exhausted"))?;

        let cancellation = if source.provider == ProviderKind::Codex {
            let sessions = self.sessions.lock();
            let owner = match sessions.get(&session_id) {
                Some(entry) => {
                    anyhow::ensure!(
                        runtime_id.is_nil() || entry.runtime_id == runtime_id,
                        "rewind belongs to a stale runtime"
                    );
                    entry.stable.is_none().then_some(entry.runtime_id)
                }
                // The lifecycle lock excludes a replacement. Stop can target
                // this cold request's identity without controlling a resident.
                None => Some(runtime_id),
            };
            owner.map(|runtime_id| {
                cancellation.bind_runtime(runtime_id, || {});
                cancellation
            })
        } else {
            None
        };

        // Resolve the executable before touching the worktree. Even native
        // transcript operations are immediately followed by a replacement
        // prompt, so accepting a rewind that cannot resume would strand the
        // user at a provider state the UI cannot continue.
        let binary = self.provider_binary(source.provider)?;
        let retained_turn_count = turn_count.saturating_sub(1);
        let previous_turn_count = source.turns.len();
        let rollback_turns = source.provider_turns_after(retained_turn_count);
        let provider_turn_count = source
            .turns
            .iter()
            .take(retained_turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let provider_resume_at = retained_turn_count
            .checked_sub(1)
            .and_then(|index| source.turns.get(index))
            .and_then(|turn| turn.provider_resume_at.clone());

        let capture_lane = self.checkpoint_capture_lane(&cwd)?;
        let _checkpoint_mutation = capture_lane.preempt();
        if let Some(cancellation) = cancellation.filter(|cancellation| cancellation.is_cancelled())
        {
            cancellation.settle_operation();
            bail!("rewind stopped before changing worktree checkpoints");
        }

        let turn_start_ref = crate::checkpoint::turn_start_ref(session_id, turn_count);
        let retained_ref = crate::checkpoint::checkpoint_ref(session_id, retained_turn_count);
        let restore_ref = if crate::checkpoint::has_ref(&cwd, &turn_start_ref) {
            turn_start_ref
        } else {
            retained_ref
        };
        if !crate::checkpoint::has_ref(&cwd, &restore_ref) {
            bail!("the checkpoint before this message is unavailable");
        }

        let safety_ref = format!("refs/waku/revert-backup-{session_id}-{}", Uuid::new_v4());
        crate::checkpoint::capture_ref(&cwd, &safety_ref)
            .context("could not create a rewind safety snapshot")?;
        if let Err(error) = crate::checkpoint::restore_ref(&cwd, &restore_ref) {
            return Err(restore_rewind_safety(
                &cwd,
                &safety_ref,
                "could not restore the selected checkpoint",
                error,
            ));
        }

        let provider_rewind = self
            .rewind_provider_response(
                &source,
                &cwd,
                &binary,
                retained_turn_count,
                rollback_turns,
                provider_turn_count,
                provider_resume_at,
                cancellation,
            )
            .and_then(|result| {
                anyhow::ensure!(
                    !cancellation.is_some_and(|cancellation| cancellation.is_cancelled()),
                    "rewind stopped before selecting new provider history"
                );
                Ok(result)
            });
        let (provider_cursor, message_ids, reset_native_session) = match provider_rewind {
            Ok(result) => result,
            Err(error) => {
                if let Some(cancellation) =
                    cancellation.filter(|cancellation| cancellation.is_cancelled())
                {
                    // This path only uses a source-preserving native operation.
                    // Settle Stop after the owned peer and original files settle.
                    crate::checkpoint::restore_ref(&cwd, &safety_ref).with_context(||
                        format!("stopped rewind could not restore the original worktree; safety snapshot: {safety_ref}"))?;
                    let _ = crate::checkpoint::delete_ref(&cwd, &safety_ref);
                    cancellation.settle_operation();
                    return Err(error.context("rewind stopped; the original worktree was restored"));
                }
                return Err(restore_rewind_safety(
                    &cwd,
                    &safety_ref,
                    "could not complete the provider rewind",
                    error,
                ));
            }
        };

        let _ = crate::checkpoint::delete_ref(&cwd, &safety_ref);
        let cleanup_warning = crate::checkpoint::delete_turn_refs_after(
            &cwd,
            session_id,
            retained_turn_count,
            previous_turn_count,
        )
        .err()
        .map(|error| error.to_string());

        // Every provider resumes from the newly stored cursor on the next
        // prompt. Removing the resident source driver also prevents its late
        // events from racing the rewound transcript: the hub retires the
        // runtime id, so anything the detached teardown still emits dies
        // with it.
        let removed = self.remove_runtime(session_id, None, false);
        drop_detached(removed);

        let mut rewound = source.clone();
        rewound.history_generation = next_history_generation;
        if !message_ids.is_empty() {
            for turn in rewound.turns.iter_mut().take(retained_turn_count) {
                if let Some(remapped) = turn
                    .provider_resume_at
                    .as_ref()
                    .and_then(|message_id| message_ids.get(message_id))
                    .cloned()
                {
                    turn.provider_resume_at = Some(remapped);
                }
            }
        }
        if reset_native_session {
            rewound.provider_cursor = None;
            rewound.provider_session_id = None;
        } else if let Some(cursor) = provider_cursor {
            rewound.provider_cursor = Some(cursor);
        }
        rewound.truncate_after_turn(retained_turn_count);
        rewound.status = SessionStatus::Idle;
        if retained_turn_count == 0 {
            // A completed user action is a durable task, even when no provider
            // turns or cursor remain. Keep a real receipt rather than a draft.
            rewound.push_message(
                crate::model::MessageRole::System,
                tr!("transcript.rewind_completed"),
            );
        }

        let pinned = self.sessions.lock().keys().copied().collect();
        let mut state = self.task_state.lock();
        let existing = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| anyhow!("the task was removed while it was being rewound"))?;
        *existing = rewound.clone();
        state.mark_session_dirty(session_id);
        self.task_store
            .save(&mut state)
            .context("could not save the rewound task")?;
        trim_resident_transcripts(&mut state, &pinned);
        drop(state);
        if let Some(cancellation) = cancellation {
            cancellation.settle_operation();
            if cancellation.is_cancelled() {
                self.notify_task_state();
                bail!(
                    "rewind completed before Stop; task history and files reflect the rewind; its follow-up prompt was withheld"
                );
            }
        }
        Ok((rewound, cleanup_warning))
    }

    pub(super) fn fork_provider_response(
        &self,
        source: &AgentSession,
        cwd: &Path,
        fork_title: &str,
        turn_count: usize,
        provider_turn_count: usize,
        turns_to_remove: usize,
    ) -> anyhow::Result<(ProviderResumeCursor, HashMap<String, String>)> {
        match source.provider {
            ProviderKind::Claude => {
                let Some(ProviderResumeCursor::Claude { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Claude's native session is unavailable");
                };
                let resume_at = source
                    .turns
                    .get(turn_count.saturating_sub(1))
                    .and_then(|turn| turn.provider_resume_at.clone());
                let fork = fork_provider_session(ProviderSessionForkRequest::Claude {
                    session_id: session_id.clone(),
                    resume_at,
                    turn_count: provider_turn_count,
                    title: fork_title.to_owned(),
                })?;
                Ok((fork.cursor, fork.message_ids))
            }
            ProviderKind::Codex
            | ProviderKind::DeepSeek
            | ProviderKind::Muse
            | ProviderKind::OhMyPi
            | ProviderKind::Pi => Ok((
                self.fork_response_with_driver(source, cwd, turns_to_remove)?,
                HashMap::new(),
            )),
            ProviderKind::Cursor => {
                let fork = fork_provider_session(ProviderSessionForkRequest::Cursor {
                    source: source.clone(),
                    turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Amp => {
                let Some(ProviderResumeCursor::Amp {
                    thread_id,
                    fork_context,
                }) = source.provider_cursor.as_ref()
                else {
                    bail!("Amp's native thread is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Amp {
                    binary: self.provider_binary(ProviderKind::Amp)?,
                    cwd: cwd.to_owned(),
                    thread_id: thread_id.clone(),
                    fork_context: fork_context.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::OpenCode => {
                let Some(ProviderResumeCursor::OpenCode { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("OpenCode's native session is unavailable");
                };
                // No cwd: a session carries its own `location`, so there is no
                // server working directory to fork against.
                let fork = fork_provider_session(ProviderSessionForkRequest::OpenCode {
                    binary: self.provider_binary(ProviderKind::OpenCode)?,
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Grok => {
                let Some(ProviderResumeCursor::Grok { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Grok Build's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Grok {
                    binary: self.provider_binary(ProviderKind::Grok)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Copilot => {
                let Some(ProviderResumeCursor::Copilot { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("GitHub Copilot's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Copilot {
                    binary: self.provider_binary(ProviderKind::Copilot)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                    title: fork_title.to_owned(),
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            // Unreachable through the UI, which hides branching for providers
            // that answer `supports_conversation_fork` with false.
            ProviderKind::Antigravity
            | ProviderKind::Devin
            | ProviderKind::Droid
            | ProviderKind::Fx
            | ProviderKind::Goose
            | ProviderKind::Kimi => {
                bail!(
                    "{} cannot branch a conversation at a turn",
                    source.provider.display_name()
                )
            }
        }
    }

    pub(super) fn fork_response_with_driver(
        &self,
        source: &AgentSession,
        cwd: &Path,
        turns_to_remove: usize,
    ) -> anyhow::Result<ProviderResumeCursor> {
        let driver = self
            .sessions
            .lock()
            .get(&source.id)
            .map(|entry| entry.driver.clone());
        if let Some(driver) = driver {
            return driver.fork(turns_to_remove);
        }

        match source.provider {
            ProviderKind::Codex
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Codex { .. })
                ) =>
            {
                bail!("Codex's native thread is unavailable");
            }
            ProviderKind::DeepSeek
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::DeepSeek { .. })
                ) =>
            {
                bail!("DeepSeek Harness's native session is unavailable");
            }
            ProviderKind::Pi
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Pi {
                        session_file: Some(_),
                        ..
                    })
                ) =>
            {
                bail!("Pi's native session file is unavailable");
            }
            ProviderKind::OhMyPi
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::OhMyPi {
                        session_file: Some(_),
                        ..
                    })
                ) =>
            {
                bail!("Oh My Pi's native session file is unavailable");
            }
            ProviderKind::Muse
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Muse { .. })
                ) =>
            {
                bail!("Muse Code's native session is unavailable");
            }
            _ => {}
        }

        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, _event_receiver) = driver::event_channel(wake);
        let driver = driver::start_local(
            source.provider,
            DriverStartOptions {
                binary: self.provider_binary(source.provider)?,
                cwd: cwd.to_owned(),
                mode: source.runtime_mode,
                model: source.model.clone(),
                reasoning_effort: source.reasoning_effort.clone(),
                service_tier: source.service_tier.clone(),
                context_window: source.context_window.clone(),
                agent_preset: source.agent_preset.clone(),
                computer_use_enabled: false,
                // A fork/rollback driver is a one-shot process, not the
                // task's live runtime; it never receives a scoped token.
                agent: None,
                read_own_transcript: false,
                subagents: None,
                computer_use_runtime: None,
                mcp_servers: Vec::new(),
                http_mcp_capability_recorder: None,
                provider_cursor: source.provider_cursor.clone(),
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                ephemeral: false,
            },
            event_sender,
        )?;
        driver.fork(turns_to_remove)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn rewind_provider_response(
        &self,
        source: &AgentSession,
        cwd: &Path,
        binary: &Path,
        retained_turn_count: usize,
        rollback_turns: usize,
        provider_turn_count: usize,
        provider_resume_at: Option<String>,
        cancellation: Option<&crate::server::RuntimeCancellation>,
    ) -> anyhow::Result<(Option<ProviderResumeCursor>, HashMap<String, String>, bool)> {
        if rollback_turns == 0 {
            return Ok((None, HashMap::new(), false));
        }
        let reset_native_session = retained_turn_count == 0
            && matches!(
                source.provider,
                ProviderKind::Codex
                    | ProviderKind::Claude
                    | ProviderKind::Copilot
                    | ProviderKind::Cursor
                    | ProviderKind::Grok
            );
        if reset_native_session {
            return Ok((None, HashMap::new(), true));
        }

        match source.provider {
            ProviderKind::Codex => {
                // Keep the source conversation intact until task persistence
                // selects the fork. A lost reply then leaves a recoverable
                // original instead of an unknown in-place history mutation.
                let Some(ProviderResumeCursor::Codex { thread_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Codex's native thread is unavailable");
                };
                let stopped =
                    || cancellation.is_some_and(|cancellation| cancellation.is_cancelled());
                let cursor = crate::codex_session::fork_session_without_recent_turns(
                    binary,
                    cwd,
                    thread_id,
                    rollback_turns,
                    cancellation.map(|_| &stopped as &dyn Fn() -> bool),
                )?;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Claude => {
                let Some(ProviderResumeCursor::Claude { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Claude's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Claude {
                    session_id: session_id.clone(),
                    resume_at: provider_resume_at,
                    turn_count: provider_turn_count,
                    title: format!("{} (rewind)", source.display_title()),
                })?;
                Ok((Some(fork.cursor), fork.message_ids, false))
            }
            ProviderKind::OpenCode => {
                let driver = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|entry| entry.driver.clone());
                let cursor = if let Some(driver) = driver {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("OpenCode returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::OpenCode { session_id, .. }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("OpenCode's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::OpenCode {
                        binary: binary.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Amp => {
                let Some(ProviderResumeCursor::Amp {
                    thread_id,
                    fork_context,
                }) = source.provider_cursor.as_ref()
                else {
                    bail!("Amp's native thread is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Amp {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    thread_id: thread_id.clone(),
                    fork_context: fork_context.clone(),
                    turn_count: provider_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Cursor => {
                let cursor = fork_provider_session(ProviderSessionForkRequest::Cursor {
                    source: source.clone(),
                    turn_count: retained_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Grok => {
                let Some(ProviderResumeCursor::Grok { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Grok Build's native session is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Grok {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Copilot => {
                let Some(ProviderResumeCursor::Copilot { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("GitHub Copilot's native session is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Copilot {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                    title: format!("{} (rewind)", source.display_title()),
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Muse => {
                let driver = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|entry| entry.driver.clone());
                let cursor = if let Some(driver) = driver {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("Muse Code returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::Muse { session_id, .. }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("Muse Code's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::Muse {
                        binary: binary.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::DeepSeek | ProviderKind::OhMyPi | ProviderKind::Pi => Ok((
                self.rollback_response_with_driver(source, cwd, binary, rollback_turns)?,
                HashMap::new(),
                false,
            )),
            // Unreachable through the UI, which hides rewinding for providers
            // that answer `supports_conversation_rollback` with false.
            ProviderKind::Antigravity
            | ProviderKind::Devin
            | ProviderKind::Droid
            | ProviderKind::Fx
            | ProviderKind::Goose
            | ProviderKind::Kimi => {
                bail!(
                    "{} cannot rewind a conversation to a turn",
                    source.provider.display_name()
                )
            }
        }
    }

    pub(super) fn rollback_response_with_driver(
        &self,
        source: &AgentSession,
        cwd: &Path,
        binary: &Path,
        rollback_turns: usize,
    ) -> anyhow::Result<Option<ProviderResumeCursor>> {
        let driver = self
            .sessions
            .lock()
            .get(&source.id)
            .map(|entry| entry.driver.clone());
        if let Some(driver) = driver {
            return driver.rollback(rollback_turns);
        }

        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, _event_receiver) = driver::event_channel(wake);
        let driver = driver::start_local(
            source.provider,
            DriverStartOptions {
                binary: binary.to_owned(),
                cwd: cwd.to_owned(),
                mode: source.runtime_mode,
                model: source.model.clone(),
                reasoning_effort: source.reasoning_effort.clone(),
                service_tier: source.service_tier.clone(),
                context_window: source.context_window.clone(),
                agent_preset: source.agent_preset.clone(),
                computer_use_enabled: false,
                agent: None,
                read_own_transcript: false,
                subagents: None,
                computer_use_runtime: None,
                mcp_servers: Vec::new(),
                http_mcp_capability_recorder: None,
                provider_cursor: source.provider_cursor.clone(),
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                ephemeral: false,
            },
            event_sender,
        )?;
        driver.rollback(rollback_turns)
    }

    pub(super) fn provider_binary(&self, provider: ProviderKind) -> anyhow::Result<PathBuf> {
        ensure_shell_environment();
        let settings = self.settings.get();
        let binary_override = settings
            .provider_binary_overrides
            .get(&provider)
            .map(String::as_str);
        crate::model::provider_probe(provider, binary_override)
            .path
            .ok_or_else(|| anyhow!("{} is not installed on the daemon", provider.display_name()))
    }
}
