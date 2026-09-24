//! Agent Client Protocol transport backed by the official Rust SDK.
//!
//! The SDK owns JSON-RPC framing, request IDs, response routing, cancellation,
//! unknown-method errors, stdio lifetime, and protocol type validation. Goddard
//! only adapts typed ACP messages to its provider-neutral [`DriverEvent`]s.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ClientCapabilities, ContentBlock, DeleteSessionRequest, EnvVariable,
    Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest, McpServer,
    McpServerStdio, NewSessionRequest, PermissionOptionKind, PromptRequest, PromptResponse,
    RequestId, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    ResumeSessionRequest, SelectedPermissionOutcome, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigOptionValue, SessionConfigSelectOptions, SessionId,
    SessionModeId, SessionModeState, SessionNotification, SetSessionConfigOptionRequest,
    SetSessionModeRequest, StopReason, TextContent,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, ConnectTo, ConnectionTo, Handled, LineDirection,
    Responder, UntypedMessage,
};
use anyhow::{Context as _, anyhow};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use super::activity;
use crate::driver::{
    DriverControl, DriverEventSender, DriverEventSink, DriverStartOptions, SessionOptions,
};
use crate::model::{
    ActivityKind, DriverEvent, PermissionOption, ProviderKind, ProviderModel, ProviderResumeCursor,
    RuntimeMode, UserInputAnswer, UserInputOption, UserInputQuestion,
};
use waku_protocol::model_catalog::{
    PackedModelSelection, normalize_reasoning_effort, packed_suffix_has, resolve_packed_model,
};

enum CommandMessage {
    Prompt(String),
    Steer(String),
    Cancel,
    Respond {
        request_id: String,
        option_id: String,
    },
    RespondUserInput {
        request_id: String,
        answers: Vec<UserInputAnswer>,
    },
    Options(SessionOptions),
    /// Headless teardown: remove the provider-side session record, then end
    /// the actor. The sender acknowledges the request resolved one way or
    /// the other so the caller is not racing the process's exit.
    DeleteSession(std::sync::mpsc::Sender<()>),
    Shutdown,
}

pub struct AcpDriver {
    commands: smol::channel::Sender<CommandMessage>,
    supports_steer: bool,
    mode: RuntimeMode,
    computer_use: Option<super::support::HeadlessComputerUseRuntime>,
    native_computer_use: Option<super::computer_use::ComputerUseRuntime>,
}

/// Per-provider launch details. Everything after process launch is ACP.
struct AcpLaunch {
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn launch_for(provider: ProviderKind, reasoning_effort: Option<&str>) -> anyhow::Result<AcpLaunch> {
    match provider {
        ProviderKind::Cursor => Ok(AcpLaunch {
            args: vec!["acp".into()],
            env: Vec::new(),
        }),
        ProviderKind::Grok => {
            let mut args = vec!["agent".into()];
            if let Some(effort) = reasoning_effort.filter(|effort| !effort.is_empty()) {
                args.push("--reasoning-effort".into());
                args.push(effort.to_owned());
            }
            args.push("stdio".into());
            Ok(AcpLaunch {
                args,
                env: vec![("GROK_OAUTH2_REFERRER".into(), "waku".into())],
            })
        }
        ProviderKind::Devin => Ok(AcpLaunch {
            args: vec!["acp".into()],
            env: Vec::new(),
        }),
        ProviderKind::Fx => Ok(AcpLaunch {
            args: vec!["acp".into()],
            env: Vec::new(),
        }),
        ProviderKind::Kimi => Ok(AcpLaunch {
            args: vec!["acp".into()],
            env: Vec::new(),
        }),
        // Droid documents its ACP transport as `droid exec --output-format
        // acp`; CLI flags such as --model and --auto are ignored in this mode
        // because sessions are configured over the protocol instead.
        ProviderKind::Droid => Ok(AcpLaunch {
            args: vec!["exec".into(), "--output-format".into(), "acp".into()],
            env: Vec::new(),
        }),
        ProviderKind::OpenCode => Ok(AcpLaunch {
            args: vec!["acp".into()],
            env: Vec::new(),
        }),
        ProviderKind::Goose => Ok(AcpLaunch {
            args: vec!["acp".into()],
            env: Vec::new(),
        }),
        _ => Err(anyhow!(
            "{} does not speak the Agent Client Protocol",
            provider.display_name()
        )),
    }
}

impl AcpDriver {
    pub fn start(
        provider: ProviderKind,
        options: DriverStartOptions,
        events: DriverEventSender,
    ) -> anyhow::Result<Self> {
        let DriverStartOptions {
            binary,
            cwd,
            mode,
            model,
            reasoning_effort,
            service_tier,
            context_window,
            agent_preset: _,
            computer_use_enabled,
            agent: agent_env,
            read_own_transcript: _,
            subagents: _,
            integrations: _,
            provider_cursor,
            eval,
            sandbox,
            allow_model_fallback,
        } = options;
        let fork_context = match &provider_cursor {
            Some(ProviderResumeCursor::Cursor { fork_context, .. }) => fork_context.clone(),
            _ => None,
        };
        let resume_session_id = match provider_cursor {
            Some(cursor) if cursor.provider() == provider => {
                let id = cursor.native_id();
                (!id.is_empty()).then(|| id.to_owned())
            }
            Some(cursor) => {
                return Err(anyhow!(
                    "cannot resume {} from a {} cursor",
                    provider.display_name(),
                    cursor.provider().display_name()
                ));
            }
            None => None,
        };

        let launch = launch_for(provider, reasoning_effort.as_deref())?;
        let computer_use = (provider == ProviderKind::Grok && computer_use_enabled)
            .then(|| super::support::HeadlessComputerUseRuntime::start(provider, events.clone()))
            .transpose()?;
        let native_computer_use = (provider != ProviderKind::Grok && computer_use_enabled)
            .then(|| super::computer_use::ComputerUseRuntime::start(events.clone()))
            .transpose()?;
        let native_computer_use_config = native_computer_use
            .as_ref()
            .map(|runtime| runtime.config.clone());
        let grok_title_home = computer_use
            .as_ref()
            .and_then(super::support::HeadlessComputerUseRuntime::grok_home)
            .map(ToOwned::to_owned);
        let stderr_lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let agent = match &sandbox {
            Some(vm) => guest_transport(
                &binary,
                &cwd,
                launch,
                computer_use.as_ref().map(|runtime| &runtime.config),
                agent_env.as_ref(),
                vm,
                stderr_lines.clone(),
            )?,
            None => AgentTransport::Host(sdk_agent(
                &binary,
                &cwd,
                launch,
                computer_use.as_ref().map(|runtime| &runtime.config),
                agent_env.as_ref(),
                stderr_lines.clone(),
            )?),
        };
        let (commands, command_rx) = smol::channel::unbounded();
        let provider_name = provider.display_name();
        let thread_events = events.clone();

        thread::Builder::new()
            .name(format!("waku-{}-acp", provider.id()))
            .spawn(move || {
                if let Err(error) = crate::command_env::unblock_sigchld_for_current_thread() {
                    let _ = thread_events.send(DriverEvent::Error(format!(
                        "{provider_name}: failed to normalize the provider signal mask: {error}"
                    )));
                    let _ = thread_events.send(DriverEvent::ProcessExited);
                    return;
                }
                let result = smol::block_on(run_sdk_connection(
                    agent,
                    provider,
                    cwd,
                    mode,
                    model,
                    reasoning_effort,
                    service_tier,
                    context_window,
                    allow_model_fallback,
                    resume_session_id,
                    fork_context,
                    grok_title_home,
                    native_computer_use_config,
                    eval,
                    command_rx,
                    thread_events.clone(),
                ));
                if let Err(error) = result {
                    let stderr = super::support::provider_stderr_error(stderr_lines.lock().clone());
                    let detail = stderr.unwrap_or_else(|| error.to_string());
                    let _ = thread_events
                        .send(DriverEvent::Error(format!("{provider_name}: {detail}")));
                }
                let _ = thread_events.send(DriverEvent::ProcessExited);
            })
            .with_context(|| format!("failed to start {provider_name} ACP runtime"))?;

        Ok(Self {
            commands,
            // Droid's behavior under a concurrent session/prompt is
            // unverified, so steering stays off until a live session proves
            // it queues or interleaves safely.
            supports_steer: provider != ProviderKind::Fx && provider != ProviderKind::Droid,
            mode,
            computer_use,
            native_computer_use,
        })
    }
}

/// What `run_sdk_connection` drives — the SDK-managed host process, or a
/// line transport over a process spawned inside the session's sandbox VM.
enum AgentTransport {
    Host(AcpAgent),
    Guest(GuestAcpTransport),
}

type GuestLines = agent_client_protocol::Lines<
    std::pin::Pin<Box<dyn futures::Sink<String, Error = std::io::Error> + Send>>,
    std::pin::Pin<Box<dyn futures::Stream<Item = std::io::Result<String>> + Send>>,
>;

/// The line transport plus the child that owns it: dropping the child kills
/// the in-guest process — the same teardown the SDK's process-group guard
/// gives host spawns.
struct GuestAcpTransport {
    lines: GuestLines,
    _child: crate::sandbox::DriverChild,
}

impl agent_client_protocol::ConnectTo<Client> for AgentTransport {
    async fn connect_to(
        self,
        client: impl agent_client_protocol::ConnectTo<Agent>,
    ) -> agent_client_protocol::Result<()> {
        match self {
            Self::Host(agent) => ConnectTo::<Client>::connect_to(agent, client).await,
            Self::Guest(transport) => {
                ConnectTo::<Client>::connect_to(transport.lines, client).await
            }
        }
    }
}

/// Spawn the ACP agent inside the session's VM and wrap its pipes as the
/// SDK's `Lines` transport: guest stdin writes are channel sends into the
/// VM (never blocking), stdout is pumped line-wise onto a stream, and stderr
/// drains into the shared tail the connection error path reports. The
/// guardian shell is a host-side teardown aid — a guest process dies with
/// its VM, so the spawn is the bare provider argv.
fn guest_transport(
    binary: &Path,
    cwd: &Path,
    mut launch: AcpLaunch,
    computer_use: Option<&super::support::HeadlessComputerUseConfig>,
    agent_env: Option<&crate::agent::AgentLaunchEnv>,
    vm: &Arc<crate::sandbox::ShuruVm>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
) -> anyhow::Result<AgentTransport> {
    use std::io::{BufRead as _, BufReader, Write as _};

    let (computer_args, computer_env) =
        super::support::grok_computer_use_launch_configuration(computer_use);
    let mut environment = crate::command_env::shell_environment()
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<Vec<_>>();
    environment.append(&mut launch.env);
    environment.extend(computer_env);
    if let Some(agent_env) = agent_env {
        crate::command_env::merge_agent_environment(&mut environment, agent_env);
    }
    let mut command = std::process::Command::new(binary);
    command
        .current_dir(cwd)
        .args(launch.args.iter().chain(computer_args.iter()))
        .envs(environment);
    let mut child = crate::sandbox::spawn(&command, Some(vm))
        .context("could not spawn the provider process in the sandbox VM")?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("sandboxed {} stdin unavailable", binary.display()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("sandboxed {} stdout unavailable", binary.display()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("sandboxed {} stderr unavailable", binary.display()))?;

    // Mirror the SDK's capped stderr tail so connection errors report the
    // provider's own diagnostics.
    thread::Builder::new()
        .name("waku-acp-guest-stderr".into())
        .spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let mut lines = stderr_lines.lock();
                if lines.len() == 128 {
                    lines.remove(0);
                }
                lines.push(line.to_owned());
            }
        })
        .context("could not start the sandbox stderr drain")?;

    let (lines_tx, lines_rx) = futures::channel::mpsc::unbounded();
    thread::Builder::new()
        .name("waku-acp-guest-stdout".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let done = line.is_err();
                if lines_tx.unbounded_send(line).is_err() || done {
                    break;
                }
            }
        })
        .context("could not start the sandbox stdout pump")?;

    let incoming: std::pin::Pin<Box<dyn futures::Stream<Item = std::io::Result<String>> + Send>> =
        Box::pin(lines_rx);
    let outgoing: std::pin::Pin<Box<dyn futures::Sink<String, Error = std::io::Error> + Send>> =
        Box::pin(futures::sink::unfold(
            stdin,
            |mut writer, line: String| async move {
                writer.write_all(line.as_bytes())?;
                writer.write_all(b"\n")?;
                Ok::<_, std::io::Error>(writer)
            },
        ));

    Ok(AgentTransport::Guest(GuestAcpTransport {
        lines: agent_client_protocol::Lines::new(outgoing, incoming),
        _child: child,
    }))
}

fn sdk_agent(
    binary: &Path,
    cwd: &Path,
    mut launch: AcpLaunch,
    computer_use: Option<&super::support::HeadlessComputerUseConfig>,
    agent_env: Option<&crate::agent::AgentLaunchEnv>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
) -> anyhow::Result<AcpAgent> {
    let binary = binary
        .to_str()
        .ok_or_else(|| anyhow!("the ACP executable path is not valid UTF-8"))?;
    let cwd = cwd
        .to_str()
        .ok_or_else(|| anyhow!("the ACP working directory is not valid UTF-8"))?;
    let (computer_args, computer_env) =
        super::support::grok_computer_use_launch_configuration(computer_use);
    launch.args.extend(computer_args);
    let mut environment = crate::command_env::shell_environment()
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect::<Vec<_>>();
    environment.append(&mut launch.env);
    environment.extend(computer_env);
    if let Some(agent_env) = agent_env {
        crate::command_env::merge_agent_environment(&mut environment, agent_env);
    }

    // `AcpAgentConfig` deliberately contains only argv and environment. macOS
    // `env -C` supplies the session cwd without a shell, preserving exact
    // argument boundaries and the SDK's process-group lifecycle management.
    // On unix a guardian shell watches the daemon pid so a crashed daemon
    // cannot orphan the agent.
    #[cfg(unix)]
    let (program, args) = {
        let mut args = vec![
            "-c".to_owned(),
            crate::command_env::DAEMON_GUARDIAN_SCRIPT.to_owned(),
            "waku-acp-guardian".to_owned(),
            "/usr/bin/env".to_owned(),
            "-C".to_owned(),
            cwd.to_owned(),
            binary.to_owned(),
        ];
        args.extend(launch.args);
        ("/bin/sh", args)
    };
    #[cfg(not(unix))]
    let (program, args) = {
        let mut args = vec!["-C".to_owned(), cwd.to_owned(), binary.to_owned()];
        args.extend(launch.args);
        ("/usr/bin/env", args)
    };
    let config = AcpAgentConfig::new(program).args(args).envs(environment);
    Ok(AcpAgent::new(config).with_debug(move |line, direction| {
        if direction != LineDirection::Stderr || line.trim().is_empty() {
            return;
        }
        let mut lines = stderr_lines.lock();
        if lines.len() == 128 {
            lines.remove(0);
        }
        lines.push(line.to_owned());
    }))
}

/// Builds a short-lived ACP process for session discovery or history replay.
///
/// Catalog work runs on the daemon request thread, never a render path. It
/// intentionally shares the production launch contract so provider argv and
/// environment quirks cannot drift between a resumed task and the picker that
/// discovered it.
pub(crate) fn catalog_agent(
    provider: ProviderKind,
    binary: &Path,
    cwd: &Path,
) -> anyhow::Result<AcpAgent> {
    let launch = launch_for(provider, None)?;
    sdk_agent(
        binary,
        cwd,
        launch,
        None,
        None,
        Arc::new(Mutex::new(Vec::new())),
    )
}

const DEVIN_ACP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// Devin's ACP session advertises the models it will actually accept. The CLI
/// `devin models list` catalog is the interactive product list and includes
/// ids such as `adaptive` that this agent rejects with "Model not found".
pub(crate) fn discover_devin_models_via_acp(binary: &Path) -> Vec<ProviderModel> {
    let Ok(cwd) = crate::acp_session::catalog_working_directory() else {
        return Vec::new();
    };
    let Ok(agent) = catalog_agent(ProviderKind::Devin, binary, &cwd) else {
        return Vec::new();
    };
    let request = Client
        .builder()
        .name("waku-devin-model-discovery")
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            connection
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(ClientCapabilities::new().terminal(false))
                        .client_info(Implementation::new("waku", env!("CARGO_PKG_VERSION"))),
                )
                .block_task()
                .await?;
            let response = connection
                .send_request(NewSessionRequest::new(cwd.clone()))
                .block_task()
                .await?;
            let models = models_from_session_config_options(
                response.config_options.as_deref().unwrap_or_default(),
            );
            let _ = connection
                .send_request(DeleteSessionRequest::new(response.session_id))
                .block_task()
                .await;
            Ok(models)
        });
    smol::block_on(smol::future::race(
        async move { request.await.map_err(|_| ()) },
        async move {
            smol::Timer::after(DEVIN_ACP_DISCOVERY_TIMEOUT).await;
            Err(())
        },
    ))
    .ok()
    .unwrap_or_default()
}

