use super::*;
use crate::model::ActivityKind;

fn activity_history() -> AgentSession {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    session.begin_turn("inspect history");
    for block in 0..32 {
        let activities = (0..6)
            .map(|item| {
                let mut activity = ActivityItem::new(
                    Some(format!("call-{block}-{item}")),
                    ActivityKind::Tool,
                    format!("tool {block}-{item}"),
                    None,
                    true,
                );
                activity.output = Some("historical output\n".repeat(256));
                activity
            })
            .collect::<Vec<_>>();
        session.transcript_blocks.push(TranscriptBlock {
            after_message: session.messages.len(),
            turn_id: session.active_turn_id(),
            activities: activities.into(),
        });
    }
    session
}

#[test]
fn activity_updates_preserve_unmatched_shared_history_and_select_the_latest_row() {
    for selection in ["source", "title", "missing"] {
        let mut session = activity_history();
        for (block, item) in [(3, 0), (8, 2), (8, 4)] {
            let activity = &mut session.transcript_blocks[block].activities[item];
            activity.source_id = Some("reused-call".into());
            activity.title = "pending tool".into();
            activity.complete = false;
        }
        // Title-only updates skip completed rows even when they are newer.
        session.transcript_blocks[20].activities[0].title = "pending tool".into();
        session.updated_at = 123;
        let saved = session.clone();
        let identity = session.transcript_blocks.snapshot_identity();
        let update = ActivityItem::new(
            (selection != "title").then(|| {
                if selection == "missing" {
                    "absent-call".into()
                } else {
                    "reused-call".into()
                }
            }),
            ActivityKind::Command,
            "pending tool",
            Some("updated detail".into()),
            true,
        );
        let update_id = update.id;
        let result = update_transcript_activity(&mut session, update);
        if selection == "missing" {
            assert_eq!(result.unwrap_err().id, update_id);
            assert!(session.transcript_blocks.snapshot_identity() == identity);
            assert_eq!(session.updated_at, 123);
        } else {
            assert_eq!(
                result.unwrap(),
                (saved.transcript_blocks[8].activities[4].id, false)
            );
            let changed = &session.transcript_blocks[8].activities[4];
            assert!(changed.complete);
            assert_eq!(changed.detail.as_deref(), Some("updated detail"));
            assert_eq!(changed.kind, ActivityKind::Command);
            assert!(!saved.transcript_blocks[8].activities[4].complete);
            assert!(saved.transcript_blocks[8].activities[4].detail.is_none());
        }
        for (index, (current, previous)) in session
            .transcript_blocks
            .iter()
            .zip(saved.transcript_blocks.iter())
            .enumerate()
        {
            if selection != "missing" && index == 8 {
                for item in [0, 1, 2, 3, 5] {
                    assert!(std::ptr::eq(
                        &current.activities[item],
                        &previous.activities[item]
                    ));
                }
            } else {
                assert!(
                    std::ptr::eq(current, previous),
                    "{selection}: detached block {index}"
                );
                assert!(
                    current.activities.snapshot_identity()
                        == previous.activities.snapshot_identity()
                );
            }
        }
    }
}

#[test]
fn reasoning_completion_preserves_skipped_history_and_noop_identity() {
    let mut session = activity_history();
    for (block, item, complete) in [(4, 1, false), (8, 4, false), (20, 0, true)] {
        session.transcript_blocks[block].activities[item] = ActivityItem::from_reasoning(
            ReasoningBlock {
                content: "thinking\n".repeat(64 * 1024).into(),
                started_at_ms: 1,
                finished_at_ms: 2,
            },
            complete,
        );
    }
    // Unfinished tool work after the reasoning is outside this transition.
    session.transcript_blocks[31].activities[5].complete = false;
    let saved = session.clone();
    complete_latest_reasoning_activity(&mut session);
    assert!(session.transcript_blocks[8].activities[4].complete);
    assert!(!session.transcript_blocks[4].activities[1].complete);
    assert!(!session.transcript_blocks[31].activities[5].complete);
    assert!(!saved.transcript_blocks[8].activities[4].complete);
    assert!(
        std::ptr::eq(
            session.transcript_blocks[8].activities[4]
                .reasoning
                .as_ref()
                .unwrap()
                .content
                .as_ptr(),
            saved.transcript_blocks[8].activities[4]
                .reasoning
                .as_ref()
                .unwrap()
                .content
                .as_ptr(),
        ),
        "completing reasoning copied its text"
    );
    for index in 0..session.transcript_blocks.len() {
        if index == 8 {
            for item in [0, 1, 2, 3, 5] {
                assert!(std::ptr::eq(
                    &session.transcript_blocks[index].activities[item],
                    &saved.transcript_blocks[index].activities[item],
                ));
            }
        } else {
            assert!(std::ptr::eq(
                &session.transcript_blocks[index],
                &saved.transcript_blocks[index],
            ));
        }
    }
    // Preserve the existing newest-unfinished selection even for old leftovers.
    complete_latest_reasoning_activity(&mut session);
    assert!(session.transcript_blocks[4].activities[1].complete);
    assert!(!saved.transcript_blocks[4].activities[1].complete);
    let settled = session.transcript_blocks.clone();
    let identity = session.transcript_blocks.snapshot_identity();
    session.updated_at = 123;
    complete_latest_reasoning_activity(&mut session);
    assert!(session.transcript_blocks.snapshot_identity() == identity);
    assert!(session.transcript_blocks.shares_items_with(&settled));
    assert_eq!(session.updated_at, 123);
}
