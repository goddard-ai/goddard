use super::*;
use waku_protocol::boss::{BossResult, EmployeeLifecycle, ExpiryCause};

fn queued_fixture(root: &Path) -> (WakuBackend, Uuid, Uuid, Uuid) {
    let (backend, boss) = summon_test_backend(root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 1, 1);
    let (held_task, held) = hold_model_slot(&backend, ProviderKind::Codex, "gpt-5.5");
    let BossResult::Summoned {
        session_id, state, ..
    } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                root,
                "queued archive race",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!("expected queued employee")
    };
    assert_eq!(state, EmployeeLifecycle::Queued);
    (backend, session_id, held_task, held)
}

#[test]
fn delayed_queued_archive_preserves_a_readmitted_ticket_across_restart() {
    let root = std::env::temp_dir().join(format!("queued-archive-resume-{}", Uuid::new_v4()));
    let (backend, id, held_task, held) = queued_fixture(&root);
    let backend = Arc::new(backend);
    let generation = backend
        .boss
        .employee(id)
        .unwrap()
        .ticket
        .unwrap()
        .generation;
    let lifecycle = backend.runtime_lifecycles.session(id);
    let owner = lifecycle.lock();
    let archiving = {
        let backend = backend.clone();
        std::thread::spawn(move || backend.archive_sessions(&[id]))
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !backend
        .task_state
        .lock()
        .sessions
        .iter()
        .any(|row| row.id == id && row.archived_at.is_some())
    {
        assert!(
            std::time::Instant::now() < deadline,
            "archive did not commit"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // Resume owns the same lifecycle gate. Re-admission clears the shell's
    // archive and durably creates a replacement ticket before the old stop.
    backend
        .requeue_employee(id, |ticket, _| {
            ticket.prompt = "replacement assignment".into()
        })
        .unwrap();
    let replacement = backend
        .boss
        .employee(id)
        .unwrap()
        .ticket
        .unwrap()
        .generation;
    assert!(replacement > generation);
    drop(owner);
    archiving.join().unwrap().unwrap();
    let employee = backend.boss.employee(id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    assert!(!employee.cancelled);
    assert_eq!(employee.ticket.as_ref().unwrap().generation, replacement);
    assert_eq!(
        employee.ticket.as_ref().unwrap().prompt,
        "replacement assignment"
    );
    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    drop(backend);
    let reopened = reopen_backend(&root);
    let employee = reopened.boss.employee(id).unwrap();
    assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
    assert!(!employee.cancelled);
    assert_eq!(employee.ticket.unwrap().generation, replacement);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn queued_archive_batch_rechecks_dispatch_before_cancelling_a_runtime() {
    let root = std::env::temp_dir().join(format!("queued-archive-dispatch-{}", Uuid::new_v4()));
    let (backend, id, held_task, held) = queued_fixture(&root);
    let backend = Arc::new(backend);
    {
        let mut state = backend.task_state.lock();
        state.session_mut(id).unwrap().archived_at = Some(1234);
        backend.task_store.save(&mut state).unwrap();
    }
    let lifecycle = backend.runtime_lifecycles.session(id);
    let owner = lifecycle.lock();
    let (attempting, attempt) = crossbeam_channel::bounded(1);
    let stopping = {
        let backend = backend.clone();
        std::thread::spawn(move || {
            attempting.send(()).unwrap();
            backend.stop_queued_employees(&[id]);
        })
    };
    attempt
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let generation = backend
        .boss
        .employee(id)
        .unwrap()
        .ticket
        .unwrap()
        .generation;
    assert!(backend.boss.mark_dispatching(id, generation, None).unwrap());
    assert!(backend.boss.mark_working(id, generation).unwrap());
    let capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        id,
        runtime_generation_entry(Uuid::new_v4(), capture.clone(), &root),
    );
    let token = backend.agent.mint(id);
    drop(owner);
    stopping.join().unwrap();
    assert_eq!(
        backend.boss.employee_lifecycle(id),
        Some(EmployeeLifecycle::Working)
    );
    assert!(!backend.boss.employee(id).unwrap().cancelled);
    assert_eq!(*capture.shutdowns.lock(), 0);
    assert_eq!(backend.agent.resolve(&token), Some(id));
    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn removing_a_project_still_cancels_its_queued_ticket_durably() {
    let root = std::env::temp_dir().join(format!("queued-project-removal-{}", Uuid::new_v4()));
    let (backend, id, held_task, held) = queued_fixture(&root);
    let project = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .find(|row| row.id == id)
        .unwrap()
        .project_id;
    backend.remove_project(project).unwrap();
    let employee = backend.boss.employee(id).unwrap();
    assert!(employee.expired());
    assert!(employee.cancelled);
    assert_eq!(employee.expiry.unwrap().cause, ExpiryCause::Stopped);
    backend
        .resource_broker()
        .unwrap()
        .release_admission(held_task, held);
    drop(backend);
    let reopened = reopen_backend(&root);
    let employee = reopened.boss.employee(id).unwrap();
    assert!(employee.expired());
    assert!(employee.cancelled);
    assert_eq!(employee.expiry.unwrap().cause, ExpiryCause::Stopped);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn queued_retirement_write_failure_rejects_archive_save_and_project_removal() {
    // The failure is at the actual Boss manifest publication boundary. There
    // must be no successful catalog acknowledgment without durable stop intent.
    for mode in ["archive", "save", "remove-project"] {
        let root =
            std::env::temp_dir().join(format!("queued-retirement-{mode}-{}", Uuid::new_v4()));
        let (backend, id, held_task, held) = queued_fixture(&root);
        let mut projection = backend
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .clone();
        let project_id = projection.project_id;
        let original_title = projection.title.clone();
        let original_revision = projection.metadata_revision;
        projection.title = "retirement rename".into();
        projection.archived_at = Some(crate::model::unix_time());
        projection.updated_at += 1;
        let retire = || match mode {
            "archive" => backend.archive_sessions(&[id]),
            "remove-project" => backend.remove_project(project_id),
            _ => backend
                .handle(
                    Request {
                        request_id: Uuid::new_v4(),
                        session_id: id,
                        runtime_id: Uuid::nil(),
                        command: Command::SaveTaskState {
                            projects: Vec::new(),
                            live_session_ids: Vec::new(),
                            sessions: vec![projection.clone()],
                            session_tails: Vec::new(),
                            compact_response: Some(true),
                        },
                    },
                    EventSink::detached(),
                    None,
                )
                .map(|_| ()),
        };
        let path = backend.boss.root().join("boss.json");
        let previous = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let rejected = retire();
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, previous).unwrap();
        assert!(
            rejected.is_err(),
            "{mode} acknowledged a failed queued cancellation"
        );
        assert!(!backend.boss.employee(id).unwrap().cancelled);
        assert!(
            backend
                .task_state
                .lock()
                .projects
                .iter()
                .any(|project| project.id == project_id)
        );
        let state = backend.task_state.lock();
        let row = state.sessions.iter().find(|row| row.id == id).unwrap();
        assert!(
            row.archived_at.is_none(),
            "{mode} published an archive after cancellation was rejected"
        );
        assert_eq!(row.title, original_title);
        assert_eq!(row.metadata_revision, original_revision);
        drop(state);
        retire().unwrap();
        let employee = backend.boss.employee(id).unwrap();
        assert!(
            employee.cancelled && employee.expired(),
            "{mode} retry did not fulfill cancellation"
        );
        backend
            .resource_broker()
            .unwrap()
            .release_admission(held_task, held);
        drop(backend);
        let reopened = reopen_backend(&root);
        assert!(reopened.boss.employee(id).unwrap().cancelled);
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn queued_cancellation_survives_interrupted_finish_and_recovers_even_while_memory_paused() {
    for restart in [false, true] {
        let root =
            std::env::temp_dir().join(format!("queued-cancel-catalog-failed-{}", Uuid::new_v4()));
        let (backend, id, held_task, held) = queued_fixture(&root);
        let BossResult::Summoned {
            session_id: other,
            state,
            ..
        } = backend
            .handle_boss_operation(
                backend.boss.identity_and_session().1,
                summon_op(
                    &backend,
                    &root,
                    "uncancelled pressure hold",
                    ProviderKind::Codex,
                    Some("gpt-5.5"),
                ),
                &EventSink::detached(),
            )
            .unwrap()
        else {
            panic!("expected second queued employee");
        };
        assert_eq!(state, EmployeeLifecycle::Queued);
        let connection = rusqlite::Connection::open(backend.task_store.path()).unwrap();
        // The shell itself is memory-only, so its catalog save is a no-op.
        // Fail final stop persistence after cancellation is durable, and
        // restore the fixture schema before reopening it.
        connection
            .execute_batch("ALTER TABLE workspace_cleanup_intents RENAME TO held_cleanup_intents")
            .unwrap();
        let accepted = backend.archive_sessions(&[id]);
        connection
            .execute_batch("ALTER TABLE held_cleanup_intents RENAME TO workspace_cleanup_intents")
            .unwrap();
        drop(connection);
        assert!(
            accepted.is_ok(),
            "durable cancellation may acknowledge while finish is pending"
        );
        let employee = backend.boss.employee(id).unwrap();
        assert!(employee.cancelled);
        assert_eq!(employee.lifecycle(), EmployeeLifecycle::Queued);
        backend
            .resource_broker()
            .unwrap()
            .release_admission(held_task, held);
        if restart {
            drop(backend);
            std::fs::write(
                root.join("memory-guard/recovery.json"),
                serde_json::to_vec(
                    &serde_json::json!({id.to_string(): {"paused": true, "restarted": false}}),
                )
                .unwrap(),
            )
            .unwrap();
            let reopened = reopen_backend(&root);
            assert!(reopened.memory_guard.paused(id));
            // Recovery must consume cancellation before checking launch/memory
            // admission; it never needs a provider or a host resource slot.
            reopened
                .memory_guard
                .record_host_pressure(Some(crate::pressure::Level::Critical));
            assert!(reopened.memory_guard.host_critical());
            reopened.run_summon_scheduler();
            let other = reopened.boss.employee(other).unwrap();
            assert_eq!(other.lifecycle(), EmployeeLifecycle::Queued);
            assert!(
                !other.cancelled,
                "critical recovery must not dispatch or cancel unrelated work"
            );
            let employee = reopened.boss.employee(id).unwrap();
            assert!(employee.expired() && employee.cancelled);
            assert_eq!(employee.expiry.unwrap().cause, ExpiryCause::Stopped);
            assert!(!reopened.sessions.lock().contains_key(&id));
            drop(reopened);
        } else {
            // The archive flag remains in memory after the failed finish. A
            // repeat request must finish rather than return an empty no-op.
            backend.archive_sessions(&[id]).unwrap();
            assert!(backend.boss.employee(id).unwrap().expired());
            drop(backend);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