type PermissionResponder = Responder<RequestPermissionResponse>;
type PendingPermissions = Arc<Mutex<HashMap<String, PermissionResponder>>>;

#[derive(Clone, Copy)]
enum AcpUserInputKind {
    Cursor,
    Xai,
}

struct PendingAcpUserInput {
    kind: AcpUserInputKind,
    params: Value,
    responder: Responder<Value>,
}

type PendingAcpUserInputs = Arc<Mutex<HashMap<String, PendingAcpUserInput>>>;

/// In-flight `session/prompt` requests plus the stop signals that decide
/// how their settle should be scored. `session/cancel` is the only stop a
/// client is expected to cause, so a `cancelled` resolution it never asked
/// for means the provider interrupted the turn itself.
#[derive(Default)]
struct PendingPrompts {
    requests: Vec<PendingPrompt>,
    cancel_requested: bool,
    /// A sibling request already resolved cleanly: a `cancelled` settle
    /// after that is the preemption receipt, not the turn's outcome.
    saw_clean_settle: bool,
    /// Devin's `_cognition.ai/agent_stopped` cause, captured while requests
    /// are in flight — the harness's own verdict on why the run stopped.
    stop_cause: Option<String>,
}

struct PendingPrompt {
    request_id: RequestId,
    extension_id: Option<String>,
    session_id: String,
}

/// What the last settle of a prompt batch learned about how it ended.
#[derive(Default)]
struct PromptSettle {
    /// This client sent `session/cancel`: a `cancelled` stop reason is the
    /// user's own stop, not a provider failure.
    cancel_requested: bool,
    saw_clean_settle: bool,
    stop_cause: Option<String>,
}

impl PendingPrompts {
    fn insert(&mut self, request_id: RequestId, extension_id: Option<String>, session_id: String) {
        self.requests.push(PendingPrompt {
            request_id,
            extension_id,
            session_id,
        });
    }

    fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    fn note_cancel_requested(&mut self) {
        if !self.requests.is_empty() {
            self.cancel_requested = true;
        }
    }

    fn note_agent_stop(&mut self, cause: &str) {
        if !self.requests.is_empty() {
            self.stop_cause = Some(cause.to_owned());
        }
    }

    /// Removes the request and returns the batch's stop context once this
    /// settle empties it — `None` while other prompts remain in flight.
    fn settle_request(&mut self, request_id: &RequestId, clean: bool) -> Option<PromptSettle> {
        let index = self
            .requests
            .iter()
            .position(|prompt| &prompt.request_id == request_id)?;
        self.requests.remove(index);
        self.saw_clean_settle |= clean;
        self.take_settle()
    }

    fn settle_extension(
        &mut self,
        session_id: &str,
        extension_id: Option<&str>,
    ) -> Option<PromptSettle> {
        let index = self.requests.iter().position(|prompt| {
            prompt.session_id == session_id
                && extension_id
                    .is_none_or(|extension_id| prompt.extension_id.as_deref() == Some(extension_id))
        })?;
        self.requests.remove(index);
        self.take_settle()
    }

    fn take_settle(&mut self) -> Option<PromptSettle> {
        self.requests.is_empty().then(|| PromptSettle {
            cancel_requested: std::mem::take(&mut self.cancel_requested),
            saw_clean_settle: std::mem::take(&mut self.saw_clean_settle),
            stop_cause: self.stop_cause.take(),
        })
    }
}

type PendingPromptRequests = Arc<Mutex<PendingPrompts>>;

/// How a permission request from the provider gets answered without the
/// user. `Auto` routes through the evaluation review when the backend is
/// configured and asks like `Ask` when it is not.
#[derive(Clone)]
enum PermissionDisposition {
    /// Pick an allow option immediately.
    AutoApprove,
    /// Run the evaluation-model review first; a cleared request answers an
    /// allow-once option and anything else escalates to the user.
    Review(Arc<waku_protocol::eval::EvalSettings>),
    /// Emit the permission prompt.
    Prompt,
}

fn permission_disposition(
    provider: ProviderKind,
    mode: RuntimeMode,
    eval: Option<waku_protocol::eval::EvalSettings>,
) -> PermissionDisposition {
    match mode {
        RuntimeMode::FullAccess => PermissionDisposition::AutoApprove,
        // Providers running their own review escalate held actions to the
        // user; the rest keep the legacy blanket answer.
        RuntimeMode::AutoAcceptEdits if crate::permission_review::reviews_natively(provider) => {
            PermissionDisposition::Prompt
        }
        RuntimeMode::AutoAcceptEdits => PermissionDisposition::AutoApprove,
        RuntimeMode::Auto => {
            if crate::permission_review::reviews_natively(provider) {
                PermissionDisposition::Prompt
            } else {
                match eval {
                    Some(eval) => PermissionDisposition::Review(Arc::new(eval)),
                    None => PermissionDisposition::Prompt,
                }
            }
        }
        RuntimeMode::Ask => PermissionDisposition::Prompt,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_sdk_connection(
    agent: AgentTransport,
    provider: ProviderKind,
    cwd: std::path::PathBuf,
    mode: RuntimeMode,
    model: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    context_window: Option<String>,
    allow_model_fallback: bool,
    resume_session_id: Option<String>,
    fork_context: Option<String>,
    grok_title_home: Option<std::path::PathBuf>,
    native_computer_use: Option<super::computer_use::ComputerUseConfig>,
    eval: Option<waku_protocol::eval::EvalSettings>,
    commands: smol::channel::Receiver<CommandMessage>,
    events: DriverEventSender,
) -> agent_client_protocol::Result<()> {
    let suppress_session_updates = Arc::new(AtomicBool::new(false));
    let stream_state = Arc::new(Mutex::new(AcpStreamState::default()));
    let pending_permissions: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));
    let pending_user_inputs: PendingAcpUserInputs = Arc::new(Mutex::new(HashMap::new()));
    let prompt_requests = Arc::new(Mutex::new(PendingPrompts::default()));
    let title_refresh = super::title_refresh::NativeTitleRefresh::default();
    let first_prompt = Arc::new(Mutex::new(None::<String>));
    let disposition = permission_disposition(provider, mode, eval);

    Client
        .builder()
        .name("waku")
        .on_receive_notification(
            {
                let events = events.clone();
                let suppress_session_updates = suppress_session_updates.clone();
                let stream_state = stream_state.clone();
                let first_prompt = first_prompt.clone();
                async move |notification: SessionNotification, _connection| {
                    if !suppress_session_updates.load(Ordering::Acquire) {
                        handle_session_update(
                            provider,
                            notification,
                            &events,
                            &mut stream_state.lock(),
                            first_prompt.lock().as_deref(),
                        )?;
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_notification(
            {
                let events = events.clone();
                let prompt_requests = prompt_requests.clone();
                let grok_title_home = grok_title_home.clone();
                let title_refresh = title_refresh.clone();
                let first_prompt = first_prompt.clone();
                async move |notification: UntypedMessage, _connection| {
                    // Devin reports every agent-run stop with its own cause
                    // (`complete`, `cancelled`, external reasons); kept so
                    // the prompt settle can tell an external stop from a
                    // clean end or this client's own cancel.
                    if notification.method().trim_start_matches('_') == "cognition.ai/agent_stopped"
                        && let Some(cause) =
                            notification.params().get("cause").and_then(Value::as_str)
                    {
                        prompt_requests.lock().note_agent_stop(cause);
                    }
                    if notification.method() == "_x.ai/session/prompt_complete" {
                        if let Some(session_id) = finish_xai_prompt_complete(
                            notification.params(),
                            &prompt_requests,
                            &events,
                        ) {
                            start_grok_title_refresh(
                                grok_title_home.as_deref(),
                                &session_id,
                                &title_refresh,
                                events.clone(),
                            );
                        }
                    }
                    if let Some(title) = crate::devin_session::title_from_notification(
                        notification.method(),
                        notification.params(),
                    ) {
                        if !crate::devin_session::is_placeholder_title(
                            &title,
                            first_prompt.lock().as_deref(),
                        ) {
                            let _ = events.send(DriverEvent::AutoTitleUpdated(Some(title)));
                        }
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let events = events.clone();
                let pending_permissions = pending_permissions.clone();
                async move |request: RequestPermissionRequest, responder, _connection| {
                    handle_permission_request(
                        request,
                        responder,
                        provider,
                        &disposition,
                        &pending_permissions,
                        &events,
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let events = events.clone();
                let pending = pending_user_inputs.clone();
                async move |request: UntypedMessage, responder, _connection| {
                    let kind = match request.method() {
                        "cursor/ask_question" => AcpUserInputKind::Cursor,
                        "_x.ai/ask_user_question" | "x.ai/ask_user_question" => {
                            AcpUserInputKind::Xai
                        }
                        _ => {
                            return Ok(Handled::No {
                                message: (request, responder),
                                retry: false,
                            });
                        }
                    };
                    let request_id = responder.id().to_string();
                    let params = match kind {
                        AcpUserInputKind::Cursor => request.params().clone(),
                        AcpUserInputKind::Xai => {
                            unwrap_xai_question_params(request.params()).clone()
                        }
                    };
                    let questions = match kind {
                        AcpUserInputKind::Cursor => cursor_user_input_questions(&params),
                        AcpUserInputKind::Xai => xai_user_input_questions(&params),
                    };
                    if questions.is_empty() {
                        responder.respond(cancelled_user_input_response(kind))?;
                        return Ok(Handled::Yes);
                    }
                    pending.lock().insert(
                        request_id.clone(),
                        PendingAcpUserInput {
                            kind,
                            params,
                            responder,
                        },
                    );
                    if events
                        .send(DriverEvent::UserInputRequested {
                            request_id: request_id.clone(),
                            questions,
                        })
                        .is_err()
                        && let Some(pending) = pending.lock().remove(&request_id)
                    {
                        let _ = pending
                            .responder
                            .respond(cancelled_user_input_response(pending.kind));
                    }
                    Ok(Handled::Yes)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            let mut client_capabilities = ClientCapabilities::new().terminal(false);
            if provider == ProviderKind::Cursor {
                // Cursor only exposes its parameterized model controls to
                // clients that opt in. Goddard applies the returned config option
                // ids rather than assuming Cursor's private ids stay stable.
                let mut meta = Map::new();
                meta.insert("parameterizedModelPicker".to_owned(), Value::Bool(true));
                client_capabilities = client_capabilities.meta(meta);
            }
            let initialize = connection
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(client_capabilities)
                        .client_info(Implementation::new("waku", env!("CARGO_PKG_VERSION"))),
                )
                .block_task()
                .await?;
            let (session_id, modes, config_options) = establish_session(
                &connection,
                &initialize,
                resume_session_id.as_deref(),
                &cwd,
                &suppress_session_updates,
                native_computer_use.as_ref(),
            )
            .await?;

            if let Some(mode_id) = desired_access_mode(provider, modes.as_ref(), mode) {
                // Mode selection is opportunistic: an agent can advertise a
                // mode but reject a later transition without invalidating the
                // session itself.
                let _ = connection
                    .send_request(SetSessionModeRequest::new(session_id.clone(), mode_id))
                    .block_task()
                    .await;
            }
            let native_session_id = session_id.to_string();
            let _ = events.send(DriverEvent::Connected {
                provider_cursor: Some(ProviderResumeCursor::from_session_id(
                    provider,
                    native_session_id.clone(),
                )),
            });
            if provider == ProviderKind::Devin && resume_session_id.is_some() {
                start_devin_title_refresh(&native_session_id, None, &title_refresh, events.clone());
            }

            let mut current_model = model;
            let mut current_effort = reasoning_effort;
            let mut current_tier = service_tier;
            let mut current_window = context_window;
            apply_model(
                &connection,
                provider,
                &session_id,
                config_options.as_deref(),
                current_model.as_deref(),
                current_effort.as_deref(),
                current_tier.as_deref(),
                current_window.as_deref(),
                allow_model_fallback,
                &events,
            )
            .await;
            let mut fork_context = fork_context;
            while let Ok(command) = commands.recv().await {
                match command {
                    CommandMessage::Prompt(text) => {
                        let text = fork_context
                            .take()
                            .map(|context| {
                                crate::cursor_session::prompt_with_fork_context(&context, &text)
                            })
                            .unwrap_or(text);
                        let title_placeholder = if provider == ProviderKind::Devin {
                            let mut first = first_prompt.lock();
                            if first.is_none() {
                                *first = Some(text.clone());
                                start_devin_title_refresh(
                                    &native_session_id,
                                    first.clone(),
                                    &title_refresh,
                                    events.clone(),
                                );
                            }
                            first.clone()
                        } else {
                            None
                        };
                        let _ = events.send(DriverEvent::TurnStarted);
                        if let Err(error) = send_prompt(
                            &connection,
                            &session_id,
                            text,
                            &prompt_requests,
                            &events,
                            provider,
                            &native_session_id,
                            grok_title_home.clone(),
                            title_placeholder,
                            title_refresh.clone(),
                            stream_state.clone(),
                        ) {
                            let _ = events.send(DriverEvent::Error(error.to_string()));
                            let _ = events.send(DriverEvent::TurnFinished {
                                success: false,
                                summary: None,
                                summary_i18n: None,
                            });
                        }
                    }
                    CommandMessage::Steer(text) => {
                        if prompt_requests.lock().is_empty() {
                            let _ = events.send(DriverEvent::SteerRejected {
                                message: text,
                                reason: format!(
                                    "{} has no active turn to steer.",
                                    provider.display_name()
                                ),
                                reason_i18n: None,
                                hidden: false,
                            });
                            continue;
                        }
                        match send_prompt(
                            &connection,
                            &session_id,
                            text.clone(),
                            &prompt_requests,
                            &events,
                            provider,
                            &native_session_id,
                            grok_title_home.clone(),
                            first_prompt.lock().clone(),
                            title_refresh.clone(),
                            stream_state.clone(),
                        ) {
                            Ok(()) => {
                                let _ = events.send(DriverEvent::SteerAccepted {
                                    message: text,
                                    sent_by_task: None,
                                    hidden: false,
                                });
                            }
                            Err(error) => {
                                let _ = events.send(DriverEvent::SteerRejected {
                                    message: text,
                                    reason: error.to_string(),
                                    reason_i18n: None,
                                    hidden: false,
                                });
                            }
                        }
                    }
                    CommandMessage::Cancel => {
                        let _ = connection
                            .send_notification(CancelNotification::new(session_id.clone()));
                        prompt_requests.lock().note_cancel_requested();
                        cancel_pending_permissions(&pending_permissions);
                        cancel_pending_user_inputs(&pending_user_inputs);
                    }
                    CommandMessage::Respond {
                        request_id,
                        option_id,
                    } => {
                        if let Some(responder) = pending_permissions.lock().remove(&request_id) {
                            let _ = responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                    option_id,
                                )),
                            ));
                        }
                    }
                    CommandMessage::RespondUserInput {
                        request_id,
                        answers,
                    } => {
                        if let Some(pending) = pending_user_inputs.lock().remove(&request_id) {
                            let response = match pending.kind {
                                AcpUserInputKind::Cursor => {
                                    cursor_user_input_response(&pending.params, &answers)
                                }
                                AcpUserInputKind::Xai => {
                                    xai_user_input_response(&pending.params, &answers)
                                }
                            };
                            let _ = pending.responder.respond(response);
                        }
                    }
                    CommandMessage::Options(options) => {
                        if options.model != current_model
                            || options.reasoning_effort != current_effort
                            || options.service_tier != current_tier
                            || options.context_window != current_window
                        {
                            current_model = options.model;
                            current_effort = options.reasoning_effort;
                            current_tier = options.service_tier;
                            current_window = options.context_window;
                            apply_model(
                                &connection,
                                provider,
                                &session_id,
                                config_options.as_deref(),
                                current_model.as_deref(),
                                current_effort.as_deref(),
                                current_tier.as_deref(),
                                current_window.as_deref(),
                                allow_model_fallback,
                                &events,
                            )
                            .await;
                        }
                    }
                    CommandMessage::DeleteSession(done) => {
                        match connection
                            .send_request(DeleteSessionRequest::new(session_id.clone()))
                            .block_task()
                            .await
                        {
                            Ok(_) => {}
                            // Agents that predate `session/delete` answer
                            // -32601 — the residue stays, nothing to do.
                            Err(error) if is_missing_acp_method(&error) => {}
                            Err(error) => {
                                eprintln!(
                                    "{} session delete failed: {error}",
                                    provider.display_name()
                                );
                            }
                        }
                        let _ = done.send(());
                        break;
                    }
                    CommandMessage::Shutdown => break,
                }
            }
            cancel_pending_permissions(&pending_permissions);
            cancel_pending_user_inputs(&pending_user_inputs);
            Ok(())
        })
        .await
}

async fn establish_session(
    connection: &ConnectionTo<Agent>,
    initialize: &InitializeResponse,
    resume_session_id: Option<&str>,
    cwd: &Path,
    suppress_session_updates: &AtomicBool,
    computer_use: Option<&super::computer_use::ComputerUseConfig>,
) -> agent_client_protocol::Result<(
    SessionId,
    Option<SessionModeState>,
    Option<Vec<SessionConfigOption>>,
)> {
    if let Some(existing) = resume_session_id {
        let mut resume_error = None;
        if initialize
            .agent_capabilities
            .session_capabilities
            .resume
            .is_some()
        {
            match connection
                .send_request(
                    ResumeSessionRequest::new(existing.to_owned(), cwd)
                        .mcp_servers(computer_use_mcp_servers(computer_use)),
                )
                .block_task()
                .await
            {
                Ok(response) => {
                    return Ok((
                        SessionId::new(existing.to_owned()),
                        response.modes,
                        response.config_options,
                    ));
                }
                Err(error) => resume_error = Some(error),
            }
        }

        if initialize.agent_capabilities.load_session {
            suppress_session_updates.store(true, Ordering::Release);
            // A runtime this session just replaced — a worktree move resets
            // it — may still hold the provider-side session lock while its
            // process shuts down, so retry failures the agent flags as
            // retryable before giving up.
            let mut response = connection
                .send_request(
                    LoadSessionRequest::new(existing.to_owned(), cwd)
                        .mcp_servers(computer_use_mcp_servers(computer_use)),
                )
                .block_task()
                .await;
            for _ in 0..ACP_SESSION_LOAD_RETRIES {
                if response
                    .as_ref()
                    .err()
                    .is_none_or(|e| !session_error_retryable(e))
                {
                    break;
                }
                smol::Timer::after(ACP_SESSION_LOAD_RETRY_DELAY).await;
                response = connection
                    .send_request(
                        LoadSessionRequest::new(existing.to_owned(), cwd)
                            .mcp_servers(computer_use_mcp_servers(computer_use)),
                    )
                    .block_task()
                    .await;
            }
            suppress_session_updates.store(false, Ordering::Release);
            return match response {
                Ok(response) => Ok((
                    SessionId::new(existing.to_owned()),
                    response.modes,
                    response.config_options,
                )),
                // A failed resume must not fall through to session/new: that
                // would silently fork the task onto an empty provider
                // session and overwrite its resume cursor with the new id.
                Err(error) => Err(error),
            };
        }

        if let Some(error) = resume_error {
            return Err(error);
        }
    }

    let response = connection
        .send_request(
            NewSessionRequest::new(cwd).mcp_servers(computer_use_mcp_servers(computer_use)),
        )
        .block_task()
        .await?;
    Ok((response.session_id, response.modes, response.config_options))
}

fn computer_use_mcp_servers(
    config: Option<&super::computer_use::ComputerUseConfig>,
) -> Vec<McpServer> {
    config
        .map(|config| {
            vec![McpServer::Stdio(
                McpServerStdio::new("goddard_js_repl", config.repl_path.clone()).env(vec![
                    EnvVariable::new(
                        "GODDARD_COMPUTER_USE_SERVER",
                        config.server_path.to_string_lossy().into_owned(),
                    ),
                    EnvVariable::new(
                        "GODDARD_COMPUTER_USE_PROCESS_DIRECTORY",
                        config.process_directory.to_string_lossy().into_owned(),
                    ),
                ]),
            )]
        })
        .unwrap_or_default()
}

const ACP_SESSION_LOAD_RETRIES: usize = 6;
const ACP_SESSION_LOAD_RETRY_DELAY: Duration = Duration::from_millis(400);

/// Agents flag transient resume failures in the JSON-RPC error `data` — e.g.
/// Devin's `session_locked`, while a replaced runtime's process still holds
/// the session — with a `retryable` marker, namespaced or not.
fn session_error_retryable(error: &agent_client_protocol::Error) -> bool {
    error
        .data
        .as_ref()
        .and_then(Value::as_object)
        .is_some_and(|data| {
            data.iter().any(|(key, value)| {
                key.rsplit('/').next() == Some("retryable") && value.as_bool() == Some(true)
            })
        })
}

fn desired_access_mode(
    provider: ProviderKind,
    modes: Option<&SessionModeState>,
    mode: RuntimeMode,
) -> Option<SessionModeId> {
    let modes = modes?;
    let desired = if provider == ProviderKind::Droid {
        // Droid's autonomy modes are its access modes: every Goddard mode maps
        // onto one advertised autonomy level instead of only rescuing a
        // legacy plan state.
        let desired = match mode {
            RuntimeMode::Ask => "normal",
            RuntimeMode::AutoAcceptEdits => "auto-low",
            RuntimeMode::Auto => "auto-medium",
            RuntimeMode::FullAccess => "auto-high",
        };
        modes
            .available_modes
            .iter()
            .find(|available| available.id.to_string().eq_ignore_ascii_case(desired))?
            .id
            .clone()
    } else if provider == ProviderKind::Fx {
        let desired = if mode == RuntimeMode::Ask {
            "ask"
        } else {
            "code"
        };
        modes
            .available_modes
            .iter()
            .find(|mode| mode.id.to_string().eq_ignore_ascii_case(desired))?
            .id
            .clone()
    } else {
        // Sessions created before the interaction toggle was removed may
        // retain the provider's read-only mode. Return only those sessions to
        // the provider's ordinary execution mode; otherwise leave externally
        // selected native modes untouched.
        if !modes
            .current_mode_id
            .to_string()
            .eq_ignore_ascii_case("plan")
        {
            return None;
        }
        modes
            .available_modes
            .iter()
            .find(|mode| {
                let id = mode.id.to_string();
                id.eq_ignore_ascii_case("agent") || id.eq_ignore_ascii_case("default")
            })?
            .id
            .clone()
    };
    (modes.current_mode_id != desired).then_some(desired)
}

/// Which session config option carries reasoning effort. ACP leaves the id to
/// the agent: Kimi Code exposes it as its `thinking` level, Droid as its
/// `reasoning_effort` level, while the other agents Goddard drives keep it on
/// `mode`. Grok does not use this path: its effort rides on `session/set_model`
/// as `_meta.reasoningEffort`. Devin is excluded from the generic call because
/// its `mode` option is a permission mode, not effort.
fn reasoning_effort_config_id(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Kimi => "thinking",
        ProviderKind::Droid => "reasoning_effort",
        _ => "mode",
    }
}

fn session_config_select_entries(option: &SessionConfigOption) -> Vec<(&str, &str)> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return Vec::new();
    };
    match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .map(|option| (option.value.0.as_ref(), option.name.as_str()))
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|option| (option.value.0.as_ref(), option.name.as_str()))
            .collect(),
        _ => Vec::new(),
    }
}

