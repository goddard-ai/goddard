//! Devin's original ACP connection, owned by the stable host rather than a controller.
//!
//! Admission creates a fresh session. Reattachment never calls session/new or
//! resume: controllers attach to the host's journal, not to this transport.
//! Native responder delivery has no consumption acknowledgement in ACP v1.

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ClientCapabilities, ContentBlock, Implementation, InitializeRequest,
    NewSessionRequest, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOptions, SessionNotification,
    SetSessionConfigOptionRequest, StopReason, TextContent,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectTo, ConnectionTo, Handled, Responder,
    UntypedMessage,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::SyncSender,
};
use std::time::Duration;
use uuid::Uuid;
use waku_runtime::adapter::*;
use waku_runtime::contract::{
    ActivationBoundary, CanonicalAction, ProviderEvent, RequestClass, RequestedConfig,
    TerminateReason,
};

/// Trusted executable selection belongs to the pinned runtime, not admission data.
#[derive(Clone)]
pub struct DevinFactory {
    binary: PathBuf,
}
impl DevinFactory {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }
}

/// No resume cursor or mutable daemon callback destination is accepted.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Launch {
    cwd: PathBuf,
}

enum Command {
    Prompt {
        turn_id: Uuid,
        delivery_id: Uuid,
        text: String,
    },
    InputAnswer {
        id: String,
        response: Value,
    },
    Configure {
        revision: u64,
        target: RequestedConfig,
    },
    Cancel(Uuid),
    Answer {
        id: String,
        option: String,
    },
}
struct Turn {
    id: Uuid,
    delivery: Uuid,
    started: bool,
    cancel_requested: bool,
}
struct State {
    turn: Option<Turn>,
    permissions: HashMap<String, (Responder<RequestPermissionResponse>, Vec<String>)>,
    inputs: HashMap<String, (Responder<Value>, Value)>,
    native_session: Option<String>,
    config: Vec<SessionConfigOption>,
    revision: u64,
    configuring: bool,
}
struct Shared {
    state: Mutex<State>,
    sink: Mutex<Option<SyncSender<ProviderEvent>>>,
    stop: smol::channel::Sender<()>,
    commands: smol::channel::Sender<Command>,
    live: AtomicBool,
    stopping: AtomicBool,
    overflow: AtomicBool,
    ended: AtomicBool,
    owned_pid: AtomicU32,
}
impl Shared {
    // Every event is emitted under the state lease, so callbacks cannot reorder
    // acceptance, deltas and settlement. Never block the SDK's response pump.
    fn emit(&self, event: ProviderEvent) {
        if self.overflow.load(Ordering::Acquire) {
            return;
        }
        let sink = self.sink.lock().unwrap();
        if sink
            .as_ref()
            .is_none_or(|sink| sink.try_send(event).is_err())
        {
            self.overflow.store(true, Ordering::Release);
            self.live.store(false, Ordering::Release);
            self.commands.close();
            let _ = self.stop.try_send(());
        }
    }
    fn started(&self, state: &mut State) {
        if let Some(turn) = &mut state.turn
            && !turn.started
        {
            turn.started = true;
            self.emit(ProviderEvent::TurnStarted {
                turn_id: turn.id,
                delivery_id: turn.delivery,
            });
        }
    }
}

