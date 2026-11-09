//! Pure stream folding shared by canonical daemon state and desktop views.

/// Reconcile provider-owned snapshot data with pending composer state. A
/// separate settlement event owns notifications, checkpoints and queue drains;
/// preserve its running turn until that event is applied.
pub fn merge_task_view_snapshot(
    existing: &crate::model::AgentSession,
    snapshot: &mut crate::model::AgentSession,
    preserve_active: bool,
) {
    use crate::model::{MessageRole, SessionWorkspace, TurnStatus};
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
    // A provider snapshot can precede acknowledgment of a local steer.
    // Preserve its visible preview until delivery consumes the queue entry;
    // otherwise the refresh makes a sent prompt disappear while it waits.
    let message_ids: std::collections::HashSet<_> =
        snapshot.messages.iter().map(|message| message.id).collect();
    let queued_ids: std::collections::HashSet<_> = snapshot
        .queued_messages
        .iter()
        .filter(|queued| !queued.is_agent_owned())
        .map(|queued| queued.id)
        .collect();
    snapshot.messages.extend(
        existing
            .messages
            .iter()
            .filter(|message| {
                message.role == MessageRole::User
                    && queued_ids.contains(&message.id)
                    && !message_ids.contains(&message.id)
            })
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
    // Preparation may have assigned a checkout after this startup snapshot
    // was captured. Replaying its draft intent must not create a second
    // worktree on the next prompt while the provider stays in the first.
    if matches!(snapshot.workspace, SessionWorkspace::NewWorktree { .. })
        && matches!(existing.workspace, SessionWorkspace::Worktree { .. })
    {
        snapshot.workspace.clone_from(&existing.workspace);
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
    fn pending_steer_stays_visible_across_snapshots_until_delivered() {
        let mut local = AgentSession::new(uuid::Uuid::new_v4(), ProviderKind::Codex);
        local.begin_turn("initial prompt");
        let baseline = local.clone();
        let queued = QueuedMessage::new("provider follow up");
        let mut preview = crate::model::Message::new(MessageRole::User, &queued.content)
            .with_presentation(Some("shown follow up".into()), Vec::new(), Vec::new());
        preview.id = queued.id;
        local.messages.push(preview.clone());
        local.queued_messages.push(queued.clone());

        let mut refreshed = baseline.clone();
        merge_task_view_snapshot(&local, &mut refreshed, false);
        assert_eq!(refreshed.messages.last().unwrap().id, queued.id);
        assert_eq!(
            refreshed
                .messages
                .last()
                .unwrap()
                .display_content
                .as_deref(),
            Some("shown follow up")
        );
        assert_eq!(refreshed.queued_messages, vec![queued.clone()]);

        // An acknowledged preview becomes the canonical message, once only.
        let mut accepted = baseline.clone();
        accepted.messages.push(preview);
        merge_task_view_snapshot(&refreshed, &mut accepted, false);
        assert_eq!(accepted.messages.len(), 2);
        local = accepted;
        local.queued_messages.clear();
        let mut later = local.clone();
        merge_task_view_snapshot(&local, &mut later, false);
        assert_eq!(later.messages.len(), 2);
        assert!(later.queued_messages.is_empty());

        // Cancellation must not resurrect the preview on the next refresh.
        local.messages.retain(|message| message.id != queued.id);
        let mut cancelled = baseline;
        merge_task_view_snapshot(&local, &mut cancelled, false);
        assert_eq!(cancelled.messages.len(), 1);
        assert!(cancelled.queued_messages.is_empty());
    }

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
    fn delayed_startup_snapshot_cannot_recreate_an_assigned_worktree() {
        use crate::model::SessionWorkspace;

        let mut local = AgentSession::new(uuid::Uuid::new_v4(), ProviderKind::Codex);
        local.workspace = SessionWorkspace::NewWorktree {
            base_branch: Some("dev".into()),
        };
        local.begin_turn("first prompt");
        let baseline = local.clone();
        local.workspace = SessionWorkspace::Worktree {
            path: "/workspace/assigned".into(),
            name: "assigned".into(),
            branch: None,
            base_branch: Some("dev".into()),
            adopted_by: None,
            borrowed: false,
        };
        for preserve_active in [false, true] {
            let mut delayed = baseline.clone();
            merge_task_view_snapshot(&local, &mut delayed, preserve_active);
            delayed.begin_turn("follow up");
            assert_eq!(delayed.workspace, local.workspace);
        }

        // A concrete daemon-side move is still authoritative.
        let mut moved = baseline;
        moved.workspace = SessionWorkspace::Local;
        merge_task_view_snapshot(&local, &mut moved, false);
        assert_eq!(moved.workspace, SessionWorkspace::Local);
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
