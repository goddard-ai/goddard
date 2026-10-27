use super::*;
use waku_protocol::boss::{
    EmployeeControl, EmployeeLifecycle, EmployeeSettle, PermissionOverrides,
};

#[test]
fn audit_failed_resume_preserves_parked_prompts() {
    let root = std::env::temp_dir().join(format!("boss-resume-atomic-{}", Uuid::new_v4()));
    let (backend, boss) = summon_test_backend(&root);
    set_model_policy(&backend, ProviderKind::Codex, "gpt-5.5", 0, 0);
    let BossResult::Summoned { session_id, .. } = backend
        .handle_boss_operation(
            Some(boss),
            summon_op(
                &backend,
                &root,
                "original",
                ProviderKind::Codex,
                Some("gpt-5.5"),
            ),
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!()
    };
    backend.boss.set_employee_expired_at(session_id, 1).unwrap();
    backend.boss.retire_expired(10_000).unwrap();
    let name = backend
        .boss
        .employee_including_retired(session_id)
        .unwrap()
        .identity
        .name;
    let mut replacement = backend
        .boss
        .prepare_employee(
            boss,
            None,
            "replacement".into(),
            None,
            false,
            waku_protocol::boss::EmployeeGoal::Errand,
            None,
        )
        .unwrap();
    replacement.identity.name = name;
    backend.boss.add_employee(replacement).unwrap();
    let mut queued = crate::model::QueuedMessage::new("preserve this prompt");
    queued.source = crate::model::QueuedMessageSource::Agent {
        sent_by: Some(boss),
    };
    {
        let mut state = backend.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .unwrap();
        session.queued_messages.push(queued.clone());
        state.mark_session_dirty(session_id);
        backend.task_store.save(&mut state).unwrap();
    }
    assert!(backend.resume_employee(session_id).is_err());
    let state = backend.task_state.lock();
    assert!(
        state
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .unwrap()
            .queued_messages
            .iter()
            .any(|entry| entry.id == queued.id)
    );
    drop(state);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn audit_completion_delivery_retries_with_the_same_message_identity() {
    let root = std::env::temp_dir().join(format!("boss-completion-outbox-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, parent, _) = employee_finish_fixture(&root);
    backend
        .boss
        .set_employee_goal(employee_id, waku_protocol::boss::EmployeeGoal::Errand)
        .unwrap();
    backend
        .boss
        .begin_finishing(
            employee_id,
            false,
            false,
            waku_protocol::boss::ExpiryCause::Finished,
        )
        .unwrap();
    let completion = backend
        .boss
        .complete_expiry_with_report(
            employee_id,
            waku_protocol::boss::EmployeeExpiry::settle(waku_protocol::boss::ExpiryCause::Finished),
            "transcript index".into(),
            false,
            "done".into(),
            EmployeeSettle::TurnFinished,
        )
        .unwrap();
    let report_id = completion.report_id.unwrap();
    let report = backend.boss.pending_completion_reports().pop().unwrap();
    let continuity = backend.boss_continuity_path();
    let previous = std::fs::read(&continuity).ok();
    std::fs::create_dir_all(continuity.parent().unwrap()).unwrap();
    std::fs::write(&continuity, "invalid receipt store").unwrap();
    assert!(backend.deliver_completion_report(report.clone()).is_err());
    assert_eq!(backend.boss.pending_completion_reports().len(), 1);
    if let Some(previous) = previous {
        std::fs::write(&continuity, previous).unwrap();
    } else {
        std::fs::remove_file(&continuity).unwrap();
    }
    let manifest = backend.boss.root().join("boss.json");
    std::fs::remove_file(&manifest).unwrap();
    std::fs::create_dir(&manifest).unwrap();
    assert!(backend.deliver_completion_report(report.clone()).is_err());
    assert_eq!(backend.boss.pending_completion_reports().len(), 1);
    std::fs::remove_dir(&manifest).unwrap();
    backend.deliver_completion_report(report).unwrap();
    assert!(backend.boss.pending_completion_reports().is_empty());
    let state = backend.task_state.lock();
    let session = state.sessions.iter().find(|s| s.id == supervisor).unwrap();
    let occurrences = session
        .messages
        .iter()
        .filter(|m| m.id == report_id)
        .count()
        + session
            .queued_messages
            .iter()
            .filter(|m| m.id == report_id)
            .count();
    assert_eq!(occurrences, 1);
    let _ = parent;
    drop(state);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn audit_permission_revocation_retires_the_running_runtime() {
    let root = std::env::temp_dir().join(format!("boss-revoke-runtime-{}", Uuid::new_v4()));
    let (backend, supervisor, employee_id, _, _) = employee_finish_fixture(&root);
    backend
        .boss
        .set_employee_permissions(
            None,
            employee_id,
            PermissionOverrides {
                integration_ids: Some(vec!["linear".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(backend.sessions.lock().contains_key(&employee_id));
    backend
        .handle_boss_operation(
            Some(supervisor),
            BossOperation::Control {
                session_id: employee_id,
                action: EmployeeControl::SetPermissions {
                    permissions: PermissionOverrides {
                        integration_ids: Some(vec![]),
                        ..Default::default()
                    },
                },
            },
            &EventSink::detached(),
        )
        .unwrap();
    assert!(!backend.sessions.lock().contains_key(&employee_id));
    assert!(
        backend
            .boss
            .employee(employee_id)
            .unwrap()
            .permissions
            .integration_ids
            .is_empty()
    );
    assert_ne!(
        backend.boss.employee(employee_id).unwrap().lifecycle(),
        EmployeeLifecycle::Expired
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn audit_chain_resume_denies_foreign_callers_before_verifying_or_returning_state() {
    let root = std::env::temp_dir().join(format!("boss-chain-authority-{}", Uuid::new_v4()));
    let (backend, boss, head) = chain_backend(&root);
    let project = backend.task_state.lock().projects[0].id;
    let persona = backend.boss.document().personas[0].id;
    let outcome = backend
        .boss
        .create_outcome(Some(boss), "audit", "verified")
        .unwrap();
    let definition = chain_test_definition(project, outcome.id, persona, 0, head);
    let BossResult::State { state } = backend
        .handle_boss_operation(
            Some(boss),
            BossOperation::Chain {
                action: ChainOperation::Create {
                    request_id: Uuid::new_v4(),
                    definition,
                },
            },
            &EventSink::detached(),
        )
        .unwrap()
    else {
        panic!()
    };
    let chain = &state.chains[0];
    let attempt = &chain.steps[0].attempts[0];
    backend
        .boss
        .observe_chain(
            chain.id,
            attempt.id,
            attempt.definition_version,
            Uuid::new_v4(),
            waku_boss::chains::ChainObservation::Terminal {
                terminal: waku_protocol::chains::ChainTerminal {
                    message: "done".into(),
                    disposition: ResultDisposition::Success,
                },
            },
        )
        .unwrap();
    backend
        .boss
        .observe_chain(
            chain.id,
            attempt.id,
            attempt.definition_version,
            Uuid::new_v4(),
            waku_boss::chains::ChainObservation::Released {
                execution: true,
                workspace: true,
            },
        )
        .unwrap();
    backend
        .boss
        .seal_chain_handoff(
            chain.id,
            attempt.id,
            attempt.definition_version,
            &Default::default(),
        )
        .unwrap();
    let before = serde_json::to_value(backend.boss.document()).unwrap();
    let revision = backend.boss.document().chains[0].revision;
    let error = backend
        .handle_boss_operation(
            Some(Uuid::new_v4()),
            BossOperation::Chain {
                action: ChainOperation::Control {
                    chain_id: chain.id,
                    expected_revision: revision,
                    control: ChainControl::Resume,
                    source_revision: None,
                },
            },
            &EventSink::detached(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("only the boss or a human"));
    assert_eq!(
        serde_json::to_value(backend.boss.document()).unwrap(),
        before
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
