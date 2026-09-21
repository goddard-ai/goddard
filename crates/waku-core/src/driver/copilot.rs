//! GitHub Copilot sessions through the official `github-copilot-sdk` crate.
//!
//! The SDK owns the `copilot --server --stdio` process lifecycle and the
//! JSON-RPC plumbing other drivers hand-roll: one session per task, events on
//! `session.subscribe()`, and focused handler traits for the requests the CLI
//! delegates to its host. The driver shape still matches the rest of this
//! module — a dedicated thread owns a current-thread Tokio runtime, commands
//! go in through a channel, and `DriverEvent`s come out.
//!
//! The SDK's `Client` can multiplex sessions on one process; this driver still
//! keeps one client per session so `Drop` maps to "this task's runtime is
//! gone" the way every other transport's does.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;

use anyhow::{Context as _, anyhow};
use async_trait::async_trait;
use github_copilot_sdk::handler::{
    ExitPlanModeHandler, ExitPlanModeResult, PermissionHandler, PermissionResult, UserInputHandler,
    UserInputResponse,
};
use github_copilot_sdk::rpc::{
    PermissionDecision, PermissionDecisionApproveForSession,
    PermissionDecisionApproveForSessionApproval,
    PermissionDecisionApproveForSessionApprovalCommands,
    PermissionDecisionApproveForSessionApprovalRead,
    PermissionDecisionApproveForSessionApprovalWrite,
};
use github_copilot_sdk::session_events::{
    AssistantMessageData, AssistantMessageDeltaData, AssistantReasoningDeltaData, ContextTier,
    SessionEventType, SessionIdleData, SessionTitleChangedData, SessionUsageInfoData,
    ToolExecutionCompleteData, ToolExecutionStartData,
};
use github_copilot_sdk::types::{
    Attachment, ExitPlanModeData, MessageOptions, PermissionRequestData, PermissionRequestKind,
    RequestId, ResumeSessionConfig, SessionConfig, SessionEvent, SessionId, SetModelOptions,
};
use github_copilot_sdk::{CliProgram, Client, ClientInfo, ClientOptions};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;

use super::activity;
use crate::driver::{
    DriverControl, DriverEventSender, DriverEventSink, DriverStartOptions, SessionOptions,
};
use crate::model::{
    ActivityKind, DriverEvent, MessageAttachment, PermissionOption, ProviderResumeCursor,
    RuntimeMode, UserInputAnswer, UserInputOption, UserInputQuestion,
};

enum CommandMessage {
    Prompt {
        text: String,
        attachments: Vec<MessageAttachment>,
    },
    Cancel,
    /// Model, reasoning effort, and context tier ride one `session.set_model`
    /// call; the mode half lives in the handler's shared state.
    ApplyOptions {
        model: String,
        reasoning_effort: Option<String>,
        context_tier: Option<ContextTier>,
    },
    Shutdown,
}

/// State shared between the driver's synchronous `DriverControl` face and the
/// async handlers running inside the SDK's dispatch tasks. A pending host
/// request parks a oneshot here; `respond`/`respond_user_input` resolve it
/// directly, keeping the answer path off the command channel entirely.
#[derive(Default)]
struct Shared {
    options: Option<SessionOptions>,
    /// The evaluation backend answering `Auto`-mode permission requests,
    /// snapshotted at session start.
    eval: Option<Arc<waku_protocol::eval::EvalSettings>>,
    permissions: HashMap<String, oneshot::Sender<String>>,
    user_inputs: HashMap<String, oneshot::Sender<Vec<UserInputAnswer>>>,
}

pub struct CopilotDriver {
    commands: UnboundedSender<CommandMessage>,
    shared: Arc<Mutex<Shared>>,
}

struct CopilotRun {
    binary: std::path::PathBuf,
    cwd: std::path::PathBuf,
    model: Option<String>,
    reasoning_effort: Option<String>,
    context_window: Option<String>,
    agent_preset: Option<String>,
    agent: Option<crate::agent::AgentLaunchEnv>,
    subagents: Option<waku_protocol::model::SubagentSpec>,
    resume_session_id: Option<String>,
    events: DriverEventSender,
    shared: Arc<Mutex<Shared>>,
    commands: UnboundedReceiver<CommandMessage>,
}

