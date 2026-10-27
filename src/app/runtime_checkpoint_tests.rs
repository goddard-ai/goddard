use super::*;

#[test]
fn checkpoint_completion_rejects_a_reused_turn_count_after_rewind() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let original = session.begin_turn("original");
    session.finish_active_turn(TurnStatus::Completed);
    let count = session.turns.last().unwrap().turn_count;
    assert!(checkpoint_capture_target_exists(
        &session, session.id, original, count
    ));

    session.truncate_after_turn(0);
    let replacement = session.begin_turn("replacement");
    session.finish_active_turn(TurnStatus::Completed);
    assert_ne!(original, replacement);
    assert_eq!(session.turns.last().unwrap().turn_count, count);
    assert!(!checkpoint_capture_target_exists(
        &session, session.id, original, count
    ));
    assert!(checkpoint_capture_target_exists(
        &session,
        session.id,
        replacement,
        count
    ));
}

#[test]
fn checkpoint_targets_require_the_original_settled_nonincognito_turn() {
    let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
    let first = session.begin_turn("first");
    let count = session.turns.last().unwrap().turn_count;
    assert!(!checkpoint_capture_target_exists(
        &session, session.id, first, count
    ));
    session.finish_active_turn(TurnStatus::Interrupted);
    assert!(!checkpoint_capture_target_exists(
        &session,
        Uuid::new_v4(),
        first,
        count
    ));
    assert!(!checkpoint_capture_target_exists(
        &session,
        session.id,
        first,
        count + 1
    ));
    session.begin_turn("later turn");
    assert!(checkpoint_capture_target_exists(
        &session, session.id, first, count
    ));
    session.incognito = true;
    assert!(!checkpoint_capture_target_exists(
        &session, session.id, first, count
    ));
}
