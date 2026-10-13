//! The narrow stable-runtime contract shared by the daemon, adapters, and
//! the watcher.
//!
//! These types are the whole vocabulary a controller needs: session and
//! generation identity, the deduplicated action envelope, ordered event
//! envelopes, and the dispositions that distinguish recorded intent from
//! provider acceptance. They deliberately carry no product policy — the
//! daemon owns admissions, personas, and scheduling; the host only records
//! what it was asked to do and what it observed.
//!
//! Everything here serializes: the same definitions become the journal
//! record payload, the future IPC wire format, and the fake-provider
//! harness vocabulary, so no owner invents a parallel envelope.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

/// Protocol contract version implemented by this host. A controller whose
/// range does not contain it is rejected while existing sessions survive.
pub const PROTOCOL_VERSION: u32 = 1;

/// One owner of a provider session. The generation changes on provider
/// replacement — never merely because a controller detached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub session_id: Uuid,
    pub generation: u64,
}

/// The deduplicated payload of a command. Canonical identity is this value
/// alone: controller epoch, credentials, and transport metadata authorize
/// the current attempt without changing the recorded action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CanonicalAction {
    /// Create a runtime-owned provider session from a persisted,
    /// controller-authorized admission. Runtime availability alone never
    /// creates sessions.
    AdmitSession {
        session_id: Uuid,
        adapter: String,
        /// Opaque launch/config material the adapter interprets.
        launch: Value,
        requested: RequestedConfig,
    },
    /// Deliver one authorized input. `delivery_id` is the durable
    /// originating identity; `turn_id` attributes the resulting provider
    /// execution.
    Submit {
        delivery_id: Uuid,
        turn_id: Uuid,
        input: Value,
    },
    /// Cancel one named turn. Queued deliveries are preserved but held
    /// until cancellation settles and the controller explicitly permits
    /// the next dispatch.
    CancelTurn { turn_id: Uuid },
    /// Fence the generation: cancel held inputs and forbid any further
    /// dispatch. Deliberate reopening requires a new generation.
    StopSession,
    /// Release deliveries held while a cancellation settled. This is the
    /// controller's explicit policy decision, never an implicit unwind.
    PermitDispatch,
    /// Resolve one pending provider request once, under current authority
    /// and the original request identity.
    ResolveRequest {
        request_id: String,
        resolution: Value,
    },
    /// Ask for a new configuration revision. Acceptance is recorded here;
    /// the effective revision is a separate fact delivered by a
    /// `ConfigActivated` event.
    Reconfigure {
        revision: u64,
        target: RequestedConfig,
    },
}

/// Requested provider/model/effort plus adapter-opaque options. Displayed
/// separately from the effective configuration until a `ConfigActivated`
/// event confirms activation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestedConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub options: Value,
}

/// One requested action as presented to the host. `command_id` plus the
/// canonical action is the durable identity; the epoch and retry window
/// authorize this attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionEnvelope {
    pub namespace: String,
    pub runtime_instance: Uuid,
    /// The presenting controller's epoch. New mutations require the current
    /// epoch; retries of a recorded command return its stored truth under
    /// any live controller credential.
    pub epoch: u64,
    /// Runtime-issued identity with an immutable expiry. After receipts are
    /// reclaimed, expired windows still reject — an old command id cannot
    /// become new work.
    pub retry_window_id: Uuid,
    pub command_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionRef>,
    pub action: CanonicalAction,
}

/// Stable refusal codes. Rejection is a recorded disposition: it is part of
/// the command's durable truth, not a transport error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RejectCode {
    UnknownSession,
    GenerationMismatch,
    /// A stop/interrupt fence already owns this generation.
    Fenced,
    /// Dispatch is held while a cancellation settles.
    Held,
    /// The provider itself refused the action.
    ProviderRefused,
    SpawnFailed,
    InvalidAction,
    /// Reconfiguration needs a new provider generation; replacement is a
    /// later stage's machinery, so the host refuses rather than pretend.
    RequiresReplacement,
    /// The request was already resolved once — answers are never reused
    /// for a different request.
    AlreadyResolved,
    UnknownRequest,
    /// Runtime restart interrupted the owner before dispatch evidence.
    Interrupted,
    Capacity,
    Stale,
}