impl CopilotDriver {
    pub fn start(options: DriverStartOptions, events: DriverEventSender) -> anyhow::Result<Self> {
        let DriverStartOptions {
            binary,
            cwd,
            mode,
            model,
            reasoning_effort,
            service_tier,
            context_window,
            agent_preset,
            computer_use_enabled: _,
            agent,
            subagents,
            integrations: _,
            provider_cursor,
            eval,
            sandbox: _,
            allow_model_fallback: _,
        } = options;
        let resume_session_id = match provider_cursor {
            Some(ProviderResumeCursor::Copilot { session_id }) => Some(session_id),
            Some(cursor) => {
                return Err(anyhow!(
                    "cannot resume GitHub Copilot from a {} cursor",
                    cursor.provider().display_name()
                ));
            }
            None => None,
        };
        let shared = Arc::new(Mutex::new(Shared {
            options: Some(SessionOptions {
                mode,
                model: model.clone(),
                reasoning_effort: reasoning_effort.clone(),
                service_tier,
                context_window: context_window.clone(),
            }),
            eval: eval.map(Arc::new),
            ..Default::default()
        }));
        // `UnboundedSender::send` is synchronous, so the `DriverControl`
        // methods talk straight into the runtime task — no pump thread.
        let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let thread_events = events.clone();
        let thread_shared = shared.clone();
        thread::Builder::new()
            .name("waku-copilot".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = thread_events
                            .send(DriverEvent::Error(format!("GitHub Copilot: {error}")));
                        let _ = thread_events.send(DriverEvent::ProcessExited);
                        return;
                    }
                };
                runtime.block_on(run(CopilotRun {
                    binary,
                    cwd,
                    model,
                    reasoning_effort,
                    context_window,
                    agent_preset,
                    agent,
                    subagents,
                    resume_session_id,
                    events: thread_events.clone(),
                    shared: thread_shared,
                    commands: command_rx,
                }));
                let _ = thread_events.send(DriverEvent::ProcessExited);
            })
            .context("failed to start the GitHub Copilot runtime")?;

        Ok(Self { commands, shared })
    }
}

async fn run(launch: CopilotRun) {
    let events = launch.events.clone();
    if let Err(error) = run_inner(launch).await {
        let _ = events.send(DriverEvent::Error(format!("GitHub Copilot: {error}")));
    }
}

