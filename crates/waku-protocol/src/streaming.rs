//! Pure stream folding shared by canonical daemon state and desktop views.

/// Reconcile provider-owned snapshot data with pending composer state. A
/// separate settlement event owns notifications, checkpoints and queue drains;
/// preserve its running turn until that event is applied.
pub fn merge_task_view_snapshot(
    existing: &crate::model::AgentSession,
    snapshot: &mut crate::model::AgentSession,
    preserve_active: bool,
) {
    use crate::model::{MessageRole, TurnStatus};
    if preserve_active
        && let Some(active) = existing
            .turns
            .last()
            .filter(|turn| turn.status == TurnStatus::Running)
        && let Some(settled) = snapshot.turns.iter_mut().find(|turn| turn.id == active.id)
    {
        *settled = active.clone();
    }
    // Runtime setup can deliver its idle baseline before the prepared prompt
    // is dispatched. Keep that optimistic human intent and its presentation.
    if snapshot.active_turn_id().is_none()
        && let Some(turn) = existing.turns.last().filter(|turn| {
            turn.status == TurnStatus::Running
                && !turn.provider_turn_started
                && !snapshot.turns.iter().any(|known| known.id == turn.id)
        })
        && let Some(prompt) = existing
            .messages
            .iter()
            .find(|message| message.turn_id == Some(turn.id) && message.role == MessageRole::User)
    {
        snapshot.turns.push(turn.clone());
        snapshot.messages.push(prompt.clone());
        snapshot.status = existing.status;
    }
    snapshot
        .queued_messages
        .retain(|queued| queued.is_agent_owned());
    snapshot.queued_messages.extend(
        existing
            .queued_messages
            .iter()
            .filter(|queued| !queued.is_agent_owned())
            .cloned(),
    );
    let presentations: std::collections::HashMap<_, _> = existing
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .map(|message| (message.id, message))
        .collect();
    for message in &mut snapshot.messages {
        if let Some(presentation) = presentations.get(&message.id)
            && message.content == presentation.content
        {
            message
                .display_content
                .clone_from(&presentation.display_content);
            message.atoms.clone_from(&presentation.atoms);
            message.attachments.clone_from(&presentation.attachments);
        }
    }
    if existing.metadata_revision > snapshot.metadata_revision {
        crate::task_metadata::TaskMetadata::capture(existing).apply(snapshot);
        snapshot.metadata_revision = existing.metadata_revision;
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
pub fn push_reasoning_delta(content: &mut String, pending_newlines: &mut usize, delta: &str) {
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

/// Whether a text delta resumes a message a mid-stream boundary cut off.
/// Providers emit token-granular chunks and dispatch tool calls wherever a
/// sentence happens to be, so a split lands mid-thought while their own
/// transcript keeps the text as one message. `previous` must read as
/// unfinished — no terminal punctuation — and `delta` must not open a fresh
/// sentence.
pub fn text_delta_rejoins(previous: &str, delta: &str) -> bool {
    let ends_mid_thought = previous
        .trim_end()
        .chars()
        .next_back()
        .is_some_and(|c| !matches!(c, '.' | '!' | '?' | ':' | '…'));
    let opens_fresh = delta.chars().next().is_some_and(char::is_uppercase);
    ends_mid_thought && !opens_fresh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentSession, MessageRole, ProviderKind, QueuedMessage, TurnStatus};

    #[test]
    fn completion_snapshot_keeps_the_turn_settleable_and_preserves_composer_state() {
        let mut local = AgentSession::new(uuid::Uuid::new_v4(), ProviderKind::Codex);
        let turn = local.begin_turn_with_presentation(
            "provider prompt",
            Some("shown prompt".into()),
            Vec::new(),
            Vec::new(),
        );
        let mut completed = local.clone();
        completed.messages[0].display_content = None;
        completed.push_message(MessageRole::Assistant, "output generated while hidden");
        completed.finish_active_turn(TurnStatus::Completed);
        local.queued_messages.push(QueuedMessage::new("follow up"));
        merge_task_view_snapshot(&local, &mut completed, true);
        assert_eq!(completed.active_turn_id(), Some(turn));
        assert_eq!(completed.queued_messages.len(), 1);
        assert_eq!(
            completed.messages[0].display_content.as_deref(),
            Some("shown prompt")
        );
        assert_eq!(
            completed.messages.last().unwrap().content,
            "output generated while hidden"
        );
        completed.finish_active_turn(TurnStatus::Completed);
        assert_eq!(completed.active_turn_id(), None);
    }

    #[test]
    fn idle_baseline_cannot_remove_a_prompt_waiting_for_dispatch() {
        let mut local = AgentSession::new(uuid::Uuid::new_v4(), ProviderKind::Codex);
        let mut baseline = local.clone();
        let turn = local.begin_turn_with_presentation(
            "prepared prompt",
            Some("shown prompt".into()),
            Vec::new(),
            Vec::new(),
        );
        merge_task_view_snapshot(&local, &mut baseline, false);
        assert_eq!(baseline.active_turn_id(), Some(turn));
        assert_eq!(
            baseline.messages.last().unwrap().display_content.as_deref(),
            Some("shown prompt")
        );
    }
}
