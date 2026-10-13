//! Deterministic stage-2C host tests: scoped ingress grants, deduplicated
//! agent requests, service forwarding, deadline truth, and durable
//! resource claims. Barriers are explicit — `pump()` for provider truth,
//! `ManualClock` for deadlines, scripted outcomes for ambiguity.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use uuid::Uuid;

use crate::adapter::AdapterFactory;
use crate::contract::*;
use crate::fake::{FakeFactory, ManualClock, admit};
use crate::host::{Budgets, Clock, HostOptions, RuntimeHost};
use crate::journal::JournalFault;

const NS: &str = "stage2c-ns";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("waku-2c-test-{}", Uuid::new_v4().simple())))
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
    #[allow(dead_code)]
    factory: FakeFactory,
    clock: ManualClock,
    fault: JournalFault,
}

fn open(root: &TestRoot) -> Rig {
    open_with(root, Budgets::default())
}

fn open_with(root: &TestRoot, budgets: Budgets) -> Rig {
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
    fn admit(&mut self, grant: &ControlGrant) -> Uuid {
        let session_id = Uuid::new_v4();
        self.host
            .execute(
                &self.creds().0,
                self.env(grant, Uuid::new_v4(), admit(session_id)),
            )
            .expect("admission accepted");
        session_id
    }
    fn sref(&self, session_id: Uuid) -> SessionRef {
        SessionRef {
            session_id,
            generation: 1,
        }
    }
    fn grant_for(&mut self, grant: &ControlGrant, session_id: Uuid, ttl_ms: u64) -> SessionGrant {
        self.host
            .issue_grant(&self.creds().0, grant.epoch, self.sref(session_id), ttl_ms)
            .expect("grant issued")
    }
    fn request(&self, deadline_ms: u64) -> IngressRequest {
        IngressRequest {
            request_id: Uuid::new_v4(),
            class: IngressKind::Cli,
            subject: json!({ "op": "status" }),
            deadline_ms,
        }
    }
}

// -- grants ---------------------------------------------------------------

#[test]
fn grant_verifies_token_scope_and_expiry() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 60_000);

    let (c, _) = rig.creds();
    assert_eq!(rig.host.check_grant(&grant).unwrap(), rig.sref(session));

    // A forged token is indistinguishable from a dead grant.
    let mut forged = grant.clone();
    forged.token = "forged".into();
    assert_eq!(rig.host.check_grant(&forged), Err(HostError::Unauthorized));

    // A grant for a different session cannot ride this one's traffic.
    let other = rig.admit(&control);
    let mut foreign = grant.clone();
    foreign.session_id = other;
    assert_eq!(rig.host.check_grant(&foreign), Err(HostError::Unauthorized));

    // Expiry is immutable: the same grant dies at its deadline and is
    // never renewed.
    rig.clock.advance(30_000);
    let _replacement = rig.control();
    assert!(rig.host.check_grant(&grant).is_ok());
    rig.clock.advance(31_000);
    let _replacement = rig.control();
    assert_eq!(rig.host.check_grant(&grant), Err(HostError::Unauthorized));
    let _ = c;
}

#[test]
fn grant_not_issued_for_fenced_generation() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let original = rig.grant_for(&control, session, 60_000);

    // Stop the session, then a grant mint for its generation refuses.
    let receipt = rig
        .host
        .execute(
            &rig.creds().0,
            ActionEnvelope {
                session: Some(rig.sref(session)),
                ..rig.env(&control, Uuid::new_v4(), CanonicalAction::StopSession)
            },
        )
        .unwrap();
    assert!(matches!(
        receipt.disposition,
        CommandDisposition::Terminal { .. } | CommandDisposition::Uncertain { .. }
    ));
    let result = rig.host.issue_grant(
        &rig.creds().0,
        control.epoch,
        rig.sref(session),
        60_000,
    );
    assert!(matches!(result, Err(HostError::Malformed(_))));
    let _replacement = rig.control();
    assert_eq!(rig.host.check_grant(&original), Err(HostError::Unauthorized));
}