async fn run_inner(launch: CopilotRun) -> anyhow::Result<()> {
    let CopilotRun {
        binary,
        cwd,
        model,
        reasoning_effort,
        context_window,
        agent_preset,
        agent,
        subagents,
        resume_session_id,
        events,
        shared,
        mut commands,
    } = launch;

    let mut client_options = ClientOptions::default();
    client_options.program = CliProgram::Path(binary.clone());
    client_options.working_directory = cwd.clone();
    client_options.env = crate::command_env::spawn_environment(&binary, agent.as_ref());
    client_options.client_info = Some(
        ClientInfo::new()
            .with_application_name(crate::identity::APP_NAME)
            .with_application_version(env!("CARGO_PKG_VERSION")),
    );
    let client = match Client::start(client_options).await {
        Ok(client) => client,
        Err(error) => {
            let _ = events.send(DriverEvent::Error(format!("GitHub Copilot: {error}")));
            return Ok(());
        }
    };

    let handler = Arc::new(CopilotHandler {
        events: events.clone(),
        shared,
    });
    let session = match resume_session_id {
        Some(session_id) => {
            let mut resume = ResumeSessionConfig::new(SessionId::new(session_id))
                .with_working_directory(&cwd)
                .with_client_name(crate::identity::APP_NAME)
                .with_permission_handler(handler.clone())
                .with_user_input_handler(handler.clone())
                .with_exit_plan_mode_handler(handler)
                .with_include_sub_agent_streaming_events(true);
            resume.streaming = Some(true);
            resume.model = model;
            resume.reasoning_effort = reasoning_effort;
            resume.context_tier = context_window;
            resume.agent = agent_preset;
            if let Some(agents) = subagents
                .as_ref()
                .and_then(crate::subagents::copilot_custom_agents)
            {
                resume = resume.with_custom_agents(agents);
            }
            client.resume_session(resume).await
        }
        None => {
            let mut config = SessionConfig::default()
                .with_working_directory(&cwd)
                .with_client_name(crate::identity::APP_NAME)
                .with_permission_handler(handler.clone())
                .with_user_input_handler(handler.clone())
                .with_exit_plan_mode_handler(handler)
                .with_include_sub_agent_streaming_events(true);
            config.streaming = Some(true);
            config.model = model;
            config.reasoning_effort = reasoning_effort;
            config.context_tier = context_window;
            config.agent = agent_preset;
            if let Some(agents) = subagents
                .as_ref()
                .and_then(crate::subagents::copilot_custom_agents)
            {
                config = config.with_custom_agents(agents);
            }
            client.create_session(config).await
        }
    };
    let session = match session {
        Ok(session) => session,
        Err(error) => {
            let _ = events.send(DriverEvent::Error(format!("GitHub Copilot: {error}")));
            let _ = client.stop().await;
            return Ok(());
        }
    };
    let _ = events.send(DriverEvent::Connected {
        provider_cursor: Some(ProviderResumeCursor::Copilot {
            session_id: session.id().as_str().to_owned(),
        }),
    });

    let mut stream = CopilotStream::default();
    let mut events_rx = session.subscribe();
    // One select loop serves the command channel and the event subscription.
    // The SDK dispatches handler callbacks on sibling runtime tasks, so a
    // parked permission prompt never stalls the command that answers it.
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    CommandMessage::Prompt { text, attachments } => {
                        let mut message = MessageOptions::new(text);
                        let attachments = copilot_attachments(attachments);
                        if !attachments.is_empty() {
                            message = message.with_attachments(attachments);
                        }
                        if let Err(error) = session.send(message).await {
                            let _ = events.send(DriverEvent::Error(format!(
                                "GitHub Copilot: {error}"
                            )));
                        }
                    }
                    CommandMessage::Cancel => {
                        if let Err(error) = session.abort().await {
                            let _ = events.send(DriverEvent::Error(format!(
                                "GitHub Copilot: {error}"
                            )));
                        }
                    }
                    CommandMessage::ApplyOptions {
                        model,
                        reasoning_effort,
                        context_tier,
                    } => {
                        let set_model = SetModelOptions {
                            reasoning_effort,
                            context_tier,
                            ..Default::default()
                        };
                        if let Err(error) = session.set_model(&model, Some(set_model)).await {
                            let _ = events.send(DriverEvent::Error(format!(
                                "GitHub Copilot: {error}"
                            )));
                        }
                    }
                    CommandMessage::Shutdown => break,
                }
            }
            event = events_rx.recv() => {
                match event {
                    Ok(event) => handle_event(&event, &events, &mut stream),
                    Err(error)
                        if matches!(
                            error.kind(),
                            github_copilot_sdk::subscription::RecvErrorKind::Lagged(_)
                        ) =>
                    {
                        // A lagged subscriber loses transient deltas only —
                        // persisted state is recoverable through `get_events`.
                        continue;
                    }
                    Err(_) => break,
                }
            }
        }
    }

    let _ = session.disconnect().await;
    let _ = client.stop().await;
    Ok(())
}

#[derive(Default)]
struct CopilotStream {
    /// The session settles turns through `session.idle`; `assistant.turn_start`
    /// opens one here so a late idle cannot fabricate a finish.
    turn_open: bool,
    /// `assistant.message_delta` already streamed these message ids — the
    /// settled `assistant.message` must not re-emit their full content.
    streamed_messages: HashSet<String>,
    /// Tool-call start data kept so a completion can reuse its title.
    tools: HashMap<String, (ActivityKind, String)>,
}