impl AdapterFactory for DevinFactory {
    fn spawn(&self, ctx: SessionContext) -> Result<Box<dyn AdapterHandle>, SpawnError> {
        if ctx.adapter != "devin" {
            return Err(SpawnError("not a Devin admission".into()));
        }
        let launch: Launch = serde_json::from_value(ctx.launch)
            .map_err(|_| SpawnError("invalid Devin launch; expected absolute cwd only".into()))?;
        if !self.binary.is_absolute()
            || self.binary.to_str().is_none()
            || !launch.cwd.is_absolute()
            || launch.cwd.to_str().is_none()
            || !launch.cwd.is_dir()
        {
            return Err(SpawnError(
                "Devin executable and existing cwd must be absolute".into(),
            ));
        }
        // Model/effort mapping must use Devin's advertised option values; do not
        // claim the provider activated a requested configuration by echoing it.
        if ctx
            .requested
            .provider
            .as_deref()
            .is_some_and(|p| p != "devin")
            || !ctx.requested.options.is_null()
        {
            return Err(SpawnError(
                "Devin configuration requires provider devin and null options".into(),
            ));
        }
        let (commands, rx) = smol::channel::bounded(8);
        let (stop, stop_rx) = smol::channel::bounded(1);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                turn: None,
                permissions: HashMap::new(),
                inputs: HashMap::new(),
                native_session: None,
                config: Vec::new(),
                revision: 0,
                configuring: false,
            }),
            sink: Mutex::new(Some(ctx.sink)),
            stop,
            commands: commands.clone(),
            live: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            overflow: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            owned_pid: AtomicU32::new(0),
        });
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let worker = shared.clone();
        let binary = self.binary.clone();
        std::thread::Builder::new().name("runtime-devin-acp".into()).spawn(move || {
            let result = smol::block_on(run_owned(binary, launch.cwd, ctx.requested, rx, stop_rx, worker.clone(), ready_tx));
            worker.live.store(false, Ordering::Release);
            let mut state = worker.state.lock().unwrap();
            state.permissions.clear();
            state.inputs.clear();
            state.turn.take();
            if let Err(_error) = result {
                // SDK errors may include captured stderr/credentials. Keep raw
                // provider diagnostics out of the public journal.
                worker.emit(ProviderEvent::ProviderFailed { detail: "Devin ACP connection failed; pending delivery/outcome remains uncertain".into() });
            }
            drop(state);
            // An unobserved child exit must stay Stopping, including in the
            // host's disconnected-channel fallback. Retain its sender until
            // the controller explicitly reconciles the uncertain owner.
            if worker.stopping.load(Ordering::Acquire) && !worker.ended.load(Ordering::Acquire) { return; }
            // Reserve one final in-flight record after closing the SDK. Blocking
            // here cannot stall the original transport or grow event memory.
            // Drop the only sender afterward so the host also detects loss.
            if let Some(sink) = worker.sink.lock().unwrap().take() {
                let reason = if worker.overflow.load(Ordering::Acquire) {
                    TerminateReason::Backpressure
                } else if worker.stopping.load(Ordering::Acquire) && worker.ended.load(Ordering::Acquire) {
                    TerminateReason::Requested
                } else {
                    TerminateReason::Interrupted { detail: "Devin connection lost; unresolved delivery and external work remain uncertain".into() }
                };
                let _ = sink.send(ProviderEvent::Terminated { reason });
            }
        }).map_err(|e| SpawnError(format!("cannot start Devin worker: {e}")))?;
        match ready_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(()) if shared.live.load(Ordering::Acquire) => {
                Ok(Box::new(DevinHandle { commands, shared }))
            }
            _ => {
                shared.commands.close();
                let _ = shared.stop.try_send(());
                Err(SpawnError("Devin ACP initialization failed or timed out; no recovery session was substituted".into()))
            }
        }
    }
}

