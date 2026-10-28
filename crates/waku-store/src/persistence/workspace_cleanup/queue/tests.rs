use super::*;
use crate::model::{AgentSession, Project, ProviderKind, SessionWorkspace, TurnStatus};
use std::cell::Cell;

fn fixture(count: usize) -> (PathBuf, StateStore, PersistedState) {
    let root = std::env::temp_dir().join(format!("cleanup-queue-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("repo")).unwrap();
    fs::write(root.join("repo/keep"), b"user files").unwrap();
    let store = StateStore::daemon(root.join("app.db"));
    let mut state = PersistedState::fresh(root.join("repo"));
    state.sessions.clear();
    for number in 1..=count {
        let mut session = AgentSession::new(state.projects[0].id, ProviderKind::Codex);
        session.id = Uuid::from_u128(number as u128);
        session.begin_turn("fixture");
        session.finish_active_turn(TurnStatus::Completed);
        session.archived_at = Some(100);
        state.sessions.push(session);
    }
    store.save(&mut state).unwrap();
    (root, store, state)
}

fn row(store: &StateStore, id: Uuid) -> (String, i64, i64, Option<String>) {
    store.open().unwrap().query_row(
        "SELECT phase, retry_at, failures, last_error FROM workspace_cleanup_intents WHERE session_id=?1",
        [id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap()
}

#[test]
fn dispatch_is_indexed_bounded_and_does_not_hydrate_transcripts() {
    let (root, store, _) = fixture(300);
    let connection = store.open().unwrap();
    connection
        .execute_batch("DROP TABLE session_details; DROP TABLE messages;")
        .unwrap();
    let plan = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {READY}"))
        .unwrap()
        .query_map(params![1000, MAX_DESCRIPTOR_BYTES], |row| {
            row.get::<_, String>(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(plan.contains("workspace_cleanup_dispatch"), "{plan}");
    assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    let ignored = || {
        connection
            .query_row(
                "SELECT count(*) FROM workspace_cleanup_intents WHERE phase='ignored'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .unwrap()
    };
    store
        .advance_workspace_cleanup_queue(1000, || false)
        .unwrap();
    assert_eq!(ignored(), 0);
    assert!(
        store
            .advance_workspace_cleanup_queue(1000, || true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(ignored(), 128);
    let calls = Cell::new(0);
    store
        .advance_workspace_cleanup_queue(1000, || {
            calls.set(calls.get() + 1);
            calls.get() <= 3
        })
        .unwrap();
    assert_eq!(ignored(), 130);
    store
        .advance_workspace_cleanup_queue(1000, || true)
        .unwrap();
    assert_eq!(ignored(), 258);
    store
        .advance_workspace_cleanup_queue(1000, || true)
        .unwrap();
    assert_eq!(ignored(), 300);
    assert_eq!(fs::read(root.join("repo/keep")).unwrap(), b"user files");
    drop(connection);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dispatch_retains_owned_uncertain_and_later_stage_intents_across_reopen() {
    let (root, store, mut state) = fixture(10);
    let worktree = |borrowed, adopted_by| SessionWorkspace::Worktree {
        path: root.join("worktree"),
        name: "fixture".into(),
        branch: None,
        base_branch: None,
        borrowed,
        adopted_by,
    };
    state.sessions[0].workspace = worktree(false, None);
    state.sessions[1].workspace = worktree(true, None);
    state.sessions[2].workspace = worktree(false, Some(Uuid::new_v4()));
    state.sessions[3].workspace = SessionWorkspace::NewWorktree { base_branch: None };
    let project = Project::from_path(
        waku_protocol::projectless::workspace_root()
            .unwrap()
            .join(format!("2099-01-01/cleanup-{}", Uuid::new_v4())),
    );
    state.sessions[4].project_id = project.id;
    state.projects.push(project);
    for id in state
        .sessions
        .iter()
        .map(|session| session.id)
        .collect::<Vec<_>>()
    {
        state.mark_session_dirty(id);
    }
    store.save(&mut state).unwrap();
    let connection = store.open().unwrap();
    for (index, workspace) in [(5, "{broken".to_owned()), (6, "x".repeat(70_000))] {
        let id = state.sessions[index].id.to_string();
        connection
            .execute(
                "UPDATE sessions SET workspace=?2 WHERE id=?1",
                params![id, workspace],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE workspace_cleanup_intents SET workspace=?2 WHERE session_id=?1",
                params![id, workspace],
            )
            .unwrap();
    }
    for (index, phase) in [(7, "verified"), (8, "unrecognized")] {
        connection
            .execute(
                "UPDATE workspace_cleanup_intents SET phase=?2 WHERE session_id=?1",
                params![state.sessions[index].id.to_string(), phase],
            )
            .unwrap();
    }
    connection
        .execute(
            "UPDATE workspace_cleanup_intents SET generation='invalid' WHERE session_id=?1",
            [state.sessions[9].id.to_string()],
        )
        .unwrap();
    drop(connection);
    drop(store);
    let store = StateStore::daemon(root.join("app.db"));
    // Maintenance reads only the promoted catalog, even before detail loading.
    let offered = store
        .advance_workspace_cleanup_queue(1000, || true)
        .unwrap();
    assert_eq!(offered.len(), 3);
    for token in offered {
        assert!(
            [0, 4, 7]
                .iter()
                .any(|&index| state.sessions[index].id == token.session_id)
        );
        assert!(store.workspace_cleanup_run(token).unwrap().is_none());
    }
    for index in [1, 2, 3] {
        assert_eq!(
            row(&store, state.sessions[index].id),
            ("ignored".into(), 0, 0, None)
        );
    }
    for index in [0, 4, 7] {
        let value = row(&store, state.sessions[index].id);
        assert_eq!(value.0, if index == 7 { "verified" } else { "pending" });
        assert_eq!(value.1, 1030);
        assert_eq!(value.2, 0);
    }
    for (index, reason) in [
        (5, "invalid-workspace-descriptor"),
        (6, "oversized-cleanup-descriptor"),
        (8, "invalid-cleanup-phase"),
        (9, "invalid-cleanup-identity"),
    ] {
        assert_eq!(
            row(&store, state.sessions[index].id),
            (
                (if index == 8 {
                    "unrecognized"
                } else {
                    "pending"
                })
                .into(),
                1002,
                1,
                Some(reason.into())
            )
        );
    }
    store
        .advance_workspace_cleanup_queue(1001, || true)
        .unwrap();
    assert_eq!(row(&store, state.sessions[5].id).2, 1);
    store
        .advance_workspace_cleanup_queue(1002, || true)
        .unwrap();
    assert_eq!(row(&store, state.sessions[5].id).1, 1006);
    assert_eq!(row(&store, state.sessions[5].id).2, 2);
    assert_eq!(fs::read(root.join("repo/keep")).unwrap(), b"user files");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn old_queue_pages_cannot_finish_a_rearchive_or_mismatched_catalog() {
    let (root, store, state) = fixture(2);
    let first = state.sessions[0].id;
    let second = state.sessions[1].id;
    let generation = store.workspace_cleanup_generation(first).unwrap();
    let peer = StateStore::daemon(root.join("app.db"));
    peer.load().unwrap();
    let calls = Cell::new(0);
    store
        .advance_workspace_cleanup_queue(1000, || {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                let mut metadata = state.sessions[0].list_projection();
                metadata.archived_at = None;
                metadata.metadata_revision = 1;
                metadata.metadata_mutation_id = Some(Uuid::new_v4());
                peer.write_session_metadata(&metadata, 0, &HashSet::new())
                    .unwrap();
                metadata.archived_at = Some(100);
                metadata.metadata_revision = 2;
                metadata.metadata_mutation_id = Some(Uuid::new_v4());
                peer.write_session_metadata(&metadata, 1, &HashSet::new())
                    .unwrap();
                peer.open()
                    .unwrap()
                    .execute(
                        "UPDATE sessions SET workspace='changed' WHERE id=?1",
                        [second.to_string()],
                    )
                    .unwrap();
            }
            true
        })
        .unwrap();
    assert_ne!(
        store.workspace_cleanup_generation(first).unwrap(),
        generation
    );
    assert_eq!(row(&store, first), ("pending".into(), 0, 0, None));
    assert_eq!(
        row(&store, second),
        (
            "pending".into(),
            1002,
            1,
            Some("catalog-state-mismatch".into())
        )
    );
    drop(peer);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dispatch_defers_sqlite_contention_without_waiting_for_the_writer() {
    let (root, store, _) = fixture(1);
    let connection = store.open().unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let worker_store = StateStore::daemon(root.join("app.db"));
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        send.send(
            worker_store
                .advance_workspace_cleanup_queue(1000, || true)
                .is_err(),
        )
        .unwrap();
    });
    let result = receive.recv_timeout(Duration::from_secs(2));
    connection.execute_batch("ROLLBACK").unwrap();
    worker.join().unwrap();
    assert!(result.unwrap());
    assert_eq!(
        row(&store, Uuid::from_u128(1)),
        ("pending".into(), 0, 0, None)
    );
    store
        .advance_workspace_cleanup_queue(1000, || true)
        .unwrap();
    assert_eq!(row(&store, Uuid::from_u128(1)).0, "ignored");
    drop(connection);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}