fn handle_event(event: &SessionEvent, events: &impl DriverEventSink, stream: &mut CopilotStream) {
    // Sub-agent events carry `agent_id`; only the root agent's text belongs in
    // the main transcript. Tool executions stay visible regardless — a helper
    // running `bash` is real work the user should see.
    let root_agent = event.agent_id.is_none();
    match event.parsed_type() {
        SessionEventType::AssistantTurnStart => {
            stream.turn_open = true;
            let _ = events.send(DriverEvent::TurnStarted);
        }
        SessionEventType::AssistantMessageDelta if root_agent => {
            if let Some(data) = event.typed_data::<AssistantMessageDeltaData>() {
                stream.streamed_messages.insert(data.message_id.clone());
                if !data.delta_content.is_empty() {
                    let _ = events.send(DriverEvent::TextDelta(data.delta_content));
                }
            }
        }
        SessionEventType::AssistantMessage if root_agent => {
            if let Some(data) = event.typed_data::<AssistantMessageData>()
                && !data.content.is_empty()
                && !stream.streamed_messages.contains(&data.message_id)
            {
                // Streaming was off or the provider sent no deltas — the final
                // message is the only place the text exists.
                let _ = events.send(DriverEvent::TextDelta(data.content));
            }
        }
        SessionEventType::AssistantReasoningDelta if root_agent => {
            if let Some(data) = event.typed_data::<AssistantReasoningDeltaData>()
                && !data.delta_content.is_empty()
            {
                let _ = events.send(DriverEvent::ReasoningDelta(data.delta_content));
            }
        }
        SessionEventType::ToolExecutionStart => {
            if let Some(data) = event.typed_data::<ToolExecutionStartData>() {
                let kind = ActivityKind::from_tool_name(&data.tool_name);
                let title = tool_title(&data.tool_name, data.arguments.as_ref());
                stream
                    .tools
                    .insert(data.tool_call_id.clone(), (kind, title.clone()));
                let _ = events.send(DriverEvent::RichActivity(
                    activity::tool_activity(
                        Some(data.tool_call_id),
                        kind,
                        title,
                        data.arguments.as_ref(),
                        None,
                        None,
                        false,
                        false,
                    )
                    .with_tool_name(Some(&data.tool_name)),
                ));
            }
        }
        SessionEventType::ToolExecutionComplete => {
            if let Some(data) = event.typed_data::<ToolExecutionCompleteData>() {
                let (kind, title) = stream
                    .tools
                    .remove(&data.tool_call_id)
                    .unwrap_or((ActivityKind::Tool, tr!("activity.tool")));
                let output = data
                    .result
                    .as_ref()
                    .and_then(|result| serde_json::to_value(result).ok())
                    .or_else(|| {
                        data.error
                            .as_ref()
                            .and_then(|error| serde_json::to_value(error).ok())
                    });
                let item = activity::tool_activity(
                    Some(data.tool_call_id),
                    kind,
                    title,
                    None,
                    output.as_ref(),
                    output.as_ref(),
                    !data.success,
                    true,
                );
                let _ = events.send(DriverEvent::RichActivity(item));
            }
        }
        SessionEventType::SessionIdle => {
            let aborted = event
                .typed_data::<SessionIdleData>()
                .and_then(|data| data.aborted)
                .unwrap_or(false);
            if std::mem::take(&mut stream.turn_open) {
                let _ = events.send(DriverEvent::TurnFinished {
                    success: !aborted,
                    summary: None,
                    summary_i18n: None,
                });
            }
        }
        SessionEventType::SessionTitleChanged => {
            if let Some(data) = event.typed_data::<SessionTitleChangedData>() {
                let _ = events.send(DriverEvent::AutoTitleUpdated(Some(data.title)));
            }
        }
        SessionEventType::SessionUsageInfo => {
            if let Some(data) = event.typed_data::<SessionUsageInfoData>() {
                let _ = events.send(DriverEvent::UsageUpdated {
                    context_tokens: u64::try_from(data.current_tokens).ok(),
                    context_window: u64::try_from(data.token_limit).ok(),
                });
            }
        }
        SessionEventType::SessionError => {
            // Transient `model_call` errors retry inside the CLI's agent loop;
            // surfacing them would flash the UI for a recoverable condition.
            if event.is_transient_error() {
                return;
            }
            let message = event
                .data
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_owned();
            let _ = events.send(DriverEvent::Error(message.clone()));
            if std::mem::take(&mut stream.turn_open) {
                let _ = events.send(DriverEvent::TurnFinished {
                    success: false,
                    summary: Some(message),
                    summary_i18n: None,
                });
            }
        }
        _ => {}
    }
}

/// The permission request's most specific human-readable target — command,
/// path, or URL — falling back to the generic tool prompt.
fn permission_title(
    data: &PermissionRequestData,
) -> (String, Option<waku_protocol::WireTranslation>) {
    for key in [
        "command",
        "fileName",
        "path",
        "url",
        "toolName",
        "serverName",
        "tool",
    ] {
        if let Some(value) = permission_string(data, key) {
            return (value, None);
        }
    }
    let pair = localized!("permission.run_a_tool");
    (pair.0, Some(pair.1))
}

