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
         `goddard-agent prompt {sender_id} --text '<reply>'`.\n\n{prompt}"
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

/// Boss notifications yield to open turns, composer-owned follow-ups, and
/// a user stop — the user halted the Boss, so parked deliveries wait for
/// the next user-initiated resume instead of reactivating it. Check the
/// durable queue: settlement can run before the client dequeues the user's
/// next prompt, and restart recovery has no in-memory user queue.
pub(super) fn boss_notification_waiting(
    target: Uuid,
    agent: &crate::agent::AgentState,
    boss: &crate::boss::BossService,
    task_state: &Mutex<PersistedState>,
) -> bool {
    boss.is_boss(target)
        && (agent.user_stopped(target)
            || agent.has_open_turn(target)
            || agent.is_working(target)
            || task_state
                .lock()
                .sessions
                .iter()
                .find(|session| session.id == target)
                .is_none_or(|session| {
                    session.active_turn_id().is_some()
                        || session.queued_messages.iter().any(|message| {
                            matches!(message.source, crate::model::QueuedMessageSource::User)
                        })
                }))
}

pub(super) fn automation_prompt_waiting(
    target: Uuid,
    message_id: Option<Uuid>,
    agent: &crate::agent::AgentState,
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    automations: &AutomationService,
) -> bool {
    if !message_id.is_some_and(|id| automations.is_run_message(id)) {
        return false;
    }
    if agent.has_open_turn(target) {
        return true;
    }
    // A submitted human prompt already owns a turn even before the provider
    // emits TurnStarted. An automation must not adopt that turn's identity.
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == target)
    else {
        return false;
    };
    if let Err(error) = task_store.hydrate(session) {
        eprintln!("could not check automation target {target}: {error:#}");
        return true;
    }
    session.active_turn_id().is_some()
}

/// Only finalized plans bypass Boss notification and sender-settlement holds.
/// Providers without steering finish cancellation before opening the handoff.
pub(super) fn agent_prompt_waiting(
    target: Uuid,
    entry: &crate::agent::AgentPrompt,
    driver: &DriverHandle,
    agent: &crate::agent::AgentState,
    boss: &crate::boss::BossService,
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    automations: &AutomationService,
) -> bool {
    if agent.user_stopped(target)
        || (entry
            .sender
            .is_some_and(|sender| boss.is_boss_principal(sender))
            && boss.employee(target).is_some()
            && agent.has_open_turn(target)
            && !driver.supports_steer())
    {
        // Unsupported providers must finish cancellation before a Boss
        // correction opens another turn, including while parked for input.
        return true;
    }
    if automation_prompt_waiting(
        target,
        entry.queued_id,
        agent,
        task_state,
        task_store,
        automations,
    ) {
        return true;
    }
    if entry.is_plan_finalization() {
        return agent.has_open_turn(target) && !driver.supports_steer();
    }
    boss_notification_waiting(target, agent, boss, task_state)
        || (agent.is_working(target)
            && !(employee_report_interrupts(entry) && driver.supports_steer()))
        || employee_update_streaming(target, entry.sender, agent, boss)
}

