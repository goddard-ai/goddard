use super::*;
use crate::boss_rotation::{BossRotationConfig, RotationJournal};
use crate::model::MessageRole;

impl WakuBackend {
    /// The Boss scheduler checks settled chats, including after restart.
    /// Fresh chats use the same durable initialization and lazy provider
    /// launch as BossOperation::Open.
    pub(super) fn reconcile_boss_rotation(&self, now: u64) -> anyhow::Result<()> {
        let settings = self.settings.get();
        if !settings.boss_experiment_enabled || !self.boss.is_active() {
            return Ok(());
        }
        self.boss
            .with_operation_lock(|| self.rotate_boss_chat(&settings, now))
    }

    fn rotate_boss_chat(&self, settings: &crate::DaemonSettings, now: u64) -> anyhow::Result<()> {
        let (identity, active) = self.boss.identity_and_session();
        let Some(active) = active else {
            return Ok(());
        };
        let path = self.data_dir.join("boss/rotation.json");
        let mut journal = RotationJournal::load(&path)?;
        if journal.intent.is_none() && settings.boss_rotation_disabled {
            return Ok(());
        }
        let mut state = self.task_state.lock();
        if journal.intent.is_none() {
            let Some(old) = state.session_mut(active) else {
                return Ok(());
            };
            self.task_store.hydrate(old)?;
            if !boss_chat_settled(old, &self.agent) {
                return Ok(());
            }
            let Some(usage) = old.context_usage else {
                return Ok(());
            };
            let policy = BossRotationConfig::from_settings(settings);
            if !policy.should_rotate(usage.tokens, usage.window) {
                return Ok(());
            }
            journal.active_session_id = Some(active);
            journal.begin(identity.id, active, Uuid::new_v4(), now)?;
            eprintln!(
                "Boss session rotation: boss_id={} old_session_id={} new_session_id={} boss_rotation_context_threshold={} context_tokens={} context_window={}",
                identity.id,
                active,
                journal
                    .intent
                    .as_ref()
                    .expect("begun rotation")
                    .new_session_id,
                policy.context_threshold,
                usage.tokens,
                usage.window.unwrap_or_default(),
            );
            journal.intent.as_mut().expect("begun rotation").reason = Some(format!(
                "boss_rotation_context_threshold={} exceeded (context_tokens={}, context_window={})",
                policy.context_threshold,
                usage.tokens,
                usage.window.unwrap_or_default(),
            ));
            journal.persist(&path)?;
        }
        let intent = journal.intent.as_ref().expect("pending rotation").clone();
        anyhow::ensure!(
            intent.boss_id == identity.id,
            "rotation belongs to another Boss"
        );
        anyhow::ensure!(
            active == intent.old_session_id || active == intent.new_session_id,
            "Boss rotation session pointer diverged"
        );
        let marker = "Boss session rotated.";
        let predecessor = format!("<boss-rotation-link:{}>", intent.old_session_id);
        if active == intent.old_session_id {
            let old = state
                .session_mut(active)
                .ok_or_else(|| anyhow!("rotation source session is missing"))?;
            self.task_store.hydrate(old)?;
            // Hold the task lease through publication: prompt recording
            // cannot open a turn between this check and the archive flag.
            if !boss_chat_settled(old, &self.agent) {
                return Ok(());
            }
            if !state
                .sessions
                .iter()
                .any(|session| session.id == intent.new_session_id)
            {
                let old = state
                    .sessions
                    .iter()
                    .find(|session| session.id == active)
                    .expect("rotation source is present");
                let mut next = AgentSession::new(old.project_id, old.provider);
                next.id = intent.new_session_id;
                next.title = identity.name;
                next.model = old.model.clone();
                next.runtime_mode = old.runtime_mode;
                next.environment = old.environment();
                next.reasoning_effort = old.reasoning_effort.clone();
                next.service_tier = old.service_tier.clone();
                next.context_window = old.context_window.clone();
                next.agent_preset = old.agent_preset.clone();
                next.agent_rename_allowed = false;
                next.boss_managed = true;
                next.pending_provider_context = Some(boss_rotation_handoff(old, marker));
                next.push_message(MessageRole::System, marker);
                next.push_message(MessageRole::System, &predecessor);
                next.messages
                    .last_mut()
                    .expect("rotation link was appended")
                    .hidden = true;
                state.push_session(next);
                // Stage the initialized chat before publishing its id.
                self.task_store.save(&mut state)?;
            }
            self.boss
                .replace_session_id(active, intent.new_session_id)?;
        }
        anyhow::ensure!(
            state
                .sessions
                .iter()
                .any(|session| session.id == intent.new_session_id),
            "published rotation destination is missing"
        );
        let next = state
            .session_mut(intent.new_session_id)
            .ok_or_else(|| anyhow!("rotation destination is missing"))?;
        self.task_store.hydrate(next)?;
        anyhow::ensure!(
            next.boss_managed && next.project_id == identity.id,
            "rotation destination is not a Boss chat"
        );
        if !next
            .messages
            .iter()
            .any(|message| message.role == MessageRole::System && message.content == marker)
        {
            next.push_message(MessageRole::System, marker);
        }
        if !next
            .messages
            .iter()
            .any(|message| message.content == predecessor)
        {
            next.push_message(MessageRole::System, &predecessor);
            next.messages
                .last_mut()
                .expect("rotation link was appended")
                .hidden = true;
        }
        let old = state
            .session_mut(intent.old_session_id)
            .ok_or_else(|| anyhow!("rotation source session is missing"))?;
        self.task_store.hydrate(old)?;
        if !old
            .messages
            .iter()
            .any(|message| message.role == MessageRole::System && message.content == marker)
        {
            old.push_message(MessageRole::System, marker);
        }
        let old_link = format!("<boss-rotation-link:{}>", intent.old_session_id);
        if !old
            .messages
            .iter()
            .any(|message| message.content == old_link)
        {
            old.push_message(MessageRole::System, &old_link);
            old.messages
                .last_mut()
                .expect("rotation link was appended")
                .hidden = true;
        }
        old.archived_at.get_or_insert(now);
        old.updated_at = now;
        self.task_store.save(&mut state)?;
        drop(state);
        // Same runtime/credential retirement as ordinary archival. The
        // transcript stays in the task store under its old session id.
        self.retire_archived_runtimes(&[intent.old_session_id]);
        journal.commit(now)?;
        journal.persist(&path)?;
        // The retired chat's unsent composer draft belongs to its
        // replacement — re-key the persisted slot so clients that load
        // drafts after the swap still see the text. A draft-store failure
        // must not fault the committed rotation.
        if settings.composer_drafts_experiment_enabled
            && let Err(error) = self.composer_drafts.move_draft(
                crate::persistence::ComposerDraftKey::Session(intent.old_session_id),
                crate::persistence::ComposerDraftKey::Session(intent.new_session_id),
            )
        {
            eprintln!("Boss session rotation: composer draft carry failed: {error}");
        }
        self.boss.router_defer_context(intent.new_session_id);
        if let Some(notifier) = self.task_notifier.lock().clone() {
            notifier();
        }
        Ok(())
    }
}

