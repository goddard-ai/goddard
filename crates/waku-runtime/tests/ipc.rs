//! Stage-2C transport checks over real Unix sockets: roles, fencing,
//! attach/detach replay, agent ingress, and the service bridge — all
//! through the private IPC a daemon and agent would actually use.
//! Determinism comes from `wait_ms: 0` polls and scripted fakes, not
//! sleeps.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use uuid::Uuid;

use waku_runtime::adapter::AdapterFactory;
use waku_runtime::client::{ClientError, RuntimeClient};
use waku_runtime::contract::*;
use waku_runtime::fake::{FakeFactory, admit, submit};
use waku_runtime::host::{Budgets, HostOptions, RuntimeHost, SystemClock};
use waku_runtime::registry::AdapterRegistry;
use waku_runtime::server::RuntimeServer;
use waku_runtime::wire::{ConnectAuth, WireResponse};

const NS: &str = "ipc-test-ns";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("waku-ipc-test-{}", Uuid::new_v4().simple())))
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
    root: TestRoot,
    factory: FakeFactory,
    controller: String,
    emergency: String,
    _server: RuntimeServer,
}

fn serve() -> Rig {
    let root = TestRoot::new();
    let factory = FakeFactory::new();
    let mut registry = AdapterRegistry::new();
    registry.register(
        "fake",
        Arc::new(factory.clone()) as Arc<dyn AdapterFactory>,
    );
    let host = RuntimeHost::open(
        root.path(),
        HostOptions {
            namespace: NS.into(),
            budgets: Budgets::default(),
            journal_fault: None,
        },
        Arc::new(registry),
        Box::new(SystemClock),
    )
    .expect("host opens");
    let controller = host.credentials().controller.clone();
    let emergency = host.credentials().emergency.clone();
    let server = RuntimeServer::bind(host, root.path(), Duration::from_millis(5))
        .expect("server binds");
    Rig {
        root,
        factory,
        controller,
        emergency,
        _server: server,
    }
}

impl Rig {
    fn controller(&self) -> RuntimeClient {
        let (client, _) = RuntimeClient::controller(
            self.root.path(),
            &self.controller,
            &ClientHello::current(),
        )
        .expect("controller connects");
        client
    }
    fn env(&self, grant: &ControlGrant, command: Uuid, action: CanonicalAction) -> ActionEnvelope {
        // The client learns the instance from the hello — tests mint
        // envelopes for the live instance via `reconcile`.
        ActionEnvelope {
            namespace: NS.into(),
            runtime_instance: self.instance(),
            epoch: grant.epoch,
            retry_window_id: grant.retry_window_id,
            command_id: command,
            session: None,
            action,
        }
    }
    fn instance(&self) -> Uuid {
        let mut client = RuntimeClient::emergency(self.root.path(), &self.emergency)
            .expect("emergency connects")
            .0;
        client.status().unwrap().runtime_instance
    }
    fn admit(&self, client: &mut RuntimeClient, grant: &ControlGrant) -> Uuid {
        let session_id = Uuid::new_v4();
        let receipt = client
            .execute(self.env(grant, Uuid::new_v4(), admit(session_id)))
            .expect("admission accepted");
        assert!(matches!(
            receipt.disposition,
            CommandDisposition::Terminal { .. }
        ));
        session_id
    }
    fn sref(&self, session_id: Uuid) -> SessionRef {
        SessionRef {
            session_id,
            generation: 1,
        }
    }
    fn cursor(&self, session_id: Uuid, applied: u64) -> EventCursor {
        EventCursor {
            session_id,
            generation: 1,
            applied,
        }
    }
}

// -- handshake and auth ------------------------------------------------------

#[test]
fn wrong_credential_and_foreign_namespace_are_refused() {
    let rig = serve();
    // Bad token: handshake fails before any request exists.
    let result = RuntimeClient::controller(
        rig.root.path(),
        "wrong-credential",
        &ClientHello::current(),
    );
    assert!(matches!(
        result,
        Err(ClientError::Host(HostError::Unauthorized))
    ));
    // Wrong namespace on the connect frame itself.
    let mut stream = waku_runtime::ipc::connect(rig.root.path()).unwrap();
    waku_runtime::ipc::send(
        &mut stream,
        &waku_runtime::wire::Connect {
            namespace: "foreign".into(),
            auth: ConnectAuth::Controller {
                credential: rig.controller.clone(),
                client: ClientHello::current(),
            },
        },
    )
    .unwrap();
    let response: Option<WireResponse> = waku_runtime::ipc::recv(&mut stream).unwrap();
    assert_eq!(
        response,
        Some(WireResponse::Error {
            error: HostError::Unauthorized
        })
    );
}