/// How a command ended. `event_sequence` attributes the deciding provider
/// event when one exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TerminalOutcome {
    Completed,
    Cancelled,
    /// The session lost its owner or provider before a real result.
    Interrupted,
    /// Rejected before provider acceptance; kept distinct from `Rejected`
    /// so held/queued commands cancelled by a stop are terminal, not
    /// merely refused.
    Refused,
}

/// Where a command's truth became ambiguous. Ambiguity is held for
/// reconciliation; none of these permit an automatic resend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UncertainStage {
    /// Lost the runtime between recording intent and dispatch evidence.
    DispatchUnknown,
    /// The provider could not prove whether it accepted the command.
    AcceptanceUnknown,
    /// The provider accepted; the runtime was lost before the outcome.
    OutcomeUnknown,
    /// The provider may have consumed a delivered answer while its
    /// acknowledgement was lost.
    AnswerDeliveryUnknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CommandDisposition {
    /// Intent persisted; not yet sent toward the provider.
    Recorded,
    /// Persisted but held — cancellation is settling or the controller has
    /// not yet permitted dispatch.
    DispatchPending,
    /// Dispatch began; acceptance is not yet recorded. A command recovered
    /// in this state becomes uncertain, never silently retried.
    DispatchAttempted,
    /// The provider confirmed accepting this exact command.
    ProviderAccepted,
    /// Refused with a stable reason.
    Rejected { code: RejectCode, detail: String },
    /// Resolved to an end state.
    Terminal {
        outcome: TerminalOutcome,
        event_sequence: Option<u64>,
    },
    /// Ambiguous after failure; held for reconciliation. Never resent.
    Uncertain {
        stage: UncertainStage,
        detail: String,
    },
}

impl CommandDisposition {
    /// Whether this disposition can still change. Uncertainty is not
    /// terminal: qualified evidence may resolve it.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            CommandDisposition::Rejected { .. } | CommandDisposition::Terminal { .. }
        )
    }
}

/// What kind of provider request waits on a controller resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestClass {
    /// A daemon-routed tool execution request.
    Tool,
    /// A permission/approval decision.
    Approval,
    /// Free-form provider input request.
    Input,
}

/// Answer-delivery truth, separate from the controller's decision to
/// answer. A decision recorded and a provider that consumed the answer are
/// two different facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AnswerDisposition {
    Pending,
    /// Delivery to the provider was attempted; consumption unknown.
    Attempted,
    /// The provider confirmed consuming this answer, or qualified adapter
    /// evidence (`RequestAnswered`) proved it.
    Accepted,
    Rejected {
        detail: String,
    },
    /// The answer may have been consumed while the acknowledgement was
    /// lost. Reconciliation evidence may upgrade this; it is never resent.
    Uncertain {
        detail: String,
    },
}

/// Boundary at which an activated configuration revision takes effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivationBoundary {
    Immediate,
    NextTurn,
    /// Applies only through a new provider generation — never a silent
    /// mid-turn swap.
    NextGeneration,
}

/// Why a generation ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TerminateReason {
    /// Explicit stop/drain intent was carried out.
    Requested,
    /// The provider exited on its own.
    Exited { detail: String },
    /// The runtime lost or fenced the owner without confirmation.
    Interrupted { detail: String },
    /// Intake exceeded protected budgets and the provider could not pause.
    Backpressure,
}

/// Why an oversized provider message was dropped from the ordered stream.
/// The drop marker itself is sequenced, so replay is contiguous and the
/// loss is visible rather than silent truncation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DropReason {
    Oversize,
}