// -- ingress dedup and forwarding ------------------------------------------

#[test]
fn ingress_dedups_identity_and_rejects_payload_mismatch() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 60_000);

    let request = rig.request(2_000_000);
    let first = rig.host.ingress_submit(&grant, request.clone()).unwrap();
    assert_eq!(first.disposition, IngressDisposition::Pending);

    // Same identity + same payload returns the recorded truth.
    let again = rig.host.ingress_submit(&grant, request.clone()).unwrap();
    assert_eq!(again, first);

    // Same identity + different payload is refused, not recorded.
    let mut mutated = request.clone();
    mutated.subject = json!({ "op": "exec" });
    assert_eq!(
        rig.host.ingress_submit(&grant, mutated),
        Err(HostError::PayloadMismatch)
    );
}

#[test]
fn ingress_forward_resolve_and_single_terminal_truth() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 60_000);

    let request = rig.request(2_000_000);
    rig.host.ingress_submit(&grant, request.clone()).unwrap();

    // The service drain marks it forwarded and hands it to the daemon.
    let forwarded = rig
        .host
        .ingress_drain(&rig.creds().0, control.epoch)
        .unwrap();
    assert_eq!(forwarded.len(), 1);
    assert_eq!(forwarded[0].request_id, request.request_id);
    let record = rig.host.ingress_query(&grant, request.request_id).unwrap();
    assert_eq!(record.disposition, IngressDisposition::Forwarded);
    assert!(record.forwarded);

    // The daemon resolves; the outcome writes once.
    let resolved = rig
        .host
        .ingress_resolve(
            &rig.creds().0,
            control.epoch,
            request.request_id,
            ForwardedOutcome::Completed {
                result: json!({ "ok": true }),
            },
        )
        .unwrap();
    assert_eq!(
        resolved.disposition,
        IngressDisposition::Completed {
            result: json!({ "ok": true })
        }
    );
    // A second contradictory answer cannot rewrite the terminal truth.
    let still = rig
        .host
        .ingress_resolve(
            &rig.creds().0,
            control.epoch,
            request.request_id,
            ForwardedOutcome::Failed {
                detail: "contradiction".into(),
            },
        )
        .unwrap();
    assert_eq!(still.disposition, resolved.disposition);
}

#[test]
fn ingress_deadline_splits_expired_from_uncertain() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 10_000_000);

    // One request reaches the daemon before the deadline; one is
    // submitted after the drain and is never dispatched.
    let with_daemon = rig.request(1_500_000);
    rig.host
        .ingress_submit(&grant, with_daemon.clone())
        .unwrap();
    let forwarded = rig
        .host
        .ingress_drain(&rig.creds().0, control.epoch)
        .unwrap();
    assert_eq!(forwarded.len(), 1);
    assert_eq!(forwarded[0].request_id, with_daemon.request_id);
    let never = rig.request(1_500_000);
    rig.host.ingress_submit(&grant, never.clone()).unwrap();

    rig.clock.set(1_600_000);
    rig.host.pump();

    // Never-dispatched expires; dispatched-but-unresolved is uncertain —
    // the daemon may have completed it.
    let expired = rig.host.ingress_query(&grant, never.request_id).unwrap();
    assert_eq!(expired.disposition, IngressDisposition::Expired);
    let uncertain = rig
        .host
        .ingress_query(&grant, with_daemon.request_id)
        .unwrap();
    assert!(matches!(
        uncertain.disposition,
        IngressDisposition::Uncertain { .. }
    ));
}

#[test]
fn forwarded_but_unresolved_survives_controller_replacement() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control1 = rig.control();
    let session = rig.admit(&control1);
    let grant = rig.grant_for(&control1, session, 60_000);
    let request = rig.request(2_000_000);
    rig.host.ingress_submit(&grant, request.clone()).unwrap();

    // Daemon 1 drains and dies before answering. Daemon 2's epoch still
    // owes the request — the daemon dedups by identity on its side.
    let _ = rig.host.ingress_drain(&rig.creds().0, control1.epoch).unwrap();
    let control2 = rig.control();
    let forwarded = rig
        .host
        .ingress_drain(&rig.creds().0, control2.epoch)
        .unwrap();
    assert_eq!(forwarded.len(), 1);
    assert_eq!(forwarded[0].request_id, request.request_id);
    assert_eq!(forwarded[0].deadline_ms, request.deadline_ms);
}