#[test]
fn malformed_frames_close_without_harming_the_server() {
    use std::io::Write;
    let rig = serve();
    let mut stream = waku_runtime::ipc::connect(rig.root.path()).unwrap();
    // Garbage where a frame belongs: the server drops the connection.
    stream.write_all(b"not-a-frame-at-all").unwrap();
    let response: Option<WireResponse> = waku_runtime::ipc::recv(&mut stream).unwrap();
    assert!(response.is_none());
    // The server is untouched: a clean client still connects and works.
    let mut client = rig.controller();
    let status = client.status().unwrap();
    assert_eq!(status.epoch, 0);
}

// -- epochs and attach --------------------------------------------------------

#[test]
fn stale_epoch_is_fenced_over_ipc() {
    let rig = serve();
    let mut daemon1 = rig.controller();
    let grant1 = daemon1.acquire_control(&ClientHello::current()).unwrap();
    let mut daemon2 = rig.controller();
    let grant2 = daemon2.acquire_control(&ClientHello::current()).unwrap();
    assert_ne!(grant1.epoch, grant2.epoch);

    // The old controller cannot record new work.
    let session_id = Uuid::new_v4();
    let receipt = daemon1.execute(rig.env(&grant1, Uuid::new_v4(), admit(session_id)));
    assert!(matches!(
        receipt,
        Err(ClientError::Host(HostError::StaleEpoch { .. }))
    ));
    // The new controller admits the same session id it never saw before —
    // its command id is what dedups, and this is fresh work.
    let receipt = daemon2
        .execute(rig.env(&grant2, Uuid::new_v4(), admit(session_id)))
        .unwrap();
    assert!(matches!(
        receipt.disposition,
        CommandDisposition::Terminal { .. }
    ));
}

#[test]
fn detach_reattach_replays_each_event_once() {
    let rig = serve();
    let mut daemon1 = rig.controller();
    let grant1 = daemon1.acquire_control(&ClientHello::current()).unwrap();
    let session = rig.admit(&mut daemon1, &grant1);
    let fake = rig.factory.for_session(session).unwrap();
    fake.emit_live();
    let turn = Uuid::new_v4();
    daemon1
        .execute(
            ActionEnvelope {
                session: Some(rig.sref(session)),
                ..rig.env(&grant1, Uuid::new_v4(), submit(turn, Uuid::new_v4(), "go"))
            },
        )
        .unwrap();
    fake.emit(ProviderEvent::TurnStarted {
        turn_id: turn,
        delivery_id: Uuid::new_v4(),
    });
    fake.emit_delta(turn, "one");

    // Daemon 1 applies through sequence 3 and detaches (drops its client).
    let page = daemon1
        .events(grant1.epoch, rig.cursor(session, 0), 0)
        .unwrap();
    assert_eq!(page.events.len(), 3);
    daemon1
        .acknowledge(grant1.epoch, rig.cursor(session, 3))
        .unwrap();
    drop(daemon1);
    fake.emit_delta(turn, "two");
    fake.emit_completed(turn);

    // Daemon 2 attaches: new epoch, reconcile, replay from the durable
    // cursor — same turn, in order, nothing duplicated.
    let mut daemon2 = rig.controller();
    let grant2 = daemon2.acquire_control(&ClientHello::current()).unwrap();
    let inventory = daemon2.reconcile(grant2.epoch).unwrap();
    assert_eq!(inventory.sessions.len(), 1);
    assert_eq!(inventory.sessions[0].status, SessionStatus::Live);
    let page = daemon2
        .events(grant2.epoch, rig.cursor(session, 3), 0)
        .unwrap();
    let seqs: Vec<u64> = page.events.iter().map(|e| e.sequence).collect();
    assert_eq!(seqs, vec![4, 5]);
    assert!(matches!(
        page.events.last().unwrap().event,
        ProviderEvent::TurnCompleted { .. }
    ));
}

