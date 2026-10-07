//! Provider backend and driver-event wire translation for `goddard-daemon`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::{
    AgentPromptDelivery, AgentWorkspace, Backend, Command, EventSink, Request, ResponsePayload,
    SessionDetailTail, WireDriverEvent, WorkspaceOperation, WorkspaceResult,
};
use anyhow::{Context as _, anyhow, bail};
use parking_lot::{Condvar, Mutex};
use serde_json::Value;
use uuid::Uuid;

use crate::attachments::AttachmentStore;
use crate::auto_prompts::AutoPromptService;
use crate::automations::AutomationService;
use crate::driver::{self, DriverHandle, DriverStartOptions, SessionOptions};
use crate::model::{
    AgentAskOutcome, AgentModelOption, AgentProjectMapResult, AgentSession, AgentSessionSearchHit,
    BackgroundWorkEvent, Checkpoint, CheckpointStatus, DriverEvent, PermissionOption, Project,
    ProjectKind, ProjectMapIntent, ProjectMapRanking, ProviderKind, ProviderModelOption,
    ProviderResumeCursor, ProviderSessionCatalogStatus, SessionStatus, SessionWorkspace,
    TurnStatus, UserInputQuestion, detail_prefix_signature,
};
use crate::persistence::{ComposerDraftStore, PersistedState, StateStore};
use crate::settings::DaemonSettingsStore;
#[cfg(test)]
use serde_json::json;
use waku_protocol::custom_commands::CustomCommand;
use waku_protocol::eval::{EvalAnswer, EvalQuestion, Evaluation};
#[cfg(test)]
use waku_protocol::event_from_wire;
use waku_protocol::persistence::{
    SessionMessageMatch, SessionMessageSearchScope, parse_session_message_search,
    resolve_named_search_project,
};
use waku_protocol::provider_session::{ProviderSessionFork, ProviderSessionForkRequest};
use waku_protocol::routing::{RouteCandidate, RouteTarget};
use waku_protocol::{decode_enum, event_to_wire};

/// How many fully hydrated transcripts the daemon keeps resident.
///
/// Hydration is a cache: consumers reload a released session from the store on
/// demand. Without a cap, a daemon that lives for days adopts the transcript
/// of every session its clients have touched — SaveTaskState pushes, hydrate
/// requests, forks, checkpoints — and resident memory grows without bound.
const RESIDENT_TRANSCRIPT_WINDOW: usize = 24;

/// Shared daemon-owned Jev path for both the public `Command::Evaluate`
/// surface and decisions embedded in higher-level commands. Every attempted
/// call keeps the same timeout, decision-log record, and backend boundary.
fn evaluate_with_feature(
    settings_store: &DaemonSettingsStore,
    secrets: &crate::integrations::SecretStore,
    state: Value,
    questions: BTreeMap<String, EvalQuestion>,
    feature: &str,
    timeout_secs: Option<u64>,
) -> anyhow::Result<Evaluation> {
    let settings = crate::inference::resolve_eval(&settings_store.get(), secrets)
        .ok_or_else(|| anyhow!("no evaluation backend is configured"))?;
    let started = std::time::Instant::now();
    let timeout_secs = timeout_secs.unwrap_or(crate::eval::EVAL_TIMEOUT_SECS);
    let result = if feature == "provider-switch" {
        crate::eval::evaluate_with_timeout_retry_503(&settings, &state, &questions, timeout_secs)
    } else {
        crate::eval::evaluate_with_timeout(&settings, &state, &questions, timeout_secs)
    };
    let mut record = crate::eval::EvalDecisionRecord::empty("evaluate");
    record.feature = feature.to_owned();
    record.backend = Some(settings.provider);
    record.latency_ms = Some(started.elapsed().as_millis() as u64);
    record.model = result
        .as_ref()
        .ok()
        .map(|evaluation| evaluation.model.clone());
    record.usage = result
        .as_ref()
        .ok()
        .map(|evaluation| evaluation.usage.clone());
    record.state = Some(state);
    record.questions = Some(questions);
    record.answers = result
        .as_ref()
        .ok()
        .map(|evaluation| evaluation.answers.clone());
    record.error = result.as_ref().err().map(|error| error.to_string());
    crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
    result
}

