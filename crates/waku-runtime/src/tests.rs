//! Deterministic stage-1 scenarios. Every barrier is explicit — `pump()`
//! is when provider truth becomes durable, `ManualClock` is when time
//! moves, scripted adapter outcomes are when the provider goes ambiguous.
//! No sleeps, no timing.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

use crate::adapter::{AdapterFactory, DispatchOutcome, OfflineMatrix, Termination};
use crate::contract::*;
use crate::fake::{FakeFactory, ManualClock, admit, submit};
use crate::host::{Budgets, Clock, HostOptions, RuntimeHost};
use crate::journal::JournalFault;

const NS: &str = "test-ns";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("waku-runtime-test-{}", Uuid::new_v4().simple())))
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Rig {
    host: RuntimeHost,
    factory: FakeFactory,
    clock: ManualClock,
    fault: JournalFault,
}

fn open(root: &TestRoot, budgets: Budgets) -> Rig {
    let factory = FakeFactory::new();
    let clock = ManualClock::new(1_000_000);
    let fault = Arc::new(AtomicBool::new(false));
    let options = HostOptions {
        namespace: NS.into(),
        budgets,
        journal_fault: Some(fault.clone()),
    };
    let host = RuntimeHost::open(
        root.path(),
        options,
        Arc::new(factory.clone()) as Arc<dyn AdapterFactory>,
        Box::new(clock.clone()) as Box<dyn Clock>,
    )
    .expect("host opens");
    Rig {
        host,
        factory,
        clock,
        fault,
    }
}

impl Rig {
    fn creds(&self) -> (String, String) {
        (
            self.host.credentials().controller.clone(),
            self.host.credentials().emergency.clone(),
        )
    }
    fn control(&mut self) -> ControlGrant {
        let (c, _) = self.creds();
        self.host
            .acquire_control(&c, &ClientHello::current())
            .expect("control acquired")
    }
    fn env(&self, grant: &ControlGrant, command: Uuid, action: CanonicalAction) -> ActionEnvelope {
        ActionEnvelope {
            namespace: NS.into(),
            runtime_instance: self.host.instance(),
            epoch: grant.epoch,
            retry_window_id: grant.retry_window_id,
            command_id: command,
            session: None,
            action,
        }
    }
    fn session_env(
        &self,
        grant: &ControlGrant,
        command: Uuid,
        session_id: Uuid,
        action: CanonicalAction,
    ) -> ActionEnvelope {
        let mut env = self.env(grant, command, action);
        env.session = Some(SessionRef {
            session_id,
            generation: 1,
        });
        env
    }
    /// Admit a fake session and return its session id.
    fn admit(&mut self, grant: &ControlGrant) -> (Uuid, CommandReceipt) {
        let session_id = Uuid::new_v4();
        let receipt = self
            .host
            .execute(
                &self.creds().0,
                self.env(grant, Uuid::new_v4(), admit(session_id)),
            )
            .expect("admission accepted");
        (session_id, receipt)
    }
    fn cursor(&self, session_id: Uuid, applied: u64) -> EventCursor {
        EventCursor {
            session_id,
            generation: 1,
            applied,
        }
    }
    fn events(&mut self, grant: &ControlGrant, session_id: Uuid, applied: u64) -> EventPage {
        let (c, _) = self.creds();
        self.host
            .events(&c, grant.epoch, self.cursor(session_id, applied))
            .expect("events read")
    }
}

fn disposition(receipt: &CommandReceipt) -> &CommandDisposition {
    &receipt.disposition
}

// -- attach / detach -----------------------------------------------------

#[test]
fn detached_controller_streaming_continues_and_replays_once() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant1 = rig.control();
    let (session, _) = rig.admit(&grant1);
    let fake = rig.factory.for_session(session).unwrap();
    fake.emit_live();

    let turn = Uuid::new_v4();
    let submit_cmd = Uuid::new_v4();
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant1,
                submit_cmd,
                session,
                submit(turn, Uuid::new_v4(), "go"),
            ),
        )
        .unwrap();
    assert_eq!(*disposition(&receipt), CommandDisposition::ProviderAccepted);
    fake.emit(ProviderEvent::TurnStarted {
        turn_id: turn,
        delivery_id: Uuid::new_v4(),
    });
    fake.emit_delta(turn, "one");
    fake.emit_delta(turn, "two");
    rig.host.pump();

    // The first controller applies the stream through seq 3.
    let page = rig.events(&grant1, session, 0);
    assert_eq!(page.events.len(), 4); // live + started + 2 deltas
    assert_eq!(page.high_water, 4);
    let (c, _) = rig.creds();
    rig.host
        .acknowledge(&c, grant1.epoch, rig.cursor(session, 2))
        .unwrap();

    // The daemon is replaced mid-stream: a new controller takes a new
    // epoch while the provider keeps working against the same runtime.
    fake.emit_delta(turn, "three");
    let grant2 = rig.control();
    assert_ne!(grant1.epoch, grant2.epoch);
    fake.emit_delta(turn, "four");
    fake.emit_completed(turn);
    rig.host.pump();

    // Reattach from the durable cursor: replay continues where the old
    // daemon left off — same turn, each event once, one terminal.
    let inventory = rig.host.reconcile(&rig.creds().0, grant2.epoch).unwrap();
    assert_eq!(inventory.sessions.len(), 1);
    // The turn completed; the session itself stays live.
    assert_eq!(inventory.sessions[0].status, SessionStatus::Live);
    let page = rig.events(&grant2, session, 2);
    let seqs: Vec<u64> = page.events.iter().map(|e| e.sequence).collect();
    assert_eq!(seqs, vec![3, 4, 5, 6, 7]);
    assert!(matches!(
        page.events.last().unwrap().event,
        ProviderEvent::TurnCompleted { .. }
    ));
    // Exactly once: the provider saw exactly one submit and one live marker
    // worth of dispatch for the whole stream.
    assert_eq!(fake.dispatch_count(), 1);
    // The submit command was terminally attributed to its turn's event.
    match rig
        .host
        .command(&rig.creds().0, submit_cmd)
        .unwrap()
        .disposition
    {
        CommandDisposition::Terminal {
            outcome: TerminalOutcome::Completed,
            event_sequence: Some(seq),
        } => assert_eq!(seq, 7),
        other => panic!("expected terminal completion, got {other:?}"),
    }
}

