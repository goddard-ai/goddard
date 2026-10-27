//! Real-boundary observations for the Boss rotation safety oracle.

use super::*;
use crate::model::ActivityKind;
use waku_boss::rotation_oracle::{
    AdmissionDecision, CompactionStage, ContextSampleSource, LifecycleObservation, OracleViolation,
    ProviderAdmission, ProviderCompaction, ProviderTransport, RotationLifecycleStage,
    RotationObservation,
};

fn install_rotation_observer(backend: &WakuBackend) -> Arc<Mutex<Vec<RotationObservation>>> {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let sink = observations.clone();
    *backend.runtime_lifecycles.rotation_observer.lock() = Some(Arc::new(move |observation| {
        sink.lock().push(observation);
    }));
    observations
}

fn boss_runtime(backend: &WakuBackend, session_id: Uuid, runtime_id: Uuid) -> Arc<CaptureDriver> {
    let capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            plan_gate: None,
            runtime_id,
            driver: backend.boss_driver_boundary(
                session_id,
                runtime_id,
                DriverHandle::from_control(capture.clone()),
            ),
            stable: None,
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: backend.default_cwd.clone(),
        },
    );
    capture
}

#[test]
fn frozen_boss_admission_fails_closed_and_human_input_stays_queued() {
    let root = std::env::temp_dir().join(format!("boss-admission-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let observed = install_rotation_observer(&backend);
    let capture = boss_runtime(&backend, boss, runtime);

    waku_boss::rotation_continuity::ContinuityLedger::update(
        &backend.boss_continuity_path(),
        |ledger| ledger.freeze_coordinator(boss, Uuid::new_v4(), 0),
    )
    .unwrap()
    .unwrap();

    assert!(backend.ensure_coordinator_available(boss).is_err());
    let error = backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: boss,
                runtime_id: runtime,
                command: Command::Boss {
                    operation: waku_protocol::boss::BossOperation::View,
                },
            },
            EventSink::detached(),
            Some(boss),
        )
        .unwrap_err();
    assert!(error.to_string().contains("fenced"));

    let request_id = Uuid::new_v4();
    backend
        .handle(
            Request {
                request_id,
                session_id: boss,
                runtime_id: runtime,
                command: Command::Prompt {
                    prompt: "continue the coordinated work".into(),
                    turn_id: None,
                    message_id: None,
                    hidden: false,
                    attachments: Vec::new(),
                },
            },
            EventSink::detached(),
            None,
        )
        .unwrap();

    assert!(capture.prompts.lock().is_empty());
    assert!(
        backend
            .task_state
            .lock()
            .session_mut(boss)
            .unwrap()
            .queued_messages
            .iter()
            .any(|m| m.content == "continue the coordinated work")
    );
    backend
        .sessions
        .lock()
        .get(&boss)
        .unwrap()
        .driver
        .prompt("direct call also denied".into());
    assert!(capture.prompts.lock().is_empty());
    let observations = observed.lock().clone();
    let admission = observations
        .iter()
        .find_map(|observation| match observation {
            RotationObservation::ProviderAdmission(ProviderAdmission {
                operation_id,
                session_id,
                runtime_id,
                owner_generation,
                boundary,
                decision,
            }) => Some((
                operation_id.clone(),
                *session_id,
                *runtime_id,
                *owner_generation,
                *boundary,
                decision.clone(),
            )),
            _ => None,
        })
        .expect("provider command crossed the admission seam");
    assert!(admission.0.starts_with("boss-driver:"));
    assert_eq!(admission.1, boss);
    assert_eq!(admission.2, runtime);
    assert_eq!(admission.3, Some(0));
    assert!(matches!(admission.5, AdmissionDecision::Blocked { .. }));
    assert!(
        observations
            .iter()
            .any(|observation| { matches!(observation, RotationObservation::Ownership(_)) })
    );

    let report = waku_boss::rotation_oracle::RotationOracle::evaluate(observations);
    assert_eq!(report.unsafe_admissions, 0);
    assert!(
        !report
            .violations
            .iter()
            .any(|v| matches!(v, OracleViolation::MissingAdmissionGuard { .. }))
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn manual_boss_refresh_is_best_effort_even_with_automatic_rotation_disabled() {
    use waku_boss::rotation_continuity::ContinuityLedger;
    let root = std::env::temp_dir().join(format!("boss-compact-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let observed = install_rotation_observer(&backend);
    let capture = boss_runtime(&backend, boss, runtime);
    // Manual refresh uses ordinary provider startup, independently of the
    // automatic threshold preference and strict prevention qualification.
    let mut settings = backend.settings.get();
    settings.boss_rotation_disabled = true;
    backend.settings.replace(settings).unwrap();

    backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: boss,
                runtime_id: runtime,
                command: Command::Compact,
            },
            EventSink::detached(),
            None,
        )
        .unwrap();

    // Compact never reaches the provider; the active chat remains usable
    // until the scheduler performs a settled session swap.
    assert!(capture.prompts.lock().is_empty());
    let ledger = ContinuityLedger::load(&root.join("boss/continuity.json")).unwrap();
    assert!(ledger.rotation_request.is_some());
    assert!(!backend.boss_incident_fenced(boss));
    let driver = backend.sessions.lock().get(&boss).unwrap().driver.clone();
    driver.compact();
    driver.prompt("continue the healthy chat".into());
    assert_eq!(capture.prompts.lock().len(), 1);

    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_ok()
    );
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(boss, next);
    let ledger = ContinuityLedger::load(&root.join("boss/continuity.json")).unwrap();
    assert!(ledger.rotation_request.is_none());

    driver.prompt("retired credentials cannot coordinate".into());
    assert_eq!(capture.prompts.lock().len(), 1);

    let observations = observed.lock().clone();
    assert!(
        !observations.iter().any(|observation| {
            matches!(observation, RotationObservation::ProviderCompaction(_))
        })
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn unreadable_continuity_holds_boss_growth_and_preserves_the_handoff() {
    let root = std::env::temp_dir().join(format!("boss-ledger-fence-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let capture = boss_runtime(&backend, boss, runtime);
    let path = root.join("boss/continuity.json");
    std::fs::write(&path, b"not a continuity ledger").unwrap();
    {
        let mut state = backend.task_state.lock();
        state.session_mut(boss).unwrap().pending_provider_context = Some("retained handoff".into());
        backend.task_store.save(&mut state).unwrap();
    }
    let driver = backend.sessions.lock().get(&boss).unwrap().driver.clone();
    driver.prompt("denied growth".into());
    driver.steer("denied continuation".into());
    assert!(capture.prompts.lock().is_empty());
    assert!(capture.steers.lock().is_empty());
    assert!(backend.boss_incident_fenced(boss));
    assert!(super::super::prompt_delivery::boss_incident_fenced(
        &backend.boss,
        boss
    ));
    // Ledger corruption must not hold unrelated employees' deliveries.
    assert!(!super::super::prompt_delivery::boss_incident_fenced(
        &backend.boss,
        Uuid::new_v4()
    ));
    super::super::events::boss_continuity_note_event(
        &backend.boss,
        &backend.task_state,
        &backend.task_store,
        boss,
        &DriverEvent::TurnStarted,
    );
    assert_eq!(
        backend
            .task_state
            .lock()
            .session_mut(boss)
            .unwrap()
            .pending_provider_context
            .as_deref(),
        Some("retained handoff"),
    );
    std::fs::write(&path, b"{}").unwrap();
    backend
        .task_state
        .lock()
        .sessions
        .retain(|session| session.id != boss);
    driver.prompt("missing session cannot coordinate".into());
    assert!(capture.prompts.lock().is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn provider_events_feed_context_transport_and_compaction_observations() {
    let root = std::env::temp_dir().join(format!("boss-event-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let observed = install_rotation_observer(&backend);
    let capture = boss_runtime(&backend, boss, runtime);
    let (sender, receiver) = driver::test_event_channel();
    let mut compacting = ActivityItem::new(
        Some("compact-1".into()),
        ActivityKind::Tool,
        "Compacting context",
        None,
        false,
    );
    compacting.title_i18n = Some(localized!("activity.compacting_context").1);
    for event in [
        DriverEvent::UsageUpdated {
            context_tokens: Some(81),
            context_window: Some(100),
        },
        DriverEvent::TurnStarted,
        DriverEvent::RichActivity(compacting),
        DriverEvent::ProcessExited,
    ] {
        sender.send(event).unwrap();
    }
    drop(sender);

    runtime_generation_forward(
        &backend,
        boss,
        runtime,
        receiver,
        EventSink::detached(),
        DriverHandle::from_control(capture),
    );

    let observations = observed.lock().clone();
    assert!(observations.iter().any(|observation| {
        matches!(
            observation,
            RotationObservation::ContextSample(sample)
                if sample.context_tokens == 81
                    && sample.context_window == Some(100)
                    && sample.source == ContextSampleSource::ProviderReport
        )
    }));
    assert!(observations.iter().any(|observation| {
        matches!(
            observation,
            RotationObservation::ProviderTransport(ProviderTransport {
                status: waku_boss::rotation_oracle::ProviderTransportStatus::Accepted,
                ..
            })
        )
    }));
    assert!(observations.iter().any(|observation| {
        matches!(
            observation,
            RotationObservation::ProviderCompaction(ProviderCompaction {
                stage: CompactionStage::Started,
                ..
            })
        )
    }));
    let report = waku_boss::rotation_oracle::RotationOracle::evaluate(observations);
    assert_eq!(report.context_samples, 1);
    assert_eq!(report.provider_acceptances, 1);
    assert_eq!(report.compactions, 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn rotation_journal_never_reports_publication_before_readiness() {
    let root = std::env::temp_dir().join(format!("boss-lifecycle-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    install_pending_boss_rotation(&backend, crate::model::unix_time());
    let observed = install_rotation_observer(&backend);
    {
        let mut state = backend.task_state.lock();
        state.session_mut(old).unwrap().context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );
    let stages = observed
        .lock()
        .iter()
        .filter_map(|o| match o {
            RotationObservation::Lifecycle(LifecycleObservation { stage, .. }) => Some(*stage),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        waku_boss::boss_rotation::RotationJournal::load(&backend.boss_journal_path())
            .unwrap()
            .intent
            .is_some()
    );
    assert!(stages.contains(&RotationLifecycleStage::SuccessorPrepared));
    assert!(!stages.contains(&RotationLifecycleStage::PointerPublished));
    assert!(!stages.contains(&RotationLifecycleStage::OwnershipCommitted));
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    let report =
        waku_boss::rotation_oracle::RotationOracle::evaluate(observed.lock().iter().cloned());
    assert_eq!(report.unsafe_admissions, 0);
    assert_eq!(report.compactions, 0);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn retired_boss_generation_is_fenced_at_the_driver_boundary() {
    let root = std::env::temp_dir().join(format!("boss-fence-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    let observed = install_rotation_observer(&backend);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    install_legacy_boss_rotation(&backend, crate::model::unix_time()).unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(old, next);

    // A runtime handle for the retired generation — even one minted after
    // the pointer moved — must not reach the provider.
    let stale_runtime = Uuid::new_v4();
    let stale_capture = boss_runtime(&backend, old, stale_runtime);
    // Rotation teardown is detached and may already have reaped the
    // inserted entry — the fence lives in the driver boundary, not in the
    // runtime registry.
    let stale_driver = backend.boss_driver_boundary(
        old,
        stale_runtime,
        DriverHandle::from_control(stale_capture.clone()),
    );
    stale_driver.prompt("stale generation still tries to coordinate".into());
    stale_driver.steer("steer is denied too".into());
    stale_driver.compact();
    assert!(stale_capture.prompts.lock().is_empty());
    assert!(stale_capture.steers.lock().is_empty());
    assert!(
        waku_boss::rotation_continuity::ContinuityLedger::load(&root.join("boss/continuity.json"))
            .unwrap()
            .rotation_request
            .is_none()
    );

    let fresh_runtime = Uuid::new_v4();
    let fresh_capture = boss_runtime(&backend, next, fresh_runtime);
    let fresh_driver = backend.sessions.lock().get(&next).unwrap().driver.clone();
    fresh_driver.prompt("the committed owner coordinates".into());
    assert_eq!(
        *fresh_capture.prompts.lock(),
        vec!["the committed owner coordinates"]
    );

    let guard = observed.lock();
    let admissions = guard
        .iter()
        .filter_map(|observation| match observation {
            RotationObservation::ProviderAdmission(admission) => Some(admission),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(admissions.iter().any(|admission| {
        admission.session_id == old
            && matches!(&admission.decision, AdmissionDecision::Blocked { .. })
    }));
    assert!(admissions.iter().any(|admission| {
        admission.session_id == next && matches!(&admission.decision, AdmissionDecision::Unguarded)
    }));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn stale_boss_credential_is_denied_after_rotation() {
    let root = std::env::temp_dir().join(format!("boss-stale-cred-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    install_legacy_boss_rotation(&backend, crate::model::unix_time()).unwrap();
    assert_ne!(old, backend.boss.identity_and_session().1.unwrap());

    // Every scoped command minted under the retired generation is denied —
    // the pointer, not the credential, defines who may coordinate.
    for command in [
        Command::AgentMergeSubmit,
        Command::AgentComputerUseReset,
        Command::AgentProjectMap {
            query: "audit".into(),
            path: None,
            max_tokens: None,
            intent: crate::model::ProjectMapIntent::Understand,
            anchors: Vec::new(),
            known_paths: Vec::new(),
        },
    ] {
        let error = backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: old,
                    runtime_id: Uuid::new_v4(),
                    command,
                },
                EventSink::detached(),
                Some(old),
            )
            .expect_err("stale-generation commands must fail closed");
        assert!(
            error.to_string().contains("fenced Boss generation"),
            "unexpected denial: {error:#}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn boss_handoff_ignores_uncorrelated_turn_started_and_retains_content() {
    use waku_boss::rotation_continuity::{ContinuityDeliveryStatus, ContinuityLedger};
    let root = std::env::temp_dir().join(format!("boss-handoff-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    install_legacy_boss_rotation(&backend, crate::model::unix_time()).unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    let ledger_path = root.join("boss/continuity.json");

    // The bound package is prepared on the successor before any transport.
    let pending = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .any(|session| session.id == next && session.pending_provider_context.is_some());
    assert!(pending, "the successor carries the durable handoff");
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    let delivery = ledger
        .pending_for_session(next)
        .expect("a prepared handoff delivery is recorded");
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Prepared);

    // An unrelated turn-start cannot consume a package that was never
    // submitted to this provider.
    super::super::events::boss_continuity_note_event(
        &backend.boss,
        &backend.task_state,
        &backend.task_store,
        next,
        &DriverEvent::TurnStarted,
    );
    assert_eq!(
        ContinuityLedger::load(&ledger_path)
            .unwrap()
            .pending_for_session(next)
            .unwrap()
            .status,
        ContinuityDeliveryStatus::Prepared,
    );
    assert!(
        backend
            .task_state
            .lock()
            .session_mut(next)
            .unwrap()
            .pending_provider_context
            .is_some()
    );

    // Submission wraps the package into the outbound prompt — and leaves
    // the pending state untouched.
    let retained_handoff = backend
        .task_state
        .lock()
        .session_mut(next)
        .unwrap()
        .pending_provider_context
        .clone()
        .unwrap();
    let wrapped = backend.boss_outbound_prompt(next, "continue".into());
    assert!(wrapped.contains("<boss-session-rotation>"));
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    let delivery = ledger.pending_for_session(next).unwrap();
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Prepared);
    let still_pending = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .any(|session| session.id == next && session.pending_provider_context.is_some());
    assert!(
        still_pending,
        "submission must not consume the bound package"
    );

    // Generic turn starts are not exact-package receipts, even after wrapping.
    super::super::events::boss_continuity_note_event(
        &backend.boss,
        &backend.task_state,
        &backend.task_store,
        next,
        &DriverEvent::TurnStarted,
    );
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    assert!(ledger.pending_for_session(next).is_some());
    assert_eq!(
        backend
            .task_state
            .lock()
            .session_mut(next)
            .unwrap()
            .pending_provider_context
            .as_ref(),
        Some(&retained_handoff)
    );
    let capture = boss_runtime(&backend, next, Uuid::new_v4());
    backend
        .queue_agent_prompt(
            next,
            "recovered report".into(),
            None,
            &EventSink::detached(),
        )
        .unwrap();
    assert_eq!(capture.prompts.lock().len(), 1);
    assert!(!backend.agent.has_queued(next));
    // Ordinary delivery does not manufacture exact-package acceptance.
    assert_eq!(
        ContinuityLedger::load(&ledger_path)
            .unwrap()
            .pending_for_session(next)
            .unwrap()
            .status,
        ContinuityDeliveryStatus::Prepared
    );
    assert!(
        backend
            .boss_outbound_prompt(next, "follow up".into())
            .contains("<boss-session-rotation>")
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn failed_turn_cannot_reject_or_retry_an_unconfirmed_boss_delivery() {
    use waku_boss::rotation_continuity::{ContinuityDeliveryStatus, ContinuityLedger};
    let root = std::env::temp_dir().join(format!("boss-reject-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    install_legacy_boss_rotation(&backend, crate::model::unix_time()).unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    let ledger_path = root.join("boss/continuity.json");

    let wrapped = backend.boss_outbound_prompt(next, "continue".into());
    assert!(wrapped.contains("<boss-session-rotation>"));
    super::super::events::boss_continuity_note_event(
        &backend.boss,
        &backend.task_state,
        &backend.task_store,
        next,
        &DriverEvent::TurnFinished {
            success: false,
            summary: Some("provider rejected the request".into()),
            summary_i18n: None,
        },
    );

    // A rejected submission reconciles the same delivery — it stays
    // pending under its identifier instead of duplicating or vanishing.
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    let delivery = ledger.pending_for_session(next).unwrap();
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Prepared);
    assert_eq!(delivery.attempts, 0);
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .any(|session| { session.id == next && session.pending_provider_context.is_some() })
    );

    // The retry carries the same package and reconciles onto the same
    // delivery record.
    let retried = backend.boss_outbound_prompt(next, "continue".into());
    assert!(retried.contains("<boss-session-rotation>"));
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    let delivery = ledger.pending_for_session(next).unwrap();
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Prepared);
    assert_eq!(delivery.attempts, 0);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn durable_handoff_failure_and_missing_readiness_both_prevent_publication() {
    use waku_boss::rotation_continuity::ContinuityLedger;
    let root = std::env::temp_dir().join(format!("boss-handoff-order-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    install_pending_boss_rotation(&backend, crate::model::unix_time());
    {
        let mut state = backend.task_state.lock();
        state.session_mut(old).unwrap().context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    let obstruction = root.join("boss/continuity.lock");
    std::fs::create_dir(&obstruction).unwrap();
    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    std::fs::remove_dir(obstruction).unwrap();
    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    let ledger = ContinuityLedger::load(&root.join("boss/continuity.json")).unwrap();
    let frozen = ledger.frozen_coordinator.as_ref().unwrap();
    assert!(frozen.package.is_some());
    let capture = boss_runtime(&backend, frozen.successor, Uuid::new_v4());
    let driver = backend
        .sessions
        .lock()
        .get(&frozen.successor)
        .unwrap()
        .driver
        .clone();
    driver.prompt("unqualified successor must wait".into());
    assert!(capture.prompts.lock().is_empty());
    assert!(backend.boss_fenced_chat(frozen.successor));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn queued_event_to_retired_chat_retargets_the_successor() {
    let root = std::env::temp_dir().join(format!("boss-retarget-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    install_legacy_boss_rotation(&backend, crate::model::unix_time()).unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    let capture = boss_runtime(&backend, next, Uuid::new_v4());

    backend
        .queue_agent_prompt(
            old,
            "the employee outcome".into(),
            None,
            &EventSink::detached(),
        )
        .unwrap();

    assert_eq!(capture.prompts.lock().len(), 1);
    assert!(capture.prompts.lock()[0].contains("the employee outcome"));
    assert!(!backend.agent.has_queued(next));
    assert!(
        backend
            .task_state
            .lock()
            .session_mut(next)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.content.contains("the employee outcome"))
    );
    assert!(!backend.agent.has_queued(old));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn identity_drift_during_staging_pauses_and_preserves_evidence() {
    use crate::boss_rotation::{RotationIntent, RotationJournal};
    use waku_boss::rotation_continuity::ContinuityLedger;
    let root = std::env::temp_dir().join(format!("boss-identity-drift-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    let identity = backend.boss.identity_and_session().0;
    let staged = Uuid::new_v4();
    let journal_path = root.join("boss/rotation.json");
    let mut journal = RotationJournal {
        active_session_id: Some(old),
        ..RotationJournal::default()
    };
    journal.intent = Some(RotationIntent {
        mode: Default::default(),
        boss_id: identity.id,
        old_session_id: old,
        old_generation: 0,
        new_session_id: staged,
        created_at: crate::model::unix_time(),
        committed_at: None,
        reason: Some("context threshold".into()),
        // A revision the authoritative document no longer digests to —
        // the staged successor was built for stale grants.
        identity_digest: Some("stale-revision".into()),
        provider: None,
        model: None,
        provider_qualification: None,
        diagnostics: Default::default(),
    });
    journal.persist(&journal_path).unwrap();

    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );

    // Changed authority cannot discard the frozen checkpoint or activate it.
    let journal = RotationJournal::load(&journal_path).unwrap();
    assert!(journal.intent.is_some());
    assert_eq!(journal.active_session_id, Some(old));
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    assert!(
        backend
            .task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == staged)
    );
    let ledger = ContinuityLedger::load(&root.join("boss/continuity.json")).unwrap();
    assert!(ledger.pending_for_session(staged).is_none());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn pending_rotation_intent_blocks_chat_before_the_continuity_freeze() {
    use waku_boss::boss_rotation::RotationJournal;
    let root = std::env::temp_dir().join(format!("boss-intent-fence-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let capture = boss_runtime(&backend, boss, runtime);
    let path = backend.boss_journal_path();
    let mut journal = RotationJournal::load(&path).unwrap();
    journal.active_session_id = Some(boss);
    journal
        .begin(
            backend.boss.identity_and_session().0.id,
            boss,
            Uuid::new_v4(),
            1,
        )
        .unwrap();
    journal.persist(&path).unwrap();
    let driver = backend.sessions.lock().get(&boss).unwrap().driver.clone();
    for corrupt in [false, true] {
        if corrupt {
            std::fs::write(&path, b"invalid rotation journal").unwrap();
        }
        assert!(backend.ensure_coordinator_available(boss).is_err());
        driver.prompt("must not bypass recovery".into());
        driver.steer("must not continue".into());
        assert!(capture.prompts.lock().is_empty());
        assert!(capture.steers.lock().is_empty());
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn ordinary_boss_and_planning_chat_deliver_prompts_without_claiming_qualification() {
    let root = std::env::temp_dir().join(format!("coordinator-chat-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let observed = install_rotation_observer(&backend);
    let mut planning = AgentSession::new(
        backend.boss.identity_and_session().0.id,
        ProviderKind::Codex,
    );
    planning.boss_managed = true;
    let planning_id = planning.id;
    backend.task_state.lock().push_session(planning);
    backend
        .boss
        .add_plan(waku_protocol::boss::BossPlan {
            id: planning_id,
            session_id: planning_id,
            plan_file: "plans/test.md".into(),
            idea: "Design only".into(),
            finalized_at: None,
            approved_revisions: vec![],
            items: vec![],
            outcome: None,
            history: vec![],
        })
        .unwrap();
    for session in [boss, planning_id] {
        backend.ensure_coordinator_available(session).unwrap();
        let runtime = Uuid::new_v4();
        let capture = boss_runtime(&backend, session, runtime);
        let message_id = Uuid::new_v4();
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: session,
                    runtime_id: runtime,
                    command: Command::Prompt {
                        prompt: "continue coordinated work".into(),
                        turn_id: None,
                        message_id: Some(message_id),
                        hidden: false,
                        attachments: vec![],
                    },
                },
                EventSink::detached(),
                None,
            )
            .unwrap();
        assert_eq!(capture.prompts.lock().len(), 1);
        assert!(capture.prompts.lock()[0].contains("continue coordinated work"));
        let state = backend.task_state.lock();
        let stored = state.sessions.iter().find(|s| s.id == session).unwrap();
        assert!(stored.queued_messages.is_empty());
        assert!(
            stored
                .messages
                .iter()
                .any(|m| m.id == message_id && m.content == "continue coordinated work")
        );
        drop(state);
        // The coordinator can use its ordinary CLI surface again.
        backend
            .handle(
                Request {
                    request_id: Uuid::new_v4(),
                    session_id: session,
                    runtime_id: runtime,
                    command: Command::Boss {
                        operation: waku_protocol::boss::BossOperation::View,
                    },
                },
                EventSink::detached(),
                Some(session),
            )
            .unwrap();
        let driver = backend
            .sessions
            .lock()
            .get(&session)
            .unwrap()
            .driver
            .clone();
        driver.steer("ordinary continuation".into());
        assert!(
            capture
                .steers
                .lock()
                .iter()
                .any(|s| s == "ordinary continuation")
        );
        backend
            .task_state
            .lock()
            .session_mut(session)
            .unwrap()
            .archived_at = Some(1);
        assert!(backend.ensure_coordinator_available(session).is_err());
        driver.prompt("archived chat must stop".into());
        assert_eq!(capture.prompts.lock().len(), 1);
    }
    assert!(observed.lock().iter().any(|o| matches!(
        o,
        RotationObservation::ProviderAdmission(ProviderAdmission {
            decision: AdmissionDecision::Unguarded,
            ..
        })
    )));
    let ledger =
        waku_boss::rotation_continuity::ContinuityLedger::load(&backend.boss_continuity_path())
            .unwrap();
    assert!(ledger.provider_receipts.is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn guarded_lifecycle_freezes_queued_intake_and_restarts_without_publication() {
    use waku_boss::{boss_rotation::RotationJournal, rotation_continuity::ContinuityLedger};
    let root = std::env::temp_dir().join(format!("guarded-rotation-restart-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    install_pending_boss_rotation(&backend, crate::model::unix_time());
    let original = crate::model::QueuedMessage::new(&"large input ".repeat(10000));
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        session.queued_messages.push(original.clone());
        backend.task_store.save(&mut state).unwrap();
    }
    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );
    let path = root.join("boss/continuity.json");
    let ledger = ContinuityLedger::load(&path).unwrap();
    let frozen = ledger.frozen_coordinator.clone().unwrap();
    assert!(
        frozen
            .package
            .as_ref()
            .unwrap()
            .event_ids
            .contains(&original.id)
    );
    assert!(backend.boss_incident_fenced(old));
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    let before = backend
        .task_state
        .lock()
        .sessions
        .iter()
        .filter(|s| s.boss_managed)
        .count();
    drop(backend);
    let backend = WakuBackend::new(
        DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
        StateStore::daemon(root.join("app.db")),
    )
    .unwrap();
    for _ in 0..3 {
        assert!(
            backend
                .reconcile_boss_rotation(crate::model::unix_time())
                .is_err()
        );
        assert_eq!(backend.boss.identity_and_session().1, Some(old));
        assert_eq!(
            backend
                .task_state
                .lock()
                .sessions
                .iter()
                .filter(|s| s.boss_managed)
                .count(),
            before
        );
    }
    let restored = ContinuityLedger::load(&path).unwrap();
    assert_eq!(restored.frozen_coordinator, Some(frozen));
    assert_eq!(restored.deliveries.len(), ledger.deliveries.len());
    let mut state = backend.task_store.load().unwrap();
    let source = state.session_mut(old).unwrap();
    backend.task_store.hydrate(source).unwrap();
    assert!(source.archived_at.is_none());
    assert!(
        source
            .queued_messages
            .iter()
            .any(|m| m.id == original.id && m.content == original.content)
    );
    let journal = RotationJournal::load(&root.join("boss/rotation.json")).unwrap();
    assert_eq!(journal.generation, 0);
    assert!(journal.rotations.is_empty());
    assert!(backend.sessions.lock().is_empty());
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn guarded_lifecycle_open_tool_loop_freezes_before_waiting_for_quiescence() {
    use waku_boss::rotation_continuity::ContinuityLedger;
    let root = std::env::temp_dir().join(format!("guarded-open-turn-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    install_pending_boss_rotation(&backend, crate::model::unix_time());
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        session.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        session.begin_turn("unfinished long tool loop");
        backend.task_store.save(&mut state).unwrap();
    }
    let capture = boss_runtime(&backend, old, Uuid::new_v4());
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    let path = root.join("boss/continuity.json");
    let frozen = ContinuityLedger::load(&path)
        .unwrap()
        .frozen_coordinator
        .unwrap();
    assert!(frozen.package.is_none());
    let driver = backend.sessions.lock().get(&old).unwrap().driver.clone();
    driver.prompt("no next request".into());
    driver.respond("tool".into(), "oversized result".repeat(10000));
    driver.steer("no continuation".into());
    assert!(driver.rollback(1).is_err());
    assert!(capture.prompts.lock().is_empty());
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    assert!(
        backend
            .task_state
            .lock()
            .session_mut(old)
            .unwrap()
            .active_turn_id()
            .is_some()
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn guarded_lifecycle_reconciles_original_receipt_without_replaying_or_activating() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use waku_boss::{
        coordinator_contract::{PackageOutcome, ProviderPackageReceipt},
        rotation_continuity::{ContinuityAttempt, ContinuityLedger},
    };
    struct ReceiptQuery {
        queries: AtomicUsize,
        accepted: AtomicBool,
    }
    impl crate::driver::DriverControl for ReceiptQuery {
        fn prompt(&self, _: String) {
            panic!("receipt reconciliation must not prompt");
        }
        fn cancel(&self) {}
        fn respond(&self, _: String, _: String) {
            panic!("no tool continuation");
        }
        fn rollback(&self, _: usize) -> anyhow::Result<Option<crate::model::ProviderResumeCursor>> {
            anyhow::bail!("no resume")
        }
        fn reconcile_coordinator_package(&self, attempt: &ContinuityAttempt) -> PackageOutcome {
            self.queries.fetch_add(1, Ordering::SeqCst);
            if self.accepted.load(Ordering::SeqCst) {
                PackageOutcome::Confirmed(ProviderPackageReceipt {
                    attempt: attempt.clone(),
                    native_session: "fixture-native-session".into(),
                    native_request: "fixture-native-request".into(),
                })
            } else {
                PackageOutcome::Unknown
            }
        }
    }
    let root = std::env::temp_dir().join(format!("guarded-receipt-recovery-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    install_pending_boss_rotation(&backend, crate::model::unix_time());
    {
        let mut state = backend.task_state.lock();
        state.session_mut(old).unwrap().context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );
    let path = backend.boss_continuity_path();
    let next = ContinuityLedger::load(&path)
        .unwrap()
        .frozen_coordinator
        .unwrap()
        .successor;
    let runtime = Uuid::new_v4();
    boss_runtime(&backend, next, runtime);
    let query = Arc::new(ReceiptQuery {
        queries: AtomicUsize::new(0),
        accepted: AtomicBool::new(false),
    });
    let driver =
        backend.boss_driver_boundary(next, runtime, DriverHandle::from_control(query.clone()));
    backend.sessions.lock().get_mut(&next).unwrap().driver = driver;
    // Seed a native submission whose acknowledgement was lost. This fixture
    // creates correlation only and never qualifies or launches a real provider.
    let attempt = ContinuityLedger::update(&path, |ledger| {
        ledger.submit_package(&format!("boss-successor:{next}"), runtime, 1)
    })
    .unwrap()
    .unwrap();
    for _ in 0..2 {
        assert!(
            backend
                .reconcile_boss_rotation(crate::model::unix_time())
                .unwrap_err()
                .to_string()
                .contains("unresolved")
        );
        let ledger = ContinuityLedger::load(&path).unwrap();
        assert_eq!(ledger.deliveries[0].attempt.as_ref(), Some(&attempt));
        assert_eq!(ledger.deliveries[0].attempts, 1);
    }
    assert_eq!(query.queries.load(Ordering::SeqCst), 2);
    query.accepted.store(true, Ordering::SeqCst);
    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );
    let ledger = ContinuityLedger::load(&path).unwrap();
    assert_eq!(ledger.provider_receipts[0].attempt, attempt);
    assert_eq!(ledger.deliveries[0].attempts, 1);
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    assert!(
        backend
            .task_state
            .lock()
            .session_mut(old)
            .unwrap()
            .archived_at
            .is_none()
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn best_effort_rotation_leaves_provider_launch_to_normal_admission() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = std::env::temp_dir().join(format!("guarded-capacity-{}", Uuid::new_v4()));
    let (mut backend, _) = surface_test_backend(&root);
    let launches = Arc::new(AtomicUsize::new(0));
    let count = launches.clone();
    backend.runtime_test_start = Some(Arc::new(move |_, _, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Err(anyhow::anyhow!(
            "fixture: hard capacity exhausted or provider unavailable"
        ))
    }));
    let old = rotation_boss(&backend);
    {
        let mut state = backend.task_state.lock();
        state.session_mut(old).unwrap().context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    for _ in 0..3 {
        assert!(
            backend
                .reconcile_boss_rotation(crate::model::unix_time())
                .is_ok()
        );
        assert_ne!(backend.boss.identity_and_session().1, Some(old));
    }
    assert_eq!(launches.load(Ordering::SeqCst), 0);
    assert!(backend.sessions.lock().is_empty());
    assert!(!backend.boss_incident_fenced(old));
    assert!(
        waku_boss::boss_rotation::RotationJournal::load(&backend.boss_journal_path())
            .unwrap()
            .intent
            .is_none()
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn compaction_keeps_the_live_turn_and_restores_authoritative_context() {
    let root = std::env::temp_dir().join(format!("boss-compaction-recovery-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    boss_runtime(&backend, old, Uuid::new_v4());
    let canonical = backend.boss.canonical_boss_instructions(old).unwrap();
    backend.boss.prompt_with_context(old, "first prompt".into());
    record_boss_event(
        &backend.task_state,
        &backend.task_store,
        old,
        &DriverEvent::TurnStarted,
    )
    .unwrap();
    let turn = backend
        .task_state
        .lock()
        .session_mut(old)
        .unwrap()
        .active_turn_id();
    let mut compacting = ActivityItem::new(
        Some("compact-recovery".into()),
        ActivityKind::Tool,
        "Compacting context",
        None,
        false,
    );
    compacting.title_i18n = Some(localized!("activity.compacting_context").1);
    let event = DriverEvent::RichActivity(compacting);
    for _ in 0..2 {
        super::super::events::boss_continuity_note_event(
            &backend.boss,
            &backend.task_state,
            &backend.task_store,
            old,
            &event,
        );
    }
    backend.ensure_coordinator_available(old).unwrap();
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    {
        let mut state = backend.task_state.lock();
        let session = state.session_mut(old).unwrap();
        assert_eq!(session.active_turn_id(), turn);
        assert_eq!(
            session
                .messages
                .iter()
                .filter(|message| {
                    message.role == MessageRole::System
                        && message.content.starts_with("Boss context was compacted.")
                })
                .count(),
            1
        );
    }
    let wrapped = super::super::prompt_delivery::wrap_boss_outbound_prompt(
        &backend.task_state,
        &backend.automations.document(),
        &backend.boss,
        old,
        "continue".into(),
    );
    assert!(wrapped.contains(canonical.trim()));
    assert!(wrapped.contains("<goddard-boss-context>"));
    record_boss_event(
        &backend.task_state,
        &backend.task_store,
        old,
        &DriverEvent::TurnFinished {
            success: true,
            summary: None,
            summary_i18n: None,
        },
    )
    .unwrap();
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(old, next);
    backend.ensure_coordinator_available(next).unwrap();
    drop(backend);
    remove_tree(root);
}

#[test]
fn compaction_upgrade_recovers_unsubmitted_checkpoints_but_retains_unknown_attempts() {
    use waku_boss::{
        boss_rotation::{RotationJournal, RotationMode},
        rotation_continuity::ContinuityLedger,
    };
    for submitted in [false, true] {
        let root = std::env::temp_dir().join(format!("boss-compaction-upgrade-{}", Uuid::new_v4()));
        let (backend, _) = surface_test_backend(&root);
        let old = rotation_boss(&backend);
        install_pending_boss_rotation(&backend, crate::model::unix_time());
        assert!(
            backend
                .reconcile_boss_rotation(crate::model::unix_time())
                .is_err()
        );
        let path = backend.boss_continuity_path();
        let journal = RotationJournal::load(&backend.boss_journal_path()).unwrap();
        let next = journal.intent.unwrap().new_session_id;
        ContinuityLedger::update(&path, |ledger| {
            ledger.record_compaction_incident(
                old,
                crate::model::unix_time(),
                "legacy compaction fence",
            );
            ledger.compaction_incidents.last_mut().unwrap().fenced = true;
            if submitted {
                ledger
                    .submit_package(&format!("boss-successor:{next}"), Uuid::new_v4(), 1)
                    .unwrap();
            }
        })
        .unwrap();
        let queued = Uuid::new_v4();
        backend
            .handle(
                Request {
                    request_id: queued,
                    session_id: old,
                    runtime_id: Uuid::nil(),
                    command: Command::Prompt {
                        prompt: "preserve this human message".into(),
                        turn_id: None,
                        message_id: Some(queued),
                        attachments: Vec::new(),
                        hidden: false,
                    },
                },
                EventSink::detached(),
                None,
            )
            .unwrap();
        drop(backend);
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let result = backend.reconcile_boss_rotation(crate::model::unix_time());
        let journal = RotationJournal::load(&backend.boss_journal_path()).unwrap();
        let ledger = ContinuityLedger::load(&path).unwrap();
        if submitted {
            assert!(result.is_err());
            assert_eq!(backend.boss.identity_and_session().1, Some(old));
            assert_eq!(journal.intent.unwrap().mode, RotationMode::Strict);
            assert!(ledger.frozen_coordinator.is_some());
            assert_eq!(ledger.deliveries[0].attempts, 1);
            assert!(backend.ensure_coordinator_available(old).is_err());
        } else {
            result.unwrap();
            assert_eq!(backend.boss.identity_and_session().1, Some(next));
            assert!(journal.intent.is_none());
            assert_eq!(
                journal.rotations.last().unwrap().mode,
                RotationMode::BestEffort
            );
            assert!(ledger.frozen_coordinator.is_none());
            backend.ensure_coordinator_available(next).unwrap();
            // Reconciliation is idempotent and cannot create another chat.
            backend
                .reconcile_boss_rotation(crate::model::unix_time())
                .unwrap();
            assert_eq!(backend.boss.identity_and_session().1, Some(next));
        }
        let target = if submitted { old } else { next };
        let mut state = backend.task_store.load().unwrap();
        let session = state.session_mut(target).unwrap();
        backend.task_store.hydrate(session).unwrap();
        assert!(session.queued_messages.iter().any(
            |message| message.id == queued && message.content == "preserve this human message"
        ));
        drop(backend);
        remove_tree(root);
    }
}

#[test]
fn cleared_boss_replacement_allows_refresh_and_recovers_legacy_freezes_after_restart() {
    use waku_boss::{
        boss_rotation::{RotationJournal, RotationMode},
        rotation_continuity::ContinuityLedger,
    };
    use waku_protocol::boss::{BossOperation, BossResult};

    for recovery in ["clear", "legacy", "interrupted"] {
        let root = std::env::temp_dir().join(format!("boss-clear-refresh-{}", Uuid::new_v4()));
        let (backend, _) = surface_test_backend(&root);
        let old = rotation_boss(&backend);
        let now = crate::model::unix_time();
        install_pending_boss_rotation(&backend, now);
        assert!(backend.reconcile_boss_rotation(now).is_err());
        let path = backend.boss_continuity_path();
        let checkpoint = ContinuityLedger::load(&path).unwrap();
        assert!(checkpoint.coordinator_frozen(old));
        let BossResult::Session {
            session: cleared, ..
        } = backend
            .handle_boss_operation(None, BossOperation::Clear, &EventSink::detached())
            .unwrap()
        else {
            panic!("expected cleared chat")
        };
        assert!(
            ContinuityLedger::load(&path)
                .unwrap()
                .frozen_coordinator
                .is_none()
        );
        if recovery != "clear" {
            // Old clears deleted deliveries but left the unrelated freeze.
            // Also cover an already-paused best-effort refresh on that owner.
            ContinuityLedger::update(&path, |ledger| {
                ledger.frozen_coordinator = checkpoint.frozen_coordinator.clone();
                if recovery == "interrupted" {
                    // A crash before ledger cleanup also retains the package.
                    ledger.deliveries = checkpoint.deliveries.clone();
                }
            })
            .unwrap();
            install_pending_boss_rotation(&backend, now);
            let journal_path = backend.boss_journal_path();
            let mut journal = RotationJournal::load(&journal_path).unwrap();
            journal.intent.as_mut().unwrap().mode = RotationMode::BestEffort;
            journal.record_attempt_failure(
                "another coordinator replacement remains unresolved".into(),
                now,
            );
            journal.persist(&journal_path).unwrap();
        } else {
            ContinuityLedger::update(&path, |ledger| {
                ledger.request_rotation(now, "refresh after clear")
            })
            .unwrap();
        }
        let queued = crate::model::QueuedMessage::agent("preserve this follow-up", None);
        {
            let mut state = backend.task_state.lock();
            state
                .session_mut(cleared.id)
                .unwrap()
                .queued_messages
                .push(queued.clone());
            backend.task_store.save(&mut state).unwrap();
        }
        drop(backend);
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        backend.reconcile_boss_rotation(now).unwrap();
        let next = backend.boss.identity_and_session().1.unwrap();
        assert_ne!(next, cleared.id);
        backend.reconcile_boss_rotation(now).unwrap();
        assert_eq!(backend.boss.identity_and_session().1, Some(next));
        assert!(
            ContinuityLedger::load(&path)
                .unwrap()
                .frozen_coordinator
                .is_none()
        );
        assert!(
            RotationJournal::load(&backend.boss_journal_path())
                .unwrap()
                .intent
                .is_none()
        );
        backend.ensure_coordinator_available(next).unwrap();
        assert!(backend.ensure_coordinator_available(old).is_err());
        assert!(backend.ensure_coordinator_available(cleared.id).is_err());
        {
            let mut state = backend.task_store.load().unwrap();
            let session = state.session_mut(next).unwrap();
            backend.task_store.hydrate(session).unwrap();
            assert_eq!(
                session
                    .queued_messages
                    .iter()
                    .filter(|message| message.id == queued.id)
                    .count(),
                1
            );
        }
        // The recovered owner can cross the actual provider admission boundary.
        let capture = boss_runtime(&backend, next, Uuid::new_v4());
        let driver = backend.sessions.lock().get(&next).unwrap().driver.clone();
        backend
            .drain_agent_queue(next, &driver, &EventSink::detached())
            .unwrap();
        assert_eq!(capture.prompts.lock().len(), 1);
        assert!(capture.prompts.lock()[0].contains("preserve this follow-up"));
        drop(backend);
        remove_tree(root);
    }
}

#[test]
fn cleared_boss_replacement_preserves_unknown_submissions_and_effects() {
    use waku_boss::rotation_continuity::ContinuityLedger;
    use waku_protocol::boss::BossOperation;

    for submitted in [false, true] {
        let root = std::env::temp_dir().join(format!("boss-clear-unknown-{}", Uuid::new_v4()));
        let (backend, _) = surface_test_backend(&root);
        rotation_boss(&backend);
        let now = crate::model::unix_time();
        install_pending_boss_rotation(&backend, now);
        assert!(backend.reconcile_boss_rotation(now).is_err());
        let path = backend.boss_continuity_path();
        ContinuityLedger::update(&path, |ledger| {
            let frozen = ledger.frozen_coordinator.as_ref().unwrap();
            if submitted {
                ledger
                    .submit_package(
                        &format!("boss-successor:{}", frozen.successor),
                        Uuid::new_v4(),
                        now,
                    )
                    .unwrap();
            } else {
                let binding = frozen.package.as_ref().unwrap().binding.clone();
                ledger
                    .begin_coordinator_effect("unknown-effect", &binding)
                    .unwrap();
            }
        })
        .unwrap();
        let before = ContinuityLedger::load(&path).unwrap();
        backend
            .handle_boss_operation(None, BossOperation::Clear, &EventSink::detached())
            .unwrap();
        drop(backend);
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        assert!(backend.reconcile_boss_rotation(now).is_err());
        let after = ContinuityLedger::load(&path).unwrap();
        assert_eq!(after.frozen_coordinator, before.frozen_coordinator);
        assert_eq!(after.deliveries, before.deliveries);
        assert_eq!(after.coordinator_effects, before.coordinator_effects);
        assert!(backend.sessions.lock().is_empty());
        drop(backend);
        remove_tree(root);
    }
}

#[test]
fn best_effort_rotation_recovers_each_side_of_pointer_publication_once() {
    use waku_boss::{
        boss_rotation::{RotationJournal, RotationMode},
        rotation_continuity::ContinuityLedger,
    };
    for published in [false, true] {
        let root = std::env::temp_dir().join(format!("boss-ordinary-restart-{}", Uuid::new_v4()));
        let (backend, _) = surface_test_backend(&root);
        let old = rotation_boss(&backend);
        install_pending_boss_rotation(&backend, crate::model::unix_time());
        assert!(
            backend
                .reconcile_boss_rotation(crate::model::unix_time())
                .is_err()
        );
        let mut journal = RotationJournal::load(&backend.boss_journal_path()).unwrap();
        let intent = journal.intent.as_mut().unwrap();
        intent.mode = RotationMode::BestEffort;
        let next = intent.new_session_id;
        journal.persist(&backend.boss_journal_path()).unwrap();
        ContinuityLedger::update(&backend.boss_continuity_path(), |ledger| {
            ledger.release_unsubmitted_replacement(old, next)
        })
        .unwrap()
        .unwrap();
        let queued = crate::model::QueuedMessage::new("keep the next assignment");
        {
            let mut state = backend.task_state.lock();
            state
                .session_mut(old)
                .unwrap()
                .queued_messages
                .push(queued.clone());
            backend.task_store.save(&mut state).unwrap();
        }
        if published {
            backend.boss.replace_session_id(old, next).unwrap();
        }
        drop(backend);
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        for _ in 0..2 {
            backend
                .reconcile_boss_rotation(crate::model::unix_time())
                .unwrap();
        }
        assert_eq!(backend.boss.identity_and_session().1, Some(next));
        let journal = RotationJournal::load(&backend.boss_journal_path()).unwrap();
        assert!(journal.intent.is_none());
        assert_eq!(journal.rotations.len(), 1);
        let mut state = backend.task_store.load().unwrap();
        let successor = state.session_mut(next).unwrap();
        backend.task_store.hydrate(successor).unwrap();
        assert_eq!(
            successor
                .queued_messages
                .iter()
                .filter(|message| message.id == queued.id)
                .count(),
            1
        );
        backend.ensure_coordinator_available(next).unwrap();
        assert!(backend.ensure_coordinator_available(old).is_err());
        drop(backend);
        remove_tree(root);
    }
}

#[test]
fn best_effort_rotation_rebinds_changed_configuration_without_holding_chat() {
    use waku_boss::boss_rotation::{RotationJournal, RotationMode};
    let root = std::env::temp_dir().join(format!("boss-ordinary-rebind-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    install_pending_boss_rotation(&backend, crate::model::unix_time());
    assert!(
        backend
            .reconcile_boss_rotation(crate::model::unix_time())
            .is_err()
    );
    let mut journal = RotationJournal::load(&backend.boss_journal_path()).unwrap();
    journal.intent.as_mut().unwrap().mode = RotationMode::BestEffort;
    let staged = journal.intent.as_ref().unwrap().new_session_id;
    journal.persist(&backend.boss_journal_path()).unwrap();
    {
        let mut state = backend.task_state.lock();
        let old = state.session_mut(old).unwrap();
        old.model = Some("updated-model".into());
        old.context_usage = Some(crate::model::ContextUsage {
            tokens: 76,
            window: Some(100),
        });
        backend.task_store.save(&mut state).unwrap();
    }
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    backend.ensure_coordinator_available(old).unwrap();
    assert!(backend.boss_fenced_chat(staged));
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(next, old);
    assert_ne!(next, staged);
    assert_eq!(
        backend
            .task_state
            .lock()
            .session_mut(next)
            .unwrap()
            .model
            .as_deref(),
        Some("updated-model")
    );
    drop(backend);
    remove_tree(root);
}