#[test]
fn runtime_restart_uncertains_forwarded_and_refuses_pending() {
    let root = TestRoot::new();
    let (grant, request_pending, request_forwarded) = {
        let mut rig = open(&root);
        let control = rig.control();
        let session = rig.admit(&control);
        let grant = rig.grant_for(&control, session, 3_600_000);
        let forwarded = rig.request(2_000_000);
        rig.host.ingress_submit(&grant, forwarded.clone()).unwrap();
        let _ = rig
            .host
            .ingress_drain(&rig.creds().0, control.epoch)
            .unwrap();
        // Opened after the drain — provably never dispatched.
        let pending = rig.request(2_000_000);
        rig.host.ingress_submit(&grant, pending.clone()).unwrap();
        (grant, pending.request_id, forwarded.request_id)
        // rig drops — the host "dies" without draining.
    };

    // Reopen the namespace: crash recovery fences the session, marks the
    // forwarded request uncertain, and refuses pending execution. Historical
    // outcomes remain queryable under the recorded grant.
    let mut rig = open(&root);
    assert!(matches!(
        rig.host.recovery(),
        RecoveryState::Interrupted { sessions: 1 }
    ));
    let control = rig.control();
    let inventory = rig.host.reconcile(&rig.creds().0, control.epoch).unwrap();
    assert_eq!(inventory.sessions[0].status, SessionStatus::Interrupted);

    // The forwarded request is uncertain — never silently replayed.
    let record = rig
        .host
        .ingress_query(&grant, request_forwarded)
        .unwrap();
    assert!(matches!(
        record.disposition,
        IngressDisposition::Uncertain { .. }
    ));
    // Crash fencing revokes pending execution authority. Never dispatch it
    // automatically under the replacement controller.
    let drained = rig
        .host
        .ingress_drain(&rig.creds().0, control.epoch)
        .unwrap();
    assert!(drained.is_empty());
    assert!(matches!(rig.host.ingress_query(&grant, request_pending).unwrap().disposition,
        IngressDisposition::Refused { .. }));
}

// -- claims -----------------------------------------------------------------

#[test]
fn claims_register_release_and_survive_detachment() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control1 = rig.control();
    let session = rig.admit(&control1);
    let claim = ResourceClaim {
        claim_id: Uuid::new_v4(),
        session: rig.sref(session),
        resources: vec!["native_builds:1".into()],
        detail: json!({ "purpose": "cli worker" }),
    };
    rig.host
        .register_claim(&rig.creds().0, control1.epoch, claim.clone())
        .unwrap();

    // The daemon is replaced; reconciliation still sees the live claim —
    // detachment cannot silently release it.
    let control2 = rig.control();
    let inventory = rig.host.reconcile(&rig.creds().0, control2.epoch).unwrap();
    assert_eq!(inventory.sessions[0].claims, vec![claim.clone()]);

    // A stale epoch cannot release it.
    assert!(matches!(
        rig.host
            .release_claim(&rig.creds().0, control1.epoch, claim.claim_id),
        Err(HostError::StaleEpoch { .. })
    ));
    // The current controller releases explicitly.
    rig.host
        .release_claim(&rig.creds().0, control2.epoch, claim.claim_id)
        .unwrap();
    let inventory = rig.host.reconcile(&rig.creds().0, control2.epoch).unwrap();
    assert!(inventory.sessions[0].claims.is_empty());
}