#[test]
fn reload_storm_never_duplicates_across_epochs() {
    // Twenty-five deterministic detach/reattach cycles across three
    // concurrent sessions — every event applied once, every accepted
    // command dispatched exactly once.
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let sessions: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    let mut grant = rig.control();
    for id in &sessions {
        rig.host
            .execute(&rig.creds().0, rig.env(&grant, Uuid::new_v4(), admit(*id)))
            .unwrap();
    }
    let mut applied = vec![0u64; sessions.len()];
    for cycle in 0..25usize {
        // New daemon: fresh epoch, same runtime, same sessions.
        grant = rig.control();
        let (c, _) = rig.creds();
        rig.host.reconcile(&c, grant.epoch).unwrap();
        for (i, id) in sessions.iter().enumerate() {
            let fake = rig.factory.for_session(*id).unwrap();
            fake.emit_delta(Uuid::new_v4(), &format!("cycle-{cycle}"));
            let turn = Uuid::new_v4();
            rig.host
                .execute(
                    &rig.creds().0,
                    rig.session_env(
                        &grant,
                        Uuid::new_v4(),
                        *id,
                        submit(turn, Uuid::new_v4(), "work"),
                    ),
                )
                .unwrap();
            let page = rig.events(&grant, *id, applied[i]);
            // Strictly contiguous: no duplicates, no gaps.
            for (offset, e) in page.events.iter().enumerate() {
                assert_eq!(e.sequence, applied[i] + 1 + offset as u64);
            }
            applied[i] = page.high_water;
        }
    }
    for id in &sessions {
        // One admission + twenty-five submits, exactly.
        assert_eq!(rig.factory.for_session(*id).unwrap().dispatch_count(), 25);
    }
}

// -- fencing -------------------------------------------------------------

#[test]
fn stale_epoch_cannot_mutate_but_reads_truth() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant1 = rig.control();
    let (session, _) = rig.admit(&grant1);
    let grant2 = rig.control();

    // A stale-epoch mutation is fenced before it is recorded.
    let err = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant1,
                Uuid::new_v4(),
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "stale"),
            ),
        )
        .unwrap_err();
    assert!(matches!(err, HostError::StaleEpoch { .. }));
    assert_eq!(
        rig.factory.for_session(session).unwrap().dispatch_count(),
        0
    );

    // But a retry of a *recorded* command still reads truth: the same id
    // and canonical payload returns the recorded disposition rather than
    // dispatching again — even presented under the stale epoch, because
    // dedup answers data, not new work.
    let cmd = Uuid::new_v4();
    let action = submit(Uuid::new_v4(), Uuid::new_v4(), "fresh");
    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(&grant2, cmd, session, action.clone()),
        )
        .unwrap();
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(&grant1, cmd, session, action),
        )
        .unwrap();
    assert_eq!(*disposition(&receipt), CommandDisposition::ProviderAccepted);
    assert_eq!(
        rig.factory.for_session(session).unwrap().dispatch_count(),
        1
    );
}

#[test]
fn payload_mismatch_under_recorded_id_rejects() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let cmd = Uuid::new_v4();
    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                cmd,
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "a"),
            ),
        )
        .unwrap();
    let err = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                cmd,
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "b"),
            ),
        )
        .unwrap_err();
    assert_eq!(err, HostError::PayloadMismatch);
    assert_eq!(
        rig.factory.for_session(session).unwrap().dispatch_count(),
        1
    );
}

#[test]
fn ambiguous_acceptance_is_held_uncertain_never_resent() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    fake.script_dispatch(DispatchOutcome::Unknown {
        detail: "transport died mid-dispatch".into(),
    });
    let cmd = Uuid::new_v4();
    let action = submit(Uuid::new_v4(), Uuid::new_v4(), "maybe");
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(&grant, cmd, session, action.clone()),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Uncertain {
            stage: UncertainStage::AcceptanceUnknown,
            ..
        }
    ));
    // The daemon restarts and asks again with the same persisted identity:
    // it gets the recorded uncertainty, not a second dispatch.
    let grant2 = rig.control();
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(&grant2, cmd, session, action),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Uncertain { .. }
    ));
    assert_eq!(fake.dispatch_count(), 1);
}

