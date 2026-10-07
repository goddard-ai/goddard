use super::*;

/// Pump provider events for one runtime into the client's event stream.
///
/// Besides serialization this maintains the agent-surface bookkeeping: turn
/// state decides where queue-mode prompts wait, a finished turn drains that
/// queue in submission order, a `steerAccepted` echo is annotated with the
/// sending task, and a dead runtime drops its credential and registry entry
/// with it.
/// Managed chats must remain readable without a connected UI. Streaming deltas
/// update the resident projection; durable writes occur at semantic boundaries.
pub(super) fn record_boss_event(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    event: &DriverEvent,
) -> anyhow::Result<()> {
    use crate::model::{ActivityItem, MessageRole, ReasoningBlock, TranscriptBlock};
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|entry| entry.id == session_id)
    else {
        return Ok(());
    };
    task_store.hydrate(session)?;
    if matches!(
        event,
        DriverEvent::PromptSubmitted { .. } | DriverEvent::TurnStarted
    ) {
        anyhow::ensure!(
            session.archived_at.is_none(),
            "Boss chat is archived; reopen the active Boss chat"
        );
    }
    let mut activity = None;
    match event {
        DriverEvent::PromptSubmitted {
            message,
            turn_id,
            message_id,
            sent_by_task,
            hidden,
            report_trigger,
            reference_context,
        } => {
            session.adopt_submitted_prompt(
                message,
                *turn_id,
                *message_id,
                *sent_by_task,
                *hidden,
                report_trigger.clone(),
            );
            if let Some(prompt) = session
                .messages
                .iter_mut()
                .find(|message| message.id == *message_id)
            {
                prompt.reference_context = reference_context.clone();
            }
        }
        DriverEvent::TurnStarted => {
            if session.active_turn_id().is_none() {
                session.begin_provider_turn();
            }
            session.mark_active_turn_provider_started();
            session.status = SessionStatus::Working;
        }
        DriverEvent::TurnParked => session.status = SessionStatus::Background,
        DriverEvent::TextDelta(text) => {
            let turn = session.active_turn_id();
            let boundary = session.transcript_blocks.last().is_some_and(|block| {
                block.turn_id == turn && block.after_message == session.messages.len()
            });
            if !boundary
                && session.messages.last().is_some_and(|message| {
                    message.role == MessageRole::Assistant && message.turn_id == turn
                })
            {
                session.messages.last_mut().unwrap().content.push_str(text);
            } else {
                session.push_message(MessageRole::Assistant, text);
            }
        }
        DriverEvent::ReasoningDelta(text) => {
            let turn = session.active_turn_id();
            let reasoning = session
                .transcript_blocks
                .last_mut()
                .filter(|block| {
                    block.turn_id == turn && block.after_message == session.messages.len()
                })
                .and_then(|block| block.activities.last_mut())
                .and_then(|item| item.reasoning.as_mut());
            if let Some(reasoning) = reasoning {
                reasoning.content.push_str(text);
            } else {
                activity = Some(ActivityItem::from_reasoning(
                    ReasoningBlock {
                        content: text.clone(),
                        started_at_ms: crate::model::unix_time() * 1000,
                        finished_at_ms: 0,
                    },
                    false,
                ));
            }
        }
        DriverEvent::Activity {
            id,
            kind,
            title,
            detail,
            complete,
        } => {
            activity = Some(ActivityItem::new(
                id.clone(),
                *kind,
                title,
                detail.clone(),
                *complete,
            ));
        }
        DriverEvent::RichActivity(item) => activity = Some(item.clone()),
        DriverEvent::UsageUpdated {
            context_tokens,
            context_window,
        } => {
            let usage = session.context_usage.get_or_insert(Default::default());
            if let Some(tokens) = context_tokens {
                usage.tokens = *tokens;
            }
            if let Some(window) = context_window {
                usage.window = Some(*window);
            }
        }
        DriverEvent::Permission { .. } | DriverEvent::UserInputRequested { .. } => {
            session.status = SessionStatus::Waiting
        }
        DriverEvent::TurnFinished {
            success, summary, ..
        } => {
            session.finish_active_turn(if *success {
                TurnStatus::Completed
            } else {
                TurnStatus::Failed
            });
            session.status = if *success {
                SessionStatus::Idle
            } else {
                SessionStatus::Failed
            };
            if let Some(summary) = summary.as_ref().filter(|_| !*success) {
                session.push_message(MessageRole::System, summary);
            }
            for block in &mut session.transcript_blocks {
                for item in &mut block.activities {
                    item.complete = true;
                    if let Some(reasoning) = &mut item.reasoning {
                        reasoning.finished_at_ms = crate::model::unix_time() * 1000;
                    }
                }
            }
        }
        DriverEvent::Error(message) | DriverEvent::LocalizedError { message, .. } => {
            session.push_message(MessageRole::System, message);
            session.status = SessionStatus::Failed;
        }
        DriverEvent::ProcessExited => {
            if session.active_turn_id().is_some() {
                session.interrupt_active_turn(crate::model::TurnInterruption::Provider);
            }
            if session.status.is_busy() {
                session.status = SessionStatus::Failed;
            }
        }
        _ => return Ok(()),
    }
    if let Some(mut item) = activity {
        let turn = session.active_turn_id();
        let existing = item.source_id.as_ref().and_then(|id| {
            session
                .transcript_blocks
                .iter_mut()
                .filter(|block| block.turn_id == turn)
                .flat_map(|block| &mut block.activities)
                .find(|stored| stored.source_id.as_ref() == Some(id))
        });
        if let Some(existing) = existing {
            item.id = existing.id;
            *existing = item;
        } else {
            if !session.transcript_blocks.last().is_some_and(|block| {
                block.turn_id == turn && block.after_message == session.messages.len()
            }) {
                session.transcript_blocks.push(TranscriptBlock {
                    after_message: session.messages.len(),
                    turn_id: turn,
                    activities: Vec::new(),
                });
            }
            session
                .transcript_blocks
                .last_mut()
                .unwrap()
                .activities
                .push(item);
        }
    }
    session.updated_at = crate::model::unix_time();
    state.mark_session_dirty(session_id);
    if !matches!(
        event,
        DriverEvent::TextDelta(_) | DriverEvent::ReasoningDelta(_)
    ) {
        task_store.save(&mut state)?;
    }
    Ok(())
}

