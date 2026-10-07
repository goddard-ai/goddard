use super::*;

/// The provider-facing envelope for a task-to-task prompt: names the
/// sending task so the receiving agent knows another of the user's agents —
/// not the user — is talking, and has the id it needs to answer through
/// `goddard-agent`. The transcript keeps `prompt` verbatim; `None` means
/// send it unwrapped (unattributed sender, or a task messaging itself).
pub(super) fn agent_prompt_envelope(
    task_state: &Mutex<PersistedState>,
    target: Uuid,
    sender: Option<Uuid>,
    prompt: &str,
) -> Option<String> {
    let sender_id = sender.filter(|sender| *sender != target)?;
    let state = task_state.lock();
    let sender_session = state
        .sessions
        .iter()
        .find(|session| session.id == sender_id);
    let relation = if sender_session.is_some_and(|sender| sender.side_chat_of == Some(target)) {
        "your side chat"
    } else {
        "the agent of another Goddard task"
    };
    let title = sender_session
        .map(|session| {
            session
                .display_title()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|title| !title.is_empty());
    let origin = match title {
        Some(title) => format!("{relation} \"{title}\" (task id {sender_id})"),
        None => format!("{relation} (task id {sender_id})"),
    };
    Some(format!(
        "The message below is from {origin}, sent through `goddard-agent` on \
         the user's behalf — the user can see this exchange. To send a \
         message back, run \
         `goddard-agent prompt '{{\"task_id\":\"{sender_id}\",\"prompt\":\"<reply>\"}}'`.\n\n{prompt}"
    ))
}

/// Employee updates may wake their supervisor only after the employee's
/// open turn settles, so transcript reads include the complete report.
pub(super) fn employee_update_streaming(
    target: Uuid,
    sender: Option<Uuid>,
    agent: &crate::agent::AgentState,
    boss: &crate::boss::BossService,
) -> bool {
    sender.is_some_and(|sender| {
        agent.has_open_turn(sender)
            && boss
                .employee(sender)
                .is_some_and(|employee| boss.report_target(&employee) == Some(target))
    })
}

/// Urgent reports keep their interrupt delivery after waiting for the sender.
pub(super) fn employee_report_interrupts(entry: &crate::agent::AgentPrompt) -> bool {
    entry.report_trigger.as_ref().is_some_and(|trigger| {
        matches!(
            trigger.kind,
            crate::model::ReportTriggerKind::Blocker | crate::model::ReportTriggerKind::Interrupted
        )
    })
}

/// Deliver one queued agent prompt to a live session. A session with an
/// open but parked turn is messaged through the provider's steer path so
/// the prompt folds into the waiting turn; anything else begins a normal
/// new turn whose `promptSubmitted` broadcast carries the sender's
/// provenance.
pub(super) fn deliver_agent_prompt(
    session_id: Uuid,
    driver: &DriverHandle,
    mut entry: crate::agent::AgentPrompt,
    sink: &EventSink,
    agent: &crate::agent::AgentState,
    auto_prompts: &AutoPromptService,
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    boss: &crate::boss::BossService,
    automations: &AutomationService,
) -> anyhow::Result<()> {
    boss.require_active(session_id)?;
    if driver.supports_steer()
        && (agent.has_parked_turn(session_id)
            || (employee_report_interrupts(&entry) && agent.has_open_turn(session_id)))
    {
        let mut entry = entry;
        entry.transport =
            agent_prompt_envelope(task_state, session_id, entry.sender, &entry.prompt);
        // The report joins the parked turn rather than opening one — its
        // marker sits at the accepted steer boundary.
        if let Some(trigger) = &mut entry.report_trigger {
            trigger.boundary = crate::model::ReportTriggerBoundary::Steer;
        }
        let prompt = entry
            .transport
            .clone()
            .unwrap_or_else(|| entry.prompt.clone());
        agent.record_pending_steer(session_id, entry);
        driver.steer(prompt);
        return Ok(());
    }
    let turn_id = Uuid::new_v4();
    auto_prompts.note_nonhuman_turn(session_id, turn_id);
    // A parked prompt reuses its mirrored chip's id as the delivered
    // message's id: clients folding `promptSubmitted` drop the chip and
    // adopt the transcript row in one move.
    let message_id = entry.queued_id.unwrap_or_else(Uuid::new_v4);
    if let Some(trigger) = &mut entry.report_trigger {
        trigger.boundary = crate::model::ReportTriggerBoundary::Opening;
        trigger.event_id = message_id;
    }
    let reference_context = entry
        .report_trigger
        .as_ref()
        .and_then(|trigger| trigger.reference_context.clone())
        .or_else(|| {
            let sender = entry.sender.filter(|sender| {
                boss.employee(*sender)
                    .is_some_and(|employee| boss.report_target(&employee) == Some(session_id))
            })?;
            let state = task_state.lock();
            let session = state.sessions.iter().find(|session| session.id == sender)?;
            let project = state
                .projects
                .iter()
                .find(|project| project.id == session.project_id)?;
            Some(crate::model::ReferenceContext {
                project_root: project.path.clone(),
                worktree: session.workspace.path().map(Path::to_path_buf),
            })
        });
    persist_agent_prompt(
        task_state,
        task_store,
        session_id,
        &entry.prompt,
        turn_id,
        message_id,
        entry.sender,
        entry.queued_id,
        entry.hidden,
        entry.report_trigger.clone(),
        reference_context.clone(),
    )?;
    sink.send(event_to_wire(DriverEvent::PromptSubmitted {
        message: entry.prompt.clone(),
        turn_id,
        message_id,
        sent_by_task: entry.sender,
        hidden: entry.hidden,
        report_trigger: entry.report_trigger.clone(),
        reference_context,
    })?)?;
    send_agent_queue_changed(task_state, sink, session_id);
    let handoff = {
        let mut state = task_state.lock();
        state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .and_then(|session| session.pending_provider_context.take())
    };
    let prompt = agent_prompt_envelope(task_state, session_id, entry.sender, &entry.prompt)
        .unwrap_or(entry.prompt);
    let prompt = handoff.map_or(prompt.clone(), |context| {
        format!("{context}\n\nNext assignment: {prompt}")
    });
    driver.prompt(wrap_boss_outbound_prompt(
        task_state,
        &automations.document(),
        boss,
        session_id,
        prompt,
    ));
    Ok(())
}

/// The boss-context half of an outbound boss prompt: the persona wrap
/// always applies, then every prompt to the boss carries the compact work
/// header — orientation is never gated. A full digest the router asked for
/// but could not steer replaces the header on the next prompt — whichever
/// path sends it.
pub(super) fn wrap_boss_outbound_prompt(
    task_state: &Mutex<PersistedState>,
    automations: &waku_protocol::automations::AutomationsState,
    boss: &crate::boss::BossService,
    session_id: Uuid,
    prompt: String,
) -> String {
    let prompt = boss.prompt_with_context(session_id, prompt);
    if !boss.is_boss(session_id) {
        return prompt;
    }
    let pending = boss.router_take_pending(session_id);
    let work = crate::boss_context::work_context(&task_state.lock(), &boss.document(), automations);
    // The deferred verdict's full digest supersedes the header it contains.
    let block = if pending && !work.digest.is_empty() {
        work.digest
    } else {
        work.header
    };
    if block.is_empty() {
        return prompt;
    }
    format!("<goddard-boss-context>\n{block}\n</goddard-boss-context>\n\n{prompt}")
}

/// Mirror an accepted agent prompt into the daemon's stored copy of the
/// task, so the message and its sender provenance persist even when no
/// client is attached to adopt it. `queued_id` names the parked chip the
/// prompt is delivering out of — it leaves the queue in the same write.
pub(super) fn persist_agent_prompt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    message: &str,
    turn_id: Uuid,
    message_id: Uuid,
    sent_by_task: Option<Uuid>,
    queued_id: Option<Uuid>,
    hidden: bool,
    report_trigger: Option<crate::model::ReportTrigger>,
    reference_context: Option<crate::model::ReferenceContext>,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        bail!("task {session_id} is unknown to the daemon");
    };
    task_store.hydrate(session)?;
    anyhow::ensure!(session.archived_at.is_none(), "task is archived");
    let dequeued = queued_id.is_some_and(|queued_id| {
        let before = session.queued_messages.len();
        session
            .queued_messages
            .retain(|queued| queued.id != queued_id);
        session.queued_messages.len() != before
    });
    if session.adopt_submitted_prompt(
        message,
        turn_id,
        message_id,
        sent_by_task,
        hidden,
        report_trigger,
    ) || dequeued
    {
        if let Some(prompt) = session
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
        {
            prompt.reference_context = reference_context;
        }
        state.mark_session_dirty(session_id);
        task_store.save(&mut state)?;
    }
    Ok(())
}