#[test]
fn stop_fence_blocks_delayed_work_and_claims_only_confirmed_stop() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();

    let turn = Uuid::new_v4();
    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                submit(turn, Uuid::new_v4(), "x"),
            ),
        )
        .unwrap();
    // Termination cannot be proven: the session stays visibly stopping.
    fake.script_terminate(Termination::Unconfirmed {
        detail: "provider wedged".into(),
    });
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::StopSession,
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Uncertain {
            stage: UncertainStage::OutcomeUnknown,
            ..
        }
    ));
    assert_eq!(
        rig.host.status(&rig.creds().0).unwrap().sessions[0].status,
        SessionStatus::Stopping
    );

    // A delayed prompt cannot dispatch once the fence is recorded.
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "late"),
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Rejected {
            code: RejectCode::Fenced,
            ..
        }
    ));
    assert_eq!(fake.dispatch_count(), 1);
}

#[test]
fn cancel_holds_dispatch_until_controller_permits() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    let turn = Uuid::new_v4();
    fake.emit(ProviderEvent::TurnStarted {
        turn_id: turn,
        delivery_id: Uuid::new_v4(),
    });
    rig.host.pump();

    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::CancelTurn { turn_id: turn },
            ),
        )
        .unwrap();

    // A queued delivery is preserved but held — not dispatched while the
    // cancellation settles.
    let held_cmd = Uuid::new_v4();
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                held_cmd,
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "queued"),
            ),
        )
        .unwrap();
    assert_eq!(*disposition(&receipt), CommandDisposition::DispatchPending);
    assert_eq!(fake.dispatch_count(), 0);
    assert_eq!(fake.cancels(), vec![turn]);

    // Explicit controller policy releases the queue in order.
    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::PermitDispatch,
            ),
        )
        .unwrap();
    assert_eq!(fake.dispatch_count(), 1);
    match rig
        .host
        .command(&rig.creds().0, held_cmd)
        .unwrap()
        .disposition
    {
        CommandDisposition::ProviderAccepted => {}
        other => panic!("held command should be accepted, got {other:?}"),
    }
}

// -- crash vs reload -----------------------------------------------------

#[test]
fn runtime_crash_interrupts_and_fences_without_replay() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    fake.emit_live();
    fake.emit_delta(Uuid::new_v4(), "before");
    rig.host.pump();
    let accepted_cmd = Uuid::new_v4();
    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                accepted_cmd,
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "in flight"),
            ),
        )
        .unwrap();
    assert_eq!(fake.dispatch_count(), 1);

    // The runtime dies — no drain. A new instance adopts the journal.
    drop(rig.host);
    let mut rig2 = open(&root, Budgets::default());
    assert!(matches!(
        rig2.host.recovery(),
        RecoveryState::Interrupted { sessions: 1 }
    ));
    // No adapter respawned, no turn replayed.
    assert!(rig2.factory.adapters().is_empty());
    let grant2 = rig2.control();

    // The same work is honestly interrupted and fenced.
    match rig2
        .host
        .command(&rig2.creds().0, accepted_cmd)
        .unwrap()
        .disposition
    {
        CommandDisposition::Uncertain {
            stage: UncertainStage::OutcomeUnknown,
            ..
        } => {}
        other => panic!("accepted command should be uncertain, got {other:?}"),
    }
    let receipt = rig2
        .host
        .execute(
            &rig2.creds().0,
            rig2.session_env(
                &grant2,
                Uuid::new_v4(),
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "after crash"),
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Rejected {
            code: RejectCode::Fenced,
            ..
        }
    ));

    // New admissions wait on inventory reconciliation.
    let denied = rig2
        .host
        .execute(
            &rig2.creds().0,
            rig2.env(&grant2, Uuid::new_v4(), admit(Uuid::new_v4())),
        )
        .unwrap();
    assert!(matches!(
        disposition(&denied),
        CommandDisposition::Rejected {
            code: RejectCode::InvalidAction,
            ..
        }
    ));
    let inventory = rig2.host.reconcile(&rig2.creds().0, grant2.epoch).unwrap();
    assert_eq!(inventory.sessions[0].status, SessionStatus::Interrupted);

    // Preserved transcript replays: the pre-crash events are still there.
    let page = rig2.events(&grant2, session, 0);
    assert_eq!(page.events.len(), 2);
    // And now admission is permitted again.
    let allowed = rig2
        .host
        .execute(
            &rig2.creds().0,
            rig2.env(&grant2, Uuid::new_v4(), admit(Uuid::new_v4())),
        )
        .unwrap();
    assert!(matches!(
        disposition(&allowed),
        CommandDisposition::Terminal {
            outcome: TerminalOutcome::Completed,
            ..
        }
    ));
}

#[test]
fn clean_drain_starts_fresh() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);

    // Idle drain defers while work lives.
    let report = rig
        .host
        .drain(&rig.creds().0, grant.epoch, DrainPolicy::WhenIdle)
        .unwrap();
    assert_eq!(
        report,
        DrainReport::Deferred {
            live_sessions: vec![session]
        }
    );

    let report = rig
        .host
        .drain(&rig.creds().0, grant.epoch, DrainPolicy::Forced)
        .unwrap();
    assert_eq!(report, DrainReport::Drained { sessions: 1 });
    drop(rig.host);

    let rig2 = open(&root, Budgets::default());
    assert_eq!(rig2.host.recovery(), RecoveryState::Clean);
    assert!(rig2.factory.adapters().is_empty());
}