/// Urgent reports keep their interrupt delivery after waiting for the sender.
pub(super) fn employee_report_interrupts(entry: &crate::agent::AgentPrompt) -> bool {
    entry.report_trigger.as_ref().is_some_and(|trigger| {
        matches!(
            trigger.kind,
            crate::model::ReportTriggerKind::Blocker
                | crate::model::ReportTriggerKind::Interrupted
                | crate::model::ReportTriggerKind::PlanFinalized
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
    // An incident-fenced Boss chat holds its queued work: delivering now
    // would record a turn the fence-denied provider can never confirm.
    if boss_incident_fenced(boss, session_id) {
        agent.requeue_front(session_id, entry);
        return Ok(());
    }
    let automation_prompt = entry
        .queued_id
        .is_some_and(|id| automations.is_run_message(id));
    if agent.user_stopped(session_id)
        || (!entry.is_plan_finalization()
            && boss_notification_waiting(session_id, agent, boss, task_state))
        || (entry.is_plan_finalization()
            && agent.has_open_turn(session_id)
            && !driver.supports_steer())
    {
        agent.requeue_front(session_id, entry);
        return Ok(());
    }
    if !automation_prompt
        && driver.supports_steer()
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
        if !agent.record_pending_steer(session_id, entry) {
            // A concurrent drain already steered this parked chip — its
            // echo completes the delivery and unmirrors the chip.
            return Ok(());
        }
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
    match persist_agent_prompt(
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
        boss.is_boss(session_id),
        automation_prompt,
    )? {
        PromptPersistence::Submitted => {}
        PromptPersistence::Waiting => {
            agent.requeue_front(session_id, entry);
            return Ok(());
        }
        PromptPersistence::Duplicate => {
            send_agent_queue_changed(task_state, sink, session_id);
            return Ok(());
        }
    }
    automations.note_prompt_delivered(session_id, message_id, turn_id)?;
    if automation_prompt {
        // Reserve the turn before the provider emits TurnStarted, so a queue
        // drain cannot deliver a second automation into the same runtime.
        agent.note_driver_event(session_id, &DriverEvent::TurnStarted);
    }
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
    let boss_chat = boss.is_boss(session_id);
    let handoff = {
        let mut state = task_state.lock();
        state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .and_then(|session| {
                // The Boss's bound context package survives submission: it is
                // consumed only when provider transport acceptance records it
                // (`TurnStarted` in the event forwarder). A rejected or
                // unconfirmed send retries with the same delivery identity.
                if boss_chat {
                    session.pending_provider_context.clone()
                } else {
                    session.pending_provider_context.take()
                }
            })
    };
    let prompt = agent_prompt_envelope(task_state, session_id, entry.sender, &entry.prompt)
        .unwrap_or(entry.prompt);
    let carried_handoff = handoff.is_some();
    let prompt = handoff.map_or(prompt.clone(), |context| {
        format!(
            "{}\n\nNext assignment: {prompt}",
            provider_context_block(&context)
        )
    });
    driver.prompt(wrap_boss_outbound_prompt(
        task_state,
        &automations.document(),
        boss,
        session_id,
        prompt,
    ));
    if boss_chat && carried_handoff {
        mark_boss_delivery_submitted(boss, session_id);
    }
    Ok(())
}

/// Mark daemon-owned context so submitted-prompt mirrors can keep it out of
/// the human transcript while sending the complete text to the provider.
pub(super) fn provider_context_block(context: &str) -> String {
    format!("<goddard-provider-context>\n{context}\n</goddard-provider-context>")
}

/// Whether the continuity ledger fences this session. An incident-fenced
/// Boss chat must hold parked deliveries — anything sent now drops at the
/// ownership fence after opening a turn the provider never confirms.
pub(super) fn boss_incident_fenced(boss: &crate::boss::BossService, session_id: Uuid) -> bool {
    use waku_boss::rotation_continuity::ContinuityLedger;
    ContinuityLedger::load(&boss.root().join("continuity.json"))
        .map(|ledger| ledger.has_incident(session_id))
        .unwrap_or(false)
}

/// The bound package just went out over provider transport. Submission is
/// recorded without consuming the pending state — only a `TurnStarted`
/// acceptance clears it.
pub(super) fn mark_boss_delivery_submitted(boss: &crate::boss::BossService, session_id: Uuid) {
    use waku_boss::rotation_continuity::ContinuityLedger;
    let path = boss.root().join("continuity.json");
    let Ok(mut ledger) = ContinuityLedger::load(&path) else {
        return;
    };
    let Some(delivery_id) = ledger
        .pending_for_session(session_id)
        .map(|delivery| delivery.delivery_id.clone())
    else {
        return;
    };
    ledger.mark_submitted(&delivery_id, crate::model::unix_time());
    if let Err(error) = ledger.persist(&path) {
        eprintln!("could not record a Boss continuity submission: {error:#}");
    }
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

pub(super) enum PromptPersistence {
    Submitted,
    Waiting,
    Duplicate,
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
    boss_notification: bool,
    automation_prompt: bool,
) -> anyhow::Result<PromptPersistence> {
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
    if boss_notification || automation_prompt {
        // Claim the durable id under the same lock as the user queue and
        // active turn. A competing settlement/scheduler delivery may have
        // rehydrated this entry before the first delivery removed its mirror.
        if session.active_turn_id().is_some()
            || (boss_notification
                && !report_trigger.as_ref().is_some_and(|trigger| {
                    trigger.kind == crate::model::ReportTriggerKind::PlanFinalized
                })
                && session.queued_messages.iter().any(|message| {
                    matches!(message.source, crate::model::QueuedMessageSource::User)
                }))
        {
            return Ok(PromptPersistence::Waiting);
        }
    }
    let dequeued = queued_id.is_some_and(|queued_id| {
        let before = session.queued_messages.len();
        session
            .queued_messages
            .retain(|queued| queued.id != queued_id);
        session.queued_messages.len() != before
    });
    // A redriven delivery folds onto the copy that already landed — the
    // trigger's event id is the dedupe identity whether the first delivery
    // persisted by message id or a steered echo's own.
    let duplicate = (boss_notification || automation_prompt)
        && session
            .messages
            .iter()
            .any(|message| message.id == message_id)
        || report_trigger.as_ref().is_some_and(|trigger| {
            session.messages.iter().any(|message| {
                message
                    .report_trigger
                    .as_ref()
                    .is_some_and(|held| held.event_id == trigger.event_id)
            })
        });
    if duplicate {
        if dequeued {
            state.mark_session_dirty(session_id);
            task_store.save(&mut state)?;
        }
        return Ok(PromptPersistence::Duplicate);
    }
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
    Ok(PromptPersistence::Submitted)
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
    // A redriven echo — a second pending steer for the same parked report,
    // or a stale one resolving after the queue delivery landed — folds
    // onto the row the first delivery wrote instead of repeating it.
    if let Some(trigger) = &report_trigger
        && session.messages.iter().any(|stored| {
            stored
                .report_trigger
                .as_ref()
                .is_some_and(|held| held.event_id == trigger.event_id)
        })
    {
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

/// Record the runtime-confirmed effective configuration so a reload
/// projects what the provider actually activated — not the last requested
/// pair. `updated_at` advances so a client save snapshotted before the
/// activation cannot supersede the confirmed fields. The activation mark
/// itself is durable: a redelivery from an older generation, or a lower
/// revision of the same one, is rejected before any field can move back.
pub(super) fn record_effective_config(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    activation: ConfigActivation,
    provider: Option<&str>,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) {
    let provider = provider.and_then(|id| {
        ProviderKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.id() == id)
    });
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };
    if session
        .config_activation
        .is_some_and(|current| current > activation)
    {
        return;
    }
    if task_store.hydrate(session).is_err() {
        return;
    }
    let mut changed = session.config_activation != Some(activation);
    session.config_activation = Some(activation);
    if let Some(provider) = provider.filter(|provider| *provider != session.provider) {
        session.provider = provider;
        changed = true;
    }
    if let Some(model) = model.filter(|model| session.model.as_deref() != Some(*model)) {
        session.model = Some(model.to_owned());
        changed = true;
    }
    if let Some(effort) =
        reasoning_effort.filter(|effort| session.reasoning_effort.as_deref() != Some(*effort))
    {
        session.reasoning_effort = Some(effort.to_owned());
        changed = true;
    }
    if !changed {
        return;
    }
    session.updated_at = crate::model::unix_time();
    state.mark_session_dirty(session_id);
    if let Err(error) = task_store.save(&mut state) {
        eprintln!(
            "goddard-daemon could not persist the effective configuration for task {session_id}: {error:#}"
        );
    }
}
