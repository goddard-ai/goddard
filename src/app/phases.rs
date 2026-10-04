//! Activity-based Planning/Executing labels. Model handoffs are evaluated
//! before the next prompt in routing.rs; phase labels never select a model.

use waku_protocol::model::ActivityItem;
use waku_protocol::routing::{PhaseSignal, SessionPhase};

use super::*;

impl Waku {
    pub(super) fn phase_classification_enabled(&self) -> bool {
        self.state.phase_routing_enabled
    }

    /// Track observed activity without evaluation calls or model changes.
    pub(super) fn note_phase_activity(
        &mut self,
        session_id: Uuid,
        item: &ActivityItem,
        cx: &mut Context<Self>,
    ) {
        if !self.phase_classification_enabled() {
            return;
        }
        let Some(session) = self.state.session_mut(session_id) else {
            return;
        };
        let previous = session.phase;
        let phase = session.phase.get_or_insert(SessionPhase::Planning);
        if *phase == SessionPhase::Planning && item.phase_signal() == PhaseSignal::Committing {
            *phase = SessionPhase::Executing;
        }
        if session.phase != previous {
            session.updated_at = unix_time();
            self.state.mark_session_dirty(session_id);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::model::{ActivityFileChange, AgentSession};

    fn activity(kind: ActivityKind, target: Option<&str>, failed: bool) -> ActivityItem {
        let mut item = ActivityItem::new(None, kind, "Edit", target.map(str::to_owned), true);
        item.display_target = target.map(str::to_owned);
        item.failed = failed;
        item
    }

    fn file_change(path: &str) -> ActivityItem {
        let mut item = ActivityItem::new(None, ActivityKind::FileChange, "Edit", None, true);
        item.file_changes = vec![ActivityFileChange {
            path: path.to_owned(),
            additions: None,
            deletions: None,
            status: None,
            diff: None,
        }];
        item
    }

    fn turn_block(turn_id: Uuid, activities: Vec<ActivityItem>) -> TranscriptBlock {
        TranscriptBlock {
            after_message: 0,
            turn_id: Some(turn_id),
            activities,
        }
    }

    #[test]
    fn file_changes_to_code_commit_the_boundary() {
        assert_eq!(
            file_change("src/main.rs").phase_signal(),
            PhaseSignal::Committing
        );
    }

    #[test]
    fn doc_only_writes_stay_ambiguous() {
        for target in ["PLAN.md", "docs/design.md", "notes.txt", "spec-api.rst"] {
            assert_eq!(
                file_change(target).phase_signal(),
                PhaseSignal::Ambiguous,
                "{target} should read as planning output"
            );
        }
        // A write that touches the plan and real code together commits.
        let mut mixed = file_change("plan.md");
        mixed
            .file_changes
            .push(file_change("src/lib.rs").file_changes.pop().unwrap());
        assert_eq!(mixed.phase_signal(), PhaseSignal::Committing);
    }

    #[test]
    fn commands_are_ambiguous_and_reads_are_planning() {
        assert_eq!(
            activity(ActivityKind::Command, Some("cargo test"), false).phase_signal(),
            PhaseSignal::Ambiguous
        );
        assert_eq!(
            activity(ActivityKind::FileRead, Some("src/main.rs"), false).phase_signal(),
            PhaseSignal::Planning
        );
        assert_eq!(
            activity(ActivityKind::Plan, Some("steps"), false).phase_signal(),
            PhaseSignal::Planning
        );
    }

    #[test]
    fn failed_activity_never_commits() {
        assert_eq!(
            file_change("src/main.rs").with_failed(true).phase_signal(),
            PhaseSignal::Ambiguous
        );
    }

    #[test]
    fn truncation_rederives_phase_from_surviving_activities() {
        let mut session =
            AgentSession::new(Uuid::new_v4(), waku_protocol::model::ProviderKind::Claude);
        let turn_one = Uuid::new_v4();
        let turn_two = Uuid::new_v4();
        let turn = |id| waku_protocol::model::AgentTurn {
            id,
            turn_count: 1,
            status: waku_protocol::model::TurnStatus::Completed,
            provider_turn_started: true,
            provider_resume_at: None,
            started_at: unix_time(),
            completed_at: Some(unix_time()),
            checkpoint: None,
        };
        session.turns = vec![turn(turn_one), turn(turn_two)];
        session
            .messages
            .push(waku_protocol::model::Message::new_for_turn(
                MessageRole::Assistant,
                "done",
                turn_two,
            ));
        // Turn one explored, turn two edited — the session earned
        // Executing. Rewinding to turn one drops the edit's block, and the
        // phase follows the surviving evidence back to Planning.
        session.transcript_blocks = vec![
            turn_block(
                turn_one,
                vec![activity(ActivityKind::FileRead, Some("a.rs"), false)],
            ),
            turn_block(turn_two, vec![file_change("src/main.rs")]),
        ];
        session.phase = Some(SessionPhase::Executing);
        session.truncate_after_turn(1);
        assert_eq!(session.phase, Some(SessionPhase::Planning));
    }
}