// -- bounded behavior ----------------------------------------------------

#[test]
fn unpausable_provider_is_interrupted_at_budget() {
    let root = TestRoot::new();
    let budgets = Budgets {
        session_memory_bytes: 256,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    let turn = Uuid::new_v4();
    for i in 0..8 {
        fake.emit_delta(turn, &format!("{}{i}", "x".repeat(100)));
    }
    rig.host.pump();
    let inv = rig.host.reconcile(&rig.creds().0, grant.epoch).unwrap();
    assert_eq!(inv.sessions[0].status, SessionStatus::Interrupted);
    assert_eq!(inv.sessions[0].fence, Some(FenceKind::Backpressure));
    // The fence is durable before termination, so nothing else dispatches.
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "more"),
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Rejected {
            code: RejectCode::Fenced,
            ..
        }
    ));
}

#[test]
fn pausable_provider_backpressures_instead_of_dying() {
    let root = TestRoot::new();
    let budgets = Budgets {
        session_memory_bytes: 256,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    fake.set_matrix(OfflineMatrix {
        can_pause_intake: true,
        ..Default::default()
    });
    let turn = Uuid::new_v4();
    for i in 0..8 {
        fake.emit_delta(turn, &format!("{}{}", "x".repeat(100), i));
    }
    rig.host.pump();
    let inv = rig.host.reconcile(&rig.creds().0, grant.epoch).unwrap();
    // Paused at intake, not interrupted: the session stays live and its
    // remaining events wait in the bounded channel.
    assert_eq!(inv.sessions[0].status, SessionStatus::Live);
    assert_eq!(inv.sessions[0].fence, None);
}

#[test]
fn acknowledge_reclaims_and_expired_cursor_blocks_recovery() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    let turn = Uuid::new_v4();
    for i in 0..5 {
        fake.emit_delta(turn, &format!("delta-{i}"));
    }
    rig.host.pump();
    let page = rig.events(&grant, session, 0);
    assert_eq!(page.high_water, 5);

    let (c, _) = rig.creds();
    rig.host
        .acknowledge(&c, grant.epoch, rig.cursor(session, 4))
        .unwrap();
    // The first four events are reconstructible now — retained floor moved.
    let err = rig
        .host
        .events(&rig.creds().0, grant.epoch, rig.cursor(session, 2))
        .unwrap_err();
    assert_eq!(
        err,
        HostError::CursorExpired {
            cursor: 2,
            floor: 5
        }
    );
    // From the floor forward, replay still works.
    let page = rig.events(&grant, session, 4);
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.retained_floor, 5);
}

#[test]
fn oversized_message_is_a_sequenced_drop_not_silent_truncation() {
    let root = TestRoot::new();
    let budgets = Budgets {
        max_message_bytes: 128,
        session_memory_bytes: u64::MAX,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    let turn = Uuid::new_v4();
    fake.emit_delta(turn, &"x".repeat(4096));
    fake.emit_delta(turn, "small");
    rig.host.pump();
    let page = rig.events(&grant, session, 0);
    assert_eq!(page.events.len(), 2);
    assert!(matches!(
        page.events[0].event,
        ProviderEvent::Dropped {
            reason: DropReason::Oversize,
            ..
        }
    ));
    assert!(matches!(
        page.events[1].event,
        ProviderEvent::TurnDelta { .. }
    ));
}

#[test]
fn disk_failure_buffers_then_recovers_without_losing_events() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    let turn = Uuid::new_v4();

    // Disk goes away: accepted provider work buffers in bounded memory and
    // new commands stop before provider dispatch.
    rig.fault.store(true, Ordering::SeqCst);
    fake.emit_delta(turn, "during outage");
    rig.host.pump();
    assert!(!rig.host.status(&rig.creds().0).unwrap().persistence_ok);
    assert_eq!(rig.host.status(&rig.creds().0).unwrap().buffered_records, 1);
    let err = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                submit(turn, Uuid::new_v4(), "x"),
            ),
        )
        .unwrap_err();
    assert_eq!(err, HostError::PersistenceUnavailable);
    assert_eq!(fake.dispatch_count(), 0);

    // Disk returns: buffered records flush in order and work resumes.
    rig.fault.store(false, Ordering::SeqCst);
    fake.emit_delta(turn, "after recovery");
    rig.host.pump();
    assert!(rig.host.status(&rig.creds().0).unwrap().persistence_ok);
    assert_eq!(rig.host.status(&rig.creds().0).unwrap().buffered_records, 0);
    let page = rig.events(&grant, session, 0);
    assert_eq!(page.events.len(), 2);

    // And the whole outage is durable across a later crash recovery.
    drop(rig.host);
    let mut rig2 = open(&root, Budgets::default());
    let grant2 = rig2.control();
    rig2.host.reconcile(&rig2.creds().0, grant2.epoch).unwrap();
    let page = rig2.events(&grant2, session, 0);
    assert_eq!(page.events.len(), 2);
}