fn boss_chat_settled(session: &AgentSession, agent: &crate::agent::AgentState) -> bool {
    session.archived_at.is_none()
        && !session.status.is_busy()
        && session.active_turn_id().is_none()
        && session.queued_messages.is_empty()
        && !agent.has_open_turn(session.id)
        && !agent.has_queued(session.id)
}

/// Bound the fresh context independently of the old transcript's size. The
/// previous task remains the authoritative source for details and tool output.
fn boss_rotation_handoff(old: &AgentSession, marker: &str) -> String {
    let mut recent = Vec::new();
    let mut remaining = 12_000;
    for message in old.messages.iter().rev().filter(|message| !message.hidden) {
        if remaining == 0 {
            break;
        }
        let text: String = message.content.chars().take(remaining.min(4_000)).collect();
        remaining -= text.chars().count();
        recent.push(format!("{:?}: {text}", message.role));
        if recent.len() == 6 {
            break;
        }
    }
    recent.reverse();
    format!(
        "<boss-session-rotation>\n{marker}\nContinue supervising the existing work. Your identity, employees, plans and memory are unchanged. Read the previous transcript with `goddard-agent boss transcript {}` when needed. Recent conversation (historical context):\n{}\n</boss-session-rotation>",
        old.id,
        recent.join("\n\n"),
    )
}
