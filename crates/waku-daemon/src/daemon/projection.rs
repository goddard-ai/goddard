use super::*;

pub(super) fn session_projection_precedes(
    existing: &AgentSession,
    incoming: &AgentSession,
    active_runtime_id: Option<Uuid>,
) -> bool {
    let existing_cursor = existing.runtime_event_cursor;
    let incoming_cursor = incoming.runtime_event_cursor;
    if let Some(active_runtime_id) = active_runtime_id {
        let existing_is_active =
            existing_cursor.is_some_and(|cursor| cursor.runtime_id == active_runtime_id);
        let incoming_is_active =
            incoming_cursor.is_some_and(|cursor| cursor.runtime_id == active_runtime_id);
        if existing_is_active != incoming_is_active {
            return existing_is_active;
        }
    }
    match (existing_cursor, incoming_cursor) {
        (Some(existing), Some(incoming))
            if existing.runtime_id == incoming.runtime_id && existing.epoch == incoming.epoch =>
        {
            incoming.sequence < existing.sequence
        }
        (Some(_), None) if existing.status.is_busy() => true,
        _ => incoming.updated_at < existing.updated_at,
    }
}

/// Rebuilds the complete session behind an incremental save: the wire
/// entry's scalars plus the stored prefix its tail claims. The prefix comes
/// from the resident session when loaded, else from the stored row read
/// before the merge lock. A tail whose prefix cannot be verified degrades
/// the entry to a skeleton — the merge then touches list columns only, and
/// the client's next full save heals the detail.
pub(super) fn splice_session_tail(
    sessions: &[AgentSession],
    bases: &mut HashMap<Uuid, AgentSession>,
    session: &mut AgentSession,
    tail: SessionDetailTail,
) {
    let messages_from = tail.messages_from as usize;
    let blocks_from = tail.transcript_blocks_from as usize;
    let verified = |base: &AgentSession| {
        base.messages.len() >= messages_from
            && base.transcript_blocks.len() >= blocks_from
            && detail_prefix_signature(
                &base.messages[..messages_from],
                &base.transcript_blocks[..blocks_from],
            ) == tail.prefix_signature
    };

    if let Some(resident) = sessions
        .iter()
        .find(|resident| resident.id == tail.session_id && resident.detail_loaded)
    {
        if verified(resident) {
            let mut messages = resident.messages[..messages_from].to_vec();
            messages.append(&mut session.messages);
            let mut blocks = resident.transcript_blocks[..blocks_from].to_vec();
            blocks.append(&mut session.transcript_blocks);
            session.messages = messages;
            session.transcript_blocks = blocks;
            return;
        }
        degrade_tail_session(session);
        return;
    }

    match bases.remove(&tail.session_id) {
        // No claimed prefix — the wire detail is already complete.
        _ if messages_from == 0 && blocks_from == 0 => {}
        Some(mut base) if verified(&base) => {
            base.messages.truncate(messages_from);
            base.messages.append(&mut session.messages);
            base.transcript_blocks.truncate(blocks_from);
            base.transcript_blocks
                .append(&mut session.transcript_blocks);
            session.messages = base.messages;
            session.transcript_blocks = base.transcript_blocks;
        }
        _ => degrade_tail_session(session),
    }
}

/// Keeps a full-detail save from resurrecting payloads an archive-detail
/// prune stripped: a client that hydrated the session before the sweep can
/// still hold them, and adopting its detail verbatim would write them back.
/// Unarchiving clears the marker instead — a live session accrues payloads
/// again from that point on.
pub(super) fn honor_details_pruned(existing: &AgentSession, incoming: &mut AgentSession) {
    if incoming.archived_at.is_some() {
        if existing.details_pruned || incoming.details_pruned {
            incoming.prune_transcript_payloads();
        }
    } else {
        incoming.details_pruned = false;
    }
}

/// Marks a tail-sent session as a skeleton so the merge updates only its list
/// columns, leaving the stored transcript untouched until a full save lands.
pub(super) fn degrade_tail_session(session: &mut AgentSession) {
    session.detail_loaded = false;
    session.messages.clear();
    session.transcript_blocks.clear();
    session.turns.clear();
    session.queued_messages.clear();
}