fn permission_detail(
    data: &PermissionRequestData,
) -> (String, Option<waku_protocol::WireTranslation>) {
    if let Some(detail) = permission_string(data, "description") {
        return (detail, None);
    }
    let pair = localized!("permission.agent_asks_for_permission");
    (pair.0, Some(pair.1))
}

/// The objects a permission field may live in, outermost first. The CLI sends
/// the request nested under `permissionRequest`; the SDK copies the whole
/// event into `extra`, so that is where the fields actually are.
fn permission_containers(data: &PermissionRequestData) -> impl Iterator<Item = &Value> {
    std::iter::once(&data.extra)
        .chain(data.extra.get("permissionRequest"))
        .chain(data.extra.get("request"))
}

fn permission_string(data: &PermissionRequestData, key: &str) -> Option<String> {
    permission_containers(data)
        .find_map(|container| container.get(key).and_then(Value::as_str))
        .map(str::to_owned)
}

/// The CLI's session-scoped approval, which remembers a command, path class,
/// or domain for the rest of the session. Kinds it cannot express decline the
/// option entirely rather than silently degrading to approve-once.
fn permission_for_session(data: &PermissionRequestData) -> Option<PermissionDecision> {
    let approval = match data.kind? {
        PermissionRequestKind::Read => PermissionDecisionApproveForSessionApproval::Read(
            PermissionDecisionApproveForSessionApprovalRead::default(),
        ),
        PermissionRequestKind::Write => PermissionDecisionApproveForSessionApproval::Write(
            PermissionDecisionApproveForSessionApprovalWrite::default(),
        ),
        PermissionRequestKind::Shell => {
            let identifier = permission_string(data, "commandIdentifier")
                .or_else(|| permission_string(data, "command_identifier"))?;
            PermissionDecisionApproveForSessionApproval::Commands(
                PermissionDecisionApproveForSessionApprovalCommands {
                    command_identifiers: vec![identifier],
                    ..Default::default()
                },
            )
        }
        PermissionRequestKind::Url => {
            let url = permission_string(data, "url")?;
            let domain = url
                .split_once("://")
                .map_or(url.as_str(), |(_, value)| value)
                .split(['/', '?', '#'])
                .next()
                .and_then(|authority| authority.rsplit('@').next())
                .and_then(|host| host.split(':').next())
                .filter(|host| !host.is_empty())
                .map(|host| host.to_ascii_lowercase())?;
            return Some(PermissionDecision::ApproveForSession(
                PermissionDecisionApproveForSession {
                    approval: None,
                    domain: Some(domain),
                    ..Default::default()
                },
            ));
        }
        _ => return None,
    };
    Some(PermissionDecision::ApproveForSession(
        PermissionDecisionApproveForSession {
            approval: Some(approval),
            domain: None,
            ..Default::default()
        },
    ))
}

