use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use uuid::Uuid;

use crate::attachments::{AttachmentUpload, StoredAttachment};
use crate::automations::{Automation, AutomationInput, AutomationRun, AutomationsState};
use crate::computer_use::ComputerPermissions;
use crate::custom_commands::CustomCommand;
use crate::eval::{EvalQuestion, EvalSettings, EvalUsageStats, Evaluation};
use crate::model::{
    AgentAskOutcome, AgentModelOption, AgentSession, AgentSessionSearchHit, AgentSessionTranscript,
    GoalOperation, MessageAttachment, Project, ProviderKind, ProviderProbe, ProviderResumeCursor,
    ProviderSessionCatalogStatus, ProviderSessionHistory, ProviderSessionSummary, SessionStatus,
    UserInputAnswer, UserInputQuestion,
};
use crate::persistence::{
    ComposerDraftChange, ComposerDrafts, SessionMessageMatch, SessionMessageSearchScope,
};
use crate::provider_session::{ProviderSessionFork, ProviderSessionForkRequest};
use crate::routing::{RouteCandidate, RouteDecision, RouteTarget};
use crate::settings::DaemonSettings;
use crate::skills::SkillsCatalog;
use crate::usage::PlanUsage;
use crate::usage_history::{UsageHistory, UsageWindow};
use crate::workspace::{WorkspaceOperation, WorkspaceResult};