// -- service and agent ingress --------------------------------------------------

#[test]
fn ingress_round_trip_agent_to_daemon_service() {
    let rig = serve();
    let mut daemon = rig.controller();
    let grant = daemon.acquire_control(&ClientHello::current()).unwrap();
    let session = rig.admit(&mut daemon, &grant);
    let agent_grant = daemon
        .issue_grant(grant.epoch, rig.sref(session), 120_000)
        .unwrap();

    // The agent submits under its grant.
    let mut agent = RuntimeClient::agent(rig.root.path(), &agent_grant).unwrap();
    let request_id = Uuid::new_v4();
    let record = agent
        .ingress(IngressRequest {
            request_id,
            class: IngressKind::Cli,
            subject: json!({ "command": "boss status" }),
            deadline_ms: u64::MAX / 2,
        })
        .unwrap();
    assert_eq!(record.disposition, IngressDisposition::Pending);

    // The daemon's service drains, executes, and reports the outcome.
    let mut service =
        RuntimeClient::service(rig.root.path(), &rig.controller, grant.epoch, &ClientHello::current())
            .unwrap()
            .0;
    let forwarded = service.service_drain(grant.epoch, 0).unwrap();
    assert_eq!(forwarded.len(), 1);
    assert_eq!(forwarded[0].request_id, request_id);
    service
        .service_result(
            grant.epoch,
            request_id,
            ForwardedOutcome::Completed {
                result: json!({ "answer": 42 }),
            },
        )
        .unwrap();

    // The agent's retry queries the same identity's truth.
    let record = agent.ingress_query(request_id).unwrap();
    assert_eq!(
        record.disposition,
        IngressDisposition::Completed {
            result: json!({ "answer": 42 })
        }
    );
    // Resubmitting the same request returns the recorded truth — it is
    // never executed twice.
    let again = agent
        .ingress(IngressRequest {
            request_id,
            class: IngressKind::Cli,
            subject: json!({ "command": "boss status" }),
            deadline_ms: u64::MAX / 2,
        })
        .unwrap();
    assert_eq!(again.disposition, record.disposition);
}

#[test]
fn service_binds_the_epoch_it_serves() {
    let rig = serve();
    let mut daemon = rig.controller();
    let grant1 = daemon.acquire_control(&ClientHello::current()).unwrap();
    let mut old_service = RuntimeClient::service(rig.root.path(), &rig.controller, grant1.epoch, &ClientHello::current()).unwrap().0;
    // Epoch 2 takes over — epoch 1's service can no longer connect.
    let grant2 = daemon.acquire_control(&ClientHello::current()).unwrap();
    assert!(matches!(old_service.service_drain(grant2.epoch, 0), Err(ClientError::Host(HostError::StaleEpoch { .. }))));
    let stale = RuntimeClient::service(
        rig.root.path(),
        &rig.controller,
        grant1.epoch,
        &ClientHello::current(),
    );
    assert!(matches!(
        stale,
        Err(ClientError::Host(HostError::Unauthorized))
    ));
    let live = RuntimeClient::service(
        rig.root.path(),
        &rig.controller,
        grant2.epoch,
        &ClientHello::current(),
    );
    assert!(live.is_ok());
}

#[test]
fn agent_cannot_use_another_sessions_grant_or_forged_token() {
    let rig = serve();
    let mut daemon = rig.controller();
    let grant = daemon.acquire_control(&ClientHello::current()).unwrap();
    let session_a = rig.admit(&mut daemon, &grant);
    let session_b = rig.admit(&mut daemon, &grant);
    let grant_a = daemon
        .issue_grant(grant.epoch, rig.sref(session_a), 120_000)
        .unwrap();
    let grant_b = daemon
        .issue_grant(grant.epoch, rig.sref(session_b), 120_000)
        .unwrap();

    // Agent A's grant cannot read agent B's traffic.
    let mut agent_b = RuntimeClient::agent(rig.root.path(), &grant_b).unwrap();
    let req_b = agent_b
        .ingress(IngressRequest {
            request_id: Uuid::new_v4(),
            class: IngressKind::Cli,
            subject: json!({ "op": "private" }),
            deadline_ms: u64::MAX / 2,
        })
        .unwrap();
    let mut agent_a = RuntimeClient::agent(rig.root.path(), &grant_a).unwrap();
    assert!(matches!(
        agent_a.ingress_query(req_b.request_id),
        Err(ClientError::Host(HostError::Unauthorized))
    ));

    // A forged token never connects.
    let mut forged = grant_a.clone();
    forged.token = "nope".into();
    assert!(matches!(
        RuntimeClient::agent(rig.root.path(), &forged),
        Err(ClientError::Host(HostError::Unauthorized))
    ));
}