fn tool_title(tool_name: &str, arguments: Option<&Value>) -> String {
    activity::input_title(arguments)
        .or_else(|| {
            let arguments = arguments?;
            for key in ["command", "description", "path", "fileName", "url", "query"] {
                if let Some(value) = arguments.get(key).and_then(Value::as_str) {
                    let value = value.trim();
                    if !value.is_empty() {
                        return Some(value.to_owned());
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| tool_name.to_owned())
}

fn context_tier(window: &str) -> ContextTier {
    match window.trim().to_ascii_lowercase().as_str() {
        "long_context" | "1m" | "long" => ContextTier::LongContext,
        "default" => ContextTier::Default,
        _ => ContextTier::Unknown,
    }
}

/// The SDK-dispatched callbacks for the session. Every handler parks on a
/// oneshot the `DriverControl` side resolves, so the UI's answer path is the
/// same `respond`/`respond_user_input` every other provider uses.
struct CopilotHandler {
    events: DriverEventSender,
    shared: Arc<Mutex<Shared>>,
}

#[async_trait]
impl PermissionHandler for CopilotHandler {
    async fn handle(
        &self,
        _session_id: SessionId,
        request_id: RequestId,
        data: PermissionRequestData,
    ) -> PermissionResult {
        if data.managed_settings_enabled {
            return PermissionResult::user_not_available();
        }
        let (mode, eval) = {
            let shared = self.shared.lock();
            (
                shared
                    .options
                    .as_ref()
                    .map(|options| options.mode)
                    .unwrap_or(RuntimeMode::Ask),
                shared.eval.clone(),
            )
        };
        // `Auto` never answers blindly: the review replies when a backend is
        // configured and the user does when it is not.
        let interactive = mode == RuntimeMode::Ask
            || (mode == RuntimeMode::Auto && eval.is_none())
            || data.managed_approval_required == Some(true);
        if !interactive {
            match (mode, eval) {
                (RuntimeMode::Auto, Some(eval)) => {
                    let action = crate::permission_review::PendingAction {
                        provider: "copilot",
                        tool: permission_string(&data, "toolName")
                            .or_else(|| permission_string(&data, "tool"))
                            .unwrap_or_else(|| format!("{:?}", data.kind)),
                        arguments: data.extra.to_string(),
                        call_id: request_id.to_string(),
                        detail: permission_string(&data, "description"),
                    };
                    let verdict = tokio::task::spawn_blocking(move || {
                        crate::permission_review::review_action(&eval, &action)
                    })
                    .await
                    .unwrap_or(crate::permission_review::ReviewVerdict::Escalate);
                    if verdict == crate::permission_review::ReviewVerdict::Allow {
                        return PermissionResult::approve_once();
                    }
                }
                _ => return PermissionResult::approve_once(),
            }
        }

        let key = request_id.to_string();
        let (sender, receiver) = oneshot::channel();
        self.shared.lock().permissions.insert(key.clone(), sender);
        let mut options = vec![PermissionOption::keyed(
            "allow",
            localized!("permission.allow_once"),
            true,
        )];
        if permission_for_session(&data).is_some() {
            options.push(PermissionOption::keyed(
                "allowSession",
                localized!("permission.allow_for_session"),
                true,
            ));
        }
        options.push(PermissionOption::keyed(
            "deny",
            localized!("common.deny"),
            false,
        ));
        let (title, title_i18n) = permission_title(&data);
        let (detail, detail_i18n) = permission_detail(&data);
        let _ = self.events.send(DriverEvent::Permission {
            request_id: key,
            title,
            title_i18n,
            detail,
            detail_i18n,
            options,
        });
        match receiver.await {
            Ok(option_id) => match option_id.as_str() {
                "allow" => PermissionResult::approve_once(),
                "allowSession" => permission_for_session(&data)
                    .map(PermissionResult::from)
                    .unwrap_or_else(PermissionResult::approve_once),
                _ => PermissionResult::reject(None),
            },
            // The driver went away mid-request; abstain so another attached
            // client could still answer, though there normally is none.
            Err(_) => PermissionResult::no_result(),
        }
    }
}

#[async_trait]
impl UserInputHandler for CopilotHandler {
    async fn handle(
        &self,
        _session_id: SessionId,
        question: String,
        choices: Option<Vec<String>>,
        _allow_freeform: Option<bool>,
    ) -> Option<UserInputResponse> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let options: Vec<UserInputOption> = choices
            .unwrap_or_default()
            .into_iter()
            .map(|label| UserInputOption {
                label,
                description: None,
            })
            .collect();
        let (sender, receiver) = oneshot::channel();
        self.shared
            .lock()
            .user_inputs
            .insert(request_id.clone(), sender);
        let _ = self.events.send(DriverEvent::UserInputRequested {
            request_id,
            questions: vec![UserInputQuestion {
                id: "answer".to_owned(),
                header: "Question".to_owned(),
                question,
                options,
                multi_select: false,
            }],
        });
        let answers = receiver.await.ok()?;
        let answer = answers
            .iter()
            .find(|answer| answer.question_id == "answer")
            .or_else(|| answers.first())?
            .answers
            .first()?
            .clone();
        Some(UserInputResponse {
            answer,
            was_freeform: true,
        })
    }
}

#[async_trait]
impl ExitPlanModeHandler for CopilotHandler {
    async fn handle(&self, _session_id: SessionId, data: ExitPlanModeData) -> ExitPlanModeResult {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.shared
            .lock()
            .permissions
            .insert(request_id.clone(), sender);
        let (title, title_i18n) = localized!("permission.exit_plan_mode");
        let _ = self.events.send(DriverEvent::Permission {
            request_id,
            title,
            title_i18n: Some(title_i18n),
            detail: data.summary,
            options: vec![
                PermissionOption::keyed("allow", localized!("permission.allow_once"), true),
                PermissionOption::keyed("deny", localized!("common.deny"), false),
            ],
            detail_i18n: None,
        });
        match receiver.await {
            Ok(option_id) => ExitPlanModeResult {
                approved: option_id == "allow",
                selected_action: None,
                feedback: None,
            },
            Err(_) => ExitPlanModeResult {
                approved: false,
                selected_action: None,
                feedback: None,
            },
        }
    }
}

/// Composer chips map onto the SDK's typed attachments: directories attach
/// as `Directory`, everything else — including images — as a `File` path the
/// CLI reads itself. The merged `@`-mention text stays in the prompt, the
/// same shape the CLI produces for its own mentions.
fn copilot_attachments(attachments: Vec<MessageAttachment>) -> Vec<Attachment> {
    attachments
        .into_iter()
        .map(|attachment| {
            if attachment.is_dir {
                Attachment::Directory {
                    path: attachment.path,
                    display_name: Some(attachment.name),
                }
            } else {
                Attachment::File {
                    path: attachment.path,
                    display_name: Some(attachment.name),
                    line_range: None,
                }
            }
        })
        .collect()
}

impl DriverControl for CopilotDriver {
    fn prompt(&self, prompt: String) {
        let _ = self.commands.send(CommandMessage::Prompt {
            text: prompt,
            attachments: Vec::new(),
        });
    }

    fn prompt_with_attachments(&self, prompt: String, attachments: Vec<MessageAttachment>) {
        let _ = self.commands.send(CommandMessage::Prompt {
            text: prompt,
            attachments,
        });
    }

    fn cancel(&self) {
        let _ = self.commands.send(CommandMessage::Cancel);
    }

    fn respond(&self, request_id: String, option_id: String) {
        if let Some(sender) = self.shared.lock().permissions.remove(&request_id) {
            let _ = sender.send(option_id);
        }
    }

    fn respond_user_input(&self, request_id: String, answers: Vec<UserInputAnswer>) {
        if let Some(sender) = self.shared.lock().user_inputs.remove(&request_id) {
            let _ = sender.send(answers);
        }
    }

    fn apply_options(&self, options: SessionOptions) -> bool {
        let mut shared = self.shared.lock();
        let previous = shared.options.replace(options.clone());
        if previous.as_ref() == Some(&options) {
            return true;
        }
        // Mode lives in our own permission handler, so it always absorbs.
        // Model carries effort and context tier through `session.set_model` —
        // with no explicit model there is nothing to retarget, so those
        // changes ask for a restart instead.
        let modelled_change = previous.as_ref().is_none_or(|previous| {
            previous.model != options.model
                || previous.reasoning_effort != options.reasoning_effort
                || previous.service_tier != options.service_tier
                || previous.context_window != options.context_window
        });
        if !modelled_change {
            return true;
        }
        let Some(model) = options.model else {
            return false;
        };
        let _ = self.commands.send(CommandMessage::ApplyOptions {
            model,
            reasoning_effort: options.reasoning_effort,
            context_tier: options.context_window.as_deref().map(context_tier),
        });
        true
    }

    fn rollback(&self, _turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
        Err(anyhow!(
            "conversation rollback is not supported by this provider transport"
        ))
    }
}

impl Drop for CopilotDriver {
    fn drop(&mut self) {
        let _ = self.commands.send(CommandMessage::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Drives a real agent through the SDK-backed driver. Ignored by default:
    /// it needs `copilot` installed, credentials, and the network.
    #[test]
    #[ignore = "requires an installed, authenticated copilot"]
    fn copilot_prompt_finishes_a_real_turn() {
        let binary =
            crate::command_env::find_executable("copilot").expect("copilot is not installed");
        let (events, event_rx) = crate::driver::test_event_channel();
        let driver = CopilotDriver::start(
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
                subagents: None,
                integrations: Vec::new(),
                provider_cursor: None,
            },
            events,
        )
        .expect("the Copilot session should open");

        loop {
            let event = event_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("the agent should report its session");
            match event {
                DriverEvent::Connected {
                    provider_cursor: Some(ProviderResumeCursor::Copilot { .. }),
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
}