pub const PROTOCOL_VERSION: u32 = 14;
pub const MAX_WIRE_MESSAGE_BYTES: usize = 48 * 1024 * 1024;
pub const DAEMON_TOKEN_ENV: &str = "GODDARD_DAEMON_TOKEN";
pub const DAEMON_ADDRESS_ENV: &str = "GODDARD_DAEMON_ADDRESS";
pub const APP_EXECUTABLE_ENV: &str = "GODDARD_APP_EXECUTABLE";
/// Scoped bearer credential the daemon mints for one provider session's
/// runtime and delivers through its launch environment. Unlike the master
/// daemon token it is valid only for the agent command surface, only while
/// the owning runtime is alive, and never leaves daemon memory.
pub const AGENT_TOKEN_ENV: &str = "GODDARD_AGENT_TOKEN";
/// The Waku task that owns the running provider session. Agent harnesses
/// report it so the daemon can mark the prompts they submit with the sending
/// task's provenance.
pub const AGENT_TASK_ENV: &str = "GODDARD_TASK_ID";
/// The task a side chat was spawned from. Only present on side-chat
/// sessions, so their agents can discover the linkage without parsing the
/// intro note out of a prompt.
pub const AGENT_PARENT_TASK_ENV: &str = "GODDARD_PARENT_TASK_ID";
/// Markdown-link target prefix for referencing a task in transcript text:
/// `[title](goddard://task/<task-id>)` renders as a link that opens that
/// task. Surfaced to agents through `goddard-agent` so a reply can point at
/// another task the way it would point at a file.
pub const TASK_LINK_PREFIX: &str = "goddard://task/";
/// The request-id prefix the daemon mints for `agentAsk` user-input
/// requests. Clients use it to offer clarify/dismiss actions on those cards
/// even when the session's provider has no user-input actions of its own —
/// the daemon owns these requests, so the actions always resolve.
pub const AGENT_ASK_REQUEST_PREFIX: &str = "agent-ask-";
/// The request-id prefix the daemon mints for `agentRenameSelf` permission
/// requests. The daemon owns these requests too: they outlive the turn they
/// were asked under, so clients render them pinned rather than folded away
/// with it.
pub const AGENT_RENAME_REQUEST_PREFIX: &str = "agent-rename-";

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonReady {
    pub address: String,
    pub protocol_version: u32,
    pub pid: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientMessage {
    Hello {
        protocol_version: u32,
        token: String,
        client_id: Uuid,
        #[serde(default)]
        resume_from: Vec<ReplayCursor>,
    },
    Request(Request),
    /// The only message legal before `Hello`: ask the daemon for a client
    /// token. The request parks until a connected client approves or
    /// declines it; the reply is `PairPending`, then `PairGranted` or
    /// `PairDeclined` — never `Hello`.
    PairRequest {
        protocol_version: u32,
        /// Self-reported device name shown on the approval prompt.
        device_name: String,
    },
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    pub command: Command,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReplayCursor {
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    /// Identifies the daemon process that assigned `sequence`.
    pub epoch: Uuid,
    pub sequence: u64,
}

/// The splice point for one session's incremental `SaveTaskState` entry. The
/// wire session's `messages` and `transcript_blocks` hold only what was
/// appended after the client's previous save; the daemon keeps its stored
/// prefix and appends the tail.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetailTail {
    pub session_id: Uuid,
    /// `messages[..messages_from]` of the sending session are already stored;
    /// the wire session's `messages` carry only the appended tail.
    pub messages_from: u32,
    pub transcript_blocks_from: u32,
    /// [`crate::model::detail_prefix_signature`] over the kept prefix. The
    /// daemon verifies it against its stored prefix before splicing — a
    /// mismatch (another writer, a rewind in flight, a write that never
    /// landed) merges list columns only until the next full save.
    pub prefix_signature: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    /// Resolve the daemon-owned provider runtime for an existing task.
    ///
    /// Clients use this after reconnecting or opening the same daemon from a
    /// second app. It observes the session actor without starting, replacing,
    /// or otherwise mutating the provider process.
    AttachSession,
    Start {
        options: WireDriverStartOptions,
    },
    Prompt {
        prompt: String,
        /// The ids the submitting client already gave this turn and its user
        /// message. The daemon republishes them with the submission so every
        /// other client attached to the runtime mirrors the same rows instead
        /// of minting its own; older clients omit them and the daemon mints.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<Uuid>,
        /// The prompt is provider-facing only — the internal nudge a
        /// "continue" sends to an interrupted session. No client renders a
        /// transcript row for it.
        #[serde(default, skip_serializing_if = "crate::model::is_false")]
        hidden: bool,
        /// The composer's attachment chips. `prompt` already carries their
        /// `@`-mention text; transports with a native attachment channel
        /// (Copilot) also send them structurally.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<MessageAttachment>,
    },
    Steer {
        prompt: String,
        /// The steer carries client-injected context rather than user text:
        /// the daemon records it as hidden, so the accepted echo lands as a
        /// provider-facing message no client renders as a transcript row.
        #[serde(default, skip_serializing_if = "crate::model::is_false")]
        hidden: bool,
    },
    /// Ask the live provider runtime to compact the session's context.
    /// Fire-and-forget like [`Self::Goal`]: admission, progress, and the
    /// outcome arrive as driver events — Codex answers `thread/compact/start`
    /// immediately while OpenCode 2 admits a durable inbox item that reports
    /// through `session.compaction.*`. Drivers without a dedicated RPC fall
    /// back to sending the provider's own `/compact` command.
    Compact,
    Cancel,
    CancelComputerUse,
    RefreshBackgroundWork,
    StopBackgroundWork {
        key: Value,
        control_id: String,
    },
    Respond {
        request_id: String,
        option_id: String,
    },
    RespondUserInput {
        request_id: String,
        answers: Vec<UserInputAnswer>,
    },
    /// Settle a pending user-input request with free-form clarification
    /// instead of structured answers — the "let me explain" path, after
    /// which the provider re-decides the question. Providers without the
    /// notion ignore it; the UI only offers it when the attach response
    /// advertises user-input actions.
    ClarifyUserInput {
        request_id: String,
        content: String,
    },
    /// Dismiss a pending user-input request without answering.
    CancelUserInput {
        request_id: String,
    },
    /// Ask the live provider runtime to read or mutate its persisted thread
    /// goal. Fire-and-forget: the outcome arrives as a `goalUpdated` driver
    /// event, or an `error` event when the provider refuses.
    Goal {
        operation: GoalOperation,
    },
    /// Atomically assign one client to evaluate and advance a managed goal
    /// after this turn. Other attached clients observe the resulting state.
    ClaimManagedGoalTurn {
        goal_id: Uuid,
        turn_id: Uuid,
    },
    RunComputerTool {
        request: WireComputerToolRequest,
    },
    RejectComputerTool {
        request: WireComputerToolRequest,
        reason: String,
    },
    ApplyOptions {
        options: WireSessionOptions,
    },
    Rollback {
        turns: usize,
    },
    Fork {
        turns_to_remove: usize,
    },
    GetSettings,
    UpdateSettings {
        settings: DaemonSettings,
    },
    /// Read the daemon's process-memory samples: the latest reading, and the
    /// final sample the previous daemon process wrote before it exited —
    /// the pre-restart reading that explains an unexpected exit. Diagnostics
    /// only; agent-scoped credentials are refused.
    GetDaemonStats,
    /// Open, update, or close the daemon's non-loopback WebSocket listener.
    /// `None` unexposes; `Some` atomically rebinds when the config changed —
    /// the loopback listener and its sessions are never touched. Only the
    /// primary bearer token may call it; paired devices and scoped agent
    /// credentials are refused.
    SetDaemonExposure {
        exposure: Option<crate::exposure::DaemonExposure>,
    },
    /// Replace the command carrying `command.id` — or the one with its exact
    /// `name` when the id matches nothing — or append it when neither does.
    /// A nil id always means a new command; the daemon mints the real id.
    ///
    /// Scoped agent credentials may call this: it is the agent settings
    /// surface, gated by `agent_settings_enabled` rather than the task-creation
    /// opt-in, and the daemon stamps `created_by_task` with the sender.
    UpsertCustomCommand {
        command: CustomCommand,
    },
    /// Remove a custom command, addressed by id or by exact `name`.
    /// Scoped agent credentials may call this under the same gate as
    /// [`Self::UpsertCustomCommand`].
    RemoveCustomCommand {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Read the daemon-owned custom command list. Scoped agent credentials
    /// may call this so an agent can make its writes idempotent.
    ListCustomCommands,
    ProbeProvider {
        provider: ProviderKind,
        binary_override: Option<String>,
        discover_models: bool,
        probe_version: bool,
    },
    /// Resolve how to run the provider's interactive sign-in inside a
    /// throwaway sandbox VM — the argv a terminal runs on the daemon host.
    /// Errors when the provider cannot run sandboxed or takes only
    /// environment credentials.
    SandboxSignIn {
        provider: ProviderKind,
    },
    /// Whether the provider's shared sandbox home already holds
    /// credentials — clients check after a sign-in flow exits.
    SandboxAuthStatus {
        provider: ProviderKind,
    },
    FetchPlanUsage {
        provider: ProviderKind,
        binary_override: Option<String>,
        cli_version: Option<String>,
    },
    /// Spend one banked Codex rate-limit reset credit: clears the 5-hour
    /// and weekly windows and moves the weekly anchor to the redemption
    /// moment. `redeem_request_id` is the backend's idempotency key — a
    /// retry of one logical redemption must reuse it.
    ConsumeCodexResetCredit {
        redeem_request_id: String,
    },
    ProbeComputerPermissions {
        prompt: bool,
    },
    LoadUsageHistory {
        window: UsageWindow,
        project_roots: Vec<PathBuf>,
    },
    LoadSkills {
        projects: Vec<(String, PathBuf)>,
    },
    SetSkillsEnabled {
        dirs: Vec<PathBuf>,
        enabled: bool,
    },
    TrashSkills {
        dirs: Vec<PathBuf>,
    },
    LoadTaskState,
    SaveTaskState {
        projects: Vec<Project>,
        live_session_ids: Vec<Uuid>,
        sessions: Vec<AgentSession>,
        /// Sessions whose `messages`/`transcript_blocks` carry only the tail
        /// appended since the client's last save — spliced onto the daemon's
        /// stored prefix rather than replacing it. Entries not listed here
        /// are complete sessions as before.
        #[serde(default)]
        session_tails: Vec<SessionDetailTail>,
    },
    /// Explicitly remove one daemon-owned task. Ordinary state saves are
    /// merge-only so a stale client snapshot cannot delete tasks another
    /// client just created.
    RemoveSession,
    /// Remove a project and every task it owns from the daemon catalog. Like
    /// `RemoveSession`, this is explicit because `SaveTaskState` is merge-only.
    RemoveProject {
        project_id: Uuid,
    },
    HydrateSession {
        session_id: Uuid,
    },
    SearchSessionMessages {
        query: String,
        limit: usize,
        /// Which sessions the search scans; absent means active tasks, so
        /// pre-scope clients keep their palette behavior.
        #[serde(default)]
        scope: SessionMessageSearchScope,
    },
    /// List one provider's resumable CLI conversations on the daemon host.
    ListProviderSessions {
        provider: ProviderKind,
        limit: usize,
    },
    /// Load the user-visible transcript for one provider-native conversation.
    LoadProviderSession {
        cursor: ProviderResumeCursor,
        cwd: PathBuf,
    },
    /// Run one hosted evaluation: `state` is the data under judgment and
    /// `questions` are the typed decisions the model answers about it. The
    /// daemon owns the backend call and the decision log; every eval-driven
    /// feature (model routing today, agent tools later) shares this surface.
    Evaluate {
        #[ts(type = "unknown")]
        state: Value,
        questions: BTreeMap<String, EvalQuestion>,
        /// Which eval-driven feature made the call, recorded on the decision
        /// log record. `None` — every caller before this field existed —
        /// logs as a bare `"evaluate"`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feature: Option<String>,
        /// Override for the call's latency budget in seconds. Latency-bound
        /// callers (routing) leave it `None` for the default; long-context
        /// callers such as provider-switch compaction pass a larger budget.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_secs: Option<u64>,
    },
    /// Smoke-test an evaluation backend configuration for the settings
    /// pane. Carries the full settings so unsaved field edits can be tested;
    /// the daemon makes one minimal call and writes no decision log record.
    TestEvalConnection {
        settings: EvalSettings,
    },
    /// Aggregate the eval decision log's recorded token usage for the
    /// settings pane — totals plus a per-feature breakdown.
    LoadEvalUsage,
    /// Route a new session's first prompt: evaluate the task into a class,
    /// resolve the daemon's class map against `candidates`, and answer with
    /// the provider, model, and effort to start on. `last_used` is the
    /// fallback route when no class entry applies.
    RouteTask {
        prompt: String,
        /// Lightweight project context for the classifier — the project
        /// name only; filesystem drilling is deliberately out of scope.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        candidates: Vec<RouteCandidate>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_used: Option<RouteTarget>,
    },
    /// Append a route-override record to the decision log: the user changed
    /// the model on a session that started through routing.
    RecordRouteOverride {
        session_id: Uuid,
        target: RouteTarget,
    },
    /// Read the MCP integrations catalog plus the user's configuration for
    /// each entry. Global command — nil session id.
    ListIntegrations,
    /// Connect one integration: records the variant and provider set, stores
    /// `api_key` in the daemon's secret store when supplied, and — for
    /// OAuth-backed services — starts the browser flow in the background.
    /// Completion arrives through `SettingsChanged` as `auth` flips.
    ConnectIntegration {
        id: String,
        variant_id: String,
        providers: Vec<ProviderKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key: Option<String>,
    },
    /// Change which providers receive an already-connected integration.
    SetIntegrationProviders {
        id: String,
        providers: Vec<ProviderKind>,
    },
    /// Remove one integration: drops the provider config entries it wrote
    /// and deletes the stored credential.
    DisconnectIntegration {
        id: String,
    },
    /// Re-run the OAuth browser flow for an integration whose credential is
    /// missing or was revoked.
    StartIntegrationAuth {
        id: String,
    },
    LoadComposerDrafts,
    SaveComposerDrafts {
        drafts: ComposerDrafts,
        generation: u64,
    },
    ApplyComposerDraftChanges {
        changes: Vec<ComposerDraftChange>,
    },
    StoreBlob {
        mime_type: String,
        #[serde(with = "base64_bytes")]
        #[ts(type = "string")]
        bytes: Vec<u8>,
    },
    ImportAttachment {
        name: String,
        upload: AttachmentUpload,
    },
    ImportPathAttachment {
        #[ts(type = "string")]
        path: PathBuf,
    },
    ReadBlob {
        reference: String,
    },
    ReadAttachment {
        reference: String,
        path: PathBuf,
    },
    SweepBlobs,
    /// Fork a persisted task through one completed provider turn.
    ///
    /// This is intentionally a daemon-owned operation: provider-native
    /// conversation state, Git checkpoint refs, and SQLite all live on the
    /// daemon host and must move together for remote clients.
    ForkSessionFromResponse {
        turn_count: usize,
    },
    /// Restore a task and its provider conversation to immediately before a
    /// prior user message. The client can then submit the edited replacement
    /// as an ordinary new turn.
    RewindSessionToMessage {
        turn_count: usize,
    },
    ForkProviderSession {
        request: ProviderSessionForkRequest,
    },
    Workspace {
        operation: WorkspaceOperation,
    },
    OpenTerminal {
        #[ts(type = "string")]
        cwd: PathBuf,
        cols: u16,
        rows: u16,
        /// The task whose surface opened this terminal, when one did. The
        /// daemon sweeps the PTY when that task — or a workspace containing
        /// `cwd` — is removed; `None` terminals leave only with their cwd's
        /// workspace or an explicit `CloseTerminal`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<Uuid>,
    },
    WriteTerminal {
        #[serde(with = "base64_bytes")]
        #[ts(type = "string")]
        data: Vec<u8>,
    },
    ResizeTerminal {
        cols: u16,
        rows: u16,
    },
    CloseTerminal,
    CloseSession,
    /// Scoped agent credential only: create a fully configured task and
    /// immediately start its first prompt.
    ///
    /// Agents running inside a Waku session receive a daemon-minted token
    /// restricted to the agent command surface, so a harness can
    /// reach other tasks only when the human asked it to. There is no
    /// per-call approval gate; instead the daemon marks every accepted prompt
    /// with the sending task's id, which keeps agent-originated turns
    /// visible in the target transcript.
    AgentCreateSession {
        /// Any provider Waku can drive. `None` inherits the sending task's
        /// provider; the daemon rejects the command when no sending task is
        /// known to inherit from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<ProviderKind>,
        /// An explicit provider model id, `"auto"` to let Jev routing pick
        /// provider and model for the first prompt (requires `provider` to
        /// be absent), `"default"` (or empty) to select the provider's own
        /// default model, or `None` to inherit the sending task's model
        /// when it runs the resolved provider.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Absolute path of the project the task runs in. The daemon
        /// resolves an existing project at that path, or registers a
        /// primary Git checkout. Linked worktrees are never registered.
        #[ts(type = "string")]
        project: PathBuf,
        workspace: AgentWorkspace,
        /// The ref the new worktree starts from. Required when `workspace`
        /// is `worktree`, ignored for `local`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_branch: Option<String>,
        /// The task's first prompt, delivered as a normal turn the moment
        /// its session is running. There is no idle task creation path.
        prompt: String,
        /// Reasoning effort, service tier, and context window for the new
        /// session. `None` inherits the sending task's value when it runs
        /// the resolved provider and the resolved model's catalog still
        /// lists it; `"default"` (or empty) selects the provider's own
        /// default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        service_tier: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_window: Option<String>,
    },
    /// Scoped agent credential only: submit a prompt to an existing task,
    /// addressed by Waku task id or provider-native Agent CLI thread id.
    AgentPrompt {
        /// Waku task id. Exactly one of `task_id` and `thread_id` is
        /// required.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<Uuid>,
        /// Provider-native Agent CLI thread id, resolved against
        /// daemon-known tasks. `provider` disambiguates when more than one
        /// task carries the id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<ProviderKind>,
        prompt: String,
        #[serde(default)]
        delivery: AgentPromptDelivery,
    },
    /// Set the title of the calling agent's own task. Requires a per-task grant.
    AgentRenameSelf {
        title: String,
    },
    /// Cancel a daemon-owned queued agent prompt before it delivers. The
    /// session id rides the request envelope; `queued_message_id` names the
    /// mirrored [`crate::model::QueuedMessage`] entry. User-queued follow-ups
    /// are client-owned — they are removed by editing the saved session, not
    /// through this command.
    CancelQueuedPrompt {
        queued_message_id: Uuid,
    },
    /// Read the daemon-owned friends document (friend code, friends,
    /// pending requests, transfers). Global command — nil session id.
    GetFriends,
    /// Send a friend request to a `gfr-` code. `name` is our display name
    /// as the peer will see it.
    SendFriendRequest {
        code: String,
        name: String,
    },
    /// Accept or decline an incoming friend request.
    RespondFriendRequest {
        node_id: String,
        accept: bool,
    },
    /// Withdraw a pending outgoing friend request. Local removal only —
    /// the peer's incoming card lingers until they decline it.
    WithdrawFriendRequest {
        node_id: String,
    },
    RemoveFriend {
        node_id: String,
    },
    /// Offer a file or directory to a friend. Spawns a transfer; progress
    /// arrives through `FriendsChanged` broadcasts.
    SendFileToFriend {
        node_id: String,
        #[ts(type = "string")]
        path: PathBuf,
        note: Option<String>,
    },
    /// Send a chat message to a friend — lands on their side as a session
    /// in the Friends project, like a delivered transfer's note.
    SendMessageToFriend {
        node_id: String,
        text: String,
    },
    CancelTransfer {
        transfer_id: Uuid,
    },
    /// On-demand presence check — dial the friend and report the outcome via
    /// `FriendsChanged` (updates `last_seen`/`online`). No-op if a fresher
    /// cached probe exists.
    ProbeFriend {
        node_id: String,
    },
    /// Set the display name friends see on our requests and offers. Blank
    /// resets to the default (account name).
    SetFriendDisplayName {
        name: String,
    },
    /// Set a local-only nickname for a friend — overrides their
    /// self-reported name in this install's UI and transfer links.
    /// `None` or blank clears the override.
    SetFriendNickname {
        node_id: String,
        nickname: Option<String>,
    },
    /// Read the daemon-owned automations document — definitions plus bounded
    /// run history. Global command — the session id must be nil.
    GetAutomations,
    /// Create an automation, or replace the editable fields of the one
    /// `input.id` names. Global command — the session id must be nil.
    UpsertAutomation {
        input: AutomationInput,
    },
    /// Delete an automation and its run history. Global command — the
    /// session id must be nil.
    RemoveAutomation {
        automation_id: Uuid,
    },
    /// Queue a manual run for one automation. Global command — the session
    /// id must be nil.
    RunAutomationNow {
        automation_id: Uuid,
    },
    /// Scoped agent credential only: read a task's transcript.
    ///
    /// A caller may omit both ids to read its own task. Side chats use this
    /// to pull their parent task's context on demand — they are fresh
    /// sessions with a reference, not forks, so nothing of the parent's
    /// history is in their context natively. Addressed the same way as
    /// [`Self::AgentPrompt`].
    AgentReadSession {
        /// Waku task id. Exactly one of `task_id` and `thread_id` is
        /// required for a foreign read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<Uuid>,
        /// Provider-native Agent CLI thread id, resolved against
        /// daemon-known tasks. `provider` disambiguates when more than one
        /// task carries the id.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<ProviderKind>,
        /// Restrict the answer to one turn's entries, by its 1-based turn
        /// number — `items` each carry it, so a full read names the turn to
        /// revisit.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn: Option<usize>,
    },
    /// Scoped agent credential only: search the transcripts of the tasks in
    /// the caller's own project — the same corpus the command palette's
    /// session search scans. `query` is free text plus `field:value`
    /// filters (`project:`, `status:`, `archived:`, `limit:`); see
    /// [`crate::persistence::parse_session_message_search`]. `last_turns`
    /// narrows each task's corpus to its most recent turns — the units
    /// [`Self::AgentReadSession`] numbers — dropping messages outside a
    /// turn entirely. Hits identify tasks for [`Self::AgentReadSession`]
    /// to open.
    AgentSearchSessions {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_turns: Option<usize>,
    },
    /// Scoped agent credential only: ask the session's user a structured
    /// question mid-turn. The daemon emits the session's ordinary
    /// `userInputRequested` event and parks this call until the user
    /// answers, clarifies, or dismisses — or the turn underneath it ends.
    /// The provider is never involved: to it, the `goddard-agent ask`
    /// invocation is just a long-running tool call. This is the universal
    /// fallback for providers with no native question mechanism.
    AgentAsk {
        questions: Vec<UserInputQuestion>,
    },
    /// Scoped agent credential only: list the provider/model combinations
    /// [`Self::AgentCreateSession`] accepts — an `"auto"` routing entry
    /// when the evaluation backend is configured, then every combination a
    /// started task actually used, in preference order. Agents read this
    /// instead of guessing catalog model ids.
    AgentListModels,
    /// Share one of my projects with a friend. The daemon resolves the
    /// project's name and `origin` URL from `project_path` and re-sends
    /// the friend our full shared set.
    ShareProjectWithFriend {
        node_id: String,
        #[ts(type = "string")]
        project_path: PathBuf,
    },
    /// Stop sharing `origin_url` with the friend — their incoming share
    /// and any sync link on it come down on both sides.
    UnshareProjectWithFriend {
        node_id: String,
        origin_url: String,
    },
    /// Opt in to automatic sync on a friend's shared repo — creates the
    /// link against the daemon-matched local checkout with the repo's
    /// default branch enabled and tells the friend, which creates their
    /// side of it.
    EnableFriendSync {
        node_id: String,
        origin_url: String,
    },
    /// Tear down a sync link on both sides.
    DisableFriendSync {
        link_id: String,
    },
    /// Update a link's auto-push flag and enabled-branch set. Branches
    /// absent from `enabled_branches` stop syncing; paused branches stay
    /// paused until `FriendSyncNow` re-arms them.
    SetFriendSyncConfig {
        link_id: String,
        auto_push: bool,
        enabled_branches: Vec<String>,
    },
    /// Manually sync one branch — clears its paused flag and runs a
    /// fetch + integrate now.
    FriendSyncNow {
        link_id: String,
        branch: String,
    },
    /// Act on a sync alert: retry a stopped rebase as a merge, abort the
    /// integration (pausing the branch), or dismiss a refusal.
    FriendSyncAlertAction {
        alert_id: String,
        action: crate::friends::FriendSyncAlertAction,
    },
    /// The repo's local branches and default branch, for a link's
    /// branch-toggle UI. Returns `FriendSyncBranches`.
    GetFriendSyncBranches {
        link_id: String,
    },
    /// Opt in or out of letting the friend watch this shared project's
    /// sessions — live, read-only. Independent of repo sync; toggling
    /// re-sends the shared set so the friend's incoming share row updates.
    SetFriendSessionSharing {
        node_id: String,
        origin_url: String,
        enabled: bool,
    },
    /// List the sessions a friend exposes on their shared project.
    /// Answers `FriendSessions`; errors when the friend is unreachable or
    /// hasn't enabled session sharing.
    GetFriendSessions {
        node_id: String,
        origin_url: String,
    },
    /// Open a live, read-only view of a friend's session. The response is
    /// the current snapshot; updates arrive as `Event` broadcasts under
    /// the friend's session/runtime ids until `FriendSessionClosed`.
    WatchFriendSession {
        node_id: String,
        origin_url: String,
        session_id: Uuid,
    },
    /// Stop watching a friend's session — the peer subscription closes
    /// and its events stop reaching this client's broadcast stream.
    UnwatchFriendSession {
        session_id: Uuid,
    },
    /// Read the pairing document — pending pair requests and paired
    /// clients. Global command — nil session id.
    GetPairing,
    /// Approve or decline a pending pair request.
    RespondPairRequest {
        request_id: Uuid,
        accept: bool,
    },
    /// Drop a paired client's minted token — it can no longer authenticate.
    RevokePairedClient {
        client_id: Uuid,
    },
}

/// Where an agent-created task runs. Mirrors the New Task flow's workspace
/// choices; there is no attach-a-worktree path because a task created by an
/// agent always starts fresh.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentWorkspace {
    /// The project's own directory.
    #[default]
    Local,
    /// A daemon-managed Git worktree branched from `base_branch`.
    Worktree,
}

/// How an agent prompt reaches the target session.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentPromptDelivery {
    /// Wait in a daemon-side per-session queue until the target is idle,
    /// then start a fresh turn. Submission order is preserved.
    #[default]
    Queue,
    /// Inject the prompt into the target's running turn through the
    /// provider's steer path. An error when no turn is running or the
    /// provider cannot steer.
    Steer,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireDriverStartOptions {
    pub provider: String,
    pub binary: PathBuf,
    pub cwd: PathBuf,
    pub mode: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub context_window: Option<String>,
    pub agent_preset: Option<String>,
    pub computer_use_enabled: bool,
    /// The session carries history its agent should be able to read back —
    /// suspended prior providers or a side-chat parent — so the daemon
    /// mints the scoped credential for the read surface even when the
    /// cross-task agent tools are off.
    #[serde(default)]
    pub read_own_transcript: bool,
    pub provider_cursor: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireSessionOptions {
    pub mode: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub context_window: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireComputerToolRequest {
    pub call_id: String,
    pub tool: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireDriverEvent {
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

impl WireDriverEvent {
    pub fn new(kind: impl Into<String>, payload: Value) -> Self {
        Self {
            kind: kind.into(),
            payload,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SequencedEvent {
    pub session_id: Uuid,
    pub runtime_id: Uuid,
    /// Changes whenever the daemon restarts, so a reused runtime id can begin
    /// again at sequence one without being mistaken for an old event.
    pub epoch: Uuid,
    pub sequence: u64,
    pub event: WireDriverEvent,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerMessage {
    Hello {
        protocol_version: u32,
        daemon_version: String,
        /// The commit the daemon binary was built from, when its build had a
        /// git checkout to stamp. Dev builds surface it beside the app's own.
        daemon_commit: Option<String>,
        /// Whether this daemon can place `goddard-agent` on a session's
        /// PATH — false for remote hosts provisioned with the daemon alone.
        /// Clients fall back to push context when reads would have nothing
        /// to retrieve through.
        #[serde(default)]
        agent_cli_available: bool,
    },
    Rejected {
        message: String,
    },
    Response {
        request_id: Uuid,
        outcome: ResponseOutcome,
    },
    Event(SequencedEvent),
    /// The daemon-owned project/task catalog changed through another client.
    /// Clients should invalidate their lightweight task-state snapshot; live
    /// runtime events continue through [`Self::Event`].
    TaskStateChanged {
        revision: u64,
    },
    /// The daemon's settings document changed — through another client or
    /// through the scoped agent surface. Carries the whole document;
    /// settings are small and every subscriber applies it wholesale.
    SettingsChanged {
        settings: DaemonSettings,
    },
    /// The friends document changed — new request, friend added, transfer
    /// progress. Carries the whole document; it's small and every client
    /// applies it wholesale.
    FriendsChanged {
        state: crate::friends::FriendsState,
    },
    /// The automations document changed — a definition was edited or a run
    /// recorded progress. Carries the whole document; it's small and every
    /// client applies it wholesale.
    AutomationsChanged {
        state: AutomationsState,
    },
    /// `refs/notes/qa` (or a promoted base branch) moved for this `origin`
    /// — here or on a friend's machine. Review surfaces for a project
    /// with that remote should re-read their queue.
    ReviewChanged {
        origin_url: String,
    },
    /// A watched friend session's stream ended. `revoked` means the friend
    /// turned session sharing off or unshared the project — render that,
    /// not a disconnect. Otherwise the peer went away or the session was
    /// deleted; watching again refetches a fresh snapshot.
    FriendSessionClosed {
        session_id: Uuid,
        revoked: bool,
    },
    /// The pairing document changed — a pair request arrived or resolved,
    /// or a paired client was revoked. Carries the whole document.
    PairingChanged {
        state: crate::pairing::PairingState,
    },
    /// Sent to a `PairRequest` connection once the request is registered —
    /// the client should render "waiting for approval" until the terminal
    /// `PairGranted`/`PairDeclined` arrives.
    PairPending,
    /// The pair request was approved; `token` is a bearer for `Hello`.
    PairGranted {
        token: String,
        daemon_name: String,
    },
    PairDeclined {
        message: String,
    },
    ShuttingDown,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ResponseOutcome {
    Ok { payload: ResponsePayload },
    Error { error: RpcError },
}

/// What owns one direct child of the daemon process.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DaemonChildKind {
    /// A provider runtime subtree — a guardian shell over the agent CLI.
    Runtime,
    /// A remote terminal's PTY subtree.
    Terminal,
    /// Anything else the daemon spawned (helper processes, mid-teardown
    /// subtrees, children no live entry claimed).
    Other,
}

/// One direct child of the daemon, summarized over its whole subtree —
/// provider CLIs sit under a guardian shell, so the interesting process is
/// usually a grandchild.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonChildSample {
    /// The direct child's pid — the subtree root this row summarizes.
    pub pid: u32,
    /// The heaviest member's process name — `devin`, `codex`, the login
    /// shell a terminal runs. Identifies what the subtree actually is.
    pub name: String,
    /// Resident size summed over the whole subtree, in MiB.
    pub rss_mb: u64,
    /// Processes in the subtree, the direct child included.
    pub processes: u32,
    pub kind: DaemonChildKind,
    /// The session this subtree serves — `None` when no live entry claimed
    /// it (mid-teardown, or a helper the daemon never tracked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    /// The provider behind a runtime subtree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderKind>,
}

/// How much of one session's transcript the daemon holds in memory. Only
/// resident or running sessions get a row — skeletons carry ~1 KB of list
/// columns each and are not worth per-session detail.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSessionSample {
    pub id: Uuid,
    /// Empty for incognito sessions — this struct lands in
    /// `daemon-stats.jsonl`, and incognito exists to keep data off disk.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    pub provider: ProviderKind,
    pub status: SessionStatus,
    /// Whether the full transcript is resident; `false` means only the list
    /// columns are loaded (a skeleton).
    pub detail_loaded: bool,
    /// Whether a live provider runtime is attached.
    pub running: bool,
    pub resident_messages: u32,
    pub resident_activities: u32,
    /// Rough heap estimate of the resident detail — struct sizes plus the
    /// big string payloads (message content, activity output/detail). An
    /// underestimate: deeply nested fields are counted shallowly.
    pub resident_bytes: u64,
}

/// One process-memory sample written by the daemon's stats sampler — see
/// [`Command::GetDaemonStats`]. Also the per-line shape of
/// `daemon-stats.jsonl` in the daemon's data directory.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatsSample {
    /// Unix seconds when the sample was taken.
    pub at: u64,
    /// The daemon process's own resident size. `None` on platforms without
    /// a sampler implementation, or before the first sample lands.
    pub daemon_rss_mb: Option<u64>,
    /// Resident size summed over the daemon's whole descendant tree —
    /// provider runtimes and terminals carry their memory under their own
    /// pids, so real growth shows up here rather than in `daemon_rss_mb`.
    pub children_rss_mb: Option<u64>,
    /// Live provider runtimes at sample time.
    pub runtimes: u32,
    /// Live remote terminals at sample time.
    pub terminals: u32,
    /// Per-subtree rows: one per direct child of the daemon, each carrying
    /// its whole descendant tree's resident size. `None`-empty on platforms
    /// without a sampler implementation.
    #[serde(default)]
    pub children: Vec<DaemonChildSample>,
    /// Every session the daemon knows, skeletons included.
    #[serde(default)]
    pub sessions_total: u32,
    /// Resident or running sessions and what they hold in memory.
    #[serde(default)]
    pub sessions: Vec<DaemonSessionSample>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ResponsePayload {
    Ack,
    ManagedGoalTurnClaimed {
        claimed: bool,
    },
    SessionRuntime {
        runtime_id: Option<Uuid>,
        supports_steer: bool,
        /// The transport can settle a user-input request without structured
        /// answers — clarify and dismiss are both offered on this bit.
        /// Absent on older daemons, where the card stays answer-only.
        #[serde(default)]
        supports_user_input_actions: bool,
    },
    Started {
        supports_steer: bool,
        #[serde(default)]
        supports_user_input_actions: bool,
    },
    OptionsApplied {
        applied: bool,
    },
    Cursor {
        cursor: Option<Value>,
    },
    Settings {
        settings: DaemonSettings,
    },
    /// Process-memory samples, as read for `getDaemonStats`. `previous_boot`
    /// is the last sample the prior daemon process wrote — `None` when this
    /// boot found no stats file or an older daemon wrote none.
    DaemonStats {
        #[serde(default)]
        current: Option<DaemonStatsSample>,
        #[serde(default)]
        previous_boot: Option<DaemonStatsSample>,
        /// The previous boot's last line carried the clean-exit marker —
        /// `false` reads as an abnormal death (jetsam, SIGKILL, crash).
        #[serde(default)]
        previous_boot_clean: bool,
    },
    /// The bound port after `setDaemonExposure` — `None` once unexposed.
    Exposure {
        port: Option<u16>,
    },
    /// The integrations catalog joined with the user's configuration.
    Integrations {
        snapshots: Vec<crate::integrations::IntegrationSnapshot>,
    },
    /// The daemon-owned custom command list after the change — or as read,
    /// for `listCustomCommands`.
    CustomCommands {
        commands: Vec<CustomCommand>,
    },
    ProviderProbe {
        probe: ProviderProbe,
        version: Option<String>,
    },
    /// The argv + working directory a terminal runs to sign a provider in
    /// inside a throwaway sandbox VM.
    SandboxSignIn {
        #[ts(type = "string")]
        program: PathBuf,
        args: Vec<String>,
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    SandboxAuthStatus {
        signed_in: bool,
    },
    PlanUsage {
        usage: Option<PlanUsage>,
    },
    /// The outcome of `consumeCodexResetCredit`, plus the re-fetched plan
    /// snapshot when the post-redemption read succeeded. `None` on a
    /// confirmed redemption means "applied, but the new limits could not be
    /// confirmed" — the spend still went through.
    CodexResetCredit {
        outcome: crate::usage::CodexResetCreditOutcome,
        usage: Option<PlanUsage>,
    },
    ComputerPermissions {
        permissions: ComputerPermissions,
    },
    UsageHistory {
        history: UsageHistory,
    },
    SkillsCatalog {
        catalog: SkillsCatalog,
    },
    TaskState {
        projects: Vec<Project>,
        sessions: Vec<AgentSession>,
        default_cwd: PathBuf,
        projectless_root: Option<PathBuf>,
    },
    TaskStateSaved {
        sessions: Vec<AgentSession>,
    },
    /// The daemon-owned friends document, as read for `getFriends`.
    Friends {
        state: crate::friends::FriendsState,
    },
    /// The daemon-owned automations document, as read for `getAutomations`.
    Automations {
        state: AutomationsState,
    },
    /// The daemon-owned pairing document, as read for `getPairing`.
    Pairing {
        state: crate::pairing::PairingState,
    },
    /// The automation record after an `upsertAutomation`.
    Automation {
        automation: Automation,
    },
    /// The run record a `runAutomationNow` queued.
    AutomationRun {
        run: AutomationRun,
    },
    /// A sync link's repo branches, as read for `getFriendSyncBranches`.
    FriendSyncBranches {
        link_id: String,
        branches: Vec<String>,
        default_branch: Option<String>,
    },
    /// A friend's shared sessions, as read for `getFriendSessions`.
    FriendSessions {
        sessions: Vec<crate::friends::SharedSessionSummary>,
    },
    /// The snapshot that opens a friend session watch, as read for
    /// `watchFriendSession`. Live updates follow as `Event` broadcasts.
    FriendSession {
        session: Box<AgentSession>,
    },
    Session {
        session: Option<AgentSession>,
    },
    SessionMessageMatches {
        matches: Vec<SessionMessageMatch>,
    },
    ProviderSessions {
        sessions: Vec<ProviderSessionSummary>,
        /// Why the catalog is empty; `Ready` means an empty list is genuine.
        /// Pre-status daemons omit it, deserializing to `Ready`.
        #[serde(default)]
        status: ProviderSessionCatalogStatus,
    },
    ProviderSessionHistory {
        history: ProviderSessionHistory,
        /// The launch directory the daemon resolved for the session; a
        /// `cwd_missing` catalog entry resumes in the nearest surviving
        /// ancestor rather than the recorded path.
        #[serde(default)]
        resolved_cwd: Option<PathBuf>,
    },
    ComposerDrafts {
        drafts: ComposerDrafts,
    },
    Evaluation {
        evaluation: Evaluation,
    },
    EvalUsage {
        stats: EvalUsageStats,
    },
    RouteDecision {
        decision: RouteDecision,
    },
    BlobStored {
        reference: String,
        path: PathBuf,
    },
    AttachmentStored {
        attachment: StoredAttachment,
    },
    BlobData {
        #[serde(with = "base64_bytes")]
        #[ts(type = "string")]
        bytes: Vec<u8>,
    },
    ProviderSessionForked {
        result: ProviderSessionFork,
    },
    SessionForked {
        session: AgentSession,
        checkpoint_warning: Option<String>,
    },
    SessionRewound {
        session: AgentSession,
        cleanup_warning: Option<String>,
    },
    Workspace {
        result: WorkspaceResult,
    },
    /// The daemon persisted and started an agent-created task.
    AgentSessionCreated {
        session_id: Uuid,
    },
    /// The transcript an `agentReadSession` resolved — the compact view a
    /// scoped agent caller reads.
    AgentSessionTranscript {
        transcript: AgentSessionTranscript,
    },
    /// The hits an `agentSearchSessions` resolved, ranked the way the
    /// command palette ranks session matches.
    AgentSessionSearch {
        hits: Vec<AgentSessionSearchHit>,
    },
    /// The resolution of a parked `agentAsk` — the user's answers, their
    /// free-form clarification, or a cancellation.
    AgentAskResult {
        outcome: AgentAskOutcome,
    },
    /// The preference-ordered provider/model list an `agentListModels`
    /// resolved.
    AgentModelOptions {
        options: Vec<AgentModelOption>,
    },
}

/// The i18n key and `%{name}` substitution values behind a user-facing
/// string, shipped alongside the English fallback so each client can render
/// the text in its own locale. Emitters build the pair with `localized!`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WireTranslation {
    pub key: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub args: std::collections::BTreeMap<String, String>,
}

impl WireTranslation {
    pub fn new(
        key: impl Into<String>,
        args: impl IntoIterator<Item = (&'static str, String)>,
    ) -> Self {
        Self {
            key: key.into(),
            args: args.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        }
    }

    /// Render in the calling process's locale — the client-side counterpart
    /// of the `tr!` fallback the daemon already shipped.
    pub fn render(&self) -> String {
        let mut text = crate::i18n::translate(&self.key);
        for (name, value) in &self.args {
            text = text.replace(&format!("%{{{name}}}"), value);
        }
        text
    }
}

/// An error whose display text is a known i18n key. It travels through
/// `anyhow` like any other error but keeps its key and args so the RPC
/// boundary can ship the semantic beside the fallback message.
pub struct KeyedError {
    pub message: String,
    pub i18n: WireTranslation,
}

impl KeyedError {
    /// Wrap the `(fallback, translation)` pair produced by `localized!`.
    pub fn localized(pair: (String, WireTranslation)) -> Self {
        Self {
            message: pair.0,
            i18n: pair.1,
        }
    }
}

impl std::fmt::Display for KeyedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::fmt::Debug for KeyedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyedError")
            .field("key", &self.i18n.key)
            .field("message", &self.message)
            .finish()
    }
}

impl std::error::Error for KeyedError {}

impl RpcError {
    /// The message a client should show: the i18n semantic rendered in this
    /// process's locale when present, the daemon's fallback text otherwise.
    pub fn localized_message(&self) -> String {
        self.i18n
            .as_ref()
            .map(WireTranslation::render)
            .unwrap_or_else(|| self.message.clone())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct RpcError {
    pub message: String,
    /// The i18n semantic behind `message`, when the failing side knew it.
    /// `None` for provider text and opaque errors — render `message` as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub i18n: Option<WireTranslation>,
}

impl From<anyhow::Error> for RpcError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            // `{:#}` keeps the whole context chain — `to_string` would shed
            // the actionable inner cause ("provider cannot run sandboxed")
            // behind the outermost context ("could not prepare the VM").
            message: format!("{error:#}"),
            i18n: error
                .chain()
                .find_map(|cause| cause.downcast_ref::<KeyedError>())
                .map(|keyed| keyed.i18n.clone()),
        }
    }
}

pub(crate) mod base64_bytes {
    use base64::Engine as _;
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_payloads_use_base64_json_strings() {
        let payload = ResponsePayload::BlobData {
            bytes: vec![0, 1, 2, 255],
        };
        let json = serde_json::to_value(&payload).unwrap();

        assert_eq!(json["bytes"], "AAEC/w==");
        let ResponsePayload::BlobData { bytes } = serde_json::from_value(json).unwrap() else {
            panic!("unexpected payload variant");
        };
        assert_eq!(bytes, vec![0, 1, 2, 255]);

        let command = Command::WriteTerminal {
            data: vec![0, 1, 2, 255],
        };
        let json = serde_json::to_value(&command).unwrap();
        assert_eq!(json["type"], "writeTerminal");
        assert_eq!(json["data"], "AAEC/w==");
        let Command::WriteTerminal { data } = serde_json::from_value(json).unwrap() else {
            panic!("unexpected command variant");
        };
        assert_eq!(data, vec![0, 1, 2, 255]);
    }

    #[test]
    fn save_task_state_tails_round_trip_and_default_to_full_saves() {
        let command = Command::SaveTaskState {
            projects: Vec::new(),
            live_session_ids: Vec::new(),
            sessions: Vec::new(),
            session_tails: vec![SessionDetailTail {
                session_id: Uuid::from_u128(7),
                messages_from: 3,
                transcript_blocks_from: 2,
                prefix_signature: 42,
            }],
        };

        let json = serde_json::to_value(&command).unwrap();
        assert_eq!(json["type"], "saveTaskState");
        assert_eq!(
            json["sessionTails"][0]["sessionId"],
            "00000000-0000-0000-0000-000000000007"
        );
        assert_eq!(json["sessionTails"][0]["messagesFrom"], 3);
        assert_eq!(json["sessionTails"][0]["transcriptBlocksFrom"], 2);
        assert_eq!(json["sessionTails"][0]["prefixSignature"], 42);

        // A payload that predates the field parses with no tails — every
        // session entry is a complete save, as before.
        let mut legacy = json.clone();
        legacy.as_object_mut().unwrap().remove("sessionTails");
        let Command::SaveTaskState { session_tails, .. } = serde_json::from_value(legacy).unwrap()
        else {
            panic!("unexpected command variant");
        };
        assert!(session_tails.is_empty());
    }

    #[test]
    fn response_fork_command_uses_stable_camel_case_fields() {
        let json =
            serde_json::to_value(Command::ForkSessionFromResponse { turn_count: 7 }).unwrap();

        assert_eq!(json["type"], "forkSessionFromResponse");
        assert_eq!(json["turnCount"], 7);
        assert_eq!(PROTOCOL_VERSION, 14);
    }

    #[test]
    fn message_rewind_command_uses_stable_camel_case_fields() {
        let json = serde_json::to_value(Command::RewindSessionToMessage { turn_count: 4 }).unwrap();

        assert_eq!(json["type"], "rewindSessionToMessage");
        assert_eq!(json["turnCount"], 4);
        assert_eq!(PROTOCOL_VERSION, 14);
    }

    #[test]
    fn provider_session_commands_use_stable_wire_fields() {
        let list = serde_json::to_value(Command::ListProviderSessions {
            provider: ProviderKind::Codex,
            limit: 250,
        })
        .unwrap();
        assert_eq!(list["type"], "listProviderSessions");
        assert_eq!(list["provider"], "codex");
        assert_eq!(list["limit"], 250);

        let load = serde_json::to_value(Command::LoadProviderSession {
            cursor: ProviderResumeCursor::Codex {
                thread_id: "01900000-0000-7000-8000-000000000001".into(),
            },
            cwd: PathBuf::from("/tmp/project"),
        })
        .unwrap();
        assert_eq!(load["type"], "loadProviderSession");
        assert_eq!(load["cursor"]["provider"], "codex");
        assert_eq!(
            load["cursor"]["threadId"],
            "01900000-0000-7000-8000-000000000001"
        );
        assert_eq!(load["cwd"], "/tmp/project");
    }

    #[test]
    fn agent_read_command_uses_stable_camel_case_fields() {
        let task_id = Uuid::from_u128(42);
        let by_task = serde_json::to_value(Command::AgentReadSession {
            task_id: Some(task_id),
            thread_id: None,
            provider: None,
            turn: Some(3),
        })
        .unwrap();
        assert_eq!(by_task["type"], "agentReadSession");
        assert_eq!(by_task["taskId"], task_id.to_string());
        assert_eq!(by_task["turn"], 3);
        assert!(by_task.get("threadId").is_none());
        assert!(by_task.get("provider").is_none());

        let by_thread = serde_json::to_value(Command::AgentReadSession {
            task_id: None,
            thread_id: Some("thread-9".into()),
            provider: Some(ProviderKind::Claude),
            turn: None,
        })
        .unwrap();
        assert_eq!(by_thread["threadId"], "thread-9");
        assert_eq!(by_thread["provider"], "claude");
        assert!(by_thread.get("turn").is_none());
    }

    #[test]
    fn handshake_and_replay_field_names_are_stable() {
        let session_id = Uuid::nil();
        let runtime_id = Uuid::from_u128(1);
        let message = ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            token: "secret".into(),
            client_id: Uuid::from_u128(2),
            resume_from: vec![ReplayCursor {
                session_id,
                runtime_id,
                epoch: Uuid::from_u128(3),
                sequence: 9,
            }],
        };
        let json = serde_json::to_value(message).unwrap();

        assert_eq!(json["type"], "hello");
        assert_eq!(json["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(json["resumeFrom"][0]["sessionId"], session_id.to_string());
        assert_eq!(json["resumeFrom"][0]["runtimeId"], runtime_id.to_string());
        assert_eq!(
            json["resumeFrom"][0]["epoch"],
            Uuid::from_u128(3).to_string()
        );
        assert!(json.get("protocol_version").is_none());
    }

    #[test]
    fn composer_draft_changes_have_stable_wire_keys() {
        let project_id = Uuid::from_u128(7);
        let command = Command::ApplyComposerDraftChanges {
            changes: vec![ComposerDraftChange {
                target: crate::persistence::ComposerDraftTarget::NewSession { project_id },
                draft: Some(crate::persistence::ComposerDraft {
                    text: "unfinished".into(),
                    attachments: Vec::new(),
                    annotations: Vec::new(),
                    inline_atoms: Vec::new(),
                }),
            }],
        };
        let json = serde_json::to_value(command).unwrap();

        assert_eq!(json["type"], "applyComposerDraftChanges");
        assert_eq!(json["changes"][0]["target"]["type"], "newSession");
        assert_eq!(
            json["changes"][0]["target"]["projectId"],
            project_id.to_string()
        );
        assert_eq!(json["changes"][0]["draft"]["text"], "unfinished");
    }
}