/// Park an agent prompt in the session document's follow-up queue so every
/// client renders the wait as a queued chip. The entry's id doubles as the
/// eventual transcript message id — delivery removes it.
pub(super) fn mirror_agent_queued_prompt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    queued_id: Uuid,
    prompt: &str,
    sent_by: Option<Uuid>,
    hidden: bool,
    report_trigger: Option<crate::model::ReportTrigger>,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        bail!("task {session_id} is unknown to the daemon");
    };
    task_store.hydrate(session)?;
    anyhow::ensure!(session.archived_at.is_none(), "task is archived");
    if session
        .queued_messages
        .iter()
        .any(|queued| queued.id == queued_id)
    {
        return Ok(());
    }
    let mut entry = crate::model::QueuedMessage::agent(prompt, sent_by);
    entry.id = queued_id;
    entry.hidden = hidden;
    entry.report_trigger = report_trigger;
    session.queued_messages.push(entry);
    session.updated_at = crate::model::unix_time();
    state.mark_session_dirty(session_id);
    task_store.save(&mut state)?;
    Ok(())
}

/// Drop the mirrored chip a parked-steer prompt delivered out of, once the
/// provider accepted it into the waiting turn.
pub(super) fn unmirror_agent_queued_prompt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    queued_id: Uuid,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return Ok(());
    };
    task_store.hydrate(session)?;
    let before = session.queued_messages.len();
    session
        .queued_messages
        .retain(|queued| queued.id != queued_id);
    if session.queued_messages.len() == before {
        return Ok(());
    }
    session.updated_at = crate::model::unix_time();
    state.mark_session_dirty(session_id);
    task_store.save(&mut state)?;
    Ok(())
}

