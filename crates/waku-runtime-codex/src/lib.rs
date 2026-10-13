//! Codex's original child, stdio connection and pending native requests live
//! inside the host's adapter handle. Controller replacement never drops it.
//! This crate has no product/daemon/UI dependencies. It does not adopt legacy
//! daemon children or resume threads after transport loss.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};
use uuid::Uuid;
use waku_runtime::{
    ActivationBoundary, AdapterFactory, AdapterHandle, AgentIngress, CancelOutcome,
    CancellationLimit, CanonicalAction, DeadlineBound, DispatchOutcome, Enforcement, OfflineMatrix,
    ProviderEvent, RecoveredOwner, RequestClass, RequestedConfig, SessionContext, SpawnError,
    TerminateReason, Termination, agent_ingress_env,
};

const MAX_MESSAGE: usize = 1024 * 1024;
const MAX_PENDING: usize = 128;
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Authorized admission material. No resume cursor or replaceable daemon
/// credentials are accepted. Dynamic tools use the original reply pipe;
/// daemon-address-dependent MCP/CLI ingress awaits stage 2C.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexLaunch {
    pub binary: PathBuf,
    pub cwd: PathBuf,
    #[serde(default = "default_policy")]
    pub approval_policy: String,
    #[serde(default = "default_sandbox")]
    pub sandbox: String,
    #[serde(default)]
    pub developer_instructions: Option<String>,
    #[serde(default)]
    pub dynamic_tools: Vec<Value>,
    /// Session-scoped CLI ingress the controller provisioned for this
    /// launch — env pairs and the pinned helper directory applied to the
    /// spawned child after the inherited-credential scrub.
    #[serde(default)]
    pub agent: Option<AgentIngress>,
}
fn default_policy() -> String {
    "untrusted".into()
}
fn default_sandbox() -> String {
    "read-only".into()
}

#[derive(Default)]
pub struct CodexFactory;
impl AdapterFactory for CodexFactory {
    fn fence_recovered(&self, owner: &RecoveredOwner) -> Termination {
        fence_recovered_owner(owner)
    }
    fn spawn(&self, ctx: SessionContext) -> Result<Box<dyn AdapterHandle>, SpawnError> {
        if ctx.adapter != "codex" {
            return Err(SpawnError("unsupported adapter".into()));
        }
        let marker = owner_marker(&ctx.namespace, ctx.session_id, ctx.generation);
        let launch: CodexLaunch = serde_json::from_value(ctx.launch)
            .map_err(|e| SpawnError(format!("invalid Codex launch: {e}")))?;
        Ok(Box::new(CodexAdapter::start(
            launch,
            ctx.requested,
            ctx.sink,
            marker,
        )?))
    }
}

struct Turn {
    id: Uuid,
    delivery: Uuid,
    native: Option<String>,
    started: bool,
}
struct Request {
    native_id: Value,
    answered: bool,
    tool_call: Option<String>,
    resolution: Option<[u8; 32]>,
}
struct State {
    thread: Option<String>,
    turn: Option<Turn>,
    requests: HashMap<String, Request>,
    rpcs: HashMap<String, SyncSender<Value>>,
    next_rpc: u64,
    revision: u64,
    failed: bool,
    stopping: bool,
    reader_done: bool,
    identity: Value,
}
struct WriteJob {
    message: Vec<u8>,
    receipt: SyncSender<Result<(), ()>>,
}

/// The host alone retains this handle; attachment only changes host epochs.
struct CodexAdapter {
    child: Arc<Mutex<Child>>,
    writer: Option<SyncSender<WriteJob>>,
    state: Arc<Mutex<State>>,
    sink: SyncSender<ProviderEvent>,
    requested: RequestedConfig,
    requested_revision: u64,
    launch: CodexLaunch,
}