#[test]
fn corrupt_tail_is_truncated_and_sessions_still_fence() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    fake.emit_live();
    rig.host.pump();
    drop(rig.host);

    // A torn write corrupts the tail.
    let journal = root.path().join("journal.log");
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap();
    file.write_all(&[0xde, 0xad, 0xbe, 0xef, 0x00, 0x01])
        .unwrap();
    drop(file);

    let mut rig2 = open(&root, Budgets::default());
    assert!(rig2.host.status(&rig2.creds().0).unwrap().tail_corrupt);
    assert!(matches!(
        rig2.host.recovery(),
        RecoveryState::Interrupted { .. }
    ));
    let grant2 = rig2.control();
    rig2.host.reconcile(&rig2.creds().0, grant2.epoch).unwrap();
    let page = rig2.events(&grant2, session, 0);
    assert_eq!(page.events.len(), 1); // the live marker survived
}

// -- authority -----------------------------------------------------------

#[test]
fn emergency_stop_works_without_controller_and_is_fenced() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();

    // Wrong authority cannot stop anything.
    let err = rig.host.emergency_stop("wrong-token", session).unwrap_err();
    assert_eq!(err, HostError::Unauthorized);

    // Narrow emergency authority fences the session without a control epoch.
    let status = rig.host.emergency_stop(&rig.creds().1, session).unwrap();
    assert_eq!(status, SessionStatus::Terminated);
    assert_eq!(fake.terminate_count(), 1);
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "after emergency"),
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Rejected {
            code: RejectCode::Fenced,
            ..
        }
    ));
}

#[test]
fn incompatible_hello_preserves_sessions() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    rig.admit(&grant);

    let foreign = ClientHello {
        protocol_min: PROTOCOL_VERSION + 1,
        protocol_max: PROTOCOL_VERSION + 1,
        capabilities: vec![],
        build: None,
    };
    let err = rig
        .host
        .acquire_control(&rig.creds().0, &foreign)
        .unwrap_err();
    assert!(matches!(err, HostError::Incompatible(_)));
    // The incompatible controller never touched the live session.
    assert_eq!(
        rig.host.status(&rig.creds().0).unwrap().sessions[0].status,
        SessionStatus::Live
    );
}

#[test]
fn second_host_cannot_take_the_namespace() {
    let root = TestRoot::new();
    let rig = open(&root, Budgets::default());
    let second = RuntimeHost::open(
        root.path(),
        HostOptions::new(NS),
        Arc::new(FakeFactory::new()),
        Box::new(ManualClock::new(0)),
    );
    assert!(matches!(second, Err(HostError::AlreadyOwned)));
    drop(rig);
}

// -- retry windows ---------------------------------------------------------

#[test]
fn expired_retry_window_still_rejects_after_clock_rollback() {
    let root = TestRoot::new();
    let budgets = Budgets {
        retry_window_ms: 1_000,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let turn = Uuid::new_v4();
    let cmd = Uuid::new_v4();
    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(&grant, cmd, session, submit(turn, Uuid::new_v4(), "a")),
        )
        .unwrap();
    // The turn ends so the command is terminal — sweepable on expiry.
    fake_emit_completed(&mut rig, session, turn);

    // Time passes beyond the window; a control acquisition pins the
    // persisted floor there. Then the clock rolls back — the floor keeps
    // `now` honest: expired identities stay expired.
    rig.clock.advance(60_000);
    let grant2 = rig.control();
    rig.clock.set(500);
    let err = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(&grant2, cmd, session, submit(turn, Uuid::new_v4(), "a")),
        )
        .unwrap_err();
    assert_eq!(err, HostError::ExpiredWindow(cmd));
    // And a brand-new command under the elapsed window is refused as
    // expired — never treated as fresh work.
    let mut env = rig.session_env(
        &grant2,
        Uuid::new_v4(),
        session,
        submit(Uuid::new_v4(), Uuid::new_v4(), "b"),
    );
    env.retry_window_id = grant.retry_window_id;
    let err = rig.host.execute(&rig.creds().0, env).unwrap_err();
    assert_eq!(err, HostError::ExpiredWindow(grant.retry_window_id));
}

fn fake_emit_completed(rig: &mut Rig, session: Uuid, turn: Uuid) {
    rig.factory
        .for_session(session)
        .unwrap()
        .emit_completed(turn);
    rig.host.pump();
}

// -- tools and approvals ---------------------------------------------------

#[test]
fn request_resolves_once_under_current_authority() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    fake.emit(ProviderEvent::RequestOpened {
        request_id: "req-1".into(),
        class: RequestClass::Approval,
        subject: json!({"prompt": "allow?"}),
        deadline_ms: None,
    });
    rig.host.pump();

    // Unknown request identity refuses.
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::ResolveRequest {
                    request_id: "nope".into(),
                    resolution: json!("yes"),
                },
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Rejected {
            code: RejectCode::UnknownRequest,
            ..
        }
    ));

    // The real request resolves once.
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::ResolveRequest {
                    request_id: "req-1".into(),
                    resolution: json!("yes"),
                },
            ),
        )
        .unwrap();
    assert_eq!(*disposition(&receipt), CommandDisposition::ProviderAccepted);
    assert_eq!(fake.dispatch_count(), 1);

    // A second answer under a different command id is rejected — approvals
    // are never reused for a different request.
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::ResolveRequest {
                    request_id: "req-1".into(),
                    resolution: json!("no"),
                },
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Rejected {
            code: RejectCode::AlreadyResolved,
            ..
        }
    ));
    assert_eq!(fake.dispatch_count(), 1);
}