/// Refill the in-memory prompt queue from agent entries the session
/// document still mirrors as parked. The mirror survives restarts the
/// `AgentState` map does not, so a drained or never-started runtime finds
/// its backlog here instead of losing it.
pub(super) fn rehydrate_agent_queue(
    agent: &crate::agent::AgentState,
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
) {
    let seeded = {
        let mut state = task_state.lock();
        let Some(session) = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if task_store.hydrate(session).is_err() {
            return;
        }
        session
            .queued_messages
            .iter()
            .filter(|queued| !agent.queued_steer_pending(session_id, queued.id))
            .filter_map(|queued| match queued.source {
                crate::model::QueuedMessageSource::Agent { sent_by } => {
                    Some(crate::agent::AgentPrompt {
                        prompt: queued.content.clone(),
                        transport: None,
                        sender: sent_by,
                        queued_id: Some(queued.id),
                        context: None,
                        hidden: queued.hidden,
                        report_trigger: queued.report_trigger.clone(),
                    })
                }
                crate::model::QueuedMessageSource::User => None,
            })
            .collect::<Vec<_>>()
    };
    if !seeded.is_empty() {
        agent.seed_queue(session_id, seeded);
    }
}

/// Publish the daemon-owned follow-up queue for a session so attached
/// clients redraw the chip row. Best-effort: a dead subscriber just misses
/// the frame and picks the state up on its next hydrate.
pub(super) fn send_agent_queue_changed(
    task_state: &Mutex<PersistedState>,
    sink: &EventSink,
    session_id: Uuid,
) {
    let messages = task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .map(|session| {
            session
                .queued_messages
                .iter()
                .filter(|queued| queued.is_agent_owned())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Ok(wire) = event_to_wire(DriverEvent::QueuedMessagesChanged { messages }) {
        let _ = sink.send(wire);
    }
}

/// Mirror a provider-accepted agent steer into the stored task the way
/// [`persist_agent_prompt`] mirrors a queued prompt. `hidden` keeps a
/// provider-facing steer out of the transcript the same way clients keep it,
/// and `report_trigger` is the employee report's wake record.
pub(super) fn record_agent_steer(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    message: &str,
    sent_by_task: Uuid,
    hidden: bool,
    report_trigger: Option<crate::model::ReportTrigger>,
) {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };
    if task_store.hydrate(session).is_err() {
        return;
    }
    let message_id = session.push_user_message_with_presentation(
        message,
        None,
        Vec::new(),
        Vec::new(),
        Some(sent_by_task),
    );
    if (hidden || report_trigger.is_some())
        && let Some(stored) = session
            .messages
            .iter_mut()
            .find(|stored| stored.id == message_id)
    {
        stored.hidden = hidden;
        stored.report_trigger = report_trigger;
    }
    state.mark_session_dirty(session_id);
    if let Err(error) = task_store.save(&mut state) {
        eprintln!(
            "goddard-daemon could not persist an agent steer for task {session_id}: {error:#}"
        );
    }
}

/// Keep the daemon's stored copy of a task's provider cursor current so
/// cold starts and thread-id resolution work without a client ever saving.
pub(super) fn record_provider_cursor(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    provider_cursor: &Option<ProviderResumeCursor>,
) {
    let Some(cursor) = provider_cursor else {
        return;
    };
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };
    if task_store.hydrate(session).is_err() || session.provider_cursor.as_ref() == Some(cursor) {
        return;
    }
    session.provider_cursor = Some(cursor.clone());
    state.mark_session_dirty(session_id);
    if let Err(error) = task_store.save(&mut state) {
        eprintln!(
            "goddard-daemon could not persist a provider cursor for task {session_id}: {error:#}"
        );
    }
}