struct DevinHandle {
    commands: smol::channel::Sender<Command>,
    shared: Arc<Shared>,
}
fn refused(detail: &str) -> DispatchOutcome {
    DispatchOutcome::Rejected {
        code: "devinUnavailable".into(),
        detail: detail.into(),
    }
}
fn uncertain(detail: &str) -> DispatchOutcome {
    DispatchOutcome::Unknown {
        detail: detail.into(),
    }
}
impl AdapterHandle for DevinHandle {
    fn dispatch(&mut self, action: &CanonicalAction) -> DispatchOutcome {
        if !self.alive() {
            return refused("Devin transport is not live");
        }
        let mut state = self.shared.state.lock().unwrap();
        match action {
            CanonicalAction::Submit {
                turn_id,
                delivery_id,
                input,
            } => {
                let Some(text) = input.as_str() else {
                    return refused("Devin input must be a text string");
                };
                if text.len() > 256 * 1024 {
                    return refused("Devin text input exceeds bounded command intake");
                }
                if state.turn.is_some() || state.configuring {
                    return refused("Devin already has an unresolved turn");
                }
                state.turn = Some(Turn {
                    id: *turn_id,
                    delivery: *delivery_id,
                    started: false,
                    cancel_requested: false,
                });
                if self
                    .commands
                    .try_send(Command::Prompt {
                        turn_id: *turn_id,
                        delivery_id: *delivery_id,
                        text: text.into(),
                    })
                    .is_err()
                {
                    state.turn = None;
                    return refused("Devin command intake closed or full before dispatch");
                }
                uncertain(
                    "queued on original ACP connection; provider acceptance requires turn evidence",
                )
            }
            CanonicalAction::ResolveRequest {
                request_id,
                resolution,
            } => {
                if resolution.to_string().len() > 256 * 1024 {
                    return refused("Devin response exceeds bounded command intake");
                }
                if let Some((_, schema)) = state.inputs.get(request_id) {
                    if !valid_input_response(schema, resolution) {
                        return refused(
                            "Devin input response requires original primitive field types and offered enum values",
                        );
                    }
                    if self
                        .commands
                        .try_send(Command::InputAnswer {
                            id: request_id.clone(),
                            response: resolution.clone(),
                        })
                        .is_err()
                    {
                        return refused("Devin input answer intake closed or full");
                    }
                    return uncertain(
                        "ACP input responder delivery has no consumption acknowledgement; never resend",
                    );
                }
                let Some(option) = resolution.get("optionId").and_then(Value::as_str) else {
                    return refused("permission resolution requires optionId");
                };
                let Some((_, options)) = state.permissions.get(request_id) else {
                    return refused("no original Devin responder remains");
                };
                if !options.iter().any(|id| id == option) {
                    return refused("optionId was not offered by this request");
                }
                if self
                    .commands
                    .try_send(Command::Answer {
                        id: request_id.clone(),
                        option: option.into(),
                    })
                    .is_err()
                {
                    return refused("Devin answer intake closed or full");
                }
                uncertain(
                    "ACP responder delivery has no provider-consumption acknowledgement; never resend",
                )
            }
            CanonicalAction::Reconfigure { revision, target } => {
                if state.turn.is_some() || state.configuring || *revision <= state.revision {
                    return refused(
                        "Devin configuration requires an idle session and newer revision",
                    );
                }
                if config_writes(&state.config, target).is_err() {
                    return refused(
                        "Devin configuration must use advertised model/effort values and null options",
                    );
                }
                if self
                    .commands
                    .try_send(Command::Configure {
                        revision: *revision,
                        target: target.clone(),
                    })
                    .is_err()
                {
                    return refused("Devin configuration intake closed or full");
                }
                state.configuring = true;
                uncertain("configuration queued; activation requires original provider response")
            }
            _ => refused("action not supported by Devin adapter"),
        }
    }
    fn cancel(&mut self, turn_id: Uuid) -> CancelOutcome {
        let state = self.shared.state.lock().unwrap();
        if state.turn.as_ref().is_none_or(|t| t.id != turn_id) {
            return CancelOutcome::NotRunning;
        }
        if self.commands.try_send(Command::Cancel(turn_id)).is_err() {
            return CancelOutcome::Failed {
                detail: "Devin cancel intake closed or full".into(),
            };
        }
        CancelOutcome::Unknown {
            detail:
                "ACP cancel is a notification; wait for the original prompt's cancelled response"
                    .into(),
        }
    }
    fn terminate(&mut self) -> Termination {
        self.shared.stopping.store(true, Ordering::Release);
        self.commands.close();
        let _ = self.shared.stop.try_send(());
        if self.shared.ended.load(Ordering::Acquire) {
            Termination::Confirmed
        } else {
            Termination::Unconfirmed {
                detail: "Devin cancellation/teardown requested; child exit not yet observed".into(),
            }
        }
    }
    fn alive(&self) -> bool {
        self.shared.live.load(Ordering::Acquire)
            && !self.shared.stopping.load(Ordering::Acquire)
            && !self.shared.overflow.load(Ordering::Acquire)
    }
    fn offline_matrix(&self) -> OfflineMatrix {
        OfflineMatrix {
            native_ops: vec![
                "Devin-native tools under provider defaults; grant mapping unqualified".into(),
            ],
            revocation: Enforcement::Unavailable,
            callback_deadline: DeadlineBound::Unmeasured,
            can_pause_intake: false,
            cancellation: CancellationLimit::ProviderOnly,
        }
    }
}
impl Drop for DevinHandle {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

// The watchdog follows the runtime PID, never a daemon or app PID. The main
// shell execs the provider, so the child's wait handle remains its real owner.
#[cfg(unix)]
const GUARDIAN: &str = r#"
runtime=$1
shift
provider=$$
(
  exec </dev/null
  trap '' TERM
  while kill -0 "$runtime" 2>/dev/null && kill -0 "$provider" 2>/dev/null; do
    sleep 1
  done
  kill -KILL -- "-$provider" 2>/dev/null || true
) &
exec "$@"
"#;

const MAX_LINE_BYTES: usize = 1024 * 1024;
async fn bounded_line<R: futures::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<String>> {
    use futures::AsyncBufReadExt;
    let mut line = Vec::new();
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "partial ACP record",
            ));
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let count = newline.map_or(chunk.len(), |n| n + 1);
        if line.len() + count > MAX_LINE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ACP record exceeds bounded intake",
            ));
        }
        line.extend_from_slice(&chunk[..count]);
        reader.consume_unpin(count);
        if newline.is_some() {
            line.pop();
            return String::from_utf8(line).map(Some).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid ACP UTF-8")
            });
        }
    }
}