fn session_config_select_values(option: &SessionConfigOption) -> Vec<&str> {
    session_config_select_entries(option)
        .into_iter()
        .map(|(value, _)| value)
        .collect()
}

fn cursor_model_selection(
    option: &SessionConfigOption,
    requested: &str,
) -> Option<PackedModelSelection> {
    resolve_packed_model(
        session_config_select_values(option),
        requested,
        ProviderKind::Cursor,
    )
}

fn cursor_option_id(option: &SessionConfigOption) -> String {
    option.id.to_string().to_ascii_lowercase()
}

fn cursor_option_name(option: &SessionConfigOption) -> String {
    option.name.to_ascii_lowercase()
}

fn is_cursor_thinking_option(option: &SessionConfigOption) -> bool {
    option.category == Some(SessionConfigOptionCategory::ModelConfig) && {
        let id = cursor_option_id(option);
        let name = cursor_option_name(option);
        id == "thinking" || name.contains("thinking")
    }
}

fn is_cursor_fast_option(option: &SessionConfigOption) -> bool {
    option.category == Some(SessionConfigOptionCategory::ModelConfig) && {
        let id = cursor_option_id(option);
        let name = cursor_option_name(option);
        id == "fast" || name == "fast" || name.contains("fast mode")
    }
}

fn is_cursor_context_option(option: &SessionConfigOption) -> bool {
    option.category == Some(SessionConfigOptionCategory::ModelConfig) && {
        let id = cursor_option_id(option);
        let name = cursor_option_name(option);
        id == "context" || id == "context_size" || name.contains("context")
    }
}

fn is_cursor_effort_option(option: &SessionConfigOption) -> bool {
    if !matches!(option.kind, SessionConfigKind::Select(_)) {
        return false;
    }
    let id = cursor_option_id(option);
    let name = cursor_option_name(option);
    id == "effort"
        || id == "reasoning"
        || name == "effort"
        || name == "reasoning"
        || name.contains("effort")
        || name.contains("reasoning")
}

fn find_cursor_effort_option(options: &[SessionConfigOption]) -> Option<&SessionConfigOption> {
    let candidates: Vec<&SessionConfigOption> = options
        .iter()
        .filter(|option| is_cursor_effort_option(option))
        .collect();
    candidates
        .iter()
        .copied()
        .find(|option| {
            matches!(
                option.category.as_ref(),
                Some(SessionConfigOptionCategory::Other(value))
                    if value.eq_ignore_ascii_case("model_option")
            )
        })
        .or_else(|| {
            candidates
                .iter()
                .copied()
                .find(|option| option.id.to_string().eq_ignore_ascii_case("effort"))
        })
        .or_else(|| {
            candidates
                .iter()
                .copied()
                .find(|option| option.category == Some(SessionConfigOptionCategory::ThoughtLevel))
        })
        .or_else(|| candidates.first().copied())
}

fn cursor_matching_select_value<'a>(values: &[&'a str], requested: &str) -> Option<&'a str> {
    let normalized = normalize_reasoning_effort(requested);
    values
        .iter()
        .find(|value| normalize_reasoning_effort(value) == normalized)
        .copied()
}

fn cursor_desired_effort_value(
    option: &SessionConfigOption,
    selection: &PackedModelSelection,
    reasoning_effort: Option<&str>,
) -> Option<String> {
    let values = session_config_select_values(option);
    if let Some(effort) = reasoning_effort.filter(|effort| !effort.is_empty())
        && let Some(value) = cursor_matching_select_value(&values, effort)
    {
        return Some(value.to_owned());
    }
    if selection.suffix.contains("extra-high")
        && let Some(value) = values
            .iter()
            .find(|value| normalize_reasoning_effort(value) == "xhigh")
    {
        return Some((*value).to_owned());
    }
    values
        .iter()
        .find(|value| packed_suffix_has(&selection.suffix, value))
        .map(|value| (*value).to_owned())
}

fn cursor_desired_thinking(
    selection: &PackedModelSelection,
    reasoning_effort: Option<&str>,
) -> Option<bool> {
    if let Some(effort) = reasoning_effort {
        return Some(normalize_reasoning_effort(effort) != "none");
    }
    packed_suffix_has(&selection.suffix, "thinking").then_some(true)
}

fn cursor_desired_fast(
    selection: &PackedModelSelection,
    service_tier: Option<&str>,
) -> Option<bool> {
    match service_tier {
        Some("fast") => Some(true),
        Some(_) => Some(false),
        None if packed_suffix_has(&selection.suffix, "fast") => Some(true),
        None => None,
    }
}

fn cursor_desired_context_value(
    option: &SessionConfigOption,
    context_window: Option<&str>,
) -> Option<String> {
    let requested = context_window.filter(|value| !value.is_empty())?;
    let values = session_config_select_values(option);
    let normalized = requested.replace(['_', ' '], "-");
    values
        .iter()
        .find(|value| {
            value.eq_ignore_ascii_case(requested)
                || value
                    .replace(['_', ' '], "-")
                    .eq_ignore_ascii_case(&normalized)
        })
        .map(|value| (*value).to_owned())
}

fn cursor_flag_request_value(
    option: &SessionConfigOption,
    enabled: bool,
) -> Option<SessionConfigOptionValue> {
    match &option.kind {
        SessionConfigKind::Boolean(_) => Some(SessionConfigOptionValue::boolean(enabled)),
        SessionConfigKind::Select(_) => {
            let value = if enabled { "true" } else { "false" };
            session_config_select_values(option)
                .contains(&value)
                .then(|| SessionConfigOptionValue::value_id(value))
        }
        _ => None,
    }
}

fn cursor_flag_matches(option: &SessionConfigOption, enabled: bool) -> bool {
    match &option.kind {
        SessionConfigKind::Boolean(boolean) => boolean.current_value == enabled,
        SessionConfigKind::Select(_) => {
            let value = if enabled { "true" } else { "false" };
            session_config_current_value(option) == Some(value)
        }
        _ => false,
    }
}

fn session_config_current_value(option: &SessionConfigOption) -> Option<&str> {
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    Some(select.current_value.0.as_ref())
}

async fn apply_cursor_variant_configs(
    connection: &ConnectionTo<Agent>,
    session_id: &SessionId,
    mut options: Vec<SessionConfigOption>,
    selection: &PackedModelSelection,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    context_window: Option<&str>,
) -> agent_client_protocol::Result<()> {
    // Thinking can reveal a thought-level option, so apply it first and use
    // each response's refreshed option set for the next selection.
    for target in ["thinking", "effort", "context", "fast"] {
        let Some(option) = (match target {
            "thinking" => options
                .iter()
                .find(|option| is_cursor_thinking_option(option)),
            "effort" => find_cursor_effort_option(&options),
            "context" => options
                .iter()
                .find(|option| is_cursor_context_option(option)),
            "fast" => options.iter().find(|option| is_cursor_fast_option(option)),
            _ => None,
        })
        .cloned() else {
            continue;
        };
        let value = match target {
            "thinking" => {
                let Some(enabled) = cursor_desired_thinking(selection, reasoning_effort) else {
                    continue;
                };
                if cursor_flag_matches(&option, enabled) {
                    continue;
                }
                let Some(value) = cursor_flag_request_value(&option, enabled) else {
                    continue;
                };
                value
            }
            "effort" => {
                let Some(value) = cursor_desired_effort_value(&option, selection, reasoning_effort)
                else {
                    continue;
                };
                if session_config_current_value(&option) == Some(value.as_str()) {
                    continue;
                }
                SessionConfigOptionValue::value_id(value)
            }
            "context" => {
                let Some(value) = cursor_desired_context_value(&option, context_window) else {
                    continue;
                };
                if session_config_current_value(&option) == Some(value.as_str()) {
                    continue;
                }
                SessionConfigOptionValue::value_id(value)
            }
            "fast" => {
                let Some(enabled) = cursor_desired_fast(selection, service_tier) else {
                    continue;
                };
                if cursor_flag_matches(&option, enabled) {
                    continue;
                }
                let Some(value) = cursor_flag_request_value(&option, enabled) else {
                    continue;
                };
                value
            }
            _ => continue,
        };
        options = connection
            .send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                option.id,
                value,
            ))
            .block_task()
            .await?
            .config_options;
    }
    Ok(())
}

fn find_config_option(
    config_options: &[SessionConfigOption],
    category: SessionConfigOptionCategory,
) -> Option<&SessionConfigOption> {
    config_options
        .iter()
        .find(|option| option.category.as_ref() == Some(&category))
}

fn fx_model_option(config_options: &[SessionConfigOption]) -> Option<&SessionConfigOption> {
    config_options.iter().find(|option| {
        option.category == Some(SessionConfigOptionCategory::Model)
            && option.id.to_string().eq_ignore_ascii_case("model")
    })
}

/// The advertised model selector for agents that speak session config options
/// rather than `session/set_model`. Prefers an option whose id is `model` or
/// `models` (category optional), then the first `category: model` option that
/// is not a provider/account switch.
fn advertised_model_option(config_options: &[SessionConfigOption]) -> Option<&SessionConfigOption> {
    config_options
        .iter()
        .find(|option| {
            let id = option.id.to_string();
            id.eq_ignore_ascii_case("model") || id.eq_ignore_ascii_case("models")
        })
        .or_else(|| {
            config_options.iter().find(|option| {
                option.category == Some(SessionConfigOptionCategory::Model)
                    && !option.id.to_string().eq_ignore_ascii_case("provider")
            })
        })
        .or_else(|| {
            config_options
                .iter()
                .find(|option| option.name.eq_ignore_ascii_case("model"))
        })
}

fn advertised_model_config_id(config_options: &[SessionConfigOption]) -> String {
    advertised_model_option(config_options)
        .map(|option| option.id.to_string())
        .unwrap_or_else(|| "model".to_owned())
}