#[test]
fn claim_on_fenced_generation_is_refused() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    rig.host
        .execute(
            &rig.creds().0,
            ActionEnvelope {
                session: Some(rig.sref(session)),
                ..rig.env(&control, Uuid::new_v4(), CanonicalAction::StopSession)
            },
        )
        .unwrap();
    let claim = ResourceClaim {
        claim_id: Uuid::new_v4(),
        session: rig.sref(session),
        resources: vec![],
        detail: Value::Null,
    };
    assert!(matches!(
        rig.host
            .register_claim(&rig.creds().0, control.epoch, claim),
        Err(HostError::Malformed(_))
    ));
}

// -- ingress on fenced sessions ---------------------------------------------

#[test]
fn ingress_on_fenced_session_rejects_revoked_authority() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 60_000);
    rig.host
        .execute(
            &rig.creds().0,
            ActionEnvelope {
                session: Some(rig.sref(session)),
                ..rig.env(&control, Uuid::new_v4(), CanonicalAction::StopSession)
            },
        )
        .unwrap();
    let request = rig.request(2_000_000);
    assert_eq!(rig.host.ingress_submit(&grant, request), Err(HostError::Unauthorized));
    // Nothing fenced reaches a service drain.
    let drained = rig
        .host
        .ingress_drain(&rig.creds().0, control.epoch)
        .unwrap();
    assert!(drained.is_empty());
}

#[test]
fn journal_fault_blocks_ingress_acceptance() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 60_000);
    rig.fault.store(true, std::sync::atomic::Ordering::SeqCst);
    let request = rig.request(2_000_000);
    assert_eq!(
        rig.host.ingress_submit(&grant, request),
        Err(HostError::PersistenceUnavailable)
    );
}

#[test]
fn pending_ingress_cannot_outlive_original_grant_on_reattach() {
    let root = TestRoot::new();
    let mut rig = open(&root);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 60_000);
    let request = rig.request(2_000_000);
    rig.host.ingress_submit(&grant, request).unwrap();
    rig.clock.advance(60_000);
    let replacement = rig.control();
    assert_eq!(rig.host.check_grant(&grant), Err(HostError::Unauthorized));
    assert!(rig.host.ingress_drain(&rig.creds().0, replacement.epoch).unwrap().is_empty());
    // A later wall-clock rollback cannot renew the original grant.
    rig.clock.set(1_000_000);
    assert_eq!(rig.host.check_grant(&grant), Err(HostError::Unauthorized));
}

// -- resolved-metadata retention ----------------------------------------------