#[cfg(unix)]
fn normalize_signals() -> std::io::Result<()> {
    // SAFETY: the initialized sigset is confined to this worker; only SIGCHLD
    // is unblocked so async_process can observe and reap its owned child.
    unsafe {
        let mut signals = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
        if libc::sigemptyset(signals.as_mut_ptr()) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut signals = signals.assume_init();
        if libc::sigaddset(&mut signals, libc::SIGCHLD) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let code = libc::pthread_sigmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut());
        if code != 0 {
            return Err(std::io::Error::from_raw_os_error(code));
        }
    }
    Ok(())
}

struct GroupGuard(Option<u32>);
impl Drop for GroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: spawn_process created a new process group whose leader is
        // this owned child's PID. No controller or unrelated PID is used.
        if let Some(pid) = self.0.take() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

async fn run_owned(
    binary: PathBuf,
    cwd: PathBuf,
    requested: RequestedConfig,
    commands: smol::channel::Receiver<Command>,
    stop: smol::channel::Receiver<()>,
    shared: Arc<Shared>,
    ready: SyncSender<()>,
) -> agent_client_protocol::Result<()> {
    use futures::{AsyncReadExt, AsyncWriteExt};
    #[cfg(unix)]
    normalize_signals().map_err(agent_client_protocol::Error::into_internal_error)?;
    let mut provider_args = Vec::new();
    // A runtime adapter must never lend inherited employee/controller tokens
    // or the old daemon address to a newly admitted provider session. Stable
    // session-scoped ingress is supplied by the later reconciliation stage.
    for name in std::env::vars_os().map(|(name, _)| name) {
        if let Some(name) = name
            .to_str()
            .filter(|name| name.starts_with("GODDARD_") || name.starts_with("WAKU_"))
        {
            provider_args.extend(["-u".to_owned(), name.to_owned()]);
        }
    }
    provider_args.extend([
        "-C".into(),
        cwd.to_string_lossy().into_owned(),
        binary.to_string_lossy().into_owned(),
        "acp".into(),
    ]);
    #[cfg(unix)]
    let config = AcpAgentConfig::new("/bin/sh").args(
        [
            "-c".into(),
            GUARDIAN.into(),
            "goddard-runtime-devin".into(),
            std::process::id().to_string(),
            "/usr/bin/env".into(),
        ]
        .into_iter()
        .chain(provider_args),
    );
    #[cfg(not(unix))]
    let config = AcpAgentConfig::new("/usr/bin/env").args(provider_args);
    let agent = AcpAgent::new(config);
    let (stdin, stdout, mut stderr, mut child) = agent.spawn_process()?;
    let pid = child.id();
    shared.owned_pid.store(pid, Ordering::Release);
    let mut cleanup = GroupGuard(Some(pid));
    let outgoing = futures::sink::unfold(stdin, |mut stdin, line: String| async move {
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok::<_, std::io::Error>(stdin)
    });
    let incoming = futures::stream::try_unfold(
        futures::io::BufReader::new(stdout),
        |mut reader| async move {
            bounded_line(&mut reader)
                .await
                .map(|line| line.map(|line| (line, reader)))
        },
    );
    let stderr_task = smol::spawn(async move {
        // Drain without retaining private diagnostics or unbounded lines.
        let mut buffer = [0_u8; 8192];
        while stderr.read(&mut buffer).await.is_ok_and(|n| n != 0) {}
    });
    let result = smol::future::race(
        connection(
            agent_client_protocol::Lines::new(Box::pin(outgoing), Box::pin(incoming)),
            cwd,
            requested,
            commands,
            shared.clone(),
            ready,
        ),
        smol::future::race(
            async {
                let _ = stop.recv().await;
                Ok(())
            },
            async {
                let _ = child.status().await;
                Ok(())
            },
        ),
    )
    .await;
    // All exits, including request-handler errors, pass through this owner.
    // No automatic respawn or replay follows a broken connection.
    #[cfg(unix)]
    // SAFETY: this is the process group created for the owned child above.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    cleanup.0 = None; // group kill was attempted while the child was still owned
    let ended = smol::future::race(async { child.status().await.is_ok() }, async {
        smol::Timer::after(Duration::from_secs(2)).await;
        false
    })
    .await;
    shared.ended.store(ended, Ordering::Release);
    drop(stderr_task);
    result
}