fn validate_workspace_relative_path(path: &Path) -> anyhow::Result<PathBuf> {
    let value = path.to_string_lossy().replace('\\', "/");
    if value.starts_with('/')
        || value.as_bytes().get(1) == Some(&b':')
        || value.split('/').any(|component| component == "..")
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        bail!("map paths must be relative to the session workspace");
    }
    let normalized = value
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/");
    Ok(if normalized.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(normalized)
    })
}

fn rank_project_map_candidates(
    candidates: &crate::repo_map::CandidateSet,
    evaluation: &Evaluation,
) -> Option<(Vec<String>, Vec<String>)> {
    let mut scores = Vec::with_capacity(candidates.candidates.len());
    for (index, candidate) in candidates.candidates.iter().enumerate() {
        let key = format!("candidate_{index:03}");
        let Some(EvalAnswer::Noul { noul }) = evaluation.answers.get(&key) else {
            return None;
        };
        if !noul.is_finite() || !(0.0..=1.0).contains(noul) {
            return None;
        }
        scores.push((*noul, candidate.local_score, candidate.path.clone()));
    }
    scores.sort_by(|(score_a, local_a, path_a), (score_b, local_b, path_b)| {
        score_b
            .total_cmp(score_a)
            .then_with(|| local_b.cmp(local_a))
            .then_with(|| path_a.cmp(path_b))
    });
    let selected = scores
        .iter()
        .filter(|(score, _, _)| *score >= 0.5)
        .map(|(_, _, path)| path.clone())
        .collect::<Vec<_>>();
    let mut other = scores
        .iter()
        .filter(|(score, _, _)| *score < 0.5)
        .map(|(_, _, path)| path.clone())
        .collect::<Vec<_>>();
    for path in &candidates.omitted_paths {
        if !other.contains(path) && !selected.contains(path) {
            other.push(path.clone());
        }
    }
    Some((selected, other))
}

/// How often the idle-runtime reaper scans. Eviction lag is at most this
/// plus the configured timeout, so a minute keeps the sweep cheap without
/// letting a just-expired runtime linger.
const IDLE_REAPER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Resident provider runtime lifetime when `runtime_idle_timeout_secs` is
/// unset. Thirty minutes covers stepping away without keeping every browsed
/// task's process alive for the whole workday.
const DEFAULT_RUNTIME_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Minimum spacing between pressure-driven shed passes — the OS can report
/// pressure every few seconds, and a runtime reopened under sustained
/// pressure is now active and protected anyway.
const PRESSURE_SHED_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(120);

/// Default cap for an agent's transcript search when its query carries no
/// `limit:` token.
const AGENT_SEARCH_DEFAULT_LIMIT: usize = 20;

/// How many tasks one archive proposal may name. The card shows a handful of
/// titles, and a request larger than this is almost certainly an agent bug,
/// not a reviewed set.
const AGENT_ARCHIVE_PROPOSAL_MAX_TASKS: usize = 32;

/// Cap on the reason an archive proposal shows the user.
const AGENT_ARCHIVE_PROPOSAL_MAX_REASON_CHARS: usize = 1_024;

/// How many task titles the archive card enumerates before eliding the rest.
const AGENT_ARCHIVE_CARD_TITLES: usize = 5;

/// How long an archived task is kept, in seconds, before it is removed
/// entirely. The sweep runs whenever task state loads rather than on a
/// timer, so this bounds retention without scheduling exact deletions.
const ARCHIVED_SESSION_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;

/// How long an archived task keeps its full transcript detail, in seconds.
/// Past this window [`WakuBackend::start_archive_detail_prune`] rewrites the
/// stored session so activities survive only as skeletons — kind, title and
/// status stay, while tool output, arguments, reasoning, diffs and images
/// go. Messages are a separate table and are never pruned. Boss chats are
/// exempt: rotation archives a chat the user still reads, so a chat in the
/// Boss project keeps its payloads until the outer purge removes it.
const ARCHIVED_DETAIL_RETENTION_SECONDS: u64 = 7 * 24 * 60 * 60;

/// Detail rows rewritten per prune batch. Bound so a batch transaction
/// holds the shared storage lock for tens of milliseconds, not seconds —
/// the sweep drains its backlog over repeated batches instead.
const ARCHIVE_PRUNE_BATCH: usize = 8;

/// How long a projectless project may sit in the catalog without a task.
/// Clients that provision a workspace ahead of the first prompt — mobile
/// and web save the project row, then submit — need the row to survive the
/// saves in between; anything older is an orphan a removal left behind.
const UNUSED_PROJECTLESS_GRACE_SECONDS: u64 = 24 * 60 * 60;