/// Provider-originated events. The host assigns order and session/generation
/// identity; the adapter supplies the facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProviderEvent {
    /// The adapter's transport is live; emitted once per generation.
    SessionLive,
    TurnStarted {
        turn_id: Uuid,
        delivery_id: Uuid,
    },
    TurnDelta {
        turn_id: Uuid,
        delta: Value,
    },
    TurnCompleted {
        turn_id: Uuid,
        result: Value,
    },
    TurnCancelled {
        turn_id: Uuid,
    },
    /// A provider/daemon-routed request now waits on the controller.
    RequestOpened {
        request_id: String,
        class: RequestClass,
        subject: Value,
        /// Original deadline in epoch milliseconds. Retries and attachment
        /// changes never extend it.
        deadline_ms: Option<u64>,
    },
    /// Qualified adapter evidence that a delivered answer was consumed.
    RequestAnswered {
        request_id: String,
    },
    /// Effective configuration moved to a revision at a declared boundary.
    ConfigActivated {
        revision: u64,
        boundary: ActivationBoundary,
        effective: RequestedConfig,
    },
    /// This provider failed while others stay healthy.
    ProviderFailed {
        detail: String,
    },
    /// The generation's last word. Delayed traffic after this cannot
    /// mutate the generation.
    Terminated {
        reason: TerminateReason,
    },
    /// An oversized message was refused at intake. Sequenced so replay has
    /// no silent gap.
    Dropped {
        reason: DropReason,
        bytes: u64,
    },
}

/// One durably ordered event. The sequence is monotonic within a session
/// generation and never resets on controller reload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub runtime_instance: Uuid,
    pub session_id: Uuid,
    pub generation: u64,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_revision: Option<u64>,
    pub event: ProviderEvent,
}

/// Where a consumer sits in a session generation's ordered stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventCursor {
    pub session_id: Uuid,
    pub generation: u64,
    /// Last sequence durably applied by the consumer.
    pub applied: u64,
}

/// A page of ordered events. `high_water` is the highest journaled
/// sequence at read time; polling again from it cannot skip an event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub events: Vec<EventEnvelope>,
    pub high_water: u64,
    /// First sequence still retained. A cursor below it is a blocked
    /// recovery (`CursorExpired`), never silent loss.
    pub retained_floor: u64,
}

/// What a controller proves at hello.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientHello {
    pub protocol_min: u32,
    pub protocol_max: u32,
    /// Capabilities this controller requires from the host.
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

impl ClientHello {
    pub fn current() -> Self {
        Self {
            protocol_min: PROTOCOL_VERSION,
            protocol_max: PROTOCOL_VERSION,
            capabilities: Vec::new(),
            build: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub protocol: u32,
    #[serde(default)]
    pub protocol_min: u32,
    #[serde(default)]
    pub protocol_max: u32,
    #[serde(default)]
    pub runtime_build: String,
    #[serde(default)]
    pub adapter_versions: std::collections::BTreeMap<String, String>,
    pub namespace: String,
    pub runtime_instance: Uuid,
    pub recovery: RecoveryState,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Distinguishes an ordinary start, a clean drain, and a crash recovery.
/// Controllers must reconcile after `Interrupted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RecoveryState {
    Fresh,
    Clean,
    /// The previous instance did not drain. Sessions are preserved and
    /// fenced; executing turns are interrupted/uncertain and nothing is
    /// replayed automatically.
    Interrupted {
        sessions: usize,
    },
}

/// The exclusive mutation lease issued by `acquire_control`. Taking a new
/// epoch atomically invalidates the previous controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlGrant {
    pub epoch: u64,
    pub retry_window_id: Uuid,
    pub retry_window_expires_at_ms: u64,
}

/// Session liveness as the host last confirmed it. A lost controller
/// connection is not evidence of completion or failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionStatus {
    Live,
    /// A stop fence is recorded; termination is not yet confirmed.
    Stopping,
    /// Termination was observed or confirmed.
    Terminated,
    /// The provider exited on its own; the host observed it.
    Exited,
    /// The runtime lost the owner. Preserved for reconciliation; fenced
    /// against any further dispatch.
    Interrupted,
}