impl CodexAdapter {
    fn start(
        launch: CodexLaunch,
        requested: RequestedConfig,
        sink: SyncSender<ProviderEvent>,
        marker: String,
    ) -> Result<Self, SpawnError> {
        if !launch.binary.is_absolute()
            || !launch.binary.is_file()
            || !launch.cwd.is_absolute()
            || !launch.cwd.is_dir()
        {
            return Err(SpawnError(
                "Codex binary and cwd must be existing absolute paths".into(),
            ));
        }
        if !matches!(
            launch.approval_policy.as_str(),
            "untrusted" | "on-request" | "never"
        ) || !matches!(
            launch.sandbox.as_str(),
            "read-only" | "workspace-write" | "danger-full-access"
        ) || requested.provider.as_deref().is_some_and(|p| p != "codex")
            || !requested.options.is_null()
        {
            return Err(SpawnError("unsupported Codex policy/configuration".into()));
        }
        let mut command = guarded_command(&launch.binary, &marker);
        command
            .args(["app-server", "--stdio"])
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for server in ["computer-use", "node_repl", "cua_repl"] {
            command
                .arg("-c")
                .arg(format!("mcp_servers.{server}.command=\"/usr/bin/true\""));
            command
                .arg("-c")
                .arg(format!("mcp_servers.{server}.enabled=false"));
        }
        command
            .arg("-c")
            .arg(r#"skills.config=[{name="computer-use:computer-use",enabled=false}]"#);
        // Product-session ingress requires migrated runtime leases. Never let
        // an inherited Goddard daemon capability accidentally become authority.
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if name.starts_with("GODDARD_")
                || matches!(
                    name.as_ref(),
                    "WAKU_STABLE_RUNTIME_ROOT"
                        | "WAKU_RUNTIME_HOST_BIN"
                        | "WAKU_RUNTIME_DEVIN_BINARY"
                )
            {
                command.env_remove(key);
            }
        }
        // Only the launch-provisioned ingress pairs may re-enter — never an
        // inherited daemon token or descriptor a caller smuggled in.
        if let Some(agent) = &launch.agent {
            for (name, value) in
                agent_ingress_env(agent, std::env::var_os("PATH").as_deref())
            {
                command.env(name, value);
            }
        }
        let mut child = command
            .spawn()
            .map_err(|e| SpawnError(format!("Codex spawn: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SpawnError("stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SpawnError("stdout unavailable".into()))?;
        let state = Arc::new(Mutex::new(State {
            thread: None,
            turn: None,
            requests: HashMap::new(),
            rpcs: HashMap::new(),
            next_rpc: 0,
            revision: 0,
            failed: false,
            stopping: false,
            reader_done: false,
            identity: json!({"ownerGroupPid":child.id(),"ownerMarker":marker}),
        }));
        let (writer, jobs) = mpsc::sync_channel::<WriteJob>(8);
        let write_state = state.clone();
        thread::Builder::new()
            .name("stable-codex-writer".into())
            .spawn(move || {
                let mut stdin = stdin;
                while let Ok(job) = jobs.recv() {
                    let result = stdin
                        .write_all(&job.message)
                        .and_then(|_| stdin.flush())
                        .map_err(|_| ());
                    if result.is_err() {
                        write_state.lock().failed = true;
                    }
                    let _ = job.receipt.try_send(result);
                    if result.is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| {
                let _ = child.kill();
                let _ = child.wait();
                SpawnError(e.to_string())
            })?;
        let child = Arc::new(Mutex::new(child));
        let read_child = child.clone();
        let read_state = state.clone();
        let read_sink = sink.clone();

        thread::Builder::new()
            .name("stable-codex-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    // read_line would allocate without bound on a malformed/noisy peer.
                    let mut bytes = Vec::new();
                    let read = reader
                        .by_ref()
                        .take((MAX_MESSAGE + 1) as u64)
                        .read_until(b'\n', &mut bytes);
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(_) if bytes.len() > MAX_MESSAGE => {
                            fail(
                                &read_state,
                                &read_sink,
                                &read_child,
                                "oversized Codex message",
                            );
                            break;
                        }
                        Ok(_) => match serde_json::from_slice::<Value>(&bytes) {
                            Ok(value) => receive(value, &read_state, &read_sink, &read_child),
                            Err(_) => {
                                fail(
                                    &read_state,
                                    &read_sink,
                                    &read_child,
                                    "malformed Codex message",
                                );
                                break;
                            }
                        },
                    }
                    if read_state.lock().failed {
                        break;
                    }
                }
                fail(
                    &read_state,
                    &read_sink,
                    &read_child,
                    "Codex connection closed",
                );
                // This worker can wait for the host to drain capacity; controls
                // and dispatch never wait on the intake sink.
                let stopping = read_state.lock().stopping;
                let _ = read_sink.send(ProviderEvent::Terminated {
                    reason: if stopping {
                        TerminateReason::Requested
                    } else {
                        TerminateReason::Interrupted {
                            detail: "Codex transport ended".into(),
                        }
                    },
                });
                read_state.lock().reader_done = true;
            })
            .map_err(|e| {
                kill_owned(&mut child.lock());
                SpawnError(e.to_string())
            })?;
        let adapter = Self {
            child,
            writer: Some(writer),
            state,
            sink,
            requested,
            requested_revision: 0,
            launch,
        };
        let initialization = adapter.rpc("initialize", json!({
            "clientInfo": {"name":"goddard-stable-runtime", "version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"experimentalApi":true}
        }));
        if initialization.is_err() || adapter.write(json!({"method":"initialized"})).is_err() {
            return Err(SpawnError(
                "Codex initialization failed or unconfirmed".into(),
            ));
        }
        let mut params = json!({
            "cwd":adapter.launch.cwd, "approvalPolicy":adapter.launch.approval_policy,
            "sandbox":adapter.launch.sandbox, "ephemeral":true,
        });
        if let Some(model) = &adapter.requested.model {
            params["model"] = json!(model);
        }
        if let Some(instructions) = &adapter.launch.developer_instructions {
            params["developerInstructions"] = json!(instructions);
        }
        if !adapter.launch.dynamic_tools.is_empty() {
            params["dynamicTools"] = json!(adapter.launch.dynamic_tools);
        }
        let result = adapter
            .rpc("thread/start", params)
            .map_err(|_| SpawnError("Codex thread/start failed or unconfirmed".into()))?;
        let thread = result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| SpawnError("Codex thread/start omitted identity".into()))?;
        {
            let mut state = adapter.state.lock();
            state.thread = Some(thread.to_owned());
            state.identity["codexThreadId"] = json!(thread);
        }
        let identity = adapter.state.lock().identity.clone();
        adapter.emit(ProviderEvent::ConfigActivated {
            revision: 0,
            boundary: ActivationBoundary::Immediate,
            effective: RequestedConfig {
                provider: Some("codex".into()),
                model: result["model"].as_str().map(str::to_owned),
                effort: result["reasoningEffort"].as_str().map(str::to_owned),
                options: identity,
            },
        });
        adapter.emit(ProviderEvent::SessionLive);
        Ok(adapter)
    }

    fn emit(&self, event: ProviderEvent) {
        if self.sink.try_send(event).is_err() {
            fail(
                &self.state,
                &self.sink,
                &self.child,
                "Codex intake capacity exhausted",
            );
        }
    }
    fn write(&self, value: Value) -> Result<(), ()> {
        let mut message = serde_json::to_vec(&value).map_err(|_| ())?;
        if message.len() >= MAX_MESSAGE {
            return Err(());
        }
        message.push(b'\n');
        let (receipt, received) = mpsc::sync_channel(1);
        self.writer
            .as_ref()
            .ok_or(())?
            .try_send(WriteJob { message, receipt })
            .map_err(|_| ())?;
        received.recv_timeout(RPC_TIMEOUT).map_err(|_| ())?
    }
    fn rpc(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let (tx, rx) = mpsc::sync_channel(1);
        let id = {
            let mut state = self.state.lock();
            if state.failed || state.rpcs.len() >= MAX_PENDING {
                return Err(RpcError::Unknown);
            }
            state.next_rpc += 1;
            let id = format!("runtime:{}", state.next_rpc);
            state.rpcs.insert(json!(id).to_string(), tx);
            id
        };
        // Timeouts keep their identity pending: late acceptance is never a
        // reason to resend. A bounded pending map inhibits further work.
        self.write(json!({"id":id,"method":method,"params":params}))
            .map_err(|_| RpcError::Unknown)?;
        let reply = rx
            .recv_timeout(RPC_TIMEOUT)
            .map_err(|_| RpcError::Unknown)?;
        if reply.get("error").is_some() {
            return Err(RpcError::Refused);
        }
        reply.get("result").cloned().ok_or(RpcError::Unknown)
    }
}

enum RpcError {
    Refused,
    Unknown,
}
impl AdapterHandle for CodexAdapter {
    fn dispatch(&mut self, action: &CanonicalAction) -> DispatchOutcome {
        if !self.alive() {
            return DispatchOutcome::Unknown {
                detail: "Codex transport unavailable".into(),
            };
        }
        match action {
            CanonicalAction::Submit {
                delivery_id,
                turn_id,
                input,
            } => {
                let Some(text) = input.as_str().filter(|text| text.len() < MAX_MESSAGE / 2) else {
                    return refused("input must be a text string");
                };
                let thread = {
                    let mut state = self.state.lock();
                    if state.turn.is_some() {
                        return refused("a Codex turn is still open or uncertain");
                    }
                    state.revision = self.requested_revision;
                    state.turn = Some(Turn {
                        id: *turn_id,
                        delivery: *delivery_id,
                        native: None,
                        started: false,
                    });
                    state.thread.clone()
                };
                let mut params = json!({"threadId":thread,"input":[{"type":"text","text":text}],"summary":"auto"});
                if let Some(model) = &self.requested.model {
                    params["model"] = json!(model);
                }
                if let Some(effort) = &self.requested.effort {
                    params["effort"] = json!(effort);
                }
                match self.rpc("turn/start", params) {
                    Ok(result) => {
                        let mut state = self.state.lock();
                        let mut started = false;
                        if let Some(turn) = state.turn.as_mut() {
                            if let Some(native) = result.pointer("/turn/id").and_then(Value::as_str)
                            {
                                turn.native = Some(native.into());
                            }
                            if !turn.started {
                                turn.started = true;
                                started = true;
                            }
                        }
                        drop(state);
                        if started {
                            self.emit(ProviderEvent::TurnStarted {
                                turn_id: *turn_id,
                                delivery_id: *delivery_id,
                            });
                        }

                        DispatchOutcome::Accepted
                    }
                    Err(RpcError::Refused) => {
                        self.state.lock().turn = None;
                        refused("Codex refused turn/start")
                    }
                    Err(RpcError::Unknown) => DispatchOutcome::Unknown {
                        detail: "turn/start acceptance unconfirmed; never resend".into(),
                    },
                }
            }
            CanonicalAction::ResolveRequest {
                request_id,
                resolution,
            } => {
                if serde_json::to_vec(resolution).map_or(true, |b| b.len() >= MAX_MESSAGE / 2) {
                    return refused("oversized Codex response");
                }
                let native = {
                    let mut state = self.state.lock();
                    let Some(request) = state.requests.get_mut(request_id) else {
                        return refused("unknown original request");
                    };
                    if request.answered {
                        return refused("request already answered or uncertain");
                    }
                    request.answered = true;
                    request.resolution = request
                        .tool_call
                        .as_ref()
                        .map(|_| answer_digest(resolution));
                    request.native_id.clone()
                };
                let _ = self.write(json!({"id":native,"result":resolution}));
                // A flushed pipe or serverRequest/resolved is not proof of
                // consumption: cancellation also resolves native requests.
                // Only matching dynamic-tool result evidence reconciles it.
                DispatchOutcome::Unknown {
                    detail: "reply sent/attempted on original pipe; awaiting provider consumption"
                        .into(),
                }
            }
            CanonicalAction::Reconfigure { revision, target } => {
                if target.provider.as_deref().is_some_and(|p| p != "codex")
                    || !target.options.is_null()
                {
                    return refused("provider/access changes require a new generation");
                }
                self.requested = target.clone();
                self.requested_revision = *revision;
                DispatchOutcome::Accepted
            }
            _ => refused("unsupported Codex action"),
        }
    }
    fn cancel(&mut self, turn_id: Uuid) -> CancelOutcome {
        let ids = {
            let state = self.state.lock();
            match &state.turn {
                None => return CancelOutcome::NotRunning,
                Some(turn) if turn.id != turn_id => return CancelOutcome::NotRunning,
                Some(turn) => (state.thread.clone(), turn.native.clone()),
            }
        };
        let (Some(thread), Some(turn)) = ids else {
            return CancelOutcome::Unknown {
                detail: "native turn identity not yet confirmed".into(),
            };
        };
        match self.rpc("turn/interrupt", json!({"threadId":thread,"turnId":turn})) {
            Ok(_) => CancelOutcome::Unknown {
                detail: "interrupt accepted; awaiting turn/completed".into(),
            },
            Err(RpcError::Refused) => CancelOutcome::Failed {
                detail: "Codex refused interrupt".into(),
            },
            Err(RpcError::Unknown) => CancelOutcome::Unknown {
                detail: "interrupt delivery unconfirmed".into(),
            },
        }
    }
    fn terminate(&mut self) -> Termination {
        {
            let mut state = self.state.lock();
            if !state.failed {
                state.stopping = true;
            }
        }
        self.writer.take();
        kill_owned(&mut self.child.lock());
        match self.child.lock().try_wait() {
            Ok(Some(_)) if self.state.lock().reader_done => Termination::Confirmed,
            _ => Termination::Unconfirmed {
                detail: "Codex exit unconfirmed".into(),
            },
        }
    }
    fn alive(&self) -> bool {
        !self.state.lock().failed
    }
    fn offline_matrix(&self) -> OfflineMatrix {
        OfflineMatrix {
            native_ops: vec![
                format!(
                    "native operations under fixed {} sandbox",
                    self.launch.sandbox
                ),
                "scoped CLI ingress needs a write-capable or approved exec; a strictly sandboxed command fails closed".into(),
            ],
            revocation: Enforcement::Unavailable,
            callback_deadline: DeadlineBound::Unmeasured,
            can_pause_intake: false,
            cancellation: CancellationLimit::ProviderOnly,
        }
    }
}
impl Drop for CodexAdapter {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}
fn owner_marker(namespace: &str, session: Uuid, generation: u64) -> String {
    let digest = Sha256::digest(format!("{namespace}:{session}:{generation}").as_bytes());
    format!("goddard-runtime-codex-{:x}", digest)
}

#[cfg(unix)]
fn fence_recovered_owner(owner: &RecoveredOwner) -> Termination {
    let evidence = owner
        .effective
        .as_ref()
        .map(|effective| &effective.config.options);
    let marker = owner_marker(
        &owner.namespace,
        owner.session.session_id,
        owner.session.generation,
    );
    let Some(evidence) = evidence.filter(|e| e["ownerMarker"].as_str() == Some(&marker)) else {
        return Termination::Unconfirmed {
            detail: "missing original Codex owner evidence".into(),
        };
    };
    let Some(pid) = evidence["ownerGroupPid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .filter(|p| *p > 1 && *p != std::process::id())
    else {
        return Termination::Unconfirmed {
            detail: "invalid Codex owner evidence".into(),
        };
    };
    // A durable PID is only routing. The live current-user guardian marker is
    // the ownership proof before any group is signalled. No resume or replay.
    match group_proof(pid, &marker) {
        Ok(GroupProof::Absent) => Termination::Confirmed,
        Ok(GroupProof::Owned) => {
            kill_group(pid);
            for _ in 0..100 {
                if matches!(group_proof(pid, &marker), Ok(GroupProof::Absent)) {
                    return Termination::Confirmed;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Termination::Unconfirmed {
                detail: "Codex group cleanup remains unconfirmed".into(),
            }
        }
        _ => Termination::Unconfirmed {
            detail: "Codex group ownership remains unconfirmed".into(),
        },
    }
}
#[cfg(not(unix))]
fn fence_recovered_owner(_owner: &RecoveredOwner) -> Termination {
    Termination::Unconfirmed {
        detail: "Codex recovery is unqualified on this platform".into(),
    }
}
#[cfg(unix)]
enum GroupProof {
    Absent,
    Owned,
    Foreign,
}
#[cfg(unix)]
fn group_proof(pid: u32, marker: &str) -> Result<GroupProof, ()> {
    let mut process = Command::new("/bin/ps")
        .args(["-axo", "uid=,pgid=,stat=,command="])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let mut bytes = Vec::new();
    let read = process
        .stdout
        .take()
        .ok_or(())?
        .take((4 * MAX_MESSAGE + 1) as u64)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > 4 * MAX_MESSAGE {
        let _ = process.kill();
        let _ = process.wait();
        return Err(());
    }
    if !process.wait().map_err(|_| ())?.success() {
        return Err(());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| ())?;
    let mut present = false;
    let mut own = false;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let uid = fields.next().and_then(|s| s.parse::<u32>().ok());
        let group = fields.next().and_then(|s| s.parse::<u32>().ok());
        let status = fields.next().unwrap_or_default();
        if group != Some(pid) || status.starts_with('Z') {
            continue;
        }
        present = true;
        if uid != Some(unsafe { libc::geteuid() }) {
            return Ok(GroupProof::Foreign);
        }
        own |= fields.any(|field| field == marker);
    }
    Ok(if !present {
        GroupProof::Absent
    } else if own {
        GroupProof::Owned
    } else {
        GroupProof::Foreign
    })
}

fn answer_digest(value: &Value) -> [u8; 32] {
    let canonical = json!({"success":value["success"],"contentItems":value["contentItems"]});
    Sha256::digest(canonical_json(&canonical).to_string().as_bytes()).into()
}
fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            let sorted: std::collections::BTreeMap<_, _> = fields.iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        value => value.clone(),
    }
}
fn refused(detail: &str) -> DispatchOutcome {
    DispatchOutcome::Rejected {
        code: "codex".into(),
        detail: detail.into(),
    }
}

fn receive(
    value: Value,
    shared: &Mutex<State>,
    sink: &SyncSender<ProviderEvent>,
    child: &Mutex<Child>,
) {
    let mut state = shared.lock();
    if value.get("method").is_none() {
        if let Some(id) = value.get("id")
            && let Some(reply) = state.rpcs.remove(&id.to_string())
        {
            let _ = reply.try_send(value);
        }
        return;
    }
    let method = value["method"].as_str().unwrap_or_default();
    let params = &value["params"];
    // Ignore other threads on the same server, including auxiliary activity.
    if let Some(thread) = params.get("threadId").and_then(Value::as_str)
        && state.thread.as_deref().is_some_and(|own| own != thread)
    {
        return;
    }
    if let Some(native) = params.get("turnId").and_then(Value::as_str)
        && state
            .turn
            .as_ref()
            .and_then(|t| t.native.as_deref())
            .is_some_and(|own| own != native)
    {
        return;
    }
    let event =
        if let Some(id) = value.get("id") {
            if !(id.is_string() || id.is_i64()) {
                drop(state);
                fail(shared, sink, child, "invalid Codex request identity");
                return;
            }
            let class = match method {
                "item/tool/call" => RequestClass::Tool,
                "item/tool/requestUserInput" => RequestClass::Input,
                m if m.ends_with("requestApproval") => RequestClass::Approval,
                _ => {
                    drop(state);
                    fail(shared, sink, child, "unsupported Codex native request");
                    return;
                }
            };
            let key = id.to_string(); // JSON preserves numeric vs string IDs.
            if key.len() > 256
                || params["callId"].as_str().is_some_and(|id| id.len() > 256)
                || state.requests.len() >= MAX_PENDING
                || state.requests.contains_key(&key)
            {
                drop(state);
                fail(
                    shared,
                    sink,
                    child,
                    "Codex pending request capacity/collision",
                );
                return;
            }
            state.requests.insert(
                key.clone(),
                Request {
                    native_id: id.clone(),
                    answered: false,
                    tool_call: (class == RequestClass::Tool)
                        .then(|| params["callId"].as_str().map(str::to_owned))
                        .flatten(),
                    resolution: None,
                },
            );
            Some(ProviderEvent::RequestOpened {
                request_id: key,
                class,
                subject: value.clone(),
                deadline_ms: None,
            })
        } else if method == "thread/settings/updated" {
            let settings = &params["threadSettings"];
            Some(ProviderEvent::ConfigActivated {
                revision: state.revision,
                boundary: ActivationBoundary::NextTurn,
                effective: RequestedConfig {
                    provider: Some("codex".into()),
                    model: settings["model"].as_str().map(str::to_owned),
                    effort: settings["effort"].as_str().map(str::to_owned),
                    options: state.identity.clone(),
                },
            })
        } else if method == "item/completed" && params["item"]["type"] == "dynamicToolCall" {
            let item = &params["item"];
            let key = state.requests.iter().find_map(|(key, r)| {
                let resolution = r.resolution?;
                (r.answered
                    && r.tool_call.as_deref() == item["id"].as_str()
                    && item["status"] == "completed"
                    && resolution == answer_digest(item))
                .then(|| key.clone())
            });
            key.map(|key| {
                state.requests.remove(&key);
                ProviderEvent::RequestAnswered { request_id: key }
            })
        } else if method == "serverRequest/resolved" {
            // The provider resolved this native request. An unanswered entry
            // is abandoned and can never accept a reply; an answered
            // approval/input has no consumption evidence left to expect.
            // Answered tool calls keep their entry until item/completed
            // proves consumption. Without this the pending map only grows:
            // long sessions collide with MAX_PENDING and replies are written
            // to dead native ids.
            let key = params["requestId"].to_string();
            if state
                .requests
                .get(&key)
                .is_some_and(|request| !request.answered || request.tool_call.is_none())
            {
                state.requests.remove(&key);
            }
            // Preserve the raw evidence for reconciliation without fabricating an
            // acknowledgement. This notice can also mean cancellation/abandonment.
            state.turn.as_ref().map(|turn| ProviderEvent::TurnDelta {
                turn_id: turn.id,
                delta: value.clone(),
            })
        } else if method == "turn/started" {
            state.turn.as_mut().and_then(|turn| {
                if turn.native.as_deref().is_some_and(|own| {
                    Some(own) != params.pointer("/turn/id").and_then(Value::as_str)
                }) {
                    return None;
                }
                turn.native = params
                    .pointer("/turn/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if turn.started {
                    return None;
                }
                turn.started = true;
                Some(ProviderEvent::TurnStarted {
                    turn_id: turn.id,
                    delivery_id: turn.delivery,
                })
            })
        } else if method == "turn/completed" {
            let matches = state.turn.as_ref().is_some_and(|turn| {
                turn.native.as_deref() == params.pointer("/turn/id").and_then(Value::as_str)
                    && turn.native.is_some()
            });
            if !matches {
                return;
            }
            // A turn boundary resolves every turn-scoped native request
            // upstream. Only answered tool calls still awaiting their
            // item/completed consumption proof may be legitimately
            // reconciled; everything else is dead reply capacity.
            state
                .requests
                .retain(|_, request| request.answered && request.tool_call.is_some());
            state.turn.take().map(|turn| {
                if params.pointer("/turn/status").and_then(Value::as_str) == Some("interrupted") {
                    ProviderEvent::TurnCancelled { turn_id: turn.id }
                } else {
                    ProviderEvent::TurnCompleted {
                        turn_id: turn.id,
                        result: params.clone(),
                    }
                }
            })
        } else {
            state.turn.as_ref().map(|turn| ProviderEvent::TurnDelta {
                turn_id: turn.id,
                delta: value.clone(),
            })
        };
    if method == "turn/started"
        && let Some(turn) = state.turn.as_ref()
        && turn.native.as_deref() == params.pointer("/turn/id").and_then(Value::as_str)
        && sink
            .try_send(ProviderEvent::TurnDelta {
                turn_id: turn.id,
                delta: value.clone(),
            })
            .is_err()
    {
        drop(state);
        fail(shared, sink, child, "Codex intake capacity exhausted");
        return;
    }
    if let Some(event) = event
        && sink.try_send(event).is_err()
    {
        drop(state);
        fail(shared, sink, child, "Codex intake capacity exhausted");
    }
}
fn fail(
    shared: &Mutex<State>,
    sink: &SyncSender<ProviderEvent>,
    child: &Mutex<Child>,
    detail: &str,
) {
    let mut state = shared.lock();
    if state.failed {
        return;
    }
    state.failed = true;
    state.rpcs.clear();
    if state.stopping {
        return;
    }
    let _ = sink.try_send(ProviderEvent::ProviderFailed {
        detail: detail.into(),
    });
    drop(state);
    kill_owned(&mut child.lock());
}

// Same guardian mechanism as the legacy driver, but PPID is the stable host.
// fd 9 preserves stdin for shells that replace background stdin with /dev/null.
#[cfg(unix)]
const GUARDIAN: &str = r#"
owner=$PPID
wrapper=$$
exec 9<&0
"$@" <&9 &
child=$!
(
  while kill -0 "$owner" 2>/dev/null && kill -0 "$wrapper" 2>/dev/null; do sleep 1; done
  /bin/kill -KILL -- "-$wrapper" 2>/dev/null || kill -TERM "$child" 2>/dev/null || true
) &
watcher=$!
trap '/bin/kill -KILL -- "-$wrapper" 2>/dev/null' EXIT HUP INT TERM
wait "$child"
exit $?
"#;
#[cfg(unix)]
fn guarded_command(binary: &std::path::Path, marker: &str) -> Command {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", GUARDIAN, marker])
        .arg(binary)
        .process_group(0);
    command
}
#[cfg(not(unix))]
fn guarded_command(binary: &std::path::Path, _marker: &str) -> Command {
    Command::new(binary)
}
#[cfg(unix)]
fn kill_group(pid: u32) {
    // Each adapter creates its own process group; never signal a shared group.
    if let Ok(pid) = i32::try_from(pid) {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}
#[cfg(not(unix))]
fn kill_group(_pid: u32) {}
fn kill_owned(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    kill_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests;