/// Releases resident transcripts beyond the recency window after a save.
/// `pinned` names sessions with live runtimes; dirty sessions are skipped
/// inside [`PersistedState::trim_idle_transcripts`] because they hold unsaved
/// work.
fn trim_resident_transcripts(state: &mut PersistedState, pinned: &HashSet<Uuid>) {
    state.trim_idle_transcripts(pinned, RESIDENT_TRANSCRIPT_WINDOW);
}

/// One `DaemonSessionSample` per session worth a row: resident (hydrated)
/// or running. Skeletons carry ~1 KB of list columns each and only count
/// toward `sessions_total` — a daemon with hundreds of stored tasks must
/// not turn the stats line into a session dump.
fn session_stats_sample(
    session: &AgentSession,
    running: bool,
) -> Option<waku_protocol::DaemonSessionSample> {
    (session.detail_loaded || running).then(|| waku_protocol::DaemonSessionSample {
        id: session.id,
        // Incognito sessions persist nowhere — their titles must not reach
        // `daemon-stats.jsonl` either.
        title: if session.incognito {
            String::new()
        } else {
            session.title.clone()
        },
        provider: session.provider,
        status: session.status,
        detail_loaded: session.detail_loaded,
        running,
        resident_messages: session.messages.len() as u32,
        resident_activities: session
            .transcript_blocks
            .iter()
            .map(|block| block.activities.len() as u32)
            .sum(),
        resident_bytes: resident_bytes(session),
    })
}

/// Rough heap estimate of a session's resident detail: struct sizes plus
/// the big string payloads. Underestimates nested collections — enough to
/// rank which sessions hold the daemon's memory, not to bill exact bytes.
fn resident_bytes(session: &AgentSession) -> u64 {
    fn strings<'a>(values: impl Iterator<Item = &'a String>) -> u64 {
        values.map(|value| value.capacity() as u64).sum()
    }
    fn opt_string(value: &Option<String>) -> u64 {
        value.as_ref().map_or(0, |value| value.capacity() as u64)
    }
    let mut bytes = (std::mem::size_of::<AgentSession>() + session.title.capacity()) as u64;
    for message in &session.messages {
        bytes += std::mem::size_of::<waku_protocol::model::Message>() as u64
            + message.content.capacity() as u64
            + opt_string(&message.display_content)
            + strings(
                message
                    .attachments
                    .iter()
                    .map(|a| &a.name)
                    .chain(message.attachments.iter().map(|a| &a.mention)),
            );
    }
    for block in &session.transcript_blocks {
        bytes += std::mem::size_of::<waku_protocol::model::TranscriptBlock>() as u64;
        for activity in &block.activities {
            bytes += std::mem::size_of_val(activity) as u64
                + activity.title.capacity() as u64
                + opt_string(&activity.source_id)
                + opt_string(&activity.tool_name)
                + opt_string(&activity.mcp_server)
                + opt_string(&activity.detail)
                + opt_string(&activity.arguments)
                + opt_string(&activity.output)
                + opt_string(&activity.display_target)
                + opt_string(&activity.display_description)
                + strings(activity.image_urls.iter())
                + activity
                    .reasoning
                    .as_ref()
                    .map_or(0, |block| block.content.capacity() as u64)
                + strings(activity.file_changes.iter().map(|change| &change.path))
                + strings(activity.file_changes.iter().filter_map(|c| c.diff.as_ref()));
        }
    }
    for turn in &session.turns {
        bytes += std::mem::size_of::<waku_protocol::model::AgentTurn>() as u64
            + opt_string(&turn.provider_resume_at)
            + turn.checkpoint.as_ref().map_or(0, |checkpoint| {
                checkpoint.git_ref.capacity() as u64
                    + strings(checkpoint.files.iter().map(|file| &file.path))
            });
    }
    for queued in &session.queued_messages {
        bytes += std::mem::size_of::<waku_protocol::model::QueuedMessage>() as u64
            + queued.content.capacity() as u64
            + opt_string(&queued.display_content);
    }
    bytes
}

