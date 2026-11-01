use super::*;

#[test]
fn codex_history_refresh_reads_cli_turns_persists_once_and_can_update_a_stale_window() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("codex-history-refresh-{}", Uuid::new_v4()));
    let (backend, session_id) = surface_test_backend(&root);
    let native = root.join("codex");
    std::fs::write(&native, r#"#!/usr/bin/env python3
import json, sys
from pathlib import Path
for line in sys.stdin:
    request = json.loads(line)
    if request.get('method') == 'initialize':
        print(json.dumps({'id': request['id'], 'result': {}}), flush=True)
    elif request.get('method') == 'thread/read':
        assert request['params'] == {'threadId': 'shared-thread', 'includeTurns': True}
        print(json.dumps({'id': request['id'], 'result': {'thread': {'turns': json.loads(Path(__file__).with_suffix('.json').read_text())}}}), flush=True)
"#).unwrap();
    std::fs::set_permissions(&native, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut settings = backend.settings.get();
    settings
        .provider_binary_overrides
        .insert(ProviderKind::Codex, native.display().to_string());
    backend.settings.replace(settings).unwrap();
    let original_message_id;
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .unwrap();
        backend.task_store.hydrate(session).unwrap();
        session.provider = ProviderKind::Codex;
        session.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "shared-thread".into(),
        });
        session.turns[0].provider_turn_started = true;
        original_message_id = session.messages[0].id;
        state.mark_session_dirty(session_id);
        backend.task_store.save(&mut state).unwrap();
    }
    let native_turn = |prompt: &str| {
        json!({
            "id": Uuid::new_v4(), "status": "completed", "startedAt": 10, "completedAt": 11,
            "items": [{"type": "userMessage", "content": [{"type": "text", "text": prompt}]},
                {"type": "agentMessage", "text": "native answer"}]
        })
    };
    // The CLI's persisted history gains a turn while Goddard keeps its saved copy.
    let mut turns = vec![native_turn("seed")];
    std::fs::write(
        native.with_extension("json"),
        serde_json::to_vec(&turns).unwrap(),
    )
    .unwrap();
    let refresh = || {
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id: Uuid::nil(),
                    command: Command::RefreshSessionHistory,
                },
                EventSink::detached(),
                None,
            )
            .unwrap()
    };
    refresh();
    turns.push(native_turn("sent from CLI"));
    std::fs::write(
        native.with_extension("json"),
        serde_json::to_vec(&turns).unwrap(),
    )
    .unwrap();
    for _ in 0..2 {
        let ResponsePayload::ProviderSessionHistory { history, .. } = refresh() else {
            panic!("expected refreshed history")
        };
        // The second response still serves a window behind the daemon's copy.
        assert_eq!(history.turns.len(), 2);
        let mut client = backend
            .task_store
            .load_session_detail(session_id)
            .unwrap()
            .unwrap();
        if waku_protocol::provider_session::append_codex_history(&mut client, &history) {
            backend
                .handle(
                    Request {
                        request_id: Uuid::new_v4(),
                        session_id,
                        runtime_id: Uuid::nil(),
                        command: Command::SaveTaskState {
                            projects: vec![],
                            live_session_ids: vec![session_id],
                            sessions: vec![client],
                            session_tails: vec![],
                        },
                    },
                    EventSink::detached(),
                    None,
                )
                .unwrap();
        }
        let stored = backend
            .task_store
            .load_session_detail(session_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.turns.len(), 2);
        assert_eq!(stored.messages.len(), 3);
        assert_eq!(stored.messages[0].id, original_message_id);
        assert_eq!(stored.messages[1].content, "sent from CLI");
    }
    // Failed provider reads leave the persisted transcript intact.
    std::fs::write(native.with_extension("json"), "invalid json").unwrap();
    assert!(
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id,
                    runtime_id: Uuid::nil(),
                    command: Command::RefreshSessionHistory,
                },
                EventSink::detached(),
                None
            )
            .is_err()
    );
    assert_eq!(
        backend
            .task_store
            .load_session_detail(session_id)
            .unwrap()
            .unwrap()
            .messages
            .len(),
        3
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
