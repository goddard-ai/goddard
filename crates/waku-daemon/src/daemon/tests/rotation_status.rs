//! The rotation lifecycle's durable diagnostics and the status the Boss
//! document publishes — identity, accounting, stage marks, compaction
//! correlation, and the delayed/paused/incident positions.

use super::*;
use waku_boss::boss_rotation::RotationCause;
use waku_boss::rotation_oracle::{AdmissionDecision, CompactionStage, RotationLifecycleStage};
use waku_protocol::boss::{BossRotationPhase, PersonaDefaultRole};

fn busy_boss_chat(backend: &WakuBackend, session_id: Uuid, runtime_id: Uuid) {
    let capture = Arc::new(CaptureDriver::default());
    backend.sessions.lock().insert(
        session_id,
        RuntimeEntry {
            plan_gate: None,
            runtime_id,
            driver: DriverHandle::from_control(capture),
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: backend.default_cwd.clone(),
        },
    );
    let mut state = backend.task_state.lock();
    let session = state.session_mut(session_id).unwrap();
    session.model = Some("gpt-6.1-sol".into());
    session.begin_turn("Coordinate the fleet");
    backend.task_store.save(&mut state).unwrap();
}

#[test]
fn rotation_diagnostics_cover_identity_accounting_stages_and_terminal() {
    let root = std::env::temp_dir().join(format!("boss-rotation-diagnostics-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    busy_boss_chat(&backend, old, Uuid::new_v4());
    record_boss_event(
        &backend.task_state,
        &backend.task_store,
        old,
        &DriverEvent::UsageUpdated {
            context_tokens: Some(80),
            context_window: Some(100),
        },
    )
    .unwrap();
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
    let now = crate::model::unix_time();
    backend.reconcile_boss_rotation(now).unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(old, next);

    let journal =
        crate::boss_rotation::RotationJournal::load(&root.join("boss/rotation.json")).unwrap();
    let rotation = journal.rotations.last().expect("committed rotation");
    let diagnostics = &rotation.diagnostics;
    assert_eq!(diagnostics.cause, RotationCause::ContextThreshold);
    assert_eq!(diagnostics.provider, Some(ProviderKind::Codex));
    assert_eq!(diagnostics.model.as_deref(), Some("gpt-6.1-sol"));
    assert_eq!(diagnostics.context_threshold, Some(0.75));
    let usage = diagnostics.usage.as_ref().expect("usage sample");
    assert_eq!(usage.context_tokens, Some(80));
    assert_eq!(usage.context_window, Some(100));
    // The current lifecycle admits without a measured guard — the record
    // says so rather than implying bounds.
    assert_eq!(diagnostics.guard, AdmissionDecision::Unguarded);
    assert_eq!(diagnostics.headroom, None);
    assert_eq!(diagnostics.admission_wait_ms, None);
    let boundary = diagnostics.event_boundary.as_ref().expect("event boundary");
    assert_eq!(boundary.session_id, old);
    assert!(boundary.message_count > 0);
    assert!(boundary.last_message_id.is_some());
    let instruction = diagnostics.instruction.as_ref().expect("instruction");
    let document = backend.boss.document();
    assert_eq!(instruction.persona_id, document.persona_id);
    assert_eq!(
        instruction.base_revision,
        document
            .persona_defaults
            .get(PersonaDefaultRole::Boss)
            .starting_revision
            .or(document
                .persona_defaults
                .get(PersonaDefaultRole::Boss)
                .reviewed_revision)
    );
    assert!(instruction.digest.as_ref().is_some_and(|d| d.len() == 64));
    let stages: Vec<_> = diagnostics.stages.iter().map(|mark| mark.stage).collect();
    assert_eq!(
        stages,
        [
            RotationLifecycleStage::Triggered,
            RotationLifecycleStage::IntentPersisted,
            RotationLifecycleStage::SuccessorPrepared,
            RotationLifecycleStage::PointerPublished,
            RotationLifecycleStage::PredecessorFenced,
            RotationLifecycleStage::OwnershipCommitted,
            RotationLifecycleStage::SuccessorActivated,
        ]
    );
    assert!(matches!(
        diagnostics.terminal,
        Some(waku_boss::boss_rotation::RotationTerminal::Activated { .. })
    ));
    // A generation without compaction telemetry leaves an explicit gap.
    assert!(journal.signals.iter().any(|signal| matches!(
        signal,
        crate::boss_rotation::RotationSignal::CompactionTelemetryGap {
            session_id,
            ..
        } if *session_id == old
    )));
    // The committed handoff clears the visible lifecycle position.
    assert_eq!(backend.boss.document().rotation, None);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn delayed_and_paused_refresh_publish_their_status() {
    let root = std::env::temp_dir().join(format!("boss-rotation-status-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    busy_boss_chat(&backend, old, Uuid::new_v4());
    record_boss_event(
        &backend.task_state,
        &backend.task_store,
        old,
        &DriverEvent::UsageUpdated {
            context_tokens: Some(80),
            context_window: Some(100),
        },
    )
    .unwrap();
    let start = crate::model::unix_time();
    // The open turn keeps the intent pending — each pass still republishes.
    backend.reconcile_boss_rotation(start).unwrap();
    assert_eq!(backend.boss.document().rotation, None);
    // Seed a pending intent directly so the wait has durable timestamps.
    let path = root.join("boss/rotation.json");
    let mut journal = crate::boss_rotation::RotationJournal::load(&path).unwrap();
    journal.active_session_id = Some(old);
    journal
        .begin(
            backend.boss.identity_and_session().0.id,
            old,
            Uuid::new_v4(),
            start,
        )
        .unwrap();
    journal.persist(&path).unwrap();
    backend.reconcile_boss_rotation(start + 1).unwrap();
    let status = backend.boss.document().rotation.expect("rotation status");
    assert_eq!(status.phase, BossRotationPhase::Refreshing);
    backend.reconcile_boss_rotation(start + 31).unwrap();
    let status = backend.boss.document().rotation.expect("rotation status");
    assert_eq!(status.phase, BossRotationPhase::Delayed);
    assert_eq!(status.generation, 1);
    // A reconcile error pauses recovery with the concrete cause.
    journal.intent.as_mut().unwrap().boss_id = Uuid::new_v4();
    journal.persist(&path).unwrap();
    assert!(backend.reconcile_boss_rotation(start + 32).is_err());
    let status = backend.boss.document().rotation.expect("rotation status");
    assert_eq!(status.phase, BossRotationPhase::Paused);
    assert!(
        status
            .cause
            .as_deref()
            .is_some_and(|cause| cause.contains("another Boss")),
        "paused status carries the failure cause: {:?}",
        status.cause
    );
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn detected_compaction_marks_an_incident_and_refreshes_on_the_next_tick() {
    let root = std::env::temp_dir().join(format!("boss-compaction-incident-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let old = rotation_boss(&backend);
    busy_boss_chat(&backend, old, Uuid::new_v4());
    let now = crate::model::unix_time();
    // The provider event lands in both durable records: the continuity
    // ledger fences the generation and requests the refresh; the journal
    // correlates the signal and surfaces the incident status.
    crate::daemon::boss_rotation::record_boss_compaction_signal(
        &backend.data_dir,
        old,
        Some(Uuid::new_v4()),
        CompactionStage::Completed,
        crate::boss_rotation::CompactionOrigin::ProviderAutonomous,
        now,
    );
    let continuity_path = backend.boss_continuity_path();
    let mut ledger = waku_boss::rotation_continuity::ContinuityLedger::load(&continuity_path)
        .expect("continuity ledger");
    ledger.record_compaction_incident(old, now, "compaction activity observed");
    ledger.persist(&continuity_path).unwrap();
    // The busy chat cannot rotate yet — the incident stays visible.
    backend.reconcile_boss_rotation(now).unwrap();
    let status = backend.boss.document().rotation.expect("incident status");
    assert_eq!(status.phase, BossRotationPhase::CompactionIncident);
    // Once the chat settles the same pass rotates with the incident cause.
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
    backend.reconcile_boss_rotation(now + 1).unwrap();
    let next = backend.boss.identity_and_session().1.unwrap();
    assert_ne!(old, next);
    let journal =
        crate::boss_rotation::RotationJournal::load(&root.join("boss/rotation.json")).unwrap();
    let rotation = journal.rotations.last().expect("committed rotation");
    assert_eq!(rotation.diagnostics.cause, RotationCause::Incident);
    assert!(journal.incident.is_none());
    assert!(journal.signals.iter().any(|signal| matches!(
        signal,
        crate::boss_rotation::RotationSignal::Compaction {
            session_id,
            stage: CompactionStage::Completed,
            ..
        } if *session_id == old
    )));
    assert_eq!(backend.boss.document().rotation, None);
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn manual_compact_attempt_is_journal_telemetry_and_retry_is_human_only() {
    let root = std::env::temp_dir().join(format!("boss-compact-telemetry-{}", Uuid::new_v4()));
    let (backend, _) = surface_test_backend(&root);
    let boss = rotation_boss(&backend);
    let capture = Arc::new(CaptureDriver::default());
    let runtime = Uuid::new_v4();
    backend.sessions.lock().insert(
        boss,
        RuntimeEntry {
            plan_gate: None,
            runtime_id: runtime,
            driver: DriverHandle::from_control(capture.clone()),
            last_active: std::time::Instant::now(),
            resumable: true,
            computer_use_available: false,
            provider: ProviderKind::Codex,
            cwd: root.clone(),
        },
    );
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
    let journal =
        crate::boss_rotation::RotationJournal::load(&root.join("boss/rotation.json")).unwrap();
    assert!(journal.signals.iter().any(|signal| matches!(
        signal,
        crate::boss_rotation::RotationSignal::Compaction {
            session_id,
            stage: CompactionStage::Attempted,
            origin: crate::boss_rotation::CompactionOrigin::DaemonCommand,
            ..
        } if *session_id == boss
    )));
    // The retry operation is a human control — employees are refused.
    let denied = backend.handle_boss_operation(
        Some(boss),
        waku_protocol::boss::BossOperation::RetryRotation,
        &EventSink::detached(),
    );
    assert!(denied.is_err());
    backend
        .handle_boss_operation(
            None,
            waku_protocol::boss::BossOperation::RetryRotation,
            &EventSink::detached(),
        )
        .unwrap();
    drop(backend);
    std::fs::remove_dir_all(root).unwrap();
}