/// Registry removal is the atomic handoff for a runtime or terminal, so the
/// request handler can answer immediately: the teardown itself — PTY grace
/// periods, process waits, provider unregistration — runs on this worker
/// instead of blocking the session's serialized command mailbox. Events a
/// driver emits while it unwinds stay scoped to the removed runtime id,
/// which the hub has already retired. A spawn failure falls back to the
/// inline drop.
fn drop_detached<T: Send + 'static>(value: T) {
    let _ = std::thread::Builder::new()
        .name("waku-detached-teardown".into())
        .spawn(move || drop(value));
}

/// Project-map state shared by every runtime in the daemon. One structural
/// index per workspace root, built and refreshed on background threads.
#[derive(Default)]
struct RepoMaps {
    /// Session → its workspace root. This scopes map requests and incremental
    /// refreshes to the caller's active local workspace.
    sessions: HashMap<Uuid, PathBuf>,
    /// Workspace root → index, shared across sessions in the same root.
    indexes: HashMap<PathBuf, crate::repo_map::RepoMapIndex>,
    /// Roots with a build/refresh in flight, so triggers don't pile up.
    building: HashSet<PathBuf>,
}

/// What the daemon remembers about one remote terminal: the client-chosen
/// runtime id that owns writes, the task surface that opened it (when the
/// client named one), and the cwd. Both ownership facts feed the orphan
/// sweep — a terminal whose task or workspace is removed gets no
/// `CloseTerminal`, and its PTY would otherwise run until daemon exit.
struct TerminalEntry {
    runtime_id: Uuid,
    owner: Option<Uuid>,
    cwd: PathBuf,
    terminal: crate::terminal::DaemonTerminal,
}

/// A live provider runtime: the client-chosen id that scopes its events,
/// the driver handle, and when it last did anything — forwarded an event
/// or served a request. `resumable` records whether the runtime can be
/// rebuilt from a provider cursor — seeded from the spawn options and set
/// once the driver's `Connected` handshake reports one. The idle reaper
/// reads both stamps. `provider`/`cwd` exist for the stats sampler, which
/// claims a descendant subtree by the directory its members run in.
struct RuntimeEntry {
    runtime_id: Uuid,
    driver: DriverHandle,
    last_active: std::time::Instant,
    resumable: bool,
    computer_use_available: bool,
    provider: ProviderKind,
    cwd: PathBuf,
}

/// The provider/model selection an `agent create` request carried. Every
/// `None` field inherits the sending task's configuration where the resolved
/// provider still matches it; see [`WakuBackend::create_agent_task`].
pub(crate) struct AgentCreateSelection {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub context_window: Option<String>,
}

/// A provider/model selection resolved to concrete values — the output of
/// [`WakuBackend::resolve_agent_task_selection`], shared by `agent create`
/// and summon admission. `concrete_model` is the catalog id the queue
/// counts when `model` carries a requested/inherited value or `None` for
/// the provider default.
pub(crate) struct ResolvedAgentSelection {
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub concrete_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub context_window: Option<String>,
    pub routed: Option<crate::routing::RouteRun>,
    pub sender_mode: waku_protocol::model::RuntimeMode,
    pub sender_environment: crate::model::SessionEnvironment,
}

/// A `workspace: "adopt"` target that passed validation: the finished
/// employee whose worktree it was and the workspace binding the adopter
/// takes over.
pub(crate) struct WorktreeAdoption {
    pub owner_name: String,
    pub workspace: SessionWorkspace,
}

/// A created task's first prompt. `Fixed` ships as sent. `Assignment` is
/// the Boss employee wrapper: the job text follows an "Assigned project"
/// line resolved against the task's run directory — the worktree path for
/// a worktree summon, the project path for a local one — so the employee
/// is pointed at the checkout it actually occupies, not the checkout its
/// supervisor named.
pub(crate) enum AgentTaskPrompt {
    Fixed(String),
    Assignment(String),
}

impl AgentTaskPrompt {
    /// The job text as the caller wrote it — before `Assignment` wrapping —
    /// which is what routing and blank validation judge.
    fn text(&self) -> &str {
        match self {
            Self::Fixed(prompt) | Self::Assignment(prompt) => prompt,
        }
    }

    fn is_blank(&self) -> bool {
        self.text().trim().is_empty()
    }

    fn resolve(self, workspace: &SessionWorkspace, project: &Path) -> String {
        match self {
            Self::Fixed(prompt) => prompt,
            Self::Assignment(prompt) => format!(
                "Assigned project: {}\nWork in that project and read its AGENTS.md before beginning. If a sandbox mounts the project at another path, use the guest working directory.\n\n{prompt}",
                workspace.path().unwrap_or(project).display()
            ),
        }
    }
}

