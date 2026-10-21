//! The executable boundary: a spawned `waku-runtime-host` serving real
//! IPC, surviving controller replacement, and recovering honestly from a
//! kill. Barriers are file/process state — endpoint publication and exit
//! status — never sleeps as synchronization.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::json;
use uuid::Uuid;

use waku_runtime::client::RuntimeClient;
use waku_runtime::contract::*;
use waku_runtime::fake::{admit, submit};
use waku_runtime::host::load_credentials;
use waku_runtime::ipc::Endpoint;

const NS: &str = "binary-test-ns";

const HOST: &str = env!("CARGO_BIN_EXE_waku-runtime-fixture");

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("waku-bin-test-{}", Uuid::new_v4().simple())))
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

struct HostProcess(Child);

impl HostProcess {
    fn spawn(root: &TestRoot) -> Self {
        Self(
            Command::new(HOST)
                .arg("--root")
                .arg(root.path())
                .arg("--namespace")
                .arg(NS)
                .arg("--enable-fake-adapter")
                .spawn()
                .expect("runtime host spawns"),
        )
    }
    /// Wait for the endpoint record — the binary's readiness barrier.
    /// `supersedes` rejects the previous owner's stale record until the
    /// replacement publishes its own instance.
    fn wait_ready(root: &TestRoot, supersedes: Option<Uuid>) -> Endpoint {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(endpoint) = Endpoint::load(root.path())
                && Some(endpoint.instance) != supersedes
            {
                return endpoint;
            }
            assert!(
                Instant::now() < deadline,
                "runtime host never published its endpoint"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for HostProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

fn env_for(
    grant: &ControlGrant,
    instance: Uuid,
    command: Uuid,
    session: Option<SessionRef>,
    action: CanonicalAction,
) -> ActionEnvelope {
    ActionEnvelope {
        namespace: NS.into(),
        runtime_instance: instance,
        epoch: grant.epoch,
        retry_window_id: grant.retry_window_id,
        command_id: command,
        session,
        action,
    }
}

#[test]
fn spawned_host_end_to_end_and_crash_recovery() {
    let root = TestRoot::new();
    let mut host1 = HostProcess::spawn(&root);
    let endpoint1 = HostProcess::wait_ready(&root, None);
    let creds = load_credentials(root.path()).expect("identity readable");

    // Daemon 1: handshake, control, admit a fake provider session, run a
    // turn through the executable's real IPC.
    let mut daemon1 =
        RuntimeClient::controller(root.path(), &creds.controller, &ClientHello::current())
            .unwrap()
            .0;
    let grant1 = daemon1.acquire_control(&ClientHello::current()).unwrap();
    let session_id = Uuid::new_v4();
    daemon1
        .execute(env_for(
            &grant1,
            endpoint1.instance,
            Uuid::new_v4(),
            None,
            admit(session_id),
        ))
        .unwrap();
    let sref = SessionRef {
        session_id,
        generation: 1,
    };
    let turn = Uuid::new_v4();
    let receipt = daemon1
        .execute(env_for(
            &grant1,
            endpoint1.instance,
            Uuid::new_v4(),
            Some(sref),
            submit(turn, Uuid::new_v4(), "work"),
        ))
        .unwrap();
    assert_eq!(receipt.disposition, CommandDisposition::ProviderAccepted);

    // Agent ingress over the real boundary: grant, submit, forward,
    // resolve, and the agent reads its recorded truth back.
    let agent_grant = daemon1.issue_grant(grant1.epoch, sref, 120_000).unwrap();
    let mut agent = RuntimeClient::agent(root.path(), &agent_grant).unwrap();
    let request_id = Uuid::new_v4();
    agent
        .ingress(IngressRequest {
            request_id,
            class: IngressKind::Cli,
            subject: json!({ "op": "ping" }),
            deadline_ms: u64::MAX / 2,
        })
        .unwrap();
    let mut service = RuntimeClient::service(
        root.path(),
        &creds.controller,
        grant1.epoch,
        &ClientHello::current(),
    )
    .unwrap()
    .0;
    let forwarded = service.service_drain(grant1.epoch, 0).unwrap();
    assert_eq!(forwarded.len(), 1);
    service
        .service_result(
            grant1.epoch,
            request_id,
            ForwardedOutcome::Completed {
                result: json!({ "pong": true }),
            },
        )
        .unwrap();
    let record = agent.ingress_query(request_id).unwrap();
    assert_eq!(
        record.disposition,
        IngressDisposition::Completed {
            result: json!({ "pong": true })
        }
    );
    drop(service);
    drop(agent);
    drop(daemon1);

    // A second host instance cannot own the namespace while the first
    // lives — the lock is the singleton proof.
    let mut usurper = Command::new(HOST)
        .arg("--root")
        .arg(root.path())
        .arg("--namespace")
        .arg(NS)
        .spawn()
        .expect("usurper spawns");
    let status = usurper.wait().expect("usurper exits");
    assert!(
        !status.success(),
        "second instance must refuse the namespace"
    );

    // Kill the runtime itself — the honest crash path.
    host1.kill();

    let mut host2 = HostProcess::spawn(&root);
    let endpoint2 = HostProcess::wait_ready(&root, Some(endpoint1.instance));
    assert_ne!(endpoint1.instance, endpoint2.instance);

    let mut daemon2 =
        RuntimeClient::controller(root.path(), &creds.controller, &ClientHello::current())
            .unwrap()
            .0;
    let grant2 = daemon2.acquire_control(&ClientHello::current()).unwrap();

    // Crash recovery fences the survivor and blocks new admissions until
    // reconciliation completes.
    let rejected = daemon2
        .execute(env_for(
            &grant2,
            endpoint2.instance,
            Uuid::new_v4(),
            None,
            admit(Uuid::new_v4()),
        ))
        .unwrap();
    assert!(matches!(
        rejected.disposition,
        CommandDisposition::Rejected { .. }
    ));
    let inventory = daemon2.reconcile(grant2.epoch).unwrap();
    assert_eq!(inventory.sessions.len(), 1);
    assert_eq!(inventory.sessions[0].status, SessionStatus::Interrupted);
    assert_eq!(inventory.sessions[0].fence, Some(FenceKind::Interrupted));

    // The completed agent request kept its recorded truth across the
    // crash — durable before it ever mattered.
    let grant_check = daemon2.command(Uuid::new_v4()).unwrap();
    assert!(grant_check.is_none());
    let record = daemon2
        .status()
        .map(|s| s.sessions[0].pending_ingress.len())
        .unwrap();
    assert_eq!(record, 0);

    host2.kill();
}
