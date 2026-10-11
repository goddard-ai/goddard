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
            driver: backend.observe_boss_driver(
                session_id,
                runtime_id,
                DriverHandle::from_control(capture.clone()),
            ),
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
fn boss_provider_admission_crosses_the_real_driver_boundary_unguarded() {
    let root = std::env::temp_dir().join(format!("boss-admission-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let observed = install_rotation_observer(&backend);
    let capture = boss_runtime(&backend, boss, runtime);

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

    assert!(!capture.prompts.lock().is_empty());
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
    assert!(matches!(admission.5, AdmissionDecision::Unguarded));
    assert!(
        observations
            .iter()
            .any(|observation| { matches!(observation, RotationObservation::Ownership(_)) })
    );

    let report = waku_boss::rotation_oracle::RotationOracle::evaluate(observations);
    assert!(report.unsafe_admissions >= 1);
    assert!(
        report.violations.iter().any(|violation| {
            matches!(violation, OracleViolation::MissingAdmissionGuard { .. })
        })
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn boss_manual_compaction_is_observed_as_compaction_not_rotation() {
    let root = std::env::temp_dir().join(format!("boss-compact-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let observed = install_rotation_observer(&backend);
    let capture = boss_runtime(&backend, boss, runtime);

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

    assert_eq!(capture.prompts.lock().as_slice(), ["/compact".to_owned()]);
    let observations = observed.lock().clone();
    assert!(observations.iter().any(|observation| {
        matches!(
            observation,
            RotationObservation::ProviderCompaction(ProviderCompaction {
                session_id: observed_session,
                stage: CompactionStage::Attempted,
                ..
            }) if *observed_session == boss
        )
    }));
    let report = waku_boss::rotation_oracle::RotationOracle::evaluate(observations);
    assert_eq!(report.admissions, 0);
    assert_eq!(report.compactions, 1);
    assert!(
        report
            .violations
            .iter()
            .any(|violation| { matches!(violation, OracleViolation::CompactionEntered { .. }) })
    );
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
fn rotation_journal_emits_real_ownership_and_publication_stages() {
    let root = std::env::temp_dir().join(format!("boss-lifecycle-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    let old_runtime = Uuid::new_v4();
    boss_runtime(&backend, old, old_runtime);
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

    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(old, next);

    let observations = observed.lock().clone();
    let stages = observations
        .iter()
        .filter_map(|observation| match observation {
            RotationObservation::Lifecycle(LifecycleObservation { stage, .. }) => Some(*stage),
            _ => None,
        })
        .collect::<Vec<_>>();
    for stage in [
        RotationLifecycleStage::Triggered,
        RotationLifecycleStage::IntentPersisted,
        RotationLifecycleStage::SuccessorPrepared,
        RotationLifecycleStage::PointerPublished,
        RotationLifecycleStage::PredecessorFenced,
        RotationLifecycleStage::OwnershipCommitted,
    ] {
        assert!(stages.contains(&stage), "missing {stage:?} in {stages:?}");
    }
    assert!(observations.iter().any(|observation| {
        matches!(
            observation,
            RotationObservation::Ownership(ownership)
                if ownership.session_id == old
                    && ownership.state
                        == waku_boss::rotation_oracle::OwnershipState::Fenced
        )
    }));
    assert!(observations.iter().any(|observation| {
        matches!(
            observation,
            RotationObservation::Ownership(ownership)
                if ownership.session_id == next
                    && ownership.state
                        == waku_boss::rotation_oracle::OwnershipState::Active
        )
    }));

    // The current implementation is intentionally reported unsafe: the next
    // prompt crosses the provider boundary before the new admission guard.
    let new_runtime = Uuid::new_v4();
    let capture = boss_runtime(&backend, next, new_runtime);
    backend
        .handle(
            Request {
                request_id: Uuid::new_v4(),
                session_id: next,
                runtime_id: new_runtime,
                command: Command::Prompt {
                    prompt: "continue".into(),
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
    assert!(!capture.prompts.lock().is_empty());

    let report =
        waku_boss::rotation_oracle::RotationOracle::evaluate(observed.lock().iter().cloned());
    assert_eq!(report.rotations, 1);
    assert!(report.durable_actions >= 3);
    assert!(report.ownership_observations >= 4);
    assert_eq!(report.unsafe_admissions, 1);
    assert_eq!(report.compactions, 0);
    std::fs::remove_dir_all(root).ok();
}