/// The model pick `createPlan` lands on when the op leaves provider,
/// model, or effort unset: planning sessions are design/drafting work
/// and run Codex's sol model at medium effort.
const PLANNING_PROVIDER: ProviderKind = ProviderKind::Codex;
const PLANNING_MODEL: &str = "gpt-6.1-sol";
const PLANNING_EFFORT: &str = "medium";

/// Resolve one `agent create` trait field. An explicit value wins as sent —
/// `"default"` (or empty) selects the provider's own default, and an unknown
/// id passes through untouched, matching how `model` is handled. An omitted
/// field takes the sending task's `inherited` value only when the resolved
/// model's catalog still lists it; otherwise the model's own `default`
/// applies. An empty `options` list cannot constrain, so the inherited value
/// survives.
fn resolve_agent_trait(
    explicit: Option<String>,
    inherited: Option<String>,
    options: Option<&[ProviderModelOption]>,
    default: Option<&str>,
) -> Option<String> {
    if let Some(value) = explicit {
        return match value.trim() {
            "" | "default" => None,
            value => Some(value.to_owned()),
        };
    }
    let value = inherited?;
    let options = options.unwrap_or(&[]);
    if options.is_empty() || options.iter().any(|option| option.id == value) {
        Some(value)
    } else {
        default.map(str::to_owned)
    }
}