#[test]
fn consumed_answer_with_lost_ack_reconciles_by_evidence() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    fake.emit(ProviderEvent::RequestOpened {
        request_id: "req-9".into(),
        class: RequestClass::Tool,
        subject: json!({"tool": "read"}),
        deadline_ms: None,
    });
    rig.host.pump();

    // The provider may have consumed the answer but the acknowledgement
    // was lost — held uncertain, not resent.
    fake.script_dispatch(DispatchOutcome::Unknown {
        detail: "ack lost".into(),
    });
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::ResolveRequest {
                    request_id: "req-9".into(),
                    resolution: json!({"outcome": "ok"}),
                },
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Uncertain { .. }
    ));

    // Qualified adapter evidence (the provider reporting consumption)
    // reconciles the uncertainty — without resending anything.
    fake.emit(ProviderEvent::RequestAnswered {
        request_id: "req-9".into(),
    });
    rig.host.pump();
    assert_eq!(fake.dispatch_count(), 1);
    let inv = rig.host.reconcile(&rig.creds().0, grant.epoch).unwrap();
    // The request is no longer pending — its answer is delivered.
    assert!(inv.sessions[0].pending_requests.is_empty());
}

// -- journal integrity -----------------------------------------------------

#[test]
fn persisted_floor_outlives_the_instance() {
    // Retry-window expiry is evaluated against the persisted floor, so a
    // restart with a rolled-back clock cannot reopen expired identities.
    let root = TestRoot::new();
    let budgets = Budgets {
        retry_window_ms: 1_000,
        ..Default::default()
    };
    let mut rig = open(&root, budgets.clone());
    let grant = rig.control();
    let window = grant.retry_window_id;
    rig.admit(&grant);
    rig.clock.advance(60_000);
    // A record at the advanced time pins the persisted floor before death.
    rig.control();
    drop(rig.host);

    let mut rig2 = open(&root, budgets);
    // The new instance's clock reads before the window — the persisted
    // floor is what keeps the expired identity rejected.
    rig2.clock.set(1);
    let grant2 = rig2.control();
    let env = ActionEnvelope {
        namespace: NS.into(),
        runtime_instance: rig2.host.instance(),
        epoch: grant2.epoch,
        retry_window_id: window, // the old epoch's window
        command_id: Uuid::new_v4(),
        session: None,
        action: admit(Uuid::new_v4()),
    };
    let err = rig2.host.execute(&rig2.creds().0, env).unwrap_err();
    // The old window is expired against the persisted floor — rejected as
    // expired, not resurrected by the rollback.
    assert_eq!(err, HostError::ExpiredWindow(window));
}

#[test]
fn compaction_preserves_pending_requests_and_requested_config() {
    // A small journal forces the rewrite path quickly. What survives
    // compaction must be the same truth recovery would build.
    let root = TestRoot::new();
    let budgets = Budgets {
        aggregate_journal_bytes: 8 * 1024,
        control_reserve_bytes: 256,
        compact_at_fraction: 0.3,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    let turn = Uuid::new_v4();
    for i in 0..20 {
        fake.emit_delta(turn, &format!("delta-{i}"));
    }
    fake.emit(ProviderEvent::RequestOpened {
        request_id: "req-kept".into(),
        class: RequestClass::Tool,
        subject: serde_json::json!({"tool": "run"}),
        deadline_ms: None,
    });
    rig.host.pump();
    // The daemon applies the early events; their payloads can be reclaimed
    // and the journal rewritten.
    let (c, _) = rig.creds();
    rig.host
        .acknowledge(&c, grant.epoch, rig.cursor(session, 15))
        .unwrap();

    let before = rig.host.status(&rig.creds().0).unwrap().journal_bytes;
    drop(rig.host);
    let mut rig2 = open(&root, Budgets::default());
    let grant2 = rig2.control();
    let inv = rig2.host.reconcile(&rig2.creds().0, grant2.epoch).unwrap();
    // The pending request survived the rewrite under its original
    // identity — the daemon can still resolve it after reconciliation.
    assert_eq!(
        inv.sessions[0].pending_requests,
        vec!["req-kept".to_string()]
    );
    // Retained events still replay from the floor.
    let page = rig2.events(&grant2, session, 15);
    assert!(page.retained_floor >= 16);
    assert!(!page.events.is_empty());
    let after = rig2.host.status(&rig2.creds().0).unwrap().journal_bytes;
    assert!(
        after <= before + 64 * 1024,
        "journal did not grow unbounded"
    );
}

#[test]
fn reconfigure_requires_forward_revisions() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();

    let target = RequestedConfig {
        provider: None,
        model: Some("model-b".into()),
        effort: Some("high".into()),
        options: Value::Null,
    };
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::Reconfigure {
                    revision: 1,
                    target: target.clone(),
                },
            ),
        )
        .unwrap();
    assert_eq!(*disposition(&receipt), CommandDisposition::ProviderAccepted);

    // Requested and effective are separate facts until activation.
    let inv = rig.host.reconcile(&rig.creds().0, grant.epoch).unwrap();
    assert!(inv.sessions[0].effective.is_none());
    fake.emit(ProviderEvent::ConfigActivated {
        revision: 1,
        boundary: ActivationBoundary::NextTurn,
        effective: target,
    });
    rig.host.pump();
    let inv = rig.host.reconcile(&rig.creds().0, grant.epoch).unwrap();
    assert_eq!(inv.sessions[0].effective.as_ref().unwrap().revision, 1);

    // A non-advancing revision is stale.
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                CanonicalAction::Reconfigure {
                    revision: 1,
                    target: RequestedConfig::default(),
                },
            ),
        )
        .unwrap();
    assert!(matches!(
        disposition(&receipt),
        CommandDisposition::Rejected {
            code: RejectCode::Stale,
            ..
        }
    ));
}