fn models_from_session_config_options(
    config_options: &[SessionConfigOption],
) -> Vec<ProviderModel> {
    let Some(option) = advertised_model_option(config_options) else {
        return Vec::new();
    };
    let current = session_config_current_value(option);
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for (id, name) in session_config_select_entries(option) {
        if !seen.insert(id) {
            continue;
        }
        let mut model = ProviderModel::new(id, name);
        if current == Some(id) {
            model = model.default();
        }
        models.push(model);
    }
    if !models.iter().any(|model| model.is_default)
        && let Some(first) = models.first_mut()
    {
        first.is_default = true;
    }
    models
}

fn is_devin_auto_model(requested: &str) -> bool {
    requested.eq_ignore_ascii_case("adaptive")
        || requested.eq_ignore_ascii_case("auto")
        || requested.eq_ignore_ascii_case("default")
}

/// Devin spells effort and the fast tier inside the advertised model id
/// (`swe-2-high`, `swe-2-high-fast`) while the picker stores them as separate
/// traits, so a bare base must be repacked before it can match.
fn devin_model_candidates(
    requested: &str,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
) -> Vec<String> {
    let mut candidates = vec![requested.to_owned()];
    let effort = reasoning_effort
        .map(normalize_reasoning_effort)
        .filter(|effort| !effort.is_empty() && effort != "default");
    let fast = service_tier == Some("fast");
    if let Some(effort) = effort {
        if fast {
            candidates.push(format!("{requested}-{effort}-fast"));
        }
        candidates.push(format!("{requested}-{effort}"));
    }
    if fast {
        candidates.push(format!("{requested}-fast"));
    }
    candidates
}

fn devin_default_reasoning_effort(option: &SessionConfigOption, requested: &str) -> Option<String> {
    crate::model_catalog::fold_packed_aliases(models_from_session_config_options(
        std::slice::from_ref(option),
    ))
    .into_iter()
    .find(|model| model.id.eq_ignore_ascii_case(requested))
    .and_then(|model| {
        model.default_reasoning_effort.or_else(|| {
            model
                .reasoning_efforts
                .first()
                .map(|option| option.id.clone())
        })
    })
}

fn resolve_devin_model(
    option: Option<&SessionConfigOption>,
    requested: &str,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    allow_model_fallback: bool,
) -> Option<String> {
    let Some(option) = option else {
        return (!is_devin_auto_model(requested)).then(|| requested.to_owned());
    };
    let values = session_config_select_values(option);
    let default_effort = if reasoning_effort.is_none() {
        devin_default_reasoning_effort(option, requested)
    } else {
        None
    };
    let reasoning_effort = reasoning_effort.or(default_effort.as_deref());
    for candidate in devin_model_candidates(requested, reasoning_effort, service_tier) {
        if let Some(value) = values
            .iter()
            .find(|value| value.eq_ignore_ascii_case(&candidate))
        {
            return Some((*value).to_owned());
        }
    }
    // Auto picks — and headless launches that would rather run on the agent's
    // default than fail the selection — take the advertised current model.
    if is_devin_auto_model(requested) || allow_model_fallback {
        return session_config_current_value(option)
            .map(str::to_owned)
            .or_else(|| values.first().map(|value| (*value).to_owned()));
    }
    None
}

/// Devin (and some other ACP agents) reject the removed `session/set_model`
/// method with JSON-RPC `-32601`, or with `-32002` carrying a "method not
/// found" message. Either means the method is absent, not that the chosen
/// model is invalid.
fn is_missing_acp_method(error: &agent_client_protocol::Error) -> bool {
    if error.code == agent_client_protocol::ErrorCode::MethodNotFound {
        return true;
    }
    let message = error.message.to_ascii_lowercase();
    message.contains("method not found")
        || message.contains("unknown method")
        || message.contains("method not supported")
        || message.contains("unsupported method")
}

fn fx_model_provider_switch<'a>(
    config_options: &'a [SessionConfigOption],
    model: &str,
) -> Option<(&'a SessionConfigOption, &'static str)> {
    if fx_model_option(config_options)
        .is_some_and(|option| session_config_select_values(option).contains(&model))
    {
        return None;
    }
    // Fx scopes model options to the selected account route. AI Gateway IDs
    // are provider/model pairs, while subscription IDs are flat. Selecting the
    // Gateway route returns a refreshed model option that contains these IDs.
    if !model.contains('/') {
        return None;
    }
    let provider = config_options.iter().find(|option| {
        option.category == Some(SessionConfigOptionCategory::Model)
            && option.id.to_string().eq_ignore_ascii_case("provider")
    })?;
    (session_config_current_value(provider) != Some("gateway")
        && session_config_select_values(provider).contains(&"gateway"))
    .then_some((provider, "gateway"))
}

fn set_model_params(
    session_id: &SessionId,
    model: &str,
    reasoning_effort: Option<&str>,
    provider: ProviderKind,
) -> serde_json::Value {
    let mut params = json!({"sessionId": session_id, "modelId": model});
    if provider == ProviderKind::Grok
        && let Some(effort) = reasoning_effort.filter(|effort| !effort.is_empty())
    {
        params["_meta"] = json!({"reasoningEffort": effort});
    }
    params
}

#[allow(clippy::too_many_arguments)]
async fn apply_model(
    connection: &ConnectionTo<Agent>,
    provider: ProviderKind,
    session_id: &SessionId,
    config_options: Option<&[SessionConfigOption]>,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    context_window: Option<&str>,
    allow_model_fallback: bool,
    events: &DriverEventSender,
) {
    let Some(model) = model else {
        return;
    };
    let cursor_model_option = (provider == ProviderKind::Cursor)
        .then_some(config_options)
        .flatten()
        .and_then(|options| find_config_option(options, SessionConfigOptionCategory::Model));
    if let Some(option) = cursor_model_option
        && let Some(selection) = cursor_model_selection(option, model)
    {
        match connection
            .send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                option.id.clone(),
                selection.value.as_str(),
            ))
            .block_task()
            .await
        {
            Ok(response) => {
                if let Err(error) = apply_cursor_variant_configs(
                    connection,
                    session_id,
                    response.config_options,
                    &selection,
                    reasoning_effort,
                    service_tier,
                    context_window,
                )
                .await
                {
                    let _ = events.send(DriverEvent::localized_error(localized!(
                        "errors.select_model",
                        error = error
                    )));
                }
            }
            Err(error) => {
                let _ = events.send(DriverEvent::localized_error(localized!(
                    "errors.select_model",
                    error = error
                )));
            }
        }
        return;
    }

    if provider == ProviderKind::Fx {
        let mut options = config_options.unwrap_or_default().to_vec();
        if let Some((provider_option, value)) = fx_model_provider_switch(&options, model) {
            let config_id = provider_option.id.clone();
            match connection
                .send_request(SetSessionConfigOptionRequest::new(
                    session_id.clone(),
                    config_id,
                    value,
                ))
                .block_task()
                .await
            {
                Ok(response) => options = response.config_options,
                Err(error) => {
                    let _ = events.send(DriverEvent::localized_error(localized!(
                        "errors.select_model",
                        error = error
                    )));
                    return;
                }
            }
        }
        let Some(option) = fx_model_option(&options) else {
            let _ = events.send(DriverEvent::localized_error(localized!(
                "errors.select_model",
                error = "Fx did not advertise its model configuration"
            )));
            return;
        };
        if !session_config_select_values(option).contains(&model) {
            let _ = events.send(DriverEvent::localized_error(localized!(
                "errors.select_model",
                error = format!("Fx did not advertise model {model}")
            )));
            return;
        }
        if let Err(error) = connection
            .send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                option.id.clone(),
                model,
            ))
            .block_task()
            .await
        {
            let _ = events.send(DriverEvent::localized_error(localized!(
                "errors.select_model",
                error = error
            )));
        }
        return;
    }

    if provider == ProviderKind::Devin {
        let options = config_options.unwrap_or_default();
        let option = advertised_model_option(options);
        if let Some(resolved) = resolve_devin_model(
            option,
            model,
            reasoning_effort,
            service_tier,
            allow_model_fallback,
        ) {
            if option.and_then(session_config_current_value) == Some(resolved.as_str()) {
                return;
            }
            match connection
                .send_request(SetSessionConfigOptionRequest::new(
                    session_id.clone(),
                    advertised_model_config_id(options),
                    resolved.as_str(),
                ))
                .block_task()
                .await
            {
                Ok(_) => return,
                Err(error) if is_missing_acp_method(&error) => {}
                Err(error) => {
                    let _ = events.send(DriverEvent::localized_error(localized!(
                        "errors.select_model",
                        error = error
                    )));
                    return;
                }
            }
        } else {
            if option.is_some() && !is_devin_auto_model(model) && !allow_model_fallback {
                let _ = events.send(DriverEvent::localized_error(localized!(
                    "errors.select_model",
                    error = format!("Devin did not advertise model {model}")
                )));
            }
            return;
        }
    }

    // Grok, Kimi, OpenCode, and Cursor agents that do not advertise a model
    // config option retain the legacy request unchanged. Devin prefers
    // session/set_config_option above and only reaches this fallback when
    // that method is itself missing. Fx stays on session/set_config_option.
    let request = match UntypedMessage::new(
        "session/set_model",
        set_model_params(session_id, model, reasoning_effort, provider),
    ) {
        Ok(request) => request,
        Err(error) => {
            let _ = events.send(DriverEvent::localized_error(localized!(
                "errors.select_model",
                error = error
            )));
            return;
        }
    };
    if let Err(error) = connection.send_request(request).block_task().await {
        if !is_missing_acp_method(&error) {
            let _ = events.send(DriverEvent::localized_error(localized!(
                "errors.select_model",
                error = error
            )));
        }
        return;
    }
    if provider != ProviderKind::Grok
        && provider != ProviderKind::Devin
        && let Some(effort) = reasoning_effort
    {
        // Reasoning effort is an optional config extension and is deliberately
        // non-fatal when an agent does not expose it.
        let _ = connection
            .send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                reasoning_effort_config_id(provider),
                effort,
            ))
            .block_task()
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
fn send_prompt(
    connection: &ConnectionTo<Agent>,
    session_id: &SessionId,
    text: String,
    prompt_requests: &PendingPromptRequests,
    events: &DriverEventSender,
    provider: ProviderKind,
    native_session_id: &str,
    grok_title_home: Option<std::path::PathBuf>,
    title_placeholder: Option<String>,
    title_refresh: super::title_refresh::NativeTitleRefresh,
    stream_state: Arc<Mutex<AcpStreamState>>,
) -> agent_client_protocol::Result<()> {
    stream_state.lock().produced_content = false;
    // Read before the turn runs, so the failure lookup cannot mistake an
    // earlier turn's record for this one's.
    let wire_offset = (provider == ProviderKind::Kimi)
        .then(|| crate::kimi_session::wire_offset(native_session_id));
    let extension_id =
        (provider == ProviderKind::Grok).then(|| format!("waku-{}", uuid::Uuid::new_v4()));
    let mut request = PromptRequest::new(
        session_id.clone(),
        vec![ContentBlock::Text(TextContent::new(text))],
    );
    if let Some(extension_id) = extension_id.as_ref() {
        let mut meta = serde_json::Map::new();
        meta.insert("promptId".into(), Value::String(extension_id.clone()));
        meta.insert("requestId".into(), Value::String(extension_id.clone()));
        request = request.meta(meta);
    }
    let sent = connection.send_request(request);
    let request_id = sent.id().clone();
    prompt_requests.lock().insert(
        request_id.clone(),
        extension_id,
        native_session_id.to_owned(),
    );
    let callback_request_id = request_id.clone();
    let callback_requests = prompt_requests.clone();
    let callback_events = events.clone();
    let native_session_id = native_session_id.to_owned();
    let registered = sent.on_receiving_result(async move |result| {
        let clean = matches!(
            &result,
            Ok(response) if response.stop_reason != StopReason::Cancelled
        );
        if let Some(settle) = settle_prompt_request(&callback_requests, &callback_request_id, clean)
        {
            // Only an empty turn pays for this lookup, so a healthy turn never
            // waits on Kimi's records.
            let native_failure = wire_offset
                .filter(|_| !stream_state.lock().produced_content)
                .and_then(|offset| crate::kimi_session::turn_failure(&native_session_id, offset));
            let success = finish_prompt(result, native_failure, settle, &callback_events);
            if success && provider == ProviderKind::Grok {
                start_grok_title_refresh(
                    grok_title_home.as_deref(),
                    &native_session_id,
                    &title_refresh,
                    callback_events,
                );
            } else if success && provider == ProviderKind::Devin {
                start_devin_title_refresh(
                    &native_session_id,
                    title_placeholder,
                    &title_refresh,
                    callback_events,
                );
            }
        }
        Ok(())
    });
    if registered.is_err() {
        prompt_requests.lock().settle_request(&request_id, false);
    }
    registered
}

fn settle_prompt_request(
    prompt_requests: &Mutex<PendingPrompts>,
    request_id: &RequestId,
    clean: bool,
) -> Option<PromptSettle> {
    prompt_requests.lock().settle_request(request_id, clean)
}

fn finish_xai_prompt_complete(
    params: &Value,
    prompt_requests: &Mutex<PendingPrompts>,
    events: &DriverEventSender,
) -> Option<String> {
    let Some(session_id) = params.get("sessionId").and_then(Value::as_str) else {
        return None;
    };
    let prompt_id = params.get("promptId").and_then(Value::as_str);
    let Some(settle) = prompt_requests
        .lock()
        .settle_extension(session_id, prompt_id)
    else {
        return None;
    };

    let stop_reason = match params.get("stopReason").and_then(Value::as_str) {
        Some("cancelled") => StopReason::Cancelled,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("max_turn_requests") => StopReason::MaxTurnRequests,
        Some("refusal") => StopReason::Refusal,
        _ => StopReason::EndTurn,
    };
    finish_prompt(Ok(PromptResponse::new(stop_reason)), None, settle, events)
        .then(|| session_id.to_owned())
}

fn start_grok_title_refresh(
    grok_title_home: Option<&Path>,
    native_session_id: &str,
    title_refresh: &super::title_refresh::NativeTitleRefresh,
    events: DriverEventSender,
) {
    let grok_title_home = grok_title_home.map(ToOwned::to_owned);
    let native_session_id = native_session_id.to_owned();
    title_refresh.start(
        "waku-grok-title",
        vec![
            Duration::ZERO,
            Duration::from_millis(250),
            Duration::from_millis(750),
            Duration::from_millis(1_500),
            Duration::from_secs(3),
            Duration::from_secs(5),
            Duration::from_millis(7_500),
            Duration::from_secs(10),
        ],
        events,
        move || match grok_title_home.as_deref() {
            Some(home) => crate::grok_session::generated_title_in(home, &native_session_id),
            None => crate::grok_session::generated_title(&native_session_id),
        },
    );
}

fn start_devin_title_refresh(
    native_session_id: &str,
    placeholder: Option<String>,
    title_refresh: &super::title_refresh::NativeTitleRefresh,
    events: DriverEventSender,
) {
    let native_session_id = native_session_id.to_owned();
    // Devin's generator runs after session/prompt returns (~300ms when it
    // succeeds) and writes the CLI sqlite store. It also stores the first
    // prompt immediately; that is a placeholder, filtered in the lookup.
    title_refresh.start(
        "waku-devin-title",
        vec![
            Duration::ZERO,
            Duration::from_millis(250),
            Duration::from_millis(750),
            Duration::from_millis(1_500),
            Duration::from_secs(3),
            Duration::from_secs(5),
            Duration::from_millis(7_500),
            Duration::from_secs(10),
        ],
        events,
        move || crate::devin_session::generated_title(&native_session_id, placeholder.as_deref()),
    );
}

