#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use uuid::Uuid;
use waku_runtime::client::RuntimeClient;
use waku_runtime::contract::*;
use waku_runtime::fake::{FakeFactory, admit};
use waku_runtime::host::{Budgets, HostOptions, RuntimeHost, SystemClock};
use waku_runtime::registry::AdapterRegistry;
use waku_runtime::server::RuntimeServer;

struct Rig {
    root: PathBuf,
    descriptor: PathBuf,
    credential: String,
    controller: RuntimeClient,
    grant: ControlGrant,
    agent_grant: SessionGrant,
    task: Uuid,
    _server: RuntimeServer,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Rig {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("agent-ingress-{}", Uuid::new_v4().simple()));
        let mut registry = AdapterRegistry::new();
        registry.register("fake", Arc::new(FakeFactory::new()));
        let host = RuntimeHost::open(
            &root,
            HostOptions {
                namespace: "isolated-cli".into(),
                budgets: Budgets::default(),
                journal_fault: None,
            },
            Arc::new(registry),
            Box::new(SystemClock),
        )
        .unwrap();
        let credential = host.credentials().controller.clone();
        let server = RuntimeServer::bind(host, &root, Duration::from_millis(5)).unwrap();
        let (mut controller, hello) =
            RuntimeClient::controller(&root, &credential, &ClientHello::current()).unwrap();
        let grant = controller.acquire_control(&ClientHello::current()).unwrap();
        let session = Uuid::new_v4();
        controller
            .execute(ActionEnvelope {
                namespace: hello.namespace,
                runtime_instance: hello.runtime_instance,
                epoch: grant.epoch,
                retry_window_id: grant.retry_window_id,
                command_id: Uuid::new_v4(),
                session: None,
                action: admit(session),
            })
            .unwrap();
        let agent_grant = controller
            .issue_grant(
                grant.epoch,
                SessionRef {
                    session_id: session,
                    generation: 1,
                },
                600_000,
            )
            .unwrap();
        let task = Uuid::new_v4();
        let descriptor = root.join("cli.json");
        std::fs::write(
            &descriptor,
            serde_json::to_vec(&IngressDescriptor {
                version: INGRESS_DESCRIPTOR_VERSION,
                root: root.clone(),
                task_id: task,
                grant: agent_grant.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(&descriptor, std::fs::Permissions::from_mode(0o600)).unwrap();
        Self {
            root,
            descriptor,
            credential,
            controller,
            grant,
            agent_grant,
            task,
            _server: server,
        }
    }

    fn cli(&self, id: Uuid, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_goddard-agent"));
        command
            .args(args)
            .env("GODDARD_RUNTIME_INGRESS", &self.descriptor)
            .env("GODDARD_RUNTIME_REQUEST_ID", id.to_string())
            .env("GODDARD_TASK_ID", self.task.to_string())
            .env_remove("GODDARD_DAEMON_ADDRESS")
            .env_remove("GODDARD_AGENT_TOKEN")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn service(&self) -> RuntimeClient {
        RuntimeClient::service(
            &self.root,
            &self.credential,
            self.grant.epoch,
            &ClientHello::current(),
        )
        .unwrap()
        .0
    }

    fn recorded(&self, id: Uuid) -> IngressRecord {
        let mut agent = RuntimeClient::agent(&self.root, &self.agent_grant).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(record) = agent.ingress_query(id) {
                return record;
            }
            assert!(
                Instant::now() < until,
                "CLI did not submit persisted ingress"
            );
            std::thread::yield_now();
        }
    }
}

#[test]
fn actual_cli_survives_controller_replacement_and_retries_original_identity() {
    let mut rig = Rig::new();
    let id = Uuid::new_v4();
    let child = rig
        .cli(id, &["models", "--output", "json"])
        .spawn()
        .unwrap();
    let original = rig.recorded(id);
    assert_eq!(original.class, IngressKind::Cli);
    assert_eq!(original.disposition, IngressDisposition::Pending);
    assert_eq!(original.subject["type"], "agentListModels");
    // Accepted while no daemon service is connected. Replace control 25 times;
    // the original immutable request is neither renewed nor redispatched.
    for _ in 0..25 {
        let (mut controller, _) =
            RuntimeClient::controller(&rig.root, &rig.credential, &ClientHello::current()).unwrap();
        rig.grant = controller.acquire_control(&ClientHello::current()).unwrap();
        rig.controller = controller;
    }
    let batch = rig.service().service_drain(rig.grant.epoch, 0).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].request_id, id);
    assert_eq!(batch[0].deadline_ms, original.deadline_ms);
    assert_eq!(batch[0].session, original.session);
    rig.service()
        .service_result(
            rig.grant.epoch,
            id,
            ForwardedOutcome::Completed {
                result: serde_json::to_value(waku_protocol::ResponsePayload::AgentModelOptions {
                    options: vec![],
                })
                .unwrap(),
            },
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        json!({"options":[]})
    );
    let persisted = std::fs::read(
        rig.descriptor
            .with_extension("requests")
            .join(format!("{id}.json")),
    )
    .unwrap();
    let replay = rig
        .cli(id, &["models", "--output", "json"])
        .output()
        .unwrap();
    assert!(replay.status.success());
    assert_eq!(replay.stdout, output.stdout);
    assert_eq!(
        persisted,
        std::fs::read(
            rig.descriptor
                .with_extension("requests")
                .join(format!("{id}.json"))
        )
        .unwrap()
    );
    assert!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .is_empty()
    );
    // A reused identity cannot become another command after CLI replacement.
    let mismatch = rig.cli(id, &["command", "list"]).output().unwrap();
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("mismatch"));
    assert!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn uncertain_effect_is_reported_without_resend_or_secret_exposure() {
    let rig = Rig::new();
    let id = Uuid::new_v4();
    let child = rig.cli(id, &["models"]).spawn().unwrap();
    rig.recorded(id);
    assert_eq!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .len(),
        1
    );
    rig.service()
        .service_result(
            rig.grant.epoch,
            id,
            ForwardedOutcome::Uncertain {
                detail: "effect may have occurred".into(),
            },
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("uncertain") && error.contains(&id.to_string()));
    assert!(!error.contains(&rig.agent_grant.token));
    assert!(!error.contains(rig.root.to_str().unwrap()));
    assert!(!rig.cli(id, &["models"]).output().unwrap().status.success());
    assert!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_or_foreign_binding_never_falls_back_to_daemon_credentials() {
    let rig = Rig::new();
    let id = Uuid::new_v4();
    let output = rig
        .cli(id, &["models"])
        .env("GODDARD_TASK_ID", Uuid::new_v4().to_string())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("binding mismatch"));
    assert!(
        std::fs::read_dir(rig.descriptor.with_extension("requests"))
            .unwrap()
            .next()
            .is_none()
    );
    std::fs::set_permissions(&rig.descriptor, std::fs::Permissions::from_mode(0o644)).unwrap();
    let output = rig
        .cli(id, &["models"])
        .env("GODDARD_DAEMON_ADDRESS", "127.0.0.1:1")
        .env("GODDARD_AGENT_TOKEN", "secret-test-token")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("private"));
    assert!(!error.contains("secret-test-token"));
}