// -- resolved-metadata retention --------------------------------------------

#[test]
fn expired_command_tombstones_stay_bounded_and_keep_rejecting() {
    // Resolved identities are never evicted to make room — past the bound
    // the host refuses NEW commands while recorded truth still answers.
    let root = TestRoot::new();
    let budgets = Budgets {
        retry_window_ms: 1_000,
        tombstone_limit: 4,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);

    // Submits to a missing session reject at once — six resolved commands
    // plus the admission itself are tombstone material.
    let mut tombstoned = Vec::new();
    for _ in 0..6 {
        let cmd = Uuid::new_v4();
        let receipt = rig
            .host
            .execute(
                &rig.creds().0,
                rig.session_env(
                    &grant,
                    cmd,
                    Uuid::new_v4(),
                    submit(Uuid::new_v4(), Uuid::new_v4(), "x"),
                ),
            )
            .unwrap();
        assert!(matches!(
            disposition(&receipt),
            CommandDisposition::Rejected { .. }
        ));
        tombstoned.push(cmd);
    }
    // An accepted command is unresolved evidence — it never tombstones.
    let turn = Uuid::new_v4();
    let live_env = rig.session_env(
        &grant,
        Uuid::new_v4(),
        session,
        submit(turn, Uuid::new_v4(), "live"),
    );
    let receipt = rig.host.execute(&rig.creds().0, live_env.clone()).unwrap();
    assert_eq!(*disposition(&receipt), CommandDisposition::ProviderAccepted);

    // Past the window the resolved commands tombstone; past the bound the
    // next new command refuses.
    rig.clock.advance(60_000);
    let grant2 = rig.control();
    let err = rig
        .host
        .execute(
            &rig.creds().0,
            rig.env(&grant2, Uuid::new_v4(), admit(Uuid::new_v4())),
        )
        .unwrap_err();
    assert!(matches!(err, HostError::Capacity(_)));
    assert!(rig.host.status(&rig.creds().0).unwrap().expired_command_ids >= 6);

    // Tombstoned identities still reject — even under a fresh window.
    let err = rig
        .host
        .execute(
            &rig.creds().0,
            rig.env(&grant2, tombstoned[0], admit(Uuid::new_v4())),
        )
        .unwrap_err();
    assert_eq!(err, HostError::ExpiredWindow(tombstoned[0]));
    assert!(matches!(
        rig.host
            .command(&rig.creds().0, tombstoned[0])
            .unwrap()
            .disposition,
        CommandDisposition::Rejected {
            code: RejectCode::Stale,
            ..
        }
    ));
    // Recorded truth still dedups — the bound never rewrites history and
    // never lets an old identity become new work.
    let receipt = rig.host.execute(&rig.creds().0, live_env).unwrap();
    assert_eq!(*disposition(&receipt), CommandDisposition::ProviderAccepted);
}

#[test]
fn expired_identity_rejection_survives_compaction_and_reload() {
    let root = TestRoot::new();
    let budgets = Budgets {
        retry_window_ms: 1_000,
        aggregate_journal_bytes: 16 * 1024,
        control_reserve_bytes: 512,
        compact_at_fraction: 0.4,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();

    // Four commands terminally attributed to their turns.
    let mut tombstoned = Vec::new();
    for _ in 0..4 {
        let turn = Uuid::new_v4();
        let cmd = Uuid::new_v4();
        rig.host
            .execute(
                &rig.creds().0,
                rig.session_env(&grant, cmd, session, submit(turn, Uuid::new_v4(), "x")),
            )
            .unwrap();
        fake_emit_completed(&mut rig, session, turn);
        tombstoned.push(cmd);
    }
    for _ in 0..30 {
        fake.emit_delta(Uuid::new_v4(), &"d".repeat(64));
    }
    rig.host.pump();
    rig.clock.advance(60_000);
    let grant2 = rig.control();
    let high = rig.events(&grant2, session, 0).high_water;
    // Acknowledgement reclaims the stream and drives the tombstone sweep
    // plus compaction — retained payloads stay reconstructible.
    rig.host
        .acknowledge(&rig.creds().0, grant2.epoch, rig.cursor(session, high))
        .unwrap();
    assert!(rig.host.status(&rig.creds().0).unwrap().expired_command_ids >= 5);
    assert!(
        rig.host.status(&rig.creds().0).unwrap().journal_bytes < 16 * 1024,
        "compaction did not bound the journal"
    );

    drop(rig.host);
    let mut rig2 = open(&root, Budgets::default());
    let grant3 = rig2.control();
    rig2.host.reconcile(&rig2.creds().0, grant3.epoch).unwrap();
    // Under a new instance, epoch and window the resolved identity still
    // rejects — compaction and reload changed nothing.
    let err = rig2
        .host
        .execute(
            &rig2.creds().0,
            rig2.session_env(
                &grant3,
                tombstoned[0],
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "x"),
            ),
        )
        .unwrap_err();
    assert_eq!(err, HostError::ExpiredWindow(tombstoned[0]));
}

