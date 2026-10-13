use super::*;
use crate::OfflineMatrix;
use crate::fake::{FakeFactory, ManualClock, admit, submit};
use serde_json::json;
struct TestRoot(std::path::PathBuf);
impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("runtime-qualification-{}", Uuid::new_v4())))
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
}
fn open(root: &TestRoot, budgets: Budgets) -> Rig {
    let factory = FakeFactory::new();
    let host = RuntimeHost::open(
        &root.0,
        HostOptions {
            namespace: "test".into(),
            budgets,
            journal_fault: None,
        },
        Arc::new(factory.clone()),
        Box::new(ManualClock::new(1000000)),
    )
    .unwrap();
    Rig { host, factory }
}
impl Rig {
    fn creds(&self) -> (String, String) {
        (
            self.host.credentials().controller.clone(),
            self.host.credentials().emergency.clone(),
        )
    }
    fn control(&mut self) -> ControlGrant {
        self.host
            .acquire_control(&self.creds().0, &ClientHello::current())
            .unwrap()
    }
    fn env(&self, g: &ControlGrant, id: Uuid, action: CanonicalAction) -> ActionEnvelope {
        ActionEnvelope {
            namespace: "test".into(),
            runtime_instance: self.host.instance(),
            epoch: g.epoch,
            retry_window_id: g.retry_window_id,
            command_id: id,
            session: None,
            action,
        }
    }
    fn session_env(
        &self,
        g: &ControlGrant,
        id: Uuid,
        session: Uuid,
        action: CanonicalAction,
    ) -> ActionEnvelope {
        let mut env = self.env(g, id, action);
        env.session = Some(SessionRef {
            session_id: session,
            generation: 1,
        });
        env
    }
    fn admit(&mut self, g: &ControlGrant) -> (Uuid, CommandReceipt) {
        let session = Uuid::new_v4();
        let receipt = self
            .host
            .execute(&self.creds().0, self.env(g, Uuid::new_v4(), admit(session)))
            .unwrap();
        (session, receipt)
    }
    fn cursor(&self, session: Uuid, applied: u64) -> EventCursor {
        EventCursor {
            session_id: session,
            generation: 1,
            applied,
        }
    }
    fn events(&mut self, g: &ControlGrant, session: Uuid, applied: u64) -> EventPage {
        self.host
            .events(&self.creds().0, g.epoch, self.cursor(session, applied))
            .unwrap()
    }
}

#[test]
fn failed_intent_records_never_contact_an_adapter() {
    use crate::journal::JournalRecord;
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    rig.host.journal.fail_on = Some(|record| matches!(record, JournalRecord::Admitted { .. }));
    let env = rig.env(&grant, Uuid::new_v4(), admit(Uuid::new_v4()));
    let receipt = rig.host.execute(&rig.creds().0, env).unwrap();
    assert!(matches!(
        receipt.disposition,
        CommandDisposition::Rejected { .. }
    ));
    assert!(rig.factory.adapters().is_empty());
    rig.host.journal.fail_on = None;
    let (session, _) = rig.admit(&grant);
    let fake = rig.factory.for_session(session).unwrap();
    rig.host.journal.fail_on = Some(|record| {
        matches!(
            record,
            JournalRecord::Disposition {
                disposition: CommandDisposition::DispatchAttempted,
                ..
            }
        )
    });
    let env = rig.session_env(
        &grant,
        Uuid::new_v4(),
        session,
        submit(Uuid::new_v4(), Uuid::new_v4(), "never send"),
    );
    let receipt = rig.host.execute(&rig.creds().0, env.clone()).unwrap();
    assert!(matches!(
        receipt.disposition,
        CommandDisposition::Uncertain { .. }
    ));
    assert_eq!(fake.dispatch_count(), 0);
    rig.host.journal.fail_on = None;
    let _ = rig.host.execute(&rig.creds().0, env).unwrap();
    assert_eq!(fake.dispatch_count(), 0);
}