fn finish_prompt(
    result: agent_client_protocol::Result<PromptResponse>,
    native_failure: Option<String>,
    settle: PromptSettle,
    events: &impl DriverEventSink,
) -> bool {
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            let _ = events.send(DriverEvent::Error(error.to_string()));
            let _ = events.send(DriverEvent::TurnFinished {
                success: false,
                summary: None,
                summary_i18n: None,
            });
            return false;
        }
    };
    // An agent can end a turn cleanly and still have failed upstream. Where
    // that failure is recoverable from the provider's own records, it outranks
    // the protocol's verdict: reporting success here would show the user an
    // empty answer and no reason for it.
    if let Some(failure) = native_failure {
        let _ = events.send(DriverEvent::Error(failure));
        let _ = events.send(DriverEvent::TurnFinished {
            success: false,
            summary: None,
            summary_i18n: None,
        });
        return false;
    }
    // `agent_stopped` causes that describe this client ending the run:
    // a clean finish, our own `session/cancel`, or a newer prompt taking
    // over. Any other cause names an external stop, which outranks a clean
    // `end_turn` the same way `native_failure` does.
    let external_stop = settle.stop_cause.as_deref().filter(|cause| {
        !matches!(
            *cause,
            "complete" | "cancelled" | "interrupted_by_new_prompt"
        )
    });
    let (success, summary_pair) = match response.stop_reason {
        StopReason::EndTurn => match external_stop {
            Some(cause) => (
                false,
                Some(localized!("session.agent_stopped_reason", reason = cause)),
            ),
            None => (true, None),
        },
        // A `cancelled` settle this client asked for is the user's own stop,
        // and one behind a sibling's clean finish is a preemption receipt.
        // Anything else is the provider interrupting the turn itself.
        StopReason::Cancelled if settle.cancel_requested || settle.saw_clean_settle => (true, None),
        StopReason::Cancelled => (
            false,
            Some(localized!(
                "session.agent_stopped_reason",
                reason = settle
                    .stop_cause
                    .as_deref()
                    .filter(|cause| *cause != "complete")
                    .unwrap_or("cancelled")
            )),
        ),
        StopReason::MaxTokens => (false, Some(localized!("session.agent_ran_out_of_context"))),
        StopReason::Refusal => (false, Some(localized!("session.agent_declined_turn"))),
        StopReason::MaxTurnRequests => (
            false,
            Some(localized!(
                "session.agent_stopped_reason",
                reason = "max_turn_requests"
            )),
        ),
        _ => (
            false,
            Some(localized!(
                "session.agent_stopped_reason",
                reason = "unknown"
            )),
        ),
    };
    let (summary, summary_i18n) = summary_pair
        .map(|pair| (Some(pair.0), Some(pair.1)))
        .unwrap_or((None, None));
    let _ = events.send(DriverEvent::TurnFinished {
        success,
        summary,
        summary_i18n,
    });
    success
}

fn cancel_pending_permissions(pending: &PendingPermissions) {
    for (_, responder) in pending.lock().drain() {
        let _ = responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
    }
}

fn cancel_pending_user_inputs(pending: &PendingAcpUserInputs) {
    for (_, pending) in pending.lock().drain() {
        let _ = pending
            .responder
            .respond(cancelled_user_input_response(pending.kind));
    }
}

fn cancelled_user_input_response(kind: AcpUserInputKind) -> Value {
    match kind {
        AcpUserInputKind::Cursor => json!({"answers": {}}),
        AcpUserInputKind::Xai => json!({"outcome": "cancelled"}),
    }
}

fn unwrap_xai_question_params(params: &Value) -> &Value {
    if matches!(
        params.get("method").and_then(Value::as_str),
        Some("x.ai/ask_user_question" | "_x.ai/ask_user_question")
    ) {
        params.get("params").unwrap_or(params)
    } else {
        params
    }
}

fn cursor_user_input_questions(params: &Value) -> Vec<UserInputQuestion> {
    params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|question| {
            let text = question.get("prompt").and_then(Value::as_str)?.trim();
            if text.is_empty() {
                return None;
            }
            let mut options = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    let label = option.get("label").and_then(Value::as_str)?.trim();
                    (!label.is_empty()).then(|| UserInputOption {
                        label: label.to_owned(),
                        description: Some(label.to_owned()),
                    })
                })
                .collect::<Vec<_>>();
            if options.is_empty() {
                options.push(UserInputOption {
                    label: "OK".into(),
                    description: Some("Continue".into()),
                });
            }
            Some(UserInputQuestion {
                id: question
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .unwrap_or(text)
                    .to_owned(),
                header: "Question".into(),
                question: text.to_owned(),
                options,
                multi_select: question
                    .get("allowMultiple")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

fn cursor_user_input_response(params: &Value, submitted: &[UserInputAnswer]) -> Value {
    let mut answers = serde_json::Map::new();
    for question in params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = question.get("id").and_then(Value::as_str) else {
            continue;
        };
        let values = submitted
            .iter()
            .find(|answer| answer.question_id == id)
            .map(|answer| answer.answers.as_slice())
            .unwrap_or_default();
        let value = if question
            .get("allowMultiple")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            json!(values)
        } else {
            values
                .first()
                .map_or(Value::String(String::new()), |value| json!(value))
        };
        answers.insert(id.to_owned(), value);
    }
    json!({"answers": answers})
}

fn xai_user_input_questions(params: &Value) -> Vec<UserInputQuestion> {
    params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, question)| {
            let text = question.get("question").and_then(Value::as_str)?.trim();
            if text.is_empty() {
                return None;
            }
            let mut options = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    let label = option.get("label").and_then(Value::as_str)?.trim();
                    (!label.is_empty()).then(|| UserInputOption {
                        label: label.to_owned(),
                        description: option
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|description| !description.is_empty())
                            .map(str::to_owned),
                    })
                })
                .collect::<Vec<_>>();
            if options.is_empty() {
                options.push(UserInputOption {
                    label: "OK".into(),
                    description: Some("Continue".into()),
                });
            }
            Some(UserInputQuestion {
                id: question
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .unwrap_or(text)
                    .to_owned(),
                header: format!("Question {}", index + 1),
                question: text.to_owned(),
                options,
                multi_select: question
                    .get("multiSelect")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

fn xai_user_input_response(params: &Value, submitted: &[UserInputAnswer]) -> Value {
    let mut answers = serde_json::Map::new();
    let mut annotations = serde_json::Map::new();
    for question in params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(question_text) = question.get("question").and_then(Value::as_str) else {
            continue;
        };
        let id = question
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(question_text);
        let values = submitted
            .iter()
            .find(|answer| answer.question_id == id || answer.question_id == question_text)
            .map(|answer| answer.answers.as_slice())
            .unwrap_or_default();
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let option_labels = options
            .iter()
            .filter_map(|option| option.get("label").and_then(Value::as_str))
            .collect::<Vec<_>>();
        let selected = values
            .iter()
            .filter(|value| option_labels.contains(&value.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let notes = values
            .iter()
            .filter(|value| !option_labels.contains(&value.as_str()))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let preview = if question
            .get("multiSelect")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            None
        } else {
            selected.iter().find_map(|selected| {
                options.iter().find_map(|option| {
                    (option.get("label").and_then(Value::as_str) == Some(selected.as_str()))
                        .then(|| {
                            option
                                .get("preview")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|preview| !preview.is_empty())
                                .map(str::to_owned)
                        })
                        .flatten()
                })
            })
        };
        answers.insert(
            question_text.to_owned(),
            json!(if selected.is_empty() && !notes.is_empty() {
                vec!["Other".to_owned()]
            } else {
                selected
            }),
        );
        let mut annotation = serde_json::Map::new();
        if let Some(preview) = preview {
            annotation.insert("preview".into(), Value::String(preview));
        }
        if !notes.is_empty() {
            annotation.insert("notes".into(), Value::String(notes));
        }
        if !annotation.is_empty() {
            annotations.insert(question_text.to_owned(), Value::Object(annotation));
        }
    }
    let mut response = json!({"outcome": "accepted", "answers": answers});
    if !annotations.is_empty() {
        response["annotations"] = Value::Object(annotations);
    }
    response
}

fn handle_permission_request(
    request: RequestPermissionRequest,
    responder: PermissionResponder,
    provider: ProviderKind,
    disposition: &PermissionDisposition,
    pending: &PendingPermissions,
    events: &DriverEventSender,
) -> agent_client_protocol::Result<()> {
    let request_id = responder.id().to_string();
    let params = serde_json::to_value(&request)?;
    let options = request
        .options
        .iter()
        .map(|option| PermissionOption {
            id: option.option_id.to_string(),
            label: option.name.clone(),
            label_i18n: None,
            allow: matches!(
                option.kind,
                PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
            ),
        })
        .collect::<Vec<_>>();

    if let PermissionDisposition::AutoApprove = disposition {
        let choice = request
            .options
            .iter()
            .find(|option| option.kind == PermissionOptionKind::AllowAlways)
            .or_else(|| {
                request
                    .options
                    .iter()
                    .find(|option| option.kind == PermissionOptionKind::AllowOnce)
            });
        return match choice {
            Some(choice) => responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    choice.option_id.clone(),
                )),
            )),
            None => responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            )),
        };
    }

    let event = DriverEvent::Permission {
        request_id: request_id.clone(),
        title: params
            .pointer("/toolCall/title")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| tr!("permission.run_a_tool")),
        title_i18n: params
            .pointer("/toolCall/title")
            .and_then(Value::as_str)
            .is_none()
            .then(|| localized!("permission.run_a_tool").1),
        detail: permission_reason(&params).unwrap_or_else(|| {
            params
                .pointer("/toolCall/kind")
                .and_then(Value::as_str)
                .map(|kind| tr!("permission.agent_wants_to", action = kind))
                .unwrap_or_else(|| tr!("permission.agent_asks_for_permission"))
        }),
        detail_i18n: (permission_reason(&params).is_none()).then(|| {
            params
                .pointer("/toolCall/kind")
                .and_then(Value::as_str)
                .map(|kind| localized!("permission.agent_wants_to", action = kind).1)
                .unwrap_or_else(|| localized!("permission.agent_asks_for_permission").1)
        }),
        options,
    };

    if let PermissionDisposition::Review(eval) = disposition {
        // The review grants at most allow-once: a cleared answer picks the
        // session's own once-option and never broadens future access.
        let allow = request
            .options
            .iter()
            .find(|option| option.kind == PermissionOptionKind::AllowOnce)
            .or_else(|| {
                request
                    .options
                    .iter()
                    .find(|option| option.kind == PermissionOptionKind::AllowAlways)
            })
            .map(|option| option.option_id.to_string());
        let action = crate::permission_review::PendingAction {
            provider: provider.id(),
            tool: params
                .pointer("/toolCall/kind")
                .and_then(Value::as_str)
                .or_else(|| params.pointer("/toolCall/title").and_then(Value::as_str))
                .unwrap_or("tool")
                .to_owned(),
            arguments: params
                .pointer("/toolCall/rawInput")
                .or_else(|| params.get("toolCall"))
                .map(|value| value.to_string())
                .unwrap_or_else(|| params.to_string()),
            call_id: request_id.clone(),
            detail: permission_reason(&params).or_else(|| {
                params
                    .pointer("/toolCall/title")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
        };
        pending.lock().insert(request_id.clone(), responder);
        let pending = pending.clone();
        let events = events.clone();
        crate::permission_review::review_on_thread(eval.clone(), action, move |verdict| {
            match verdict {
                crate::permission_review::ReviewVerdict::Allow => {
                    let Some(responder) = pending.lock().remove(&request_id) else {
                        return;
                    };
                    let outcome = match allow {
                        Some(option_id) => RequestPermissionOutcome::Selected(
                            SelectedPermissionOutcome::new(option_id),
                        ),
                        None => RequestPermissionOutcome::Cancelled,
                    };
                    let _ = responder.respond(RequestPermissionResponse::new(outcome));
                }
                crate::permission_review::ReviewVerdict::Escalate => {
                    if events.send(event).is_err()
                        && let Some(responder) = pending.lock().remove(&request_id)
                    {
                        let _ = responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ));
                    }
                }
            }
        });
        return Ok(());
    }

    pending.lock().insert(request_id.clone(), responder);
    if events.send(event).is_err()
        && let Some(responder) = pending.lock().remove(&request_id)
    {
        let _ = responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
    }
    Ok(())
}