#[test]
fn sustained_acknowledged_traffic_keeps_metadata_bounded() {
    // The acceptance shape: rounds of acknowledged stream traffic plus
    // resolving commands under small budgets. Events reclaim, commands
    // tombstone, compaction rewrites — no metadata grows without bound.
    let root = TestRoot::new();
    let budgets = Budgets {
        retry_window_ms: 1_000,
        tombstone_limit: 128,
        aggregate_journal_bytes: 32 * 1024,
        control_reserve_bytes: 512,
        compact_at_fraction: 0.5,
        ..Default::default()
    };
    let mut rig = open(&root, budgets);
    let mut grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();

    let mut applied = 0u64;
    for _ in 0..30 {
        grant = rig.control();
        let turn = Uuid::new_v4();
        rig.host
            .execute(
                &rig.creds().0,
                rig.session_env(
                    &grant,
                    Uuid::new_v4(),
                    session,
                    submit(turn, Uuid::new_v4(), "go"),
                ),
            )
            .unwrap();
        for _ in 0..8 {
            fake.emit_delta(turn, &"x".repeat(128));
        }
        fake.emit_completed(turn);
        rig.host.pump();
        applied = rig.events(&grant, session, applied).high_water;
        rig.host
            .acknowledge(&rig.creds().0, grant.epoch, rig.cursor(session, applied))
            .unwrap();
        rig.clock.advance(2_000);
    }

    let status = rig.host.status(&rig.creds().0).unwrap();
    // Every acknowledged payload is reclaimed and compaction holds the
    // journal inside its aggregate bound through sustained traffic.
    assert_eq!(status.retained_event_bytes, 0);
    assert!(
        status.journal_bytes < 32 * 1024,
        "journal grew past its bound: {}",
        status.journal_bytes
    );
    // Resolved identities accumulated into bounded tombstones — under the
    // cap, so the host still accepts work.
    assert!(status.expired_command_ids >= 28);
    let grant = rig.control();
    rig.host
        .execute(
            &rig.creds().0,
            rig.session_env(
                &grant,
                Uuid::new_v4(),
                session,
                submit(Uuid::new_v4(), Uuid::new_v4(), "still working"),
            ),
        )
        .unwrap();
}

#[test]
fn agent_ingress_env_pins_the_cli_on_path_and_carries_pairs() {
    let ingress = AgentIngress {
        env: [(
            "GODDARD_RUNTIME_INGRESS".to_string(),
            "/private/ingress.json".to_string(),
        )]
        .into_iter()
        .collect(),
        cli_dir: Some("/pinned/helpers".into()),
    };
    let env = agent_ingress_env(&ingress, Some(std::ffi::OsStr::new("/usr/bin")));
    assert_eq!(
        env.iter()
            .find(|(name, _)| name == "GODDARD_RUNTIME_INGRESS")
            .map(|(_, value)| value.as_os_str()),
        Some(std::ffi::OsStr::new("/private/ingress.json"))
    );
    assert_eq!(
        env.iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.as_os_str()),
        std::env::join_paths([std::path::Path::new("/pinned/helpers"), std::path::Path::new("/usr/bin"),])
            .as_deref()
            .ok()
    );
    // No inherited PATH — a standard fallback keeps the helper findable.
    let fallback = agent_ingress_env(&ingress, None);
    assert_eq!(
        fallback
            .iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.as_os_str()),
        std::env::join_paths([
            std::path::Path::new("/pinned/helpers"),
            std::path::Path::new("/usr/bin"),
            std::path::Path::new("/bin"),
            std::path::Path::new("/usr/local/bin"),
        ])
        .as_deref()
        .ok()
    );
}

#[test]
fn ingress_descriptor_uses_the_wire_field_names() {
    let descriptor = IngressDescriptor {
        version: INGRESS_DESCRIPTOR_VERSION,
        root: PathBuf::from("/private/goddard"),
        task_id: Uuid::new_v4(),
        grant: SessionGrant {
            grant_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            generation: 1,
            token: "secret".into(),
            expires_at_ms: 1,
        },
    };
    let value = serde_json::to_value(&descriptor).unwrap();
    assert!(value["taskId"].is_string());
    assert!(value["grant"].is_object());
    assert!(value["root"].is_string());
    assert!(value.get("task_id").is_none());
    assert!(value.get("runtimeRoot").is_none());
    let round_trip: IngressDescriptor = serde_json::from_value(value).unwrap();
    assert_eq!(round_trip, descriptor);
    // Unknown fields fail closed — a foreign descriptor is never trusted.
    let mut foreign = serde_json::to_value(&descriptor).unwrap();
    foreign["extra"] = json!(true);
    assert!(serde_json::from_value::<IngressDescriptor>(foreign).is_err());
}
