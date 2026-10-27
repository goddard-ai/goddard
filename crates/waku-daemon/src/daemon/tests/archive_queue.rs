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