/// The runtime-confirmed configuration — revisioned and protected from
/// generic client saves by construction: only `ConfigActivated` events
/// move it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfig {
    pub revision: u64,
    pub boundary: ActivationBoundary,
    pub config: RequestedConfig,
}

/// Per-session truth returned by inventory reconciliation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInventory {
    pub session: SessionRef,
    /// The registered owner and accepted configuration are reconciliation
    /// truth even before the provider confirms an activation boundary.
    #[serde(default)]
    pub adapter: String,
    #[serde(default)]
    pub requested: RequestedConfig,
    #[serde(default)]
    pub requested_revision: u64,
    pub status: SessionStatus,
    pub high_water: u64,
    pub acknowledged: u64,
    pub open_turns: Vec<Uuid>,
    pub pending_requests: Vec<String>,
    pub held_commands: usize,
    pub fence: Option<FenceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective: Option<EffectiveConfig>,
    /// Agent ingress requests still awaiting an outcome.
    #[serde(default)]
    pub pending_ingress: Vec<Uuid>,
    /// Live durable resource claims reconciliators must account for.
    #[serde(default)]
    pub claims: Vec<ResourceClaim>,
    pub adapter_alive: bool,
}

/// What disconnected or restarted controllers must reconcile before new
/// admissions or restart recovery are permitted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub runtime_instance: Uuid,
    pub epoch: u64,
    pub recovery: RecoveryState,
    pub sessions: Vec<SessionInventory>,
    /// Commands still holding a non-terminal disposition.
    pub open_commands: Vec<CommandReceipt>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceipt {
    pub command_id: Uuid,
    pub disposition: CommandDisposition,
}

/// Why a generation can no longer dispatch. Once recorded, no delayed
/// prompt, answer, or reconnect can start more work there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FenceKind {
    /// Controller-authorized session stop.
    Stop,
    /// Narrow local authority used while the daemon was unavailable.
    EmergencyStop,
    /// Runtime restart fenced the surviving generation.
    Interrupted,
    /// Bounded buffers forced intake closed.
    Backpressure,
    /// Explicit drain/shutdown intent.
    Drained,
}

/// Cheap health/status view for authenticated callers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub runtime_instance: Uuid,
    pub epoch: u64,
    pub recovery: RecoveryState,
    pub persistence_ok: bool,
    /// The journal tail was torn or corrupt at open and was truncated —
    /// recovery then followed the crash path.
    pub tail_corrupt: bool,
    pub admission_open: bool,
    pub sessions: Vec<SessionInventory>,
    pub retained_event_bytes: u64,
    pub journal_bytes: u64,
    /// Records accepted into emergency buffering but not yet durable.
    pub buffered_records: usize,
    /// Recovered owners whose physical cleanup is not yet proven.
    #[serde(default)]
    pub unfenced_survivors: Vec<SessionRef>,
}

/// Drain intent: defer while work is live, or fence and terminate now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DrainPolicy {
    /// Allowed only with no live/pending work; otherwise reports the
    /// blockers and changes nothing.
    WhenIdle,
    /// Identifies affected sessions, fences them, terminates owners, and
    /// persists terminal dispositions.
    Forced,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "report",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DrainReport {
    /// Work or unconfirmed termination remains. A forced attempt may
    /// already have fenced affected owners.
    Deferred {
        live_sessions: Vec<Uuid>,
    },
    Drained {
        sessions: usize,
    },
}