fn handle_session_update(
    provider: ProviderKind,
    notification: SessionNotification,
    events: &impl DriverEventSink,
    state: &mut AcpStreamState,
    title_placeholder: Option<&str>,
) -> agent_client_protocol::Result<()> {
    let update = serde_json::to_value(notification.update)?;
    let kind = update.get("sessionUpdate").and_then(Value::as_str);
    if provider == ProviderKind::Fx
        && !state.produced_content
        && kind == Some("agent_message_chunk")
        && update
            .pointer("/content/text")
            .and_then(Value::as_str)
            .is_some_and(fx_context_notice)
    {
        return Ok(());
    }
    if matches!(
        kind,
        Some(
            "agent_message_chunk"
                | "agent_thought_chunk"
                | "tool_call"
                | "tool_call_update"
                | "plan"
        )
    ) {
        state.produced_content = true;
    }
    match kind {
        Some("agent_message_chunk") => {
            if let Some(text) = update
                .pointer("/content/text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                let _ = events.send(DriverEvent::TextDelta(text.to_owned()));
            }
        }
        Some("agent_thought_chunk") => {
            if let Some(text) = update
                .pointer("/content/text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                let _ = events.send(DriverEvent::ReasoningDelta(text.to_owned()));
            }
        }
        Some("tool_call" | "tool_call_update") => tool_activity(&update, events, state),
        Some("plan") => {
            let _ = events.send(DriverEvent::Activity {
                id: Some("acp-plan".into()),
                kind: ActivityKind::Plan,
                title: tr!("activity.plan_updated"),
                detail: None,
                complete: false,
            });
        }
        Some("available_commands_update") => {
            let commands = update
                .get("availableCommands")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|command| {
                            let name = command.get("name").and_then(Value::as_str)?;
                            Some(crate::model::ReportedCommand {
                                name: name.to_owned(),
                                description: command
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !commands.is_empty() {
                let _ = events.send(DriverEvent::AvailableCommands(commands));
            }
        }
        Some("session_info_update") => {
            if update.get("title").is_some() {
                let title = update
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let skip_placeholder = provider == ProviderKind::Devin
                    && title.as_deref().is_some_and(|title| {
                        crate::devin_session::is_placeholder_title(title, title_placeholder)
                    });
                if !skip_placeholder {
                    let _ = events.send(DriverEvent::AutoTitleUpdated(title));
                }
            }
        }
        Some("usage_update") => {
            let used = update
                .get("used")
                .and_then(Value::as_u64)
                .filter(|used| *used > 0);
            let window = ["max", "limit", "size", "contextWindow", "context_window"]
                .into_iter()
                .find_map(|key| update.get(key).and_then(Value::as_u64))
                .filter(|window| *window > 0);
            if used.is_some() || window.is_some() {
                let _ = events.send(DriverEvent::UsageUpdated {
                    context_tokens: used,
                    context_window: window,
                });
            }
        }
        // `user_message_chunk` is Goddard's own prompt echoed back. Other typed
        // updates currently have no transcript representation.
        _ => {}
    }
    Ok(())
}

fn fx_context_notice(text: &str) -> bool {
    text.starts_with("[context] ") || text.starts_with("skill discovery warning: ")
}

#[derive(Default)]
struct AcpStreamState {
    tools: HashMap<String, (ActivityKind, String)>,
    /// Open subagent calls keyed by their parent's agent id. Devin surfaces
    /// them as `tool_call`s but never sends their `tool_call_update`; the
    /// agent's `subagent_completed` lifecycle update settles them all.
    subagent_calls: HashMap<String, HashSet<String>>,
    /// Agent ids whose completion already arrived, mapped to their success,
    /// so a subagent call that surfaces late still settles at once.
    subagent_finished: HashMap<String, bool>,
    /// Whether the running turn has produced anything visible. A turn that
    /// ends having produced nothing is the shape a swallowed provider error
    /// takes, which is what makes a native failure worth looking up.
    produced_content: bool,
}

/// Pull the agent's explanation out of a permission request's tool call.
fn permission_reason(params: &Value) -> Option<String> {
    let content = params
        .pointer("/toolCall/content")
        .and_then(Value::as_array)?;
    let reason = content
        .iter()
        .filter_map(|entry| {
            entry
                .pointer("/content/text")
                .or_else(|| entry.get("text"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!reason.is_empty()).then(|| truncate(&reason, 400))
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars()
        .take(max_chars)
        .chain(std::iter::once('…'))
        .collect()
}

fn tool_activity(update: &Value, events: &impl DriverEventSink, state: &mut AcpStreamState) {
    let id = update
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let status = update
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("pending");
    let mut complete = matches!(status, "completed" | "failed");
    let mut failed = status == "failed";

    // Devin delegates work to subagent chains. A subagent's calls surface as
    // ordinary `tool_call` notifications tagged with the parent's agent id
    // under `_meta`, but no `tool_call_update` ever follows them — the
    // agent's own `subagent_completed` lifecycle update is their only
    // terminal signal.
    let meta = update.get("_meta");
    let parent_agent = meta
        .and_then(|meta| meta.get("cognition.ai/subagent_context"))
        .and_then(|context| context.get("parentAgentId"))
        .and_then(Value::as_str);
    let subagent_started = meta.and_then(|meta| meta.get("cognition.ai/subagent_started"));
    let subagent_completed = meta.and_then(|meta| meta.get("cognition.ai/subagent_completed"));

    if let Some(parent) = parent_agent {
        if let Some(success) = state.subagent_finished.get(parent) {
            // The agent finished before this call surfaced — settle it now.
            complete = true;
            failed = !success;
        } else if !complete && let Some(id) = id.as_ref() {
            state
                .subagent_calls
                .entry(parent.to_owned())
                .or_default()
                .insert(id.clone());
        }
    }
    if let Some(completed) = subagent_completed {
        failed = failed
            || !completed
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(true);
    }

    let wire_kind = update.get("kind").and_then(Value::as_str);
    let wire_title = update.get("title").and_then(Value::as_str);
    let stored = id.as_ref().and_then(|id| {
        if complete {
            state.tools.remove(id)
        } else {
            state.tools.get(id).cloned()
        }
    });
    let mut kind = wire_kind
        .map(classify)
        .or_else(|| stored.as_ref().map(|(kind, _)| *kind))
        .unwrap_or(ActivityKind::Tool);
    if matches!(kind, ActivityKind::Search | ActivityKind::Tool)
        && let Some(wire_title) = wire_title
    {
        let named_kind = ActivityKind::from_tool_name(wire_title);
        if named_kind != ActivityKind::Tool {
            kind = named_kind;
        }
    }
    let arguments = update.get("rawInput").filter(|value| !value.is_null());
    let title = activity::input_title(arguments)
        .or_else(|| {
            wire_title
                .filter(|title| !title.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| {
            subagent_started
                .and_then(|started| started.get("title"))
                .and_then(Value::as_str)
                .map(|title| tr!("activity.subagent", title = title))
        })
        .or_else(|| stored.map(|(_, title)| title))
        .unwrap_or_else(|| "Tool".to_owned());
    if !complete && let Some(id) = id.as_ref() {
        state.tools.insert(id.clone(), (kind, title.clone()));
    }

    let output = update
        .get("content")
        .filter(|value| !value.is_null())
        .or_else(|| update.get("rawOutput").filter(|value| !value.is_null()))
        .or_else(|| {
            subagent_completed
                .and_then(|completed| completed.get("summary"))
                .filter(|value| !value.is_null())
        });
    let item = activity::tool_activity(
        id.clone(),
        kind,
        title,
        arguments,
        output,
        output,
        failed,
        complete,
    )
    .with_tool_name(
        arguments
            .and_then(|input| input.get("tool_name"))
            .and_then(Value::as_str)
            .or_else(|| wire_title.filter(|title| title.starts_with("mcp__"))),
    );
    let _ = events.send(DriverEvent::RichActivity(item));

    // `subagent_completed` is keyed by the agent id, not the calls it owned —
    // settle every tracked call the agent leaves behind.
    if let Some(completed) = subagent_completed {
        let agent_id = completed
            .get("agentId")
            .and_then(Value::as_str)
            .or(id.as_deref());
        let success = completed
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if let Some(agent_id) = agent_id {
            state.subagent_finished.insert(agent_id.to_owned(), success);
            if let Some(children) = state.subagent_calls.remove(agent_id) {
                for child in children {
                    let (kind, title) = state
                        .tools
                        .remove(&child)
                        .unwrap_or((ActivityKind::Tool, "Tool".to_owned()));
                    let item = activity::tool_activity(
                        Some(child),
                        kind,
                        title,
                        None,
                        None,
                        None,
                        !success,
                        true,
                    );
                    let _ = events.send(DriverEvent::RichActivity(item));
                }
            }
        }
    }
}

fn classify(kind: &str) -> ActivityKind {
    match kind {
        "execute" => ActivityKind::Command,
        "edit" | "delete" | "move" => ActivityKind::FileChange,
        "read" => ActivityKind::FileRead,
        "search" | "fetch" => ActivityKind::Search,
        "think" => ActivityKind::Reasoning,
        _ => ActivityKind::Tool,
    }
}

impl DriverControl for AcpDriver {
    fn prompt(&self, prompt: String) {
        let _ = self.commands.try_send(CommandMessage::Prompt(prompt));
    }

    fn supports_steer(&self) -> bool {
        self.supports_steer
    }

    fn steer(&self, prompt: String) {
        let _ = self.commands.try_send(CommandMessage::Steer(prompt));
    }

    fn cancel(&self) {
        let _ = self.commands.try_send(CommandMessage::Cancel);
    }

    fn cancel_computer_use(&self) {
        if let Some(computer_use) = self.computer_use.as_ref() {
            computer_use.stop();
        }
        if let Some(computer_use) = self.native_computer_use.as_ref() {
            computer_use.stop();
        }
    }

    fn respond(&self, request_id: String, option_id: String) {
        let _ = self.commands.try_send(CommandMessage::Respond {
            request_id,
            option_id,
        });
    }

    fn respond_user_input(&self, request_id: String, answers: Vec<UserInputAnswer>) {
        let _ = self.commands.try_send(CommandMessage::RespondUserInput {
            request_id,
            answers,
        });
    }

    fn apply_options(&self, options: SessionOptions) -> bool {
        if options.mode != self.mode {
            return false;
        }
        self.commands
            .try_send(CommandMessage::Options(options))
            .is_ok()
    }

    fn delete_provider_session(&self) {
        let (done, wait) = std::sync::mpsc::channel();
        if self
            .commands
            .try_send(CommandMessage::DeleteSession(done))
            .is_ok()
        {
            // The actor deletes then exits; a dead actor drops the sender
            // and ends the wait on its own.
            let _ = wait.recv_timeout(Duration::from_secs(10));
        }
    }

    fn rollback(&self, _turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        Err(anyhow!(
            "conversation rollback is not supported by this provider transport"
        ))
    }
}

impl Drop for AcpDriver {
    fn drop(&mut self) {
        self.cancel_computer_use();
        let _ = self.commands.try_send(CommandMessage::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        SessionConfigSelectOption, SessionMode, SessionModeState, ToolCallUpdate,
        ToolCallUpdateFields,
    };

    fn select_config_option(
        id: &str,
        category: SessionConfigOptionCategory,
        current: &str,
        values: &[&str],
    ) -> SessionConfigOption {
        SessionConfigOption::select(
            id.to_owned(),
            id.to_owned(),
            current.to_owned(),
            values
                .iter()
                .map(|value| SessionConfigSelectOption::new((*value).to_owned(), *value))
                .collect::<Vec<_>>(),
        )
        .category(category)
    }

    #[test]
    fn cursor_question_response_uses_native_scalar_and_array_answers() {
        let params = json!({
            "toolCallId": "ask-1",
            "questions": [
                {
                    "id": "scope",
                    "prompt": "Which scope?",
                    "options": [{"id": "workspace", "label": "Workspace"}]
                },
                {
                    "id": "checks",
                    "prompt": "Which checks?",
                    "options": [
                        {"id": "tests", "label": "Tests"},
                        {"id": "lint", "label": "Lint"}
                    ],
                    "allowMultiple": true
                }
            ]
        });

        let questions = cursor_user_input_questions(&params);
        assert_eq!(questions.len(), 2);
        assert!(!questions[0].multi_select);
        assert!(questions[1].multi_select);

        let response = cursor_user_input_response(
            &params,
            &[
                UserInputAnswer {
                    question_id: "scope".into(),
                    answers: vec!["Workspace".into()],
                },
                UserInputAnswer {
                    question_id: "checks".into(),
                    answers: vec!["Tests".into(), "Lint".into()],
                },
            ],
        );
        assert_eq!(
            response.pointer("/answers/scope"),
            Some(&json!("Workspace"))
        );
        assert_eq!(
            response.pointer("/answers/checks"),
            Some(&json!(["Tests", "Lint"]))
        );
    }

    #[test]
    fn grok_question_response_keeps_native_labels_and_annotates_custom_text() {
        let params = json!({
            "sessionId": "session-1",
            "toolCallId": "tool-1",
            "mode": "default",
            "questions": [
                {
                    "id": "environment",
                    "question": "Where should this deploy?",
                    "options": [{"label": "Preview", "preview": "Deploy to preview"}],
                    "multiSelect": false
                },
                {
                    "id": "notes",
                    "question": "Anything else?",
                    "options": [{"label": "No"}],
                    "multiSelect": false
                }
            ]
        });
        let response = xai_user_input_response(
            &params,
            &[
                UserInputAnswer {
                    question_id: "environment".into(),
                    answers: vec!["Preview".into()],
                },
                UserInputAnswer {
                    question_id: "notes".into(),
                    answers: vec!["Use the EU region".into()],
                },
            ],
        );

        assert_eq!(response["outcome"], "accepted");
        assert_eq!(
            response.pointer("/answers/Where should this deploy?/0"),
            Some(&json!("Preview"))
        );
        assert_eq!(
            response.pointer("/answers/Anything else?/0"),
            Some(&json!("Other"))
        );
        assert_eq!(
            response.pointer("/annotations/Where should this deploy?/preview"),
            Some(&json!("Deploy to preview"))
        );
        assert_eq!(
            response.pointer("/annotations/Anything else?/notes"),
            Some(&json!("Use the EU region"))
        );
    }

    #[test]
    fn legacy_read_only_sessions_return_to_the_advertised_agent_mode() {
        let modes = SessionModeState::new(
            "plan",
            vec![
                SessionMode::new("agent", "Agent"),
                SessionMode::new("plan", "Plan"),
            ],
        );

        assert_eq!(
            desired_access_mode(ProviderKind::Cursor, Some(&modes), RuntimeMode::FullAccess)
                .map(|mode| mode.to_string()),
            Some("agent".to_owned())
        );
    }

    #[test]
    fn fx_access_mode_selects_ask_or_code() {
        let modes = SessionModeState::new(
            "code",
            vec![
                SessionMode::new("ask", "Ask before sensitive actions"),
                SessionMode::new("code", "Review sensitive actions automatically"),
            ],
        );
        assert_eq!(
            desired_access_mode(ProviderKind::Fx, Some(&modes), RuntimeMode::Ask)
                .map(|mode| mode.to_string()),
            Some("ask".to_owned())
        );
        assert!(
            desired_access_mode(ProviderKind::Fx, Some(&modes), RuntimeMode::FullAccess).is_none()
        );
    }

    #[test]
    fn fx_launches_its_documented_acp_subcommand() {
        let launch = launch_for(ProviderKind::Fx, None).unwrap();
        assert_eq!(launch.args, ["acp"]);
        assert!(launch.env.is_empty());
    }

    #[test]
    fn devin_launches_its_documented_acp_subcommand() {
        let launch = launch_for(ProviderKind::Devin, None).unwrap();
        assert_eq!(launch.args, ["acp"]);
        assert!(launch.env.is_empty());
    }

    #[test]
    fn advertised_model_option_prefers_model_id_over_provider_switch() {
        let provider = select_config_option(
            "provider",
            SessionConfigOptionCategory::Model,
            "gateway",
            &["gateway", "codex"],
        );
        let model = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "adaptive",
            &["adaptive", "claude-opus-4-6-thinking"],
        );

        assert_eq!(
            advertised_model_option(&[provider, model]).map(|option| option.id.to_string()),
            Some("model".to_owned())
        );
    }

    #[test]
    fn advertised_model_option_accepts_category_less_models_id() {
        let option = SessionConfigOption::select(
            "models".to_owned(),
            "Model".to_owned(),
            "adaptive".to_owned(),
            vec![SessionConfigSelectOption::new(
                "adaptive".to_owned(),
                "adaptive",
            )],
        );

        assert_eq!(
            advertised_model_option(&[option]).map(|option| option.id.to_string()),
            Some("models".to_owned())
        );
    }

    #[test]
    fn advertised_model_config_id_falls_back_to_model() {
        assert_eq!(advertised_model_config_id(&[]), "model");
    }

    #[test]
    fn session_error_retryable_reads_namespaced_and_plain_markers() {
        let mut locked = agent_client_protocol::Error::new(-32015, "session is locked");
        locked.data = Some(json!({
            "cognition.ai/errorKind": "session_locked",
            "cognition.ai/retryable": true,
        }));
        assert!(session_error_retryable(&locked));

        let mut plain = agent_client_protocol::Error::new(-32015, "busy");
        plain.data = Some(json!({"retryable": true}));
        assert!(session_error_retryable(&plain));

        let mut not_retryable = agent_client_protocol::Error::new(-32016, "Session not found");
        not_retryable.data = Some(json!({
            "cognition.ai/errorKind": "session_not_found",
            "cognition.ai/retryable": false,
        }));
        assert!(!session_error_retryable(&not_retryable));
        assert!(!session_error_retryable(
            &agent_client_protocol::Error::invalid_params()
        ));
    }

    #[test]
    fn missing_acp_method_matches_standard_and_devin_error_shapes() {
        assert!(is_missing_acp_method(
            &agent_client_protocol::Error::method_not_found()
        ));
        assert!(is_missing_acp_method(&agent_client_protocol::Error::new(
            -32002,
            "Method not found"
        )));
        assert!(!is_missing_acp_method(
            &agent_client_protocol::Error::invalid_params()
        ));
        assert!(!is_missing_acp_method(&agent_client_protocol::Error::new(
            -32602,
            "unknown model"
        )));
    }

    #[test]
    fn devin_catalog_uses_the_advertised_model_option() {
        let mode = select_config_option(
            "mode",
            SessionConfigOptionCategory::Mode,
            "accept-edits",
            &["accept-edits", "ask"],
        );
        let model = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "swe-1-6-slow",
            &["swe-1-6-slow"],
        );
        let models = models_from_session_config_options(&[mode, model]);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "swe-1-6-slow");
        assert_eq!(models[0].name, "swe-1-6-slow");
        assert!(models[0].is_default);
    }

    #[test]
    fn devin_maps_adaptive_to_the_advertised_current_model() {
        let option = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "swe-1-6-slow",
            &["swe-1-6-slow", "swe-1-6"],
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "adaptive", None, None, false).as_deref(),
            Some("swe-1-6-slow")
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-1-6", None, None, false).as_deref(),
            Some("swe-1-6")
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "opus", None, None, false),
            None
        );
        assert_eq!(
            resolve_devin_model(None, "adaptive", None, None, false),
            None
        );
        assert_eq!(
            resolve_devin_model(None, "swe-1-6-slow", None, None, false).as_deref(),
            Some("swe-1-6-slow")
        );
    }

    #[test]
    fn devin_repacks_picker_effort_and_fast_tier_into_the_model_id() {
        let option = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "swe-2-medium",
            &["swe-2-medium", "swe-2-high", "swe-2-high-fast", "swe-2-max"],
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-2", Some("high"), None, false).as_deref(),
            Some("swe-2-high")
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-2", Some("high"), Some("fast"), false)
                .as_deref(),
            Some("swe-2-high-fast")
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-2", Some("xhigh"), Some("fast"), false),
            None
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-2", Some("medium"), Some("fast"), false)
                .as_deref(),
            Some("swe-2-medium")
        );
    }

    #[test]
    fn devin_model_fallback_uses_the_advertised_current_model() {
        let option = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "swe-2-medium",
            &["swe-2-medium", "swe-2-high", "swe-2-high-fast", "swe-2-max"],
        );
        // Memory distillation replays the session's folded base id plus its
        // stored traits, so the repack still lands on an advertised packed id.
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-2", Some("high"), None, true).as_deref(),
            Some("swe-2-high")
        );
        // When even the repack misses — a stored trait the provider dropped —
        // the fallback resolves the advertised current model instead of
        // failing the headless launch outright.
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-2", Some("xhigh"), None, true).as_deref(),
            Some("swe-2-medium")
        );
        assert_eq!(
            resolve_devin_model(Some(&option), "swe-9", None, None, true).as_deref(),
            Some("swe-2-medium")
        );
    }

    #[test]
    fn droid_launches_its_documented_acp_subcommand() {
        let launch = launch_for(ProviderKind::Droid, None).unwrap();
        assert_eq!(launch.args, ["exec", "--output-format", "acp"]);
        assert!(launch.env.is_empty());
    }

    /// Droid's autonomy modes are its access modes: every Goddard mode maps onto
    /// one advertised level. The mode ids are captured from a live
    /// `session/new` on droid 0.217.0.
    #[test]
    fn droid_access_mode_maps_every_waku_mode_onto_the_autonomy_ladder() {
        let modes = |current| {
            SessionModeState::new(
                current,
                vec![
                    SessionMode::new("normal", "Auto-approves only read operations"),
                    SessionMode::new("spec", "Build feature specs (read-only)"),
                    SessionMode::new("auto-low", "Auto-approves file edits and low-risk actions"),
                    SessionMode::new("auto-medium", "Auto-approves medium-risk actions"),
                    SessionMode::new("auto-high", "Auto-approves all actions"),
                ],
            )
        };
        let expected = [
            (RuntimeMode::Ask, "normal"),
            (RuntimeMode::AutoAcceptEdits, "auto-low"),
            (RuntimeMode::Auto, "auto-medium"),
            (RuntimeMode::FullAccess, "auto-high"),
        ];
        for (mode, id) in expected {
            assert_eq!(
                desired_access_mode(ProviderKind::Droid, Some(&modes("spec")), mode)
                    .map(|selected| selected.to_string()),
                Some(id.to_owned())
            );
        }
        // A session already sitting on the mapped mode is left untouched.
        assert!(
            desired_access_mode(
                ProviderKind::Droid,
                Some(&modes("normal")),
                RuntimeMode::Ask
            )
            .is_none()
        );
    }

    #[test]
    fn droid_reasoning_effort_rides_its_advertised_config_option() {
        assert_eq!(
            reasoning_effort_config_id(ProviderKind::Droid),
            "reasoning_effort"
        );
    }

    #[test]
    fn fx_model_option_ignores_provider_selector_in_same_category() {
        let provider = select_config_option(
            "provider",
            SessionConfigOptionCategory::Model,
            "gateway",
            &["gateway", "codex", "grok"],
        );
        let model = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "openai/gpt-5.6-sol",
            &["openai/gpt-5.6-sol", "anthropic/claude-sonnet-5"],
        );

        assert_eq!(
            fx_model_option(&[provider, model]).map(|option| option.id.to_string()),
            Some("model".to_owned())
        );
    }

    #[test]
    fn fx_gateway_model_selects_the_gateway_route_first() {
        let provider = select_config_option(
            "provider",
            SessionConfigOptionCategory::Model,
            "codex",
            &["gateway", "codex", "grok"],
        );
        let model = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "gpt-5.6-luna",
            &["gpt-5.6-sol", "gpt-5.6-luna"],
        );
        let options = [provider, model];

        let (option, value) =
            fx_model_provider_switch(&options, "openai/gpt-5.6-luna-fast").unwrap();
        assert_eq!(option.id.to_string(), "provider");
        assert_eq!(value, "gateway");
    }

    #[test]
    fn cursor_model_aliases_resolve_to_advertised_parameterized_picker_values() {
        let option = select_config_option(
            "model",
            SessionConfigOptionCategory::Model,
            "default",
            &["default", "grok-4.6", "composer-2.5", "claude-sonnet-4-6"],
        );

        assert_eq!(
            cursor_model_selection(&option, "auto"),
            Some(PackedModelSelection {
                value: "default".into(),
                suffix: String::new(),
            })
        );
        assert_eq!(
            cursor_model_selection(&option, "composer-2.5"),
            Some(PackedModelSelection {
                value: "composer-2.5".into(),
                suffix: String::new(),
            })
        );
        assert_eq!(
            cursor_model_selection(&option, "cursor-grok-4.6-xhigh-fast"),
            Some(PackedModelSelection {
                value: "grok-4.6".into(),
                suffix: "xhigh-fast".into(),
            })
        );
        assert_eq!(
            cursor_model_selection(&option, "claude-4.6-sonnet-medium-thinking"),
            Some(PackedModelSelection {
                value: "claude-sonnet-4-6".into(),
                suffix: "medium-thinking".into(),
            })
        );
    }

    #[test]
    fn cursor_model_suffix_selects_dynamic_effort_thinking_and_fast_options() {
        let selection = PackedModelSelection {
            value: "claude-opus-5".into(),
            suffix: "thinking-extra-high-fast".into(),
        };
        let effort = select_config_option(
            "effort",
            SessionConfigOptionCategory::ThoughtLevel,
            "high",
            &["low", "medium", "high", "xhigh"],
        );
        let extra_high = select_config_option(
            "reasoning",
            SessionConfigOptionCategory::ThoughtLevel,
            "high",
            &["low", "medium", "high", "extra-high"],
        );
        let context = select_config_option(
            "context",
            SessionConfigOptionCategory::ModelConfig,
            "272k",
            &["272k", "1m"],
        );

        assert_eq!(
            cursor_desired_effort_value(&effort, &selection, None).as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            cursor_desired_effort_value(&extra_high, &selection, Some("xhigh")).as_deref(),
            Some("extra-high")
        );
        assert_eq!(cursor_desired_thinking(&selection, None), Some(true));
        assert_eq!(cursor_desired_fast(&selection, None), Some(true));
        assert_eq!(
            cursor_desired_effort_value(&effort, &selection, Some("low")).as_deref(),
            Some("low")
        );
        assert_eq!(
            cursor_desired_fast(&selection, Some("default")),
            Some(false)
        );
        assert_eq!(
            cursor_desired_context_value(&context, Some("1m")).as_deref(),
            Some("1m")
        );
        assert_eq!(
            cursor_desired_thinking(&selection, Some("none")),
            Some(false)
        );
    }

    #[test]
    fn cursor_prefers_model_option_effort_over_thought_level() {
        let thought = select_config_option(
            "reasoning",
            SessionConfigOptionCategory::ThoughtLevel,
            "high",
            &["low", "medium", "high"],
        );
        let effort = select_config_option(
            "effort",
            SessionConfigOptionCategory::Other("model_option".into()),
            "max",
            &["low", "medium", "high", "max"],
        );
        let options = [thought, effort];
        let selected = find_cursor_effort_option(&options).unwrap();
        assert_eq!(selected.id.to_string(), "effort");
    }

    #[test]
    fn a_steer_only_settles_when_the_last_sdk_request_finishes() {
        let requests = Mutex::new(PendingPrompts::default());
        requests
            .lock()
            .insert(RequestId::Str("first".into()), None, "session".into());
        requests
            .lock()
            .insert(RequestId::Str("steer".into()), None, "session".into());
        assert!(settle_prompt_request(&requests, &RequestId::Str("first".into()), false).is_none());
        assert!(settle_prompt_request(&requests, &RequestId::Str("steer".into()), true).is_some());
        assert!(settle_prompt_request(&requests, &RequestId::Str("steer".into()), false).is_none());
    }

    #[test]
    fn xai_prompt_complete_settles_a_missing_standard_response_once() {
        let requests = Mutex::new(PendingPrompts::default());
        let request_id = RequestId::Str("sdk-request".into());
        requests.lock().insert(
            request_id.clone(),
            Some("waku-prompt".into()),
            "grok-session".into(),
        );
        let (events, event_rx) = crate::driver::test_event_channel();

        assert_eq!(
            finish_xai_prompt_complete(
                &json!({
                    "sessionId": "grok-session",
                    "promptId": "waku-prompt",
                    "stopReason": "end_turn"
                }),
                &requests,
                &events,
            ),
            Some("grok-session".into())
        );
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::TurnFinished {
                success: true,
                summary: None,
                summary_i18n: None,
            }
        ));
        assert!(settle_prompt_request(&requests, &request_id, false).is_none());
        assert!(event_rx.try_recv().is_err());
    }

    /// Kimi ends a failed turn with `end_turn` and no content at all, so the
    /// provider's own record is the only thing that can name the cause.
    #[test]
    fn a_recovered_provider_failure_overrides_a_clean_stop_reason() {
        let (events, event_rx) = crossbeam_channel::unbounded();

        assert!(!finish_prompt(
            Ok(PromptResponse::new(StopReason::EndTurn)),
            Some("402 membership inactive".to_owned()),
            PromptSettle::default(),
            &events
        ));

        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::Error(message) if message == "402 membership inactive"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::TurnFinished {
                success: false,
                summary: None,
                summary_i18n: None,
            }
        ));
    }

    /// Devin's model server stops a turn without the client ever sending
    /// `session/cancel`: the `cancelled` stop reason is then a failure, not
    /// the user's own stop.
    #[test]
    fn an_unrequested_cancelled_stop_reason_fails_the_turn() {
        let (events, event_rx) = crossbeam_channel::unbounded();

        assert!(!finish_prompt(
            Ok(PromptResponse::new(StopReason::Cancelled)),
            None,
            PromptSettle::default(),
            &events
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::TurnFinished {
                success: false,
                summary_i18n: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn a_requested_cancelled_stop_reason_still_settles_cleanly() {
        let (events, event_rx) = crossbeam_channel::unbounded();

        assert!(finish_prompt(
            Ok(PromptResponse::new(StopReason::Cancelled)),
            None,
            PromptSettle {
                cancel_requested: true,
                ..PromptSettle::default()
            },
            &events
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::TurnFinished {
                success: true,
                summary: None,
                summary_i18n: None,
            }
        ));
    }

    /// A cancelled settle arriving behind a sibling's clean finish is the
    /// preemption receipt for a steer, not an interruption.
    #[test]
    fn a_cancelled_settle_behind_a_clean_sibling_settles_cleanly() {
        let (events, event_rx) = crossbeam_channel::unbounded();

        assert!(finish_prompt(
            Ok(PromptResponse::new(StopReason::Cancelled)),
            None,
            PromptSettle {
                saw_clean_settle: true,
                ..PromptSettle::default()
            },
            &events
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::TurnFinished {
                success: true,
                summary: None,
                summary_i18n: None,
            }
        ));
    }

    /// Devin names an externally stopped run in `agent_stopped`'s `cause`,
    /// which fails the turn even when the prompt resolves `end_turn`.
    #[test]
    fn a_devin_stop_cause_overrides_a_clean_stop_reason() {
        let (events, event_rx) = crossbeam_channel::unbounded();

        assert!(!finish_prompt(
            Ok(PromptResponse::new(StopReason::EndTurn)),
            None,
            PromptSettle {
                stop_cause: Some("interrupted".to_owned()),
                ..PromptSettle::default()
            },
            &events
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::TurnFinished {
                success: false,
                summary_i18n: Some(_),
                ..
            }
        ));
    }

    /// `complete` and `cancelled` causes are the client's own endings, and a
    /// new-prompt interruption is a steer — none of them fail a clean stop.
    #[test]
    fn client_caused_stop_causes_keep_a_clean_stop_reason() {
        for cause in ["complete", "cancelled", "interrupted_by_new_prompt"] {
            let (events, event_rx) = crossbeam_channel::unbounded();
            assert!(finish_prompt(
                Ok(PromptResponse::new(StopReason::EndTurn)),
                None,
                PromptSettle {
                    stop_cause: Some(cause.to_owned()),
                    ..PromptSettle::default()
                },
                &events
            ));
            assert!(matches!(
                event_rx.try_recv().unwrap(),
                DriverEvent::TurnFinished { success: true, .. }
            ));
        }
    }

    #[test]
    fn typed_prompt_response_settles_the_turn() {
        let (events, event_rx) = crossbeam_channel::unbounded();
        assert!(finish_prompt(
            Ok(PromptResponse::new(StopReason::EndTurn)),
            None,
            PromptSettle::default(),
            &events
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::TurnFinished {
                success: true,
                summary: None,
                summary_i18n: None,
            }
        ));
    }

    #[test]
    fn typed_updates_preserve_text_reasoning_and_correlated_tools() {
        let (events, event_rx) = crossbeam_channel::unbounded();
        let mut state = AcpStreamState::default();
        let updates = [
            json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"thinking"}}),
            json!({"sessionUpdate":"tool_call","toolCallId":"call_1","title":"read","kind":"read","status":"pending","rawInput":{}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"call_1","status":"completed","title":"fixture.txt","content":[{"type":"content","content":{"type":"text","text":"waku probe fixture"}}]}),
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"OK"}}),
            json!({"sessionUpdate":"usage_update","used":9677,"size":500000}),
        ];
        for update in updates {
            let update = serde_json::from_value(update).unwrap();
            handle_session_update(
                ProviderKind::Cursor,
                SessionNotification::new("s", update),
                &events,
                &mut state,
                None,
            )
            .unwrap();
        }

        let seen = event_rx.try_iter().collect::<Vec<_>>();
        assert!(matches!(&seen[0], DriverEvent::ReasoningDelta(text) if text == "thinking"));
        assert!(matches!(&seen[1], DriverEvent::RichActivity(item)
                if item.kind == ActivityKind::FileRead && !item.complete));
        assert!(matches!(&seen[2], DriverEvent::RichActivity(item)
                if item.complete
                    && item.title == "fixture.txt"
                    && item.output.as_deref() == Some("waku probe fixture")));
        assert!(matches!(&seen[3], DriverEvent::TextDelta(text) if text == "OK"));
        assert!(matches!(
            &seen[4],
            DriverEvent::UsageUpdated {
                context_tokens: Some(9677),
                context_window: Some(500000),
            }
        ));
    }

    #[test]
    fn devin_subagent_calls_settle_on_the_agents_lifecycle_update() {
        // Devin surfaces a subagent's calls as `tool_call`s tagged with the
        // parent's agent id but never sends their `tool_call_update`; the
        // `subagent_completed` lifecycle update on the agent id is the only
        // terminal signal and must settle them.
        let (events, event_rx) = crossbeam_channel::unbounded();
        let mut state = AcpStreamState::default();
        let updates = [
            json!({"sessionUpdate":"tool_call_update","toolCallId":"agent1","status":"in_progress",
                   "_meta":{"cognition.ai/subagent_started":{"agentId":"agent1","title":"Echo hello","task":"run echo","isBackground":true}}}),
            json!({"sessionUpdate":"tool_call","toolCallId":"exec:0#child","title":"Ran echo","kind":"execute","rawInput":{"command":"echo hi"},
                   "_meta":{"cognition.ai/subagent_context":{"parentAgentId":"agent1"}}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"agent1","status":"completed",
                   "_meta":{"cognition.ai/subagent_completed":{"agentId":"agent1","success":true,"summary":"ran echo"}}}),
        ];
        for update in updates {
            let update = serde_json::from_value(update).unwrap();
            handle_session_update(
                ProviderKind::Devin,
                SessionNotification::new("s", update),
                &events,
                &mut state,
                None,
            )
            .unwrap();
        }

        let seen = event_rx.try_iter().collect::<Vec<_>>();
        assert_eq!(seen.len(), 4);
        assert!(matches!(&seen[0], DriverEvent::RichActivity(item)
                if item.title == "Subagent: Echo hello" && !item.complete));
        assert!(matches!(&seen[1], DriverEvent::RichActivity(item)
                if item.kind == ActivityKind::Command && !item.complete));
        assert!(matches!(&seen[2], DriverEvent::RichActivity(item)
                if item.complete && !item.failed && item.output.as_deref() == Some("ran echo")));
        assert!(matches!(&seen[3], DriverEvent::RichActivity(item)
                if item.source_id.as_deref() == Some("exec:0#child")
                    && item.kind == ActivityKind::Command
                    && item.complete
                    && !item.failed));
    }

    #[test]
    fn failed_devin_subagent_fails_its_open_and_late_calls() {
        let (events, event_rx) = crossbeam_channel::unbounded();
        let mut state = AcpStreamState::default();
        let updates = [
            json!({"sessionUpdate":"tool_call","toolCallId":"exec:0#child","title":"Ran echo","kind":"execute","rawInput":{"command":"echo hi"},
                   "_meta":{"cognition.ai/subagent_context":{"parentAgentId":"agent1"}}}),
            json!({"sessionUpdate":"tool_call_update","toolCallId":"agent1","status":"completed",
                   "_meta":{"cognition.ai/subagent_completed":{"agentId":"agent1","success":false}}}),
            // A call that surfaces only after the agent finished still settles.
            json!({"sessionUpdate":"tool_call","toolCallId":"read:0#late","title":"read","kind":"read",
                   "_meta":{"cognition.ai/subagent_context":{"parentAgentId":"agent1"}}}),
        ];
        for update in updates {
            let update = serde_json::from_value(update).unwrap();
            handle_session_update(
                ProviderKind::Devin,
                SessionNotification::new("s", update),
                &events,
                &mut state,
                None,
            )
            .unwrap();
        }

        let seen = event_rx.try_iter().collect::<Vec<_>>();
        assert_eq!(seen.len(), 4);
        assert!(matches!(&seen[2], DriverEvent::RichActivity(item)
                if item.source_id.as_deref() == Some("exec:0#child")
                    && item.complete
                    && item.failed));
        assert!(matches!(&seen[3], DriverEvent::RichActivity(item)
                if item.source_id.as_deref() == Some("read:0#late")
                    && item.complete
                    && item.failed));
    }

    #[test]
    fn fx_context_notices_do_not_become_assistant_text() {
        let (events, event_rx) = crossbeam_channel::unbounded();
        let mut state = AcpStreamState::default();
        for text in [
            "[context] skill catalog omitted 19 entries",
            "skill discovery warning: candidate was skipped",
            "Hi! How can I help?",
            "[context] is ordinary text after the answer starts",
        ] {
            let update = serde_json::from_value(json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text}
            }))
            .unwrap();
            handle_session_update(
                ProviderKind::Fx,
                SessionNotification::new("s", update),
                &events,
                &mut state,
                None,
            )
            .unwrap();
        }

        let seen = event_rx.try_iter().collect::<Vec<_>>();
        assert_eq!(seen.len(), 2);
        assert!(matches!(&seen[0], DriverEvent::TextDelta(text) if text == "Hi! How can I help?"));
        assert!(matches!(&seen[1], DriverEvent::TextDelta(text) if text.starts_with("[context]")));
        assert!(state.produced_content);
    }

    #[test]
    fn session_info_update_forwards_a_generated_title() {
        let (events, event_rx) = crossbeam_channel::unbounded();
        let mut state = AcpStreamState::default();
        let update = serde_json::from_value(json!({
            "sessionUpdate": "session_info_update",
            "title": "  Polish the native agent interface  "
        }))
        .unwrap();
        handle_session_update(
            ProviderKind::Devin,
            SessionNotification::new("s", update),
            &events,
            &mut state,
            Some("build a really polished local agent interface for rust"),
        )
        .unwrap();
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            DriverEvent::AutoTitleUpdated(Some(title))
                if title.trim() == "Polish the native agent interface"
        ));
    }

    #[test]
    fn session_info_update_skips_devins_first_prompt_placeholder() {
        let (events, event_rx) = crossbeam_channel::unbounded();
        let mut state = AcpStreamState::default();
        let update = serde_json::from_value(json!({
            "sessionUpdate": "session_info_update",
            "title": "hi"
        }))
        .unwrap();
        handle_session_update(
            ProviderKind::Devin,
            SessionNotification::new("s", update),
            &events,
            &mut state,
            Some("hi"),
        )
        .unwrap();
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn grok_launch_passes_reasoning_effort_before_stdio() {
        let launch = launch_for(ProviderKind::Grok, Some("xhigh")).unwrap();
        assert_eq!(
            launch.args,
            ["agent", "--reasoning-effort", "xhigh", "stdio"]
        );
        let bare = launch_for(ProviderKind::Grok, None).unwrap();
        assert_eq!(bare.args, ["agent", "stdio"]);
    }

    #[test]
    fn grok_set_model_includes_reasoning_effort_meta() {
        let params = set_model_params(
            &SessionId::new("sess"),
            "grok-4.6",
            Some("xhigh"),
            ProviderKind::Grok,
        );
        assert_eq!(params["modelId"], "grok-4.6");
        assert_eq!(params["_meta"]["reasoningEffort"], "xhigh");
    }

    #[test]
    fn permission_reason_preserves_the_agents_explanation() {
        let tool_call = ToolCallUpdate::new(
            "tool-1",
            serde_json::from_value::<ToolCallUpdateFields>(json!({
                "title": "rm -rf build",
                "kind": "execute",
                "content": [
                    {"type":"content","content":{"type":"text","text":"Not in allowlist: rm"}}
                ]
            }))
            .unwrap(),
        );
        let request = RequestPermissionRequest::new("s", tool_call, Vec::new());
        let params = serde_json::to_value(request).unwrap();
        assert_eq!(
            permission_reason(&params).as_deref(),
            Some("Not in allowlist: rm")
        );
    }

    /// Drives a real agent through the SDK-backed driver. Ignored by default:
    /// it needs the CLI installed, credentials, and the network.
    #[test]
    #[ignore = "requires an installed, authenticated grok"]
    fn grok_prompt_response_from_the_sdk_finishes_the_turn() {
        let binary = crate::command_env::find_executable("grok").expect("grok is not installed");
        let (events, event_rx) = crate::driver::test_event_channel();
        let driver = AcpDriver::start(
            ProviderKind::Grok,
            DriverStartOptions {
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                binary,
                cwd: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                mode: RuntimeMode::FullAccess,
                model: Some("grok-4.5".into()),
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
                agent_preset: None,
                computer_use_enabled: false,
                agent: None,
                read_own_transcript: false,
                subagents: None,
                integrations: Vec::new(),
                provider_cursor: None,
            },
            events,
        )
        .expect("the ACP session should open");

        loop {
            let event = event_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("the agent should report its session");
            match event {
                DriverEvent::Connected {
                    provider_cursor: Some(ProviderResumeCursor::Grok { .. }),
                } => break,
                DriverEvent::Error(error) => panic!("the agent reported: {error}"),
                _ => {}
            }
        }
        driver.prompt("hi".into());
        let mut finished = None;
        while let Ok(event) = event_rx.recv_timeout(Duration::from_secs(120)) {
            match event {
                DriverEvent::TurnFinished { success, .. } => {
                    finished = Some(success);
                    break;
                }
                DriverEvent::Error(error) => panic!("the agent reported: {error}"),
                _ => {}
            }
        }
        assert_eq!(finished, Some(true));
    }

    /// Covers Cursor's provider-private parameterized picker with a model id
    /// whose CLI alias carries both effort and fast-mode values.
    #[test]
    #[ignore = "requires an installed, authenticated cursor-agent"]
    fn cursor_parameterized_model_selection_finishes_a_real_turn() {
        let binary = crate::command_env::find_executable("cursor-agent")
            .expect("cursor-agent is not installed");
        let (events, event_rx) = crate::driver::test_event_channel();
        let driver = AcpDriver::start(
            ProviderKind::Cursor,
            DriverStartOptions {
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                binary,
                cwd: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                mode: RuntimeMode::FullAccess,
                model: Some("cursor-grok-4.6-xhigh".into()),
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
                agent_preset: None,
                computer_use_enabled: false,
                agent: None,
                read_own_transcript: false,
                subagents: None,
                integrations: Vec::new(),
                provider_cursor: None,
            },
            events,
        )
        .expect("the ACP session should open");

        loop {
            let event = event_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("the agent should report its session");
            match event {
                DriverEvent::Connected {
                    provider_cursor: Some(ProviderResumeCursor::Cursor { .. }),
                } => break,
                DriverEvent::Error(error) => panic!("the agent reported: {error}"),
                _ => {}
            }
        }
        driver.prompt("Reply exactly OK.".into());

        let mut produced_text = false;
        let mut finished = None;
        while let Ok(event) = event_rx.recv_timeout(Duration::from_secs(120)) {
            match event {
                DriverEvent::TextDelta(text) => produced_text |= !text.is_empty(),
                DriverEvent::TurnFinished { success, .. } => {
                    finished = Some(success);
                    break;
                }
                DriverEvent::Error(error) => panic!("the agent reported: {error}"),
                _ => {}
            }
        }
        assert!(produced_text, "the Cursor turn produced no text");
        assert_eq!(finished, Some(true));
    }

    /// The invariant Kimi's silent failures break: a turn may finish
    /// successfully or report why it did not, but it must never claim success
    /// having produced nothing at all. Holds whether or not the account is
    /// currently able to serve the request.
    #[test]
    #[ignore = "requires an installed, authenticated kimi"]
    fn kimi_never_reports_an_empty_turn_as_a_success() {
        let binary = crate::command_env::find_executable("kimi").expect("kimi is not installed");
        let (events, event_rx) = crate::driver::test_event_channel();
        let driver = AcpDriver::start(
            ProviderKind::Kimi,
            DriverStartOptions {
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                binary,
                cwd: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                mode: RuntimeMode::FullAccess,
                model: None,
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
                agent_preset: None,
                computer_use_enabled: false,
                agent: None,
                read_own_transcript: false,
                subagents: None,
                integrations: Vec::new(),
                provider_cursor: None,
            },
            events,
        )
        .expect("the ACP session should open");

        loop {
            let event = event_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("the agent should report its session");
            match event {
                DriverEvent::Connected {
                    provider_cursor: Some(ProviderResumeCursor::Kimi { .. }),
                } => break,
                DriverEvent::Error(error) => panic!("the agent reported: {error}"),
                _ => {}
            }
        }
        driver.prompt("Say hi in three words.".into());

        let mut produced_content = false;
        let mut reported_error = None;
        let mut finished = None;
        while let Ok(event) = event_rx.recv_timeout(Duration::from_secs(120)) {
            match event {
                DriverEvent::TextDelta(_) | DriverEvent::ReasoningDelta(_) => {
                    produced_content = true;
                }
                DriverEvent::Error(error) => reported_error = Some(error),
                DriverEvent::TurnFinished { success, .. } => {
                    finished = Some(success);
                    break;
                }
                _ => {}
            }
        }

        match finished.expect("the turn should settle") {
            true => assert!(
                produced_content,
                "the turn was reported successful without producing anything"
            ),
            false => assert!(
                reported_error.is_some_and(|error| !error.trim().is_empty()),
                "the turn failed without naming a reason"
            ),
        }
    }

    /// Droid reports turn failures as JSON-RPC errors on `session/prompt` or a
    /// `refusal` stop reason, never as a lying clean end-turn, so the same
    /// success-means-content invariant applies.
    #[test]
    #[ignore = "requires an installed, authenticated droid"]
    fn droid_prompt_response_from_the_sdk_finishes_the_turn() {
        let binary = crate::command_env::find_executable("droid").expect("droid is not installed");
        let (events, event_rx) = crate::driver::test_event_channel();
        let driver = AcpDriver::start(
            ProviderKind::Droid,
            DriverStartOptions {
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                binary,
                cwd: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                mode: RuntimeMode::FullAccess,
                model: None,
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
                agent_preset: None,
                agent: None,
                read_own_transcript: false,
                subagents: None,
                computer_use_enabled: false,
                integrations: Vec::new(),
                provider_cursor: None,
            },
            events,
        )
        .expect("the ACP session should open");

        loop {
            let event = event_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("the agent should report its session");
            match event {
                DriverEvent::Connected {
                    provider_cursor: Some(ProviderResumeCursor::Droid { .. }),
                } => break,
                DriverEvent::Error(error) => panic!("the agent reported: {error}"),
                _ => {}
            }
        }
        driver.prompt("Say hi in three words.".into());

        let mut produced_content = false;
        let mut reported_error = None;
        let mut finished = None;
        while let Ok(event) = event_rx.recv_timeout(Duration::from_secs(120)) {
            match event {
                DriverEvent::TextDelta(_) | DriverEvent::ReasoningDelta(_) => {
                    produced_content = true;
                }
                DriverEvent::Error(error) => reported_error = Some(error),
                DriverEvent::TurnFinished { success, .. } => {
                    finished = Some(success);
                    break;
                }
                _ => {}
            }
        }

        match finished.expect("the turn should settle") {
            true => assert!(
                produced_content,
                "the turn was reported successful without producing anything"
            ),
            false => assert!(
                reported_error.is_some_and(|error| !error.trim().is_empty()),
                "the turn failed without naming a reason"
            ),
        }
    }

    /// Live probe for the memory-injection redesign: a clean first prompt to
    /// a real Devin ACP session, then a `<project-memory>`-style steer while
    /// that turn is in flight. Once the turn settles, watch Devin's
    /// sessions.db for the generated title — a memory-flavored title means
    /// the generator reads full session context, a task-flavored one means
    /// it reads the first message.
    ///
    /// Run explicitly:
    /// `cargo test -p waku-core devin_title_probe -- --ignored --nocapture`
    #[test]
    #[ignore = "live provider probe"]
    fn devin_title_probe_memory_steer() {
        const CLEAN_PROMPT: &str = "Reply with exactly the word: atlantic";

        let binary = crate::command_env::find_executable("devin").expect("devin is not installed");
        let cwd = std::env::temp_dir().join("waku-devin-title-probe");
        std::fs::create_dir_all(&cwd).unwrap();
        let agent =
            catalog_agent(ProviderKind::Devin, &binary, &cwd).expect("the ACP agent should spawn");

        let updates = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = Arc::clone(&updates);
        let probe_cwd = cwd.clone();
        let started = std::time::Instant::now();
        let request = Client
            .builder()
            .name("waku-devin-title-probe")
            .on_receive_notification(
                async move |notification: SessionNotification, _connection| {
                    let Ok(update) = serde_json::to_value(&notification.update) else {
                        return Ok(());
                    };
                    let kind = update
                        .get("sessionUpdate")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let detail = if kind == "session_info_update" {
                        format!(" {update}")
                    } else {
                        String::new()
                    };
                    captured.lock().push(format!(
                        "{:>6.1}s {kind}{detail}",
                        started.elapsed().as_secs_f64()
                    ));
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _connection| {
                    let allow = request
                        .options
                        .iter()
                        .find(|option| option.kind == PermissionOptionKind::AllowOnce)
                        .or_else(|| {
                            request
                                .options
                                .iter()
                                .find(|option| option.kind == PermissionOptionKind::AllowAlways)
                        })
                        .or_else(|| request.options.first());
                    let outcome = match allow {
                        Some(option) => RequestPermissionOutcome::Selected(
                            SelectedPermissionOutcome::new(option.option_id.clone()),
                        ),
                        None => RequestPermissionOutcome::Cancelled,
                    };
                    responder.respond(RequestPermissionResponse::new(outcome))?;
                    Ok(Handled::Yes)
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
                connection
                    .send_request(
                        InitializeRequest::new(ProtocolVersion::V1)
                            .client_capabilities(ClientCapabilities::new().terminal(false))
                            .client_info(Implementation::new(
                                "waku-title-probe",
                                env!("CARGO_PKG_VERSION"),
                            )),
                    )
                    .block_task()
                    .await?;
                let session = connection
                    .send_request(NewSessionRequest::new(probe_cwd.clone()))
                    .block_task()
                    .await?;
                let session_id = session.session_id;
                eprintln!("probe session: {session_id}");

                let prompt = |text: &str| {
                    PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(text.to_owned()))],
                    )
                };
                let memory = "<project-memory>\n\
                    This project has persistent memory distilled from earlier sessions.\n\n\
                    ## Memory\n\
                    The telemetry pipeline stores segments in a custom columnar format \
                    codenamed ZEBRA-CONSTELLATION.\n\
                    Operators restart the night crawler service with `crawlerctl bounce`.\n\
                    </project-memory>\n\n\
                    Context for this session — no action needed.";
                // The steer lands while the first prompt is in flight — the
                // exact ordering a hidden memory injection would take.
                let first = connection.send_request(prompt(CLEAN_PROMPT));
                let second = connection.send_request(prompt(memory));
                eprintln!("first prompt: {:?}", first.block_task().await);
                eprintln!("second prompt: {:?}", second.block_task().await);
                Ok::<_, agent_client_protocol::Error>(session_id.to_string())
            });
        let session_id = smol::block_on(smol::future::race(
            async move { request.await.map_err(|error| anyhow!("{error}")) },
            async move {
                smol::Timer::after(Duration::from_secs(300)).await;
                Err(anyhow!("the probe session timed out"))
            },
        ))
        .expect("the probe session should finish");

        eprintln!("turn structure:");
        for entry in updates.lock().iter() {
            eprintln!("  {entry}");
        }

        // The generated title lands after the turn. session/load replays the
        // stored title as session_info_update, so reload on a fresh
        // connection until it reports a real title.
        for attempt in 0..30 {
            std::thread::sleep(Duration::from_secs(3));
            let titles = Arc::new(Mutex::new(Vec::<String>::new()));
            let captured = Arc::clone(&titles);
            let agent = match catalog_agent(ProviderKind::Devin, &binary, &cwd) {
                Ok(agent) => agent,
                Err(_) => continue,
            };
            let load_cwd = cwd.clone();
            let load_id = session_id.clone();
            let request = Client
                .builder()
                .name("waku-devin-title-reload")
                .on_receive_notification(
                    async move |notification: SessionNotification, _connection| {
                        if let Ok(update) = serde_json::to_value(&notification.update)
                            && update.get("sessionUpdate").and_then(Value::as_str)
                                == Some("session_info_update")
                            && let Some(title) = update.get("title").and_then(Value::as_str)
                        {
                            captured.lock().push(title.to_owned());
                        }
                        Ok(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
                    connection
                        .send_request(
                            InitializeRequest::new(ProtocolVersion::V1)
                                .client_capabilities(ClientCapabilities::new().terminal(false)),
                        )
                        .block_task()
                        .await?;
                    connection
                        .send_request(LoadSessionRequest::new(load_id, load_cwd))
                        .block_task()
                        .await?;
                    // Replay finishes around the load response; a short grace
                    // catches the trailing info update.
                    smol::Timer::after(Duration::from_millis(200)).await;
                    Ok::<_, agent_client_protocol::Error>(())
                });
            let _ = smol::block_on(smol::future::race(
                async move { request.await.map_err(|error| anyhow!("{error}")) },
                async move {
                    smol::Timer::after(Duration::from_secs(15)).await;
                    Err(anyhow!("reload timed out"))
                },
            ));
            let titles = std::mem::take(&mut *titles.lock());
            for title in &titles {
                eprintln!("reload {attempt}: title {title:?}");
            }
            if titles
                .iter()
                .any(|title| !crate::devin_session::is_placeholder_title(title, Some(CLEAN_PROMPT)))
            {
                break;
            }
        }
    }
}