// Elicitation form responses retain the native wire shape. Validate the
// requested primitive field types rather than accepting unrelated answers.
fn valid_input_response(schema: &Value, response: &Value) -> bool {
    let Some(action) = response.get("action").and_then(Value::as_str) else {
        return false;
    };
    if matches!(action, "cancel" | "decline") {
        return response.get("content").is_none();
    }
    if action != "accept" {
        return false;
    }
    let Some(content) = response.get("content").and_then(Value::as_object) else {
        return false;
    };
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return false;
    };
    if schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|fields| {
            fields
                .iter()
                .any(|f| f.as_str().is_none_or(|name| !content.contains_key(name)))
        })
    {
        return false;
    }
    content.iter().all(|(name, value)| {
        let Some(field) = properties.get(name) else {
            return false;
        };
        let typed = match field.get("type").and_then(Value::as_str) {
            Some("string") => value.is_string(),
            Some("boolean") => value.is_boolean(),
            Some("number") => value.is_number(),
            Some("integer") => value.is_i64() || value.is_u64(),
            _ => false,
        };
        typed
            && field
                .get("enum")
                .and_then(Value::as_array)
                .is_none_or(|values| values.contains(value))
    })
}

fn config_error(detail: &str) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(detail)
}

// Map only exact advertised values. Never guess aliases or use a fallback
// model endpoint, which could turn a rejected write into silent substitution.
fn config_option<'a>(
    options: &'a [SessionConfigOption],
    model: bool,
) -> Option<&'a SessionConfigOption> {
    let names: &[&str] = if model {
        &["model", "models"]
    } else {
        &["thought_level"]
    };
    let category = if model {
        SessionConfigOptionCategory::Model
    } else {
        SessionConfigOptionCategory::ThoughtLevel
    };
    options
        .iter()
        .find(|o| names.contains(&o.id.to_string().as_str()))
        .or_else(|| {
            options
                .iter()
                .find(|o| o.category.as_ref() == Some(&category) && o.id.to_string() != "provider")
        })
}

fn config_writes(
    options: &[SessionConfigOption],
    target: &RequestedConfig,
) -> Result<Vec<(String, String)>, &'static str> {
    if target.provider.as_deref().is_some_and(|p| p != "devin") || !target.options.is_null() {
        return Err("unsupported Devin configuration");
    }
    let mut writes = Vec::new();
    for (model, value) in [(true, &target.model), (false, &target.effort)] {
        let Some(value) = value else { continue };
        let option = config_option(options, model).ok_or("configuration option not advertised")?;
        let SessionConfigKind::Select(select) = &option.kind else {
            return Err("configuration is not a select");
        };
        let offered = match &select.options {
            SessionConfigSelectOptions::Ungrouped(values) => {
                values.iter().any(|o| o.value.to_string() == *value)
            }
            SessionConfigSelectOptions::Grouped(groups) => groups
                .iter()
                .any(|g| g.options.iter().any(|o| o.value.to_string() == *value)),
            _ => false,
        };
        if !offered {
            return Err("configuration value not advertised");
        }
        if select.current_value.to_string() != *value {
            writes.push((option.id.to_string(), value.clone()));
        }
    }
    Ok(writes)
}