/// Session-scoped authority minted at admission for agent ingress. The
/// host checks scope and expiry on every request and never renews them.
/// The token is the grant's proof: it rides no logs and is compared in
/// constant time, and it binds the caller to exactly one session
/// generation — it can never acquire controller authority or another
/// session's traffic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionGrant {
    pub grant_id: Uuid,
    pub session_id: Uuid,
    pub generation: u64,
    /// The bearer proof, minted at issue. Presented verbatim on connect.
    pub token: String,
    pub expires_at_ms: u64,
}

/// The descriptor format the provisioned `goddard-agent` reads for scoped
/// runtime ingress. This is the single serialization both the daemon's
/// provisioning writer and the CLI's `GODDARD_RUNTIME_INGRESS` reader use —
/// nobody invents a parallel envelope.
pub const INGRESS_DESCRIPTOR_VERSION: u32 = 1;

/// A private file binding one task's CLI invocations to a session-scoped
/// grant. The file carries the grant token, so it must stay owner-only —
/// it is never journaled, logged, or placed in a launch payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IngressDescriptor {
    pub version: u32,
    pub root: std::path::PathBuf,
    pub task_id: Uuid,
    pub grant: SessionGrant,
}

/// The agent-ingress surface an adapter applies to its provider child:
/// explicit environment pairs (descriptor path, task binding, inherited
/// reservation — never credentials) plus the pinned `goddard-agent`
/// directory to put on `PATH`. The grant itself travels only through the
/// descriptor file; this payload is safe to journal.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentIngress {
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub cli_dir: Option<std::path::PathBuf>,
}

/// Compose the provider child's agent-ingress environment: the explicit
/// pairs plus `cli_dir` prepended to `PATH`, falling back to a standard
/// search path when the inherited environment lacks one.
pub fn agent_ingress_env(
    agent: &AgentIngress,
    base_path: Option<&std::ffi::OsStr>,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut pairs: Vec<(std::ffi::OsString, std::ffi::OsString)> = agent
        .env
        .iter()
        .map(|(name, value)| (name.clone().into(), value.clone().into()))
        .collect();
    if let Some(cli_dir) = &agent.cli_dir {
        let fallback = std::ffi::OsStr::new(if cfg!(windows) {
            "C:\\Windows\\System32"
        } else {
            "/usr/bin:/bin:/usr/local/bin"
        });
        let path = std::env::join_paths(
            std::iter::once(cli_dir.clone()).chain(std::env::split_paths(
                base_path.unwrap_or(fallback),
            )),
        )
        .unwrap_or_else(|_| cli_dir.clone().into_os_string());
        if let Some(existing) = pairs
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        {
            existing.1 = path;
        } else {
            pairs.push(("PATH".into(), path));
        }
    }
    pairs
}

/// Agent-originated traffic class. CLI, MCP, and provider-callback
/// requests all ride the same scoped, deduplicated ingress; the runtime
/// records identity and forwards to the current daemon — it never
/// authorizes a new grant or executes product tools itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IngressKind {
    Cli,
    Mcp,
    Callback,
}

/// One agent request into the runtime. `request_id` is the caller-chosen
/// deduplication identity: a retry queries the recorded truth rather than
/// starting another operation, and the deadline is the caller's original
/// bound — retries and attachment changes never extend it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngressRequest {
    pub request_id: Uuid,
    pub class: IngressKind,
    /// Opaque request payload the current daemon interprets.
    pub subject: Value,
    /// Original deadline in epoch milliseconds.
    pub deadline_ms: u64,
}

/// Recorded truth for one ingress request. The distinction between
/// `Expired` and `Uncertain` is whether the request provably never reached
/// a daemon — a forwarded request past its deadline may have executed and
/// is held for reconciliation, never resent automatically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum IngressDisposition {
    /// Recorded; not yet handed to a daemon service.
    Pending,
    /// Handed to a controller's service; outcome unconfirmed.
    Forwarded,
    /// The daemon returned a result.
    Completed { result: Value },
    /// The daemon executed and reported failure.
    Failed { detail: String },
    /// Refused before dispatch — fenced generation, dead session, or a
    /// class the service declined.
    Refused { detail: String },
    /// The deadline elapsed while the request had never been dispatched.
    Expired,
    /// The daemon may have observed or completed the request; the proof
    /// is missing. Reconciliation may resolve it; nothing is resent.
    Uncertain { detail: String },
}

