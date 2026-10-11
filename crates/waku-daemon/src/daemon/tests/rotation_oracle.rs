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
fn boss_manual_compact_translates_to_a_durable_safe_refresh() {
    use waku_boss::rotation_continuity::ContinuityLedger;
    let root = std::env::temp_dir().join(format!("boss-compact-oracle-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let runtime = Uuid::new_v4();
    let observed = install_rotation_observer(&backend);
    let capture = boss_runtime(&backend, boss, runtime);
    // The occupancy heuristic is opted out — a refresh request still
    // rotates because it is a prevention request, not a preference.
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

    // The provider never sees the compact: no context leaves the fence.
    assert!(capture.prompts.lock().is_empty());
    let ledger = ContinuityLedger::load(&root.join("boss/continuity.json")).unwrap();
    assert!(ledger.rotation_request.is_some());

    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(boss, next);
    let ledger = ContinuityLedger::load(&root.join("boss/continuity.json")).unwrap();
    assert!(ledger.rotation_request.is_none());

    let observations = observed.lock().clone();
    assert!(!observations.iter().any(|observation| {
        matches!(observation, RotationObservation::ProviderCompaction(_))
    }));
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
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(old, next);

    // A runtime handle for the retired generation — even one minted after
    // the pointer moved — must not reach the provider.
    let stale_runtime = Uuid::new_v4();
    let stale_capture = boss_runtime(&backend, old, stale_runtime);
    let stale_driver = backend.sessions.lock().get(&old).unwrap().driver.clone();
    stale_driver.prompt("stale generation still tries to coordinate".into());
    stale_driver.steer("steer is denied too".into());
    assert!(stale_capture.prompts.lock().is_empty());
    assert!(stale_capture.steers.lock().is_empty());

    let fresh_runtime = Uuid::new_v4();
    let fresh_capture = boss_runtime(&backend, next, fresh_runtime);
    let fresh_driver = backend
        .sessions
        .lock()
        .get(&next)
        .unwrap()
        .driver
        .clone();
    fresh_driver.prompt("the committed owner coordinates".into());
    assert_eq!(fresh_capture.prompts.lock().len(), 1);

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
        admission.session_id == next
            && matches!(&admission.decision, AdmissionDecision::Unguarded)
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
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
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
fn boss_handoff_stays_pending_until_transport_acceptance() {
    use waku_boss::rotation_continuity::{
        ContinuityDeliveryStatus, ContinuityLedger,
    };
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
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    let ledger_path = root.join("boss/continuity.json");

    // The bound package is prepared on the successor before any transport.
    let pending = backend.task_state.lock().sessions.iter().any(|session| {
        session.id == next && session.pending_provider_context.is_some()
    });
    assert!(pending, "the successor carries the durable handoff");
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    let delivery = ledger
        .pending_for_session(next)
        .expect("a prepared handoff delivery is recorded");
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Prepared);

    // Submission wraps the package into the outbound prompt — and leaves
    // the pending state untouched.
    let wrapped = backend.boss_outbound_prompt(next, "continue".into());
    assert!(wrapped.contains("<boss-session-rotation>"));
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    let delivery = ledger.pending_for_session(next).unwrap();
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Submitted);
    let still_pending = backend.task_state.lock().sessions.iter().any(|session| {
        session.id == next && session.pending_provider_context.is_some()
    });
    assert!(still_pending, "submission must not consume the bound package");

    // Recorded transport acceptance is what consumes it.
    super::super::events::boss_continuity_note_event(
        &backend.boss,
        &backend.task_state,
        &backend.task_store,
        next,
        &DriverEvent::TurnStarted,
    );
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    assert!(ledger.pending_for_session(next).is_none());
    assert!(!backend.task_state.lock().sessions.iter().any(|session| {
        session.id == next && session.pending_provider_context.is_some()
    }));
    let plain = backend.boss_outbound_prompt(next, "follow up".into());
    assert!(!plain.contains("<boss-session-rotation>"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn rejected_delivery_keeps_boss_continuity_pending() {
    use waku_boss::rotation_continuity::{
        ContinuityDeliveryStatus, ContinuityLedger,
    };
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
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
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
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Rejected);
    assert_eq!(delivery.attempts, 1);
    assert!(backend.task_state.lock().sessions.iter().any(|session| {
        session.id == next && session.pending_provider_context.is_some()
    }));

    // The retry carries the same package and reconciles onto the same
    // delivery record.
    let retried = backend.boss_outbound_prompt(next, "continue".into());
    assert!(retried.contains("<boss-session-rotation>"));
    let ledger = ContinuityLedger::load(&ledger_path).unwrap();
    let delivery = ledger.pending_for_session(next).unwrap();
    assert_eq!(delivery.status, ContinuityDeliveryStatus::Submitted);
    assert_eq!(delivery.attempts, 2);
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
    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();
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

    let prompts = capture.prompts.lock().clone();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("the employee outcome"));
    // The first accepted delivery carries the bound handoff with it.
    assert!(prompts[0].contains("<boss-session-rotation>"));
    assert!(!backend.agent.has_queued(old));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn identity_drift_during_staging_aborts_the_rotation() {
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
    });
    journal.persist(&journal_path).unwrap();

    backend
        .reconcile_boss_rotation(crate::model::unix_time())
        .unwrap();

    // The stage unwinds rather than activating stale grants: intent
    // cleared, staged row removed, pointer untouched, delivery dropped.
    let journal = RotationJournal::load(&journal_path).unwrap();
    assert!(journal.intent.is_none());
    assert_eq!(journal.active_session_id, Some(old));
    assert_eq!(backend.boss.identity_and_session().1, Some(old));
    assert!(!backend
        .task_state
        .lock()
        .sessions
        .iter()
        .any(|session| session.id == staged));
    let ledger = ContinuityLedger::load(&root.join("boss/continuity.json")).unwrap();
    assert!(ledger.pending_for_session(staged).is_none());
    std::fs::remove_dir_all(root).ok();
}