async fn configure(
    connection: &ConnectionTo<Agent>,
    session: &agent_client_protocol::schema::v1::SessionId,
    mut options: Vec<SessionConfigOption>,
    target: &RequestedConfig,
) -> agent_client_protocol::Result<Vec<SessionConfigOption>> {
    config_writes(&options, target).map_err(config_error)?;
    // Model writes may replace the effort catalog. Revalidate against every
    // returned catalog and verify actual current values before publishing.
    for model in [true, false] {
        let value = if model { &target.model } else { &target.effort };
        let Some(value) = value else { continue };
        let partial = RequestedConfig {
            model: model.then(|| value.clone()),
            effort: (!model).then(|| value.clone()),
            ..Default::default()
        };
        for (id, value) in config_writes(&options, &partial).map_err(config_error)? {
            let response = smol::future::race(
                connection
                    .send_request(SetSessionConfigOptionRequest::new(
                        session.clone(),
                        id,
                        value.as_str(),
                    ))
                    .block_task(),
                async {
                    smol::Timer::after(Duration::from_secs(10)).await;
                    Err(config_error("Devin configuration response deadline"))
                },
            )
            .await?;
            options = response.config_options;
        }
    }
    if !config_writes(&options, target)
        .map_err(config_error)?
        .is_empty()
    {
        return Err(config_error(
            "Devin did not activate requested configuration",
        ));
    }
    Ok(options)
}

fn initial_config(
    native_session: &str,
    options: &[agent_client_protocol::schema::v1::SessionConfigOption],
    owned_pid: u32,
) -> RequestedConfig {
    use agent_client_protocol::schema::v1::{SessionConfigKind, SessionConfigOptionCategory};
    let current = |category: SessionConfigOptionCategory, names: &[&str]| {
        options
            .iter()
            .find(|option| {
                names
                    .iter()
                    .any(|name| option.id.to_string().eq_ignore_ascii_case(name))
            })
            .or_else(|| {
                options.iter().find(|option| {
                    option.category.as_ref() == Some(&category)
                        && option.id.to_string() != "provider"
                })
            })
            .and_then(|option| match &option.kind {
                SessionConfigKind::Select(select) => Some(select.current_value.to_string()),
                _ => None,
            })
    };
    RequestedConfig {
        provider: Some("devin".into()),
        model: current(SessionConfigOptionCategory::Model, &["model", "models"]),
        effort: current(
            SessionConfigOptionCategory::ThoughtLevel,
            &["thought_level"],
        ),
        options: serde_json::json!({"nativeSessionId": native_session, "ownedPid": owned_pid}),
    }
}