#[test]
fn lost_cli_response_reconciles_by_persisted_identity_after_process_replacement() {
    let rig = Rig::new();
    let id = Uuid::new_v4();
    let mut child = rig.cli(id, &["models"]).spawn().unwrap();
    rig.recorded(id);
    assert_eq!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .len(),
        1
    );
    // The effect's owner survives the caller. Kill only our disposable CLI.
    child.kill().unwrap();
    child.wait().unwrap();
    rig.service()
        .service_result(
            rig.grant.epoch,
            id,
            ForwardedOutcome::Completed {
                result: serde_json::to_value(waku_protocol::ResponsePayload::AgentModelOptions {
                    options: vec![],
                })
                .unwrap(),
            },
        )
        .unwrap();
    assert!(rig.cli(id, &["models"]).output().unwrap().status.success());
    assert!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn persisted_expiry_corruption_and_retention_fail_closed() {
    let rig = Rig::new();
    let id = Uuid::new_v4();
    let records = rig.descriptor.with_extension("requests");
    std::fs::create_dir(&records).unwrap();
    std::fs::set_permissions(&records, std::fs::Permissions::from_mode(0o700)).unwrap();
    let record = records.join(format!("{id}.json"));
    std::fs::write(&record, serde_json::to_vec(&json!({
        "taskId":rig.task, "sessionId":rig.agent_grant.session_id,
        "generation":rig.agent_grant.generation,
        "request":{"requestId":id,"class":"cli","subject":
            serde_json::to_value(waku_protocol::Command::AgentListModels).unwrap(),"deadlineMs":0},
    })).unwrap()).unwrap();
    std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o600)).unwrap();
    let output = rig.cli(id, &["models"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("expired"));
    assert!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .is_empty()
    );
    std::fs::write(&record, b"{truncated").unwrap();
    let output = rig.cli(id, &["models"]).output().unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("reconcile before retrying"));
    for i in 1..1024 {
        std::fs::write(records.join(format!("retained-{i}")), b"{}").unwrap();
    }
    let output = rig.cli(Uuid::new_v4(), &["models"]).output().unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("retention is full"));
    assert!(
        rig.service()
            .service_drain(rig.grant.epoch, 0)
            .unwrap()
            .is_empty()
    );
}