#[test]
fn emergency_authority_is_narrow_and_distinct() {
    let rig = serve();
    let mut daemon = rig.controller();
    let grant = daemon.acquire_control(&ClientHello::current()).unwrap();
    let session = rig.admit(&mut daemon, &grant);

    // Emergency can read status and stop a session — while the daemon is
    // alive or not.
    let mut local = RuntimeClient::emergency(rig.root.path(), &rig.emergency)
        .unwrap()
        .0;
    let status = local.emergency_stop(session).unwrap();
    assert_eq!(status, SessionStatus::Terminated);
    // Emergency authority cannot admit, submit, or reconcile.
    let refused = local.execute(rig.env(&grant, Uuid::new_v4(), admit(Uuid::new_v4())));
    assert!(matches!(
        refused,
        Err(ClientError::Host(HostError::Unauthorized))
    ));
}

#[test]
fn claims_and_pending_ingress_reconcile_over_ipc() {
    let rig = serve();
    let mut daemon1 = rig.controller();
    let grant1 = daemon1.acquire_control(&ClientHello::current()).unwrap();
    let session = rig.admit(&mut daemon1, &grant1);
    let agent_grant = daemon1
        .issue_grant(grant1.epoch, rig.sref(session), 120_000)
        .unwrap();
    let claim = ResourceClaim {
        claim_id: Uuid::new_v4(),
        session: rig.sref(session),
        resources: vec!["native_builds:1".into()],
        detail: json!({ "purpose": "surviving cli worker" }),
    };
    daemon1
        .register_claim(grant1.epoch, claim.clone())
        .unwrap();
    let mut agent = RuntimeClient::agent(rig.root.path(), &agent_grant).unwrap();
    agent
        .ingress(IngressRequest {
            request_id: Uuid::new_v4(),
            class: IngressKind::Mcp,
            subject: json!({ "tool": "workspace.read" }),
            deadline_ms: u64::MAX / 2,
        })
        .unwrap();
    drop(daemon1);

    // The replacement daemon reconciles before admitting: sessions,
    // claims, and pending ingress are all in the inventory.
    let mut daemon2 = rig.controller();
    let grant2 = daemon2.acquire_control(&ClientHello::current()).unwrap();
    let inventory = daemon2.reconcile(grant2.epoch).unwrap();
    let inv = &inventory.sessions[0];
    assert_eq!(inv.claims, vec![claim]);
    assert_eq!(inv.pending_ingress.len(), 1);
}

#[test]
fn large_detached_history_pages_without_skipping_or_acknowledging_unseen_events() {
    let rig = serve();
    let mut daemon = rig.controller();
    let grant = daemon.acquire_control(&ClientHello::current()).unwrap();
    let session = rig.admit(&mut daemon, &grant);
    let fake = rig.factory.for_session(session).unwrap();
    fake.emit_live();
    let turn = Uuid::new_v4();
    for _ in 0..13 { fake.emit_delta(turn, &"x".repeat(750_000)); }
    let first = daemon.events(grant.epoch, rig.cursor(session, 0), 0).unwrap();
    assert!(first.events.len() < 14);
    let applied = first.events.last().unwrap().sequence;
    assert!(applied < first.high_water);
    daemon.acknowledge(grant.epoch, rig.cursor(session, applied)).unwrap();
    let next = daemon.events(grant.epoch, rig.cursor(session, applied), 0).unwrap();
    assert_eq!(next.events.first().unwrap().sequence, applied + 1);
    assert_eq!(next.events.last().unwrap().sequence, first.high_water);
    assert_eq!(first.events.len() + next.events.len(), 14);
}