/// Applies the fields a list projection legitimately carries.
///
/// A skeleton's transcript and cursors are placeholders — only its column
/// values are real, and only while the projection is at least as new as what
/// is stored. `workspace` is a column too, but it is not merged here: a
/// client's copy may predate a move the daemon already recorded, and the
/// stored row stays authoritative either way. `status` is skipped while the
/// daemon owns a live runtime for the session: busy state belongs to that
/// runtime, not to a client's possibly-stale row. Returns whether anything
/// was applied.
pub(super) fn merge_session_list_columns(
    existing: &mut AgentSession,
    incoming: AgentSession,
    has_active_runtime: bool,
) -> bool {
    if incoming.updated_at < existing.updated_at {
        return false;
    }
    existing.title = incoming.title;
    existing.auto_title = incoming.auto_title;
    existing.agent_rename_allowed = incoming.agent_rename_allowed;
    existing.project_id = incoming.project_id;
    existing.provider = incoming.provider;
    existing.model = incoming.model;
    if !has_active_runtime {
        existing.status = incoming.status;
    }
    existing.created_at = incoming.created_at;
    existing.updated_at = incoming.updated_at;
    existing.last_reply_at = existing.last_reply_at.max(incoming.last_reply_at);
    existing.archived_at = incoming.archived_at;
    existing.pinned_at = incoming.pinned_at;
    existing.dormant_at = incoming.dormant_at;
    existing.dormant_exempt_until = incoming.dormant_exempt_until;
    existing.landed_at = incoming.landed_at;
    // Set once at creation and never mutated, but a skeleton merge should
    // still carry it: the daemon's cascade reads it without hydrating.
    existing.side_chat_of = incoming.side_chat_of;
    // The stamp is monotonic; a client's older copy must not clear it.
    existing.boss_managed |= incoming.boss_managed;
    // Same daemon-owned monotonic rule for the planning marker.
    if existing.planning.is_none() {
        existing.planning = incoming.planning;
    }
    true
}

pub(super) fn merge_stale_session_metadata(existing: &mut AgentSession, incoming: AgentSession) {
    if incoming.updated_at >= existing.updated_at {
        existing.title = incoming.title;
        existing.agent_rename_allowed = incoming.agent_rename_allowed;
        existing.project_id = incoming.project_id;
        existing.provider = incoming.provider;
        existing.model = incoming.model;
        existing.runtime_mode = incoming.runtime_mode;
        existing.reasoning_effort = incoming.reasoning_effort;
        existing.service_tier = incoming.service_tier;
        existing.context_window = incoming.context_window;
        existing.agent_preset = incoming.agent_preset;
        existing.updated_at = incoming.updated_at;
        existing.last_reply_at = incoming.last_reply_at.or(existing.last_reply_at);
        existing.archived_at = incoming.archived_at;
        existing.pinned_at = incoming.pinned_at;
        existing.dormant_at = incoming.dormant_at;
        existing.dormant_exempt_until = incoming.dormant_exempt_until;
        existing.landed_at = incoming.landed_at;
    }
    // The stamp is monotonic; a client's older copy must not clear it.
    existing.boss_managed |= incoming.boss_managed;
    // Planning metadata is daemon-owned too: adopt a marker the daemon
    // copy lacks, never replace or unfreeze one it holds.
    match (&mut existing.planning, incoming.planning) {
        (None, incoming) => existing.planning = incoming,
        (Some(current), Some(incoming)) => current.absorb(&incoming),
        (Some(_), None) => {}
    }
    if incoming.detail_loaded {
        // A detail-loaded projection carries the client's queue edits:
        // every save of a managed session lands here, so a user-owned
        // entry missing from `incoming` was removed, and keeping it would
        // resurrect the chip on the next hydration. Skeletons cleared
        // their queue without seeing it, and agent-owned mirrors stay
        // daemon-owned either way.
        existing.queued_messages.retain(|queued| {
            queued.is_agent_owned()
                || incoming
                    .queued_messages
                    .iter()
                    .any(|candidate| candidate.id == queued.id)
        });
    }
    for queued in incoming.queued_messages {
        // Client saves never create daemon-owned entries: a mirrored agent
        // prompt absent from the daemon's copy was delivered or cancelled,
        // and re-adding it would resurrect — then re-deliver — the prompt.
        if queued.is_agent_owned() {
            continue;
        }
        if !existing
            .queued_messages
            .iter()
            .any(|candidate| candidate.id == queued.id)
        {
            existing.queued_messages.push(queued);
        }
    }
}

/// Client saves never mutate the daemon-owned slice of a follow-up queue:
/// `existing` entries are the truth, `incoming` agent entries are only
/// echoes of them. Union them back before the wholesale replace so a
/// projection written before a mirror arrived cannot erase a parked prompt.
pub(super) fn preserve_daemon_queued_messages(
    existing: &AgentSession,
    incoming: &mut AgentSession,
) {
    incoming
        .queued_messages
        .retain(|queued| !queued.is_agent_owned());
    incoming.queued_messages.extend(
        existing
            .queued_messages
            .iter()
            .filter(|queued| queued.is_agent_owned())
            .cloned(),
    );
    incoming
        .queued_messages
        .sort_by_key(|queued| queued.created_at);
}

/// Ending checkpoints are produced and stored by the daemon. A second client
/// may still save a projection created just before capture completed; never
/// let that stale projection erase the canonical Git snapshot.
pub(super) fn preserve_daemon_checkpoints(existing: &AgentSession, incoming: &mut AgentSession) {
    for turn in &mut incoming.turns {
        let Some(checkpoint) = existing
            .turns
            .iter()
            .find(|candidate| candidate.turn_count == turn.turn_count)
            .and_then(|candidate| candidate.checkpoint.as_ref())
            .filter(|checkpoint| {
                matches!(
                    checkpoint.status,
                    CheckpointStatus::Ready | CheckpointStatus::Unavailable
                )
            })
        else {
            continue;
        };
        turn.checkpoint = Some(checkpoint.clone());
    }
}