#[test]
fn terminal_ingress_records_reclaim_past_their_horizon() {
    let root = TestRoot::new();
    let budgets = Budgets {
        retry_window_ms: 1_000,
        ..Default::default()
    };
    let mut rig = open_with(&root, budgets);
    let control = rig.control();
    let session = rig.admit(&control);
    // The grant outlives every horizon in this test.
    let grant = rig.grant_for(&control, session, 5_000_000);

    // One resolved request, one that expires undispatched, and one that
    // becomes uncertain — only the uncertain record is unreclaimable.
    let completed = rig.request(2_000_000);
    rig.host.ingress_submit(&grant, completed.clone()).unwrap();
    assert_eq!(
        rig.host
            .ingress_drain(&rig.creds().0, control.epoch)
            .unwrap()
            .len(),
        1
    );
    rig.host
        .ingress_resolve(
            &rig.creds().0,
            control.epoch,
            completed.request_id,
            ForwardedOutcome::Completed {
                result: json!({ "ok": true }),
            },
        )
        .unwrap();
    let forwarded = rig.request(1_200_000);
    rig.host.ingress_submit(&grant, forwarded.clone()).unwrap();
    assert_eq!(
        rig.host
            .ingress_drain(&rig.creds().0, control.epoch)
            .unwrap()
            .len(),
        1
    );
    let undispatched = rig.request(1_200_000);
    rig.host.ingress_submit(&grant, undispatched.clone()).unwrap();
    assert_eq!(rig.host.status(&rig.creds().0).unwrap().ingress_records, 3);

    // Past its deadline but inside the query horizon the never-dispatched
    // request reports its recorded `Expired` truth.
    rig.clock.set(1_200_500);
    rig.host.pump();
    assert_eq!(
        rig.host
            .ingress_query(&grant, undispatched.request_id)
            .unwrap()
            .disposition,
        IngressDisposition::Expired
    );

    // Past each record's deadline + retry horizon the terminal truth is
    // reclaimed; the uncertain record stays for reconciliation.
    rig.clock.set(2_002_000);
    rig.host.pump();
    assert_eq!(rig.host.status(&rig.creds().0).unwrap().ingress_records, 1);
    assert!(matches!(
        rig.host
            .ingress_query(&grant, forwarded.request_id)
            .unwrap()
            .disposition,
        IngressDisposition::Uncertain { .. }
    ));
    assert_eq!(
        rig.host.ingress_query(&grant, completed.request_id),
        Err(HostError::Unauthorized)
    );

    // A re-submitted identity can only re-record as already-Expired — its
    // original deadline travels with it, so it never re-dispatches.
    let again = rig.host.ingress_submit(&grant, completed.clone()).unwrap();
    assert_eq!(again.disposition, IngressDisposition::Expired);
    assert!(!again.forwarded);
    assert!(
        rig.host
            .ingress_drain(&rig.creds().0, control.epoch)
            .unwrap()
            .is_empty()
    );

    // Crash-recovery reload: folded records are swept by the same policy —
    // uncertain evidence survives, reclaimed records stay gone.
    drop(rig.host);
    let mut rig = open_with(
        &root,
        Budgets {
            retry_window_ms: 1_000,
            ..Default::default()
        },
    );
    rig.clock.set(2_002_000);
    let control = rig.control();
    rig.host.reconcile(&rig.creds().0, control.epoch).unwrap();
    rig.host.pump();
    assert!(matches!(
        rig.host
            .ingress_query(&grant, forwarded.request_id)
            .unwrap()
            .disposition,
        IngressDisposition::Uncertain { .. }
    ));
    assert_eq!(
        rig.host.ingress_query(&grant, completed.request_id),
        Err(HostError::Unauthorized)
    );
}

#[test]
fn ingress_record_limit_refuses_new_and_preserves_evidence() {
    let root = TestRoot::new();
    let budgets = Budgets {
        retry_window_ms: 1_000,
        ingress_record_limit: 3,
        ..Default::default()
    };
    let mut rig = open_with(&root, budgets);
    let control = rig.control();
    let session = rig.admit(&control);
    let grant = rig.grant_for(&control, session, 5_000_000);

    // One request the daemon will leave uncertain, then two more pending —
    // the generation's retention bound is full.
    let held = rig.request(2_000_000);
    rig.host.ingress_submit(&grant, held.clone()).unwrap();
    assert_eq!(
        rig.host
            .ingress_drain(&rig.creds().0, control.epoch)
            .unwrap()
            .len(),
        1
    );
    let a = rig.request(2_000_000);
    rig.host.ingress_submit(&grant, a.clone()).unwrap();
    rig.host
        .ingress_submit(&grant, rig.request(2_000_000))
        .unwrap();
    assert!(matches!(
        rig.host.ingress_submit(&grant, rig.request(2_000_000)),
        Err(HostError::Capacity(_))
    ));
    // Dedup still answers recorded truth under the full bound.
    let again = rig.host.ingress_submit(&grant, a).unwrap();
    assert_eq!(again.disposition, IngressDisposition::Pending);

    // Past deadline + horizon: never-dispatched records expire then
    // reclaim; the forwarded one is unresolved evidence and stays.
    rig.clock.set(2_002_000);
    rig.host.pump();
    assert_eq!(rig.host.status(&rig.creds().0).unwrap().ingress_records, 1);
    assert!(matches!(
        rig.host
            .ingress_query(&grant, held.request_id)
            .unwrap()
            .disposition,
        IngressDisposition::Uncertain { .. }
    ));
    // With room again, new submissions are accepted — and arrive honestly
    // expired against their original deadlines.
    let late = rig
        .host
        .ingress_submit(&grant, rig.request(2_000_000))
        .unwrap();
    assert_eq!(late.disposition, IngressDisposition::Expired);
}