impl IngressDisposition {
    /// Whether this disposition can still change. `Forwarded` and
    /// `Pending` are live; `Uncertain` stays reconcilable like command
    /// uncertainty.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            IngressDisposition::Completed { .. }
                | IngressDisposition::Failed { .. }
                | IngressDisposition::Refused { .. }
                | IngressDisposition::Expired
        )
    }
}

/// The host's durable record for one ingress request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngressRecord {
    pub request_id: Uuid,
    pub session: SessionRef,
    pub grant_id: Uuid,
    pub class: IngressKind,
    pub subject: Value,
    pub deadline_ms: u64,
    /// Whether the request was ever handed to a daemon service — the
    /// dispatch marker that separates `Expired` from `Uncertain`.
    pub forwarded: bool,
    pub disposition: IngressDisposition,
}

/// A request the current daemon service must answer, with its original
/// identity and deadline preserved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardedRequest {
    pub request_id: Uuid,
    pub session: SessionRef,
    pub grant_id: Uuid,
    pub class: IngressKind,
    pub subject: Value,
    pub deadline_ms: u64,
}

/// The daemon's reported outcome for one forwarded request.
/// `Uncertain` means the daemon itself cannot prove whether the work
/// already ran — the host records that honestly rather than resending.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ForwardedOutcome {
    Completed { result: Value },
    Failed { detail: String },
    Uncertain { detail: String },
}

/// A durable resource reservation bound to runtime-owned work — a broker
/// lane, a credential lease, a surviving daemon-tool worker. Claims are
/// journaled, survive controller replacement, and release only through an
/// explicit controller decision after reconciliation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceClaim {
    pub claim_id: Uuid,
    pub session: SessionRef,
    /// Opaque claim descriptors the controller understands — broker lanes,
    /// credential leases, descendant-process claims.
    pub resources: Vec<String>,
    pub detail: Value,
}

/// Every failure a caller can observe. Rejected commands are dispositions
/// inside receipts; these are authorization and precondition failures that
/// never reach the journal.
#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "error",
    content = "detail",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HostError {
    #[error("credential or namespace rejected")]
    Unauthorized,
    #[error("control epoch {presented} is stale; current epoch is {current}")]
    StaleEpoch { presented: u64, current: u64 },
    #[error("no control epoch is held")]
    NoControl,
    #[error("unknown session {0}")]
    UnknownSession(Uuid),
    #[error("generation {presented} is not the session's current generation {current}")]
    GenerationMismatch { presented: u64, current: u64 },
    #[error("retry window {0} is unknown")]
    UnknownWindow(Uuid),
    #[error("retry window {0} has expired")]
    ExpiredWindow(Uuid),
    #[error("command id is already recorded with a different payload")]
    PayloadMismatch,
    #[error("cursor {cursor} precedes the retained floor {floor}")]
    CursorExpired { cursor: u64, floor: u64 },
    #[error("capacity exhausted: {0}")]
    Capacity(String),
    #[error("durable persistence is unavailable")]
    PersistenceUnavailable,
    #[error("incompatible protocol: {0}")]
    Incompatible(String),
    #[error("crash recovery requires inventory reconciliation before new work")]
    NotReconciled,
    #[error("this runtime namespace is already owned by a live instance")]
    AlreadyOwned,
    #[error("the runtime has drained; no further work is accepted")]
    Drained,
    #[error("admission is closed while draining")]
    AdmissionClosed,
    #[error("malformed request: {0}")]
    Malformed(String),
    #[error("io error: {0}")]
    Io(String),
}