/// Record a turn's end the daemon ordered itself — a supervisor stop, a
/// queued-ticket cancel, or a mid-flight reconfigure. The turn closes
/// `Interrupted` attributed to the daemon, the session reads idle rather
/// than failed, open activities complete, and `notice` — when given —
/// lands as the transcript's interruption row. Unlike a provider-reported
/// `TurnFinished { success: false }` nothing here reads as a failure.
pub(super) fn record_daemon_interrupt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    notice: Option<&str>,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|entry| entry.id == session_id)
    else {
        return Ok(());
    };
    task_store.hydrate(session)?;
    if session.active_turn_id().is_some() {
        session.interrupt_active_turn(crate::model::TurnInterruption::Daemon);
    }
    session.status = SessionStatus::Idle;
    if let Some(notice) = notice {
        session.push_notice_message(
            crate::model::MessageRole::Assistant,
            notice,
            crate::model::TranscriptNotice::Status {
                kind: crate::model::TranscriptNoticeStatus::Interrupted,
            },
        );
    }
    for block in &mut session.transcript_blocks {
        for item in &mut block.activities {
            item.complete = true;
            if let Some(reasoning) = &mut item.reasoning {
                reasoning.finished_at_ms = crate::model::unix_time() * 1000;
            }
        }
    }
    session.updated_at = crate::model::unix_time();
    state.mark_session_dirty(session_id);
    task_store.save(&mut state)?;
    Ok(())
}

