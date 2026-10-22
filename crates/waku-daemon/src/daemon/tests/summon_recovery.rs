use super::*;
use waku_protocol::boss::{EmployeeLifecycle, NewOutcome};

#[test]
fn new_outcome_and_finisher_are_published_with_the_employee() {
    let root = std::env::temp_dir().join(format!("summon-atomic-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    let snapshots = Arc::new(Mutex::new(Vec::<waku_protocol::boss::BossState>::new()));
    let saved = snapshots.clone();
    let state_path = backend.boss.root().join("boss.json");
    backend.boss.set_task_notifier(Arc::new(move || {
        saved
            .lock()
            .push(serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap());
    }));
    let mut operation = summon_op(
        &backend,
        &root,
        "finishing assignment",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon {
        new_outcome,
        finishes_outcome,
        ..
    } = &mut operation
    {
        *new_outcome = Some(NewOutcome {
            outcome: "Complete one durable outcome".into(),
            success_criteria: "The finishing assignment succeeds".into(),
        });
        *finishes_outcome = true;
    }
    let BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(Some(boss), operation, &EventSink::detached())
        .unwrap()
    else {
        panic!("expected queued admission")
    };
    assert!(!snapshots.lock().is_empty());
    for snapshot in snapshots
        .lock()
        .iter()
        .filter(|state| !state.outcomes.is_empty())
    {
        assert_eq!(snapshot.outcomes.len(), 1);
        let outcome = &snapshot.outcomes[0];
        let employee = snapshot
            .employees
            .iter()
            .find(|employee| employee.session_id == session_id)
            .expect("a durable outcome must not precede its first employee");
        assert_eq!(employee.assignment.as_ref().unwrap().outcome_id, outcome.id);
        assert_eq!(
            employee.ticket.as_ref().unwrap().outcome_id,
            Some(outcome.id)
        );
        assert_eq!(outcome.finishing_assignment, Some(session_id));
        assert_eq!(outcome.assignments.len(), 1);
    }
    let reopened = crate::boss::BossService::open(backend.boss.root().to_path_buf()).unwrap();
    assert_eq!(
        reopened.document().outcomes[0].finishing_assignment,
        Some(session_id)
    );
    assert_eq!(
        reopened.employee(session_id).unwrap().lifecycle(),
        EmployeeLifecycle::Queued
    );
    drop(reopened);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejected_new_outcome_summons_leave_no_orphan_work() {
    for rejection in ["conflicting intent", "unknown persona", "missing project"] {
        let root = std::env::temp_dir().join(format!("summon-rejection-{}", Uuid::new_v4()));
        let (backend, boss) = summon_test_backend(&root);
        let mut operation = summon_op(
            &backend,
            &root,
            "rejected assignment",
            ProviderKind::Codex,
            Some("gpt-5.5"),
        );
        if let BossOperation::Summon {
            new_outcome,
            finishes_outcome,
            after_success,
            persona_id,
            project,
            ..
        } = &mut operation
        {
            *new_outcome = Some(NewOutcome {
                outcome: "Reject invalid assignments atomically".into(),
                success_criteria: "No orphan outcome survives rejection".into(),
            });
            match rejection {
                "conflicting intent" => {
                    *finishes_outcome = true;
                    *after_success = Some("also start more work".into());
                }
                "unknown persona" => *persona_id = Some(Uuid::new_v4()),
                "missing project" => *project = root.join("missing").display().to_string(),
                _ => unreachable!(),
            }
        }
        assert!(
            backend
                .handle_boss_operation(Some(boss), operation, &EventSink::detached())
                .is_err(),
            "{rejection} should reject admission"
        );
        assert!(backend.boss.document().employees.is_empty(), "{rejection}");
        assert!(
            backend.boss.document().outcomes.is_empty(),
            "{rejection} left an orphan outcome in memory"
        );
        // An unrelated successful write must not flush a rejected parent.
        let valid = backend
            .boss
            .create_outcome(None, "Unrelated valid outcome", "criteria")
            .unwrap();
        let reopened = crate::boss::BossService::open(backend.boss.root().to_path_buf()).unwrap();
        assert_eq!(reopened.document().outcomes.len(), 1, "{rejection}");
        assert_eq!(reopened.document().outcomes[0].id, valid.id);
        assert!(reopened.document().employees.is_empty());
        drop(reopened);
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn failed_summon_commit_keeps_outcome_and_employee_unadmitted() {
    let root = std::env::temp_dir().join(format!("summon-write-failure-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    let state_path = backend.boss.root().join("boss.json");
    let previous = Arc::new(Mutex::new(None));
    let saved = previous.clone();
    let blocked_path = state_path.clone();
    backend.boss.set_task_notifier(Arc::new(move || {
        let mut previous = saved.lock();
        if previous.is_none() {
            // Name allocation is allowed to succeed; the subsequent
            // admission write fails at the real atomic-rename boundary.
            *previous = Some(std::fs::read(&blocked_path).unwrap());
            std::fs::remove_file(&blocked_path).unwrap();
            std::fs::create_dir(&blocked_path).unwrap();
        }
    }));
    let mut operation = summon_op(
        &backend,
        &root,
        "assignment with failed persistence",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon { new_outcome, .. } = &mut operation {
        *new_outcome = Some(NewOutcome {
            outcome: "Persist the complete admission".into(),
            success_criteria: "Both outcome and employee commit".into(),
        });
    }
    let result = backend.handle_boss_operation(Some(boss), operation, &EventSink::detached());
    assert!(result.is_err());
    std::fs::remove_dir(&state_path).unwrap();
    std::fs::write(&state_path, previous.lock().take().unwrap()).unwrap();
    assert!(backend.boss.document().employees.is_empty());
    assert!(backend.boss.document().outcomes.is_empty());
    assert_eq!(
        backend.task_state.lock().sessions.len(),
        1,
        "rejected shell was removed"
    );
    let reopened = crate::boss::BossService::open(backend.boss.root().to_path_buf()).unwrap();
    assert!(reopened.document().employees.is_empty());
    assert!(reopened.document().outcomes.is_empty());
    drop(reopened);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn summon_request_id_survives_retirement_and_restart() {
    let root = std::env::temp_dir().join(format!("summon-retired-retry-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    let mut operation = summon_op(
        &backend,
        &root,
        "original assignment",
        ProviderKind::Codex,
        Some("gpt-5.5"),
    );
    if let BossOperation::Summon { request_id, .. } = &mut operation {
        *request_id = Some(Uuid::new_v4());
    }
    let BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(Some(boss), operation.clone(), &EventSink::detached())
        .unwrap()
    else {
        panic!("expected queued admission")
    };
    backend.stop_queued_employee(session_id).unwrap();
    backend.boss.set_employee_expired_at(session_id, 1).unwrap();
    assert_eq!(backend.boss.retire_expired(3_601).unwrap().len(), 1);
    assert!(backend.boss.document().employees.is_empty());
    drop(backend);
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    *backend.broker_root.lock() = Some(root.join("broker"));

    let result = backend
        .handle_boss_operation(Some(boss), operation.clone(), &EventSink::detached())
        .unwrap();
    assert!(
        matches!(result, BossResult::Summoned {
            session_id: retried,
            state: EmployeeLifecycle::Expired,
            ..
        } if retried == session_id),
        "retry must return the original retired assignment: {result:?}"
    );
    assert!(backend.boss.document().employees.is_empty());
    assert_eq!(backend.boss.document().retired_employees.len(), 1);

    if let BossOperation::Summon { prompt, .. } = &mut operation {
        *prompt = "Different work using the same request ID".into();
    }
    let error = backend
        .handle_boss_operation(Some(boss), operation, &EventSink::detached())
        .unwrap_err();
    assert!(error.to_string().contains("already used"), "{error:#}");
    assert!(backend.boss.document().employees.is_empty());
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