async fn connection(
    agent: impl ConnectTo<Client>,
    cwd: PathBuf,
    requested: RequestedConfig,
    commands: smol::channel::Receiver<Command>,
    shared: Arc<Shared>,
    ready: SyncSender<()>,
) -> agent_client_protocol::Result<()> {
    Client.builder().name("goddard-stable-devin")
        .on_receive_notification({
            let shared = shared.clone();
            async move |update: SessionNotification, _connection| {
                let mut state = shared.state.lock().unwrap();
                if state.native_session.as_deref() != Some(update.session_id.to_string().as_str()) { return Ok(()); }
                let delta = serde_json::to_value(&update.update)?;
                // Configuration/title updates are not proof that a prompt ran.
                if matches!(delta.get("sessionUpdate").and_then(Value::as_str), Some("agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update")) {
                    shared.started(&mut state);
                }
                if let Some(turn) = &state.turn { shared.emit(ProviderEvent::TurnDelta { turn_id: turn.id, delta }); }
                Ok(())
            }
        }, agent_client_protocol::on_receive_notification!())
        .on_receive_request({
            let shared = shared.clone();
            async move |request: RequestPermissionRequest, responder: Responder<RequestPermissionResponse>, _connection| {
                let mut state = shared.state.lock().unwrap();
                if state.native_session.as_deref() != Some(request.session_id.to_string().as_str()) || state.turn.is_none() || shared.stopping.load(Ordering::Acquire) {
                    return responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled));
                }
                shared.started(&mut state);
                if state.permissions.len() + state.inputs.len() >= 64 {
                    responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled))?;
                    shared.emit(ProviderEvent::ProviderFailed { detail: "Devin pending permission capacity exhausted; interrupting transport".into() });
                    shared.commands.close();
                    let _ = shared.stop.try_send(());
                    return Ok(());
                }
                let id = responder.id().to_string();
                let options = request.options.iter().map(|o| o.option_id.to_string()).collect();
                let subject = serde_json::to_value(&request)?;
                state.permissions.insert(id.clone(), (responder, options));
                shared.emit(ProviderEvent::RequestOpened { request_id: id, class: RequestClass::Approval, subject, deadline_ms: None });
                Ok(())
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let shared = shared.clone();
            async move |request: UntypedMessage, responder: Responder<Value>, _connection| {
                if request.method() != "elicitation/create" {
                    return Ok(Handled::No { message: (request, responder), retry: false });
                }
                let mut state = shared.state.lock().unwrap();
                let params = request.params();
                let schema = params.get("requestedSchema");
                // One native session owns this connection. When an extension
                // supplies a session ID, it must name that original session.
                if state.turn.is_none() || shared.stopping.load(Ordering::Acquire)
                    || params.get("sessionId").is_some_and(|id| id.as_str() != state.native_session.as_deref())
                    || state.permissions.len() + state.inputs.len() >= 64
                    || schema.is_none_or(|s| s.get("type").and_then(Value::as_str) != Some("object"))
                    || params.get("mode").is_some_and(|m| m != "form") {
                    responder.respond(serde_json::json!({"action":"cancel"}))?;
                    return Ok(Handled::Yes);
                }
                shared.started(&mut state);
                let id = responder.id().to_string();
                state.inputs.insert(id.clone(), (responder, schema.unwrap().clone()));
                shared.emit(ProviderEvent::RequestOpened { request_id: id, class: RequestClass::Input, subject: params.clone(), deadline_ms: None });
                Ok(Handled::Yes)
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_notification({
            let shared = shared.clone();
            async move |notice: UntypedMessage, _connection| {
                if notice.method().trim_start_matches('_') == "cognition.ai/agent_stopped" {
                    let cause = notice.params().get("cause").and_then(Value::as_str);
                    let state = shared.state.lock().unwrap();
                    if cause.is_some_and(|c| c != "complete" && !(c == "cancelled" && state.turn.as_ref().is_some_and(|t| t.cancel_requested))) {
                        shared.emit(ProviderEvent::ProviderFailed { detail: "Devin reported an external stop; turn outcome uncertain".into() });
                        shared.commands.close();
                        let _ = shared.stop.try_send(());
                    }
                }
                Ok(())
            }
        }, agent_client_protocol::on_receive_notification!())
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            connection.send_request(InitializeRequest::new(ProtocolVersion::V1).client_capabilities(ClientCapabilities::new().terminal(false)).client_info(Implementation::new("goddard-runtime", env!("CARGO_PKG_VERSION")))).block_task().await?;
            let session = connection.send_request(NewSessionRequest::new(cwd)).block_task().await?;
            let config = configure(&connection, &session.session_id, session.config_options.unwrap_or_default(), &requested).await?;
            let effective = initial_config(&session.session_id.to_string(), &config, shared.owned_pid.load(Ordering::Acquire));
            let session_id = session.session_id;
            {
                let mut state = shared.state.lock().unwrap();
                state.native_session = Some(session_id.to_string());
                state.config = config;
                shared.live.store(true, Ordering::Release);
                shared.emit(ProviderEvent::SessionLive);
                shared.emit(ProviderEvent::ConfigActivated { revision: 0, boundary: ActivationBoundary::Immediate, effective });
                let _ = ready.try_send(());
            }
            while let Ok(command) = commands.recv().await {
                if shared.stopping.load(Ordering::Acquire) || shared.overflow.load(Ordering::Acquire) { break; }
                match command {
                    Command::Prompt { turn_id, delivery_id, text } => {
                        let sent = connection.send_request(PromptRequest::new(session_id.clone(), vec![ContentBlock::Text(TextContent::new(text))]));
                        let callback = shared.clone();
                        sent.on_receiving_result(async move |result| {
                            let mut state = callback.state.lock().unwrap();
                            if state.turn.as_ref().is_none_or(|t| t.id != turn_id || t.delivery != delivery_id) { return Ok(()); }
                            if result.is_ok() { callback.started(&mut state); }
                            let turn = state.turn.take().unwrap();
                            match result {
                                Ok(response) if response.stop_reason == StopReason::Cancelled && turn.cancel_requested => callback.emit(ProviderEvent::TurnCancelled { turn_id }),
                                Ok(response) if response.stop_reason == StopReason::EndTurn => callback.emit(ProviderEvent::TurnCompleted { turn_id, result: serde_json::to_value(response)? }),
                                other => {
                                    let detail = match other {
                                        Ok(response) => match response.stop_reason {
                                            StopReason::MaxTokens => "Devin exhausted its context budget",
                                            StopReason::Refusal => "Devin refused the turn",
                                            StopReason::MaxTurnRequests => "Devin exhausted its turn request budget",
                                            StopReason::Cancelled => "Devin cancelled without this runtime requesting cancellation",
                                            _ => "Devin reported an unsupported stop reason",
                                        },
                                        Err(_) => "Devin prompt response failed; delivery/outcome uncertain",
                                    };
                                    callback.emit(ProviderEvent::ProviderFailed { detail: detail.into() });
                                    callback.commands.close();
                                    let _ = callback.stop.try_send(());
                                }
                            }
                            for (_, (responder, _)) in state.inputs.drain() {
                                let _ = responder.respond(serde_json::json!({"action":"cancel"}));
                            }
                            for (_, (responder, _)) in state.permissions.drain() {
                                let _ = responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled));
                            }
                            Ok(())
                        })?;
                    }
                    Command::Configure { revision, target } => {
                        let options = shared.state.lock().unwrap().config.clone();
                        // A failed/ambiguous multi-write cannot claim the old or new
                        // configuration. End transport rather than silently dispatch.
                        let options = configure(&connection, &session_id, options, &target).await?;
                        let mut state = shared.state.lock().unwrap();
                        let effective = initial_config(&session_id.to_string(), &options, shared.owned_pid.load(Ordering::Acquire));
                        state.config = options;
                        state.revision = revision;
                        state.configuring = false;
                        shared.emit(ProviderEvent::ConfigActivated { revision, boundary: ActivationBoundary::Immediate, effective });
                    }
                    Command::Cancel(turn_id) => {
                        let mut state = shared.state.lock().unwrap();
                        if let Some(turn) = &mut state.turn && turn.id == turn_id {
                            connection.send_notification(CancelNotification::new(session_id.clone()))?;
                            turn.cancel_requested = true;
                            for (_, (responder, _)) in state.inputs.drain() {
                                let _ = responder.respond(serde_json::json!({"action":"cancel"}));
                            }
                            for (_, (responder, _)) in state.permissions.drain() {
                                let _ = responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled));
                            }
                        }
                    }
                    Command::InputAnswer { id, response } => {
                        if let Some((responder, _)) = shared.state.lock().unwrap().inputs.remove(&id) {
                            responder.respond(response)?;
                        }
                    }
                    Command::Answer { id, option } => {
                        let mut state = shared.state.lock().unwrap();
                        if let Some((responder, _)) = state.permissions.remove(&id) {
                            responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option))))?;
                            // Successful enqueue on SDK transport is not provider
                            // consumption. No fabricated RequestAnswered event.
                        }
                    }
                }
            }
            Ok(())
        }).await
}

#[cfg(all(test, unix))]
mod tests;