pub(super) fn forward_driver_events(
    session_id: Uuid,
    runtime_id: Uuid,
    event_receiver: crossbeam_channel::Receiver<DriverEvent>,
    events: EventSink,
    driver: DriverHandle,
    agent: Arc<crate::agent::AgentState>,
    task_state: Arc<Mutex<PersistedState>>,
    task_store: Arc<StateStore>,
    sessions: Arc<Mutex<HashMap<Uuid, RuntimeEntry>>>,
    automations: Arc<AutomationService>,
    boss: Arc<crate::boss::BossService>,
    auto_prompts: Arc<AutoPromptService>,
    repo_maps: Arc<(Mutex<RepoMaps>, Condvar)>,
) {
    while let Ok(event) = event_receiver.recv() {
        // Every forwarded event is activity the idle reaper counts.
        if let Some(entry) = sessions.lock().get_mut(&session_id)
            && entry.runtime_id == runtime_id
        {
            entry.last_active = std::time::Instant::now();
        }
        // A provider exit that kills an open turn classifies the
        // employee's settle as an interruption — capture the flag before
        // either recorder retires the turn.
        let exited_mid_turn = matches!(&event, DriverEvent::ProcessExited)
            && (agent.has_open_turn(session_id) || {
                let mut state = task_state.lock();
                state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                    .and_then(|session| {
                        task_store
                            .hydrate(session)
                            .ok()
                            .and_then(|()| session.active_turn_id())
                    })
                    .is_some()
            });
        let managed = boss.is_managed(session_id);
        // Events from a runtime the session no longer owns — torn down by
        // a requeue, a stop, or a replacement — must not rewrite the
        // transcript or settle the employee's admission. A queued ticket
        // owns no live runtime either: anything arriving for it is the
        // previous generation dying.
        let stale = managed
            && (boss.employee_lifecycle(session_id)
                == Some(waku_protocol::boss::EmployeeLifecycle::Queued)
                || !sessions
                    .lock()
                    .get(&session_id)
                    .is_some_and(|entry| entry.runtime_id == runtime_id));
        // A settle the daemon itself ordered — a supervisor stop's
        // transcript write or a reconfigure's cancel — records the turn as
        // an intentional interruption, never a failure, and never settles
        // the employee's admission on it. A real process exit consumes the
        // flag too: the teardown it reports is its own settle. A stale
        // runtime's events never touch it — the flag belongs to the
        // runtime that currently owns the session.
        let intentional = !stale
            && matches!(&event, DriverEvent::TurnFinished { .. })
            && agent.take_daemon_interrupt(session_id);
        if !stale && matches!(&event, DriverEvent::ProcessExited) {
            agent.take_daemon_interrupt(session_id);
        }
        if managed && !stale {
            // A genuinely-completed turn keeps its verdict; only a
            // cancel-issued settle writes the intentional interruption.
            let interrupted =
                intentional && matches!(&event, DriverEvent::TurnFinished { success: false, .. });
            let recorded = if interrupted {
                record_daemon_interrupt(&task_state, &task_store, session_id, None)
            } else {
                record_boss_event(&task_state, &task_store, session_id, &event)
            };
            if let Err(error) = recorded {
                eprintln!("could not record Boss session {session_id}: {error:#}");
            }
        }
        let rejected_steer = if stale {
            None
        } else {
            agent.note_driver_event(session_id, &event)
        };
        automations.note_driver_event(session_id, &event);
        if let DriverEvent::TurnFinished { success, .. } = &event
            && !boss.is_managed(session_id)
        {
            auto_prompts.note_turn_finished(session_id, *success);
        }
        let event = match event {
            DriverEvent::Connected { provider_cursor } => {
                // The daemon keeps its own copy of the resume cursor so
                // thread-id resolution and cold starts work even when no
                // client ever saves the task.
                record_provider_cursor(&task_state, &task_store, session_id, &provider_cursor);
                // A reported cursor makes this runtime evictable — the next
                // prompt can rebuild it even if the catalog row has since
                // been skeletonized by the transcript window.
                if provider_cursor.is_some()
                    && let Some(entry) = sessions.lock().get_mut(&session_id)
                    && entry.runtime_id == runtime_id
                {
                    entry.resumable = true;
                }
                DriverEvent::Connected { provider_cursor }
            }
            DriverEvent::SteerAccepted { message, .. } => {
                let steer = agent.take_pending_steer(session_id, &message);
                // An enveloped steer echoes the envelope; the transcript and
                // attached clients show the sender's own words.
                let message = steer
                    .as_ref()
                    .filter(|steer| steer.transport.is_some())
                    .map(|steer| steer.prompt.clone())
                    .unwrap_or(message);
                let sent_by_task = steer.as_ref().and_then(|steer| steer.sender);
                let hidden = steer
                    .as_ref()
                    .is_some_and(|steer| steer.hidden || steer.context.is_some());
                let report_trigger = steer
                    .as_ref()
                    .and_then(|steer| steer.report_trigger.clone());
                if let Some(sender) = sent_by_task {
                    record_agent_steer(
                        &task_state,
                        &task_store,
                        session_id,
                        &message,
                        sender,
                        hidden,
                        report_trigger.clone(),
                    );
                }
                // The agent-surface instruction composes into every context
                // steer while it is owed, so any accepted context steer
                // delivered it; a rejection leaves it pending to retry.
                if steer.as_ref().is_some_and(|steer| steer.context.is_some()) {
                    agent.mark_surface_announced(session_id);
                    // A pending parent-index or project-memory carry
                    // settles with the steer that shipped it.
                    agent.mark_parent_index_delivered(session_id);
                    agent.mark_memory_delivered(session_id);
                }
                // A queue-drained prompt folded into the parked turn: its
                // mirrored chip's wait is over even when the steer carried
                // no sender (an automation run) to attribute.
                if let Some(queued_id) = steer.and_then(|steer| steer.queued_id)
                    && unmirror_agent_queued_prompt(&task_state, &task_store, session_id, queued_id)
                        .is_ok()
                {
                    send_agent_queue_changed(&task_state, &events, session_id);
                }
                DriverEvent::SteerAccepted {
                    message,
                    sent_by_task,
                    hidden,
                    report_trigger,
                }
            }
            DriverEvent::SteerRejected {
                message,
                reason,
                reason_i18n,
                ..
            } => {
                // A daemon-injected context steer is the daemon's own
                // delivery — swallow the rejection so no client surfaces it;
                // the session stays eligible and the next prompt retries.
                if rejected_steer
                    .as_ref()
                    .is_some_and(|steer| steer.context.is_some())
                {
                    continue;
                }
                // An enveloped steer echoes the envelope; a surfaced
                // rejection names the sender's own words.
                let message = rejected_steer
                    .as_ref()
                    .filter(|steer| steer.transport.is_some())
                    .map(|steer| steer.prompt.clone())
                    .unwrap_or(message);
                DriverEvent::SteerRejected {
                    message,
                    reason,
                    reason_i18n,
                    hidden: rejected_steer.as_ref().is_some_and(|steer| steer.hidden),
                }
            }
            event => event,
        };
        // A finished turn frees the session for the next queued prompt; a
        // `connected` greeting means a freshly (re)started runtime is idle,
        // so prompts queued while it was down deliver now.
        let drains_queue = matches!(
            &event,
            DriverEvent::TurnFinished { .. } | DriverEvent::Connected { .. }
        );
        // A settled turn is the freshness trigger for the workspace index so
        // a later on-demand map request sees the changes from this turn.
        if matches!(&event, DriverEvent::TurnFinished { .. }) {
            let cwd = repo_maps.0.lock().sessions.get(&session_id).cloned();
            if let Some(cwd) = cwd {
                spawn_repo_map_refresh(&repo_maps, cwd);
            }
        }
        let process_exited = matches!(&event, DriverEvent::ProcessExited);
        // Per-token deltas and streaming process output never enter the
        // replay journal: journaled they saturated each runtime's 2048-event
        // window and could leave a reconnecting client's bounded queue at
        // the subscriber cap — one broadcast short of a silent kick. A
        // subscriber that missed them reconciles from the session snapshot.
        let ephemeral = matches!(
            &event,
            DriverEvent::TextDelta(_)
                | DriverEvent::ReasoningDelta(_)
                | DriverEvent::BackgroundWork(BackgroundWorkEvent::OutputDelta { .. })
        );
        let settled = matches!(
            &event,
            DriverEvent::TurnFinished { .. } | DriverEvent::ProcessExited
        );
        let settle = if settled {
            Some(match &event {
                DriverEvent::ProcessExited => waku_protocol::boss::EmployeeSettle::ProcessExited {
                    mid_turn: exited_mid_turn,
                },
                _ => waku_protocol::boss::EmployeeSettle::TurnFinished,
            })
        } else {
            None
        };
        let wire = event_to_wire(event).unwrap_or_else(|error| {
            WireDriverEvent::new(
                "error",
                Value::String(format!("could not encode daemon event: {error}")),
            )
        });
        let delivered = if ephemeral {
            events.send_ephemeral(wire)
        } else {
            events.send(wire)
        };
        if delivered.is_err() {
            break;
        }
        // A parked prompt is unfinished work — an employee whose queue is
        // still full drains it into new turns instead of expiring, and the
        // settle only fires once the queue runs dry. The orphan case — a
        // provider exit, a marked stop, a failed delivery — still expires
        // with the leftovers counted as `parkedWork`.
        let mut keeps_working = false;
        if !stale
            && matches!(
                settle,
                Some(waku_protocol::boss::EmployeeSettle::TurnFinished)
            )
            && boss.is_employee(session_id)
        {
            rehydrate_agent_queue(&agent, &task_state, &task_store, session_id);
            while let Some(entry) = agent.pop_queued(session_id) {
                if agent.is_working(session_id)
                    || employee_update_streaming(session_id, entry.sender, &agent, &boss)
                {
                    // A fresh turn opened while the queue drained — the
                    // rest waits for its finish like any other queued
                    // prompt.
                    agent.requeue_front(session_id, entry);
                    keeps_working = true;
                    break;
                }
                if let Err(error) = deliver_agent_prompt(
                    session_id,
                    &driver,
                    entry,
                    &events,
                    &agent,
                    &auto_prompts,
                    &task_state,
                    &task_store,
                    &boss,
                    &automations,
                ) {
                    eprintln!(
                        "goddard-daemon could not deliver a queued employee prompt for task {session_id}: {error:#}"
                    );
                    break;
                }
                keeps_working = true;
            }
        }
        if !keeps_working
            && !stale
            && !intentional
            && let Some(settle) = settle
        {
            boss.note_settled(session_id, settle);
        }
        if drains_queue && !stale && !boss.is_employee(session_id) {
            // A restarted daemon rebuilt no in-memory queue — the session
            // document's mirrored entries are the surviving record.
            rehydrate_agent_queue(&agent, &task_state, &task_store, session_id);
            while let Some(entry) = agent.pop_queued(session_id) {
                if agent.is_working(session_id)
                    || employee_update_streaming(session_id, entry.sender, &agent, &boss)
                {
                    // A turn started while the queue drained — a human
                    // prompt, or a provider-side wake. Queue-mode messages
                    // wait for the finish rather than steer mid-turn.
                    agent.requeue_front(session_id, entry);
                    break;
                }
                if let Err(error) = deliver_agent_prompt(
                    session_id,
                    &driver,
                    entry,
                    &events,
                    &agent,
                    &auto_prompts,
                    &task_state,
                    &task_store,
                    &boss,
                    &automations,
                ) {
                    eprintln!(
                        "goddard-daemon could not deliver a queued agent prompt for task {session_id}: {error:#}"
                    );
                    break;
                }
            }
        }
        if process_exited {
            agent.clear_session(session_id);
            {
                let mut maps = repo_maps.0.lock();
                maps.sessions.remove(&session_id);
            }
            // The hub retires a runtime on the request path — CloseSession,
            // a failed Start — but a provider that exits on its own never
            // produces one. Without this the dead runtime's replay journal,
            // sequence counter, and active-runtime entry sit in the hub for
            // the rest of the daemon's life.
            events
                .for_session(session_id, runtime_id)
                .end_session_runtime();
            let removed = {
                let mut sessions = sessions.lock();
                sessions
                    .get(&session_id)
                    .is_some_and(|entry| entry.runtime_id == runtime_id)
                    .then(|| sessions.remove(&session_id))
                    .flatten()
            };
            drop_detached(removed);
            break;
        }
    }
}