pub struct WakuBackend {
    sessions: Arc<Mutex<HashMap<Uuid, RuntimeEntry>>>,
    managed_goal_claims: Mutex<HashMap<(Uuid, Uuid), HashSet<Uuid>>>,
    repo_maps: Arc<(Mutex<RepoMaps>, Condvar)>,
    terminals: Arc<Mutex<HashMap<Uuid, TerminalEntry>>>,
    /// The hub's event source once `serve` installs it — terminal-channel
    /// owner bookkeeping needs it outside request dispatch (orphan sweeps).
    /// `detached` until then, so tests never need a live hub.
    event_source: Mutex<EventSink>,
    #[cfg(all(test, unix))]
    terminal_shell: Option<alacritty_terminal::tty::Shell>,
    settings: Arc<DaemonSettingsStore>,
    /// The host sleep assertion held while `DaemonSettings::keep_awake` is
    /// on — released on drop or when the setting flips off.
    wake: Mutex<Option<crate::power::SleepAssertion>>,
    /// MCP integrations: catalog state, the credential store, and the local
    /// proxy agents reach through `goddard_<id>` server entries.
    integrations: crate::integrations::IntegrationService,
    /// The inference providers' credential store — TypeSafe, the Vercel AI
    /// Gateway, Cloudflare Workers AI, OpenRouter — keyed `inference/<id>`.
    /// The settings document only ever carries write-only slots and the
    /// `credential_configured` flags this store feeds.
    inference_secrets: crate::integrations::SecretStore,
    task_store: Arc<StateStore>,
    task_state: Arc<Mutex<PersistedState>>,
    /// The hub's `task_state_changed` broadcast once `serve` installs it —
    /// daemon-side catalog mutations (boss project knobs) replay it so
    /// every attached client re-reads the rows.
    task_notifier: Mutex<Option<crate::share::TaskNotifier>>,
    removed_session_ids: Mutex<HashSet<Uuid>>,
    removed_project_ids: Mutex<HashSet<Uuid>>,
    composer_drafts: ComposerDraftStore,
    attachments: AttachmentStore,
    usage_scan_cache: Mutex<crate::usage_history::ScanCache>,
    /// Serializes `consumeCodexResetCredit`: one host shares one ChatGPT
    /// account, so a second call while a redemption is in flight is a
    /// double-click, not a second spend.
    codex_reset_credit_lock: Mutex<()>,
    checkpoint_capture_locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    /// Scoped agent credentials, per-session prompt queues, and the live
    /// turn bookkeeping the runtime event forwarder maintains. `pub(crate)`
    /// so server tests can mint a token and connect with it the way a
    /// provider session's harness would.
    pub(crate) agent: Arc<crate::agent::AgentState>,
    /// Serializes cold-start of a stored task's runtime so two agent prompts
    /// cannot race to spawn it.
    runtime_start_locks: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
    /// Whether an archive-detail prune sweep is running on its own thread.
    /// `LoadTaskState` retriggers the sweep; the flag keeps one in flight.
    archive_detail_prune_running: Arc<std::sync::atomic::AtomicBool>,
    /// Guards spawning the idle-runtime reaper — `set_event_source` may be
    /// installed more than once in tests that bind two servers to one
    /// backend.
    idle_reaper_started: std::sync::atomic::AtomicBool,
    /// The address the daemon bound, published to provider sessions as
    /// `GODDARD_DAEMON_ADDRESS` when agent tools are enabled. Set once by the
    /// daemon executable after it binds its listener. `Arc` so the link
    /// info handler can read it inside the share runtime.
    daemon_address: Arc<Mutex<Option<String>>>,
    /// The port a runtime-opened non-loopback listener is bound to, set by
    /// the server's exposure control. LAN discovery reports it as
    /// `DaemonInfo.ws_port`; `None` while unexposed.
    exposed_port: Arc<Mutex<Option<u16>>>,
    /// Process-memory sampling for `getDaemonStats` and the
    /// `daemon-stats.jsonl` debugging file.
    stats: Arc<crate::stats::DaemonStats>,
    usage_rates_dir: std::path::PathBuf,
    /// `~/Library/Application Support/<app>` — the parent of shared
    /// per-provider sandbox homes.
    data_dir: std::path::PathBuf,
    default_cwd: std::path::PathBuf,
    /// Friend-to-friend sharing; lazily binds the iroh endpoint on first
    /// friends command so tests and headless runs pay nothing.
    share: Arc<crate::share::ShareService>,
    /// Per-device pairing tokens — requests, grants, revokes.
    pairing: Arc<crate::pairing::PairingService>,
    /// The `USER`-derived label share friends and pairers both see.
    our_name: String,
    /// Scheduled automations — definitions, run history, and the tick that
    /// dispatches them whether or not a client is attached.
    automations: Arc<AutomationService>,
    boss: Arc<crate::boss::BossService>,
    auto_prompts: Arc<AutoPromptService>,
    /// The subscriber id of the client connection that most recently sent
    /// a visible prompt to each boss principal session — the attribution a
    /// `terminal` op needs, since the agent-scoped `goddard-agent`
    /// connection that delivers the op carries no subscriber of its own.
    boss_prompt_subscribers: Mutex<HashMap<Uuid, u64>>,
    /// Summon-queue wake channel — the scheduler thread parks on the
    /// condvar between passes; summons, policy writes, releases, and the
    /// reconciliation tick set the flag to run one pass.
    summon_wake: Arc<(Mutex<bool>, Condvar)>,
    /// Guards spawning the scheduler thread — `set_event_source` and lazy
    /// Boss activation may both reach it.
    summon_scheduler_started: std::sync::atomic::AtomicBool,
    /// Tests root their own broker ledger rather than the host's.
    broker_root: Mutex<Option<PathBuf>>,
}

/// How often the summon scheduler re-scans while tickets wait — the
/// bounded reconciliation tick that catches broker-side capacity changes
/// (a freed device, an external release) no daemon event announces.
const SUMMON_RECONCILE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

fn resource_set_empty(set: &waku_protocol::resources::ResourceSet) -> bool {
    set.exclusive.is_empty()
        && set.resident_devices == 0
        && set.native_builds == 0
        && set.desktop_input == 0
}

fn claim_managed_goal_turn(
    claims: &mut HashMap<(Uuid, Uuid), HashSet<Uuid>>,
    session_id: Uuid,
    goal_id: Uuid,
    turn_id: Uuid,
) -> bool {
    claims
        .entry((session_id, goal_id))
        .or_default()
        .insert(turn_id)
}

/// Which review op moved shared state — decides which notices go out.
enum ReviewMove {
    Approved,
    Rejected,
    Promoted,
}

mod agent_tasks;
mod boss_ops;
mod boss_rotation;
mod catalog_ops;
mod driver_commands;
mod events;
mod idle;
mod lifecycle;
mod projection;
mod prompt_delivery;
mod prompts;
mod queries;
mod response;
mod response_ops;
mod rpc;
mod runtime;
mod summon;
mod task_creation;
mod transfer;

use driver_commands::*;
use events::*;
use idle::*;
use projection::*;
use prompt_delivery::*;
use response::*;
use transfer::*;

#[cfg(test)]
mod tests;