struct ClosingFactory;
struct ClosingHandle {
    sink: std::sync::mpsc::SyncSender<ProviderEvent>,
    alive: bool,
}
impl AdapterFactory for ClosingFactory {
    fn spawn(
        &self,
        ctx: crate::SessionContext,
    ) -> Result<Box<dyn crate::AdapterHandle>, crate::SpawnError> {
        ctx.sink.send(ProviderEvent::SessionLive).unwrap();
        Ok(Box::new(ClosingHandle {
            sink: ctx.sink,
            alive: true,
        }))
    }
}
impl crate::AdapterHandle for ClosingHandle {
    fn dispatch(&mut self, action: &CanonicalAction) -> DispatchOutcome {
        if let CanonicalAction::ResolveRequest { request_id, .. } = action {
            self.sink
                .send(ProviderEvent::RequestAnswered {
                    request_id: request_id.clone(),
                })
                .unwrap();
            return DispatchOutcome::Unknown {
                detail: "acknowledgement lost".into(),
            };
        }
        DispatchOutcome::Accepted
    }
    fn cancel(&mut self, _: Uuid) -> crate::CancelOutcome {
        crate::CancelOutcome::NotRunning
    }
    fn terminate(&mut self) -> Termination {
        self.alive = false;
        Termination::Confirmed
    }
    fn alive(&self) -> bool {
        self.alive
    }
    fn offline_matrix(&self) -> OfflineMatrix {
        OfflineMatrix::default()
    }
}

#[test]
fn confirmed_stop_and_pipe_closure_have_one_terminal_truth() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    rig.host.factory = Arc::new(ClosingFactory);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    let env = rig.session_env(
        &grant,
        Uuid::new_v4(),
        session,
        CanonicalAction::StopSession,
    );
    rig.host.execute(&rig.creds().0, env).unwrap();
    rig.host.pump();
    rig.host.pump();
    let inventory = rig.host.reconcile(&rig.creds().0, grant.epoch).unwrap();
    assert_eq!(inventory.sessions[0].status, SessionStatus::Terminated);
    let events = rig.events(&grant, session, 0).events;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::Terminated { .. }))
            .count(),
        1
    );
    assert!(matches!(
        events.last().unwrap().event,
        ProviderEvent::Terminated {
            reason: TerminateReason::Requested
        }
    ));
}

#[test]
fn synchronous_answer_consumption_survives_post_dispatch_bookkeeping() {
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    rig.host.factory = Arc::new(ClosingFactory);
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    rig.host.ingest(
        session,
        ProviderEvent::RequestOpened {
            request_id: "original".into(),
            class: RequestClass::Tool,
            subject: json!({}),
            deadline_ms: None,
        },
    );
    let env = rig.session_env(
        &grant,
        Uuid::new_v4(),
        session,
        CanonicalAction::ResolveRequest {
            request_id: "original".into(),
            resolution: json!({"success":true}),
        },
    );
    let receipt = rig.host.execute(&rig.creds().0, env).unwrap();
    assert!(matches!(
        receipt.disposition,
        CommandDisposition::Terminal { .. }
    ));
    let pending = &rig.host.sessions[&session].current().pending["original"];
    assert_eq!(pending.answer, AnswerDisposition::Accepted);
    assert!(pending.consumed);
}

#[test]
fn failed_acknowledgement_does_not_reclaim_replay_history() {
    use crate::journal::JournalRecord;
    let root = TestRoot::new();
    let mut rig = open(&root, Budgets::default());
    let grant = rig.control();
    let (session, _) = rig.admit(&grant);
    rig.factory.for_session(session).unwrap().emit_live();
    rig.host.pump();
    rig.host.journal.fail_on = Some(|record| matches!(record, JournalRecord::Acknowledged { .. }));
    assert_eq!(
        rig.host
            .acknowledge(&rig.creds().0, grant.epoch, rig.cursor(session, 1)),
        Err(HostError::PersistenceUnavailable)
    );
    assert_eq!(rig.host.sessions[&session].current().acknowledged, 0);
    rig.host.journal.fail_on = None;
    rig.host
        .acquire_control(&rig.creds().0, &ClientHello::current())
        .unwrap();
    assert_eq!(rig.host.sessions[&session].current().events.len(), 1);
}

#[test]
fn unqualified_crash_cleanup_blocks_admission_and_drain() {
    let root = TestRoot::new();
    let session;
    {
        let mut rig = open(&root, Budgets::default());
        rig.host.factory = Arc::new(ClosingFactory);
        let grant = rig.control();
        session = rig.admit(&grant).0;
    }
    let mut host = RuntimeHost::open(
        &root.0,
        HostOptions::new("test"),
        Arc::new(ClosingFactory),
        Box::new(ManualClock::new(1000000)),
    )
    .unwrap();
    let token = host.credentials().controller.clone();
    let grant = host
        .acquire_control(&token, &ClientHello::current())
        .unwrap();
    assert_eq!(
        host.reconcile(&token, grant.epoch),
        Err(HostError::NotReconciled)
    );
    assert_eq!(
        host.status(&token).unwrap().unfenced_survivors[0].session_id,
        session
    );
    assert!(matches!(
        host.drain(&token, grant.epoch, DrainPolicy::Forced)
            .unwrap(),
        DrainReport::Deferred { .. }
    ));
}
