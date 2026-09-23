//! Provider backend and driver-event wire translation for `goddard-daemon`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::{
    AgentPromptDelivery, AgentWorkspace, Backend, Command, EventSink, Request, ResponsePayload,
    WireDriverEvent, WorkspaceOperation, WorkspaceResult,
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
    AgentSession, AgentSessionSearchHit, Checkpoint, CheckpointStatus, DriverEvent, Project,
    ProjectMapStatus, ProviderKind, ProviderModelOption, ProviderResumeCursor,
    ProviderSessionCatalogStatus, SessionStatus, SessionWorkspace, TurnStatus,
};
use crate::persistence::{ComposerDraftStore, PersistedState, StateStore};
use crate::settings::DaemonSettingsStore;
#[cfg(test)]
use serde_json::json;
use waku_protocol::custom_commands::CustomCommand;
#[cfg(test)]
use waku_protocol::event_from_wire;
use waku_protocol::persistence::{
    SessionMessageMatch, SessionMessageSearchScope, parse_session_message_search,
    resolve_named_search_project,
};
use waku_protocol::provider_session::{ProviderSessionFork, ProviderSessionForkRequest};
use waku_protocol::{decode_enum, event_to_wire};

/// How many fully hydrated transcripts the daemon keeps resident.
///
/// Hydration is a cache: consumers reload a released session from the store on
/// demand. Without a cap, a daemon that lives for days adopts the transcript
/// of every session its clients have touched — SaveTaskState pushes, hydrate
/// requests, forks, checkpoints — and resident memory grows without bound.
const RESIDENT_TRANSCRIPT_WINDOW: usize = 24;

/// How often the idle-runtime reaper scans. Eviction lag is at most this
/// plus the configured timeout, so a minute keeps the sweep cheap without
/// letting a just-expired runtime linger.
const IDLE_REAPER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Resident provider runtime lifetime when `runtime_idle_timeout_secs` is
/// unset. Thirty minutes covers stepping away without keeping every browsed
/// task's process alive for the whole workday.
const DEFAULT_RUNTIME_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Default cap for an agent's transcript search when its query carries no
/// `limit:` token.
const AGENT_SEARCH_DEFAULT_LIMIT: usize = 20;

/// How long an archived task is kept, in seconds, before it is removed
/// entirely. The sweep runs whenever task state loads rather than on a
/// timer, so this bounds retention without scheduling exact deletions.
const ARCHIVED_SESSION_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;

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
            + strings(message.attachments.iter().map(|a| &a.name).chain(
                message.attachments.iter().map(|a| &a.mention),
            ));
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
/// index per workspace root, built and refreshed on background threads; the
/// lock pairs with the condvar so a first prompt can give a cold build a
/// bounded moment instead of always sending unmapped.
#[derive(Default)]
struct RepoMaps {
    /// Session → its workspace root and runtime, recorded for runtimes
    /// launched while the experiment is on. Drives first-prompt injection,
    /// turn-settle refresh, and `Ready` broadcasts to same-root sessions.
    sessions: HashMap<Uuid, (PathBuf, Uuid)>,
    /// Sessions still owed a map on their first visible prompt. Fresh
    /// sessions only — a resumed one already carries its transcript.
    pending: HashSet<Uuid>,
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
    provider: ProviderKind,
    cwd: PathBuf,
}

/// The provider/model selection an `agent create` request carried. Every
/// `None` field inherits the sending task's configuration where the resolved
/// provider still matches it; see [`WakuBackend::create_agent_task`].
pub(crate) struct AgentCreateSelection {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub context_window: Option<String>,
}

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
    repo_maps: Arc<(Mutex<RepoMaps>, Condvar)>,
    terminals: Arc<Mutex<HashMap<Uuid, TerminalEntry>>>,
    #[cfg(all(test, unix))]
    terminal_shell: Option<alacritty_terminal::tty::Shell>,
    settings: Arc<DaemonSettingsStore>,
    /// The host sleep assertion held while `DaemonSettings::keep_awake` is
    /// on — released on drop or when the setting flips off.
    wake: Mutex<Option<crate::power::SleepAssertion>>,
    /// MCP integrations: catalog state, the credential store, and the local
    /// proxy agents reach through `goddard_<id>` server entries.
    integrations: crate::integrations::IntegrationService,
    task_store: Arc<StateStore>,
    task_state: Arc<Mutex<PersistedState>>,
    /// Project-memory scheduling and storage; sees every finished turn via
    /// the runtime event forwarder.
    memory: Arc<crate::memory::MemoryService>,
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
    auto_prompts: Arc<AutoPromptService>,
}

impl WakuBackend {
    pub fn new(settings: DaemonSettingsStore, task_store: StateStore) -> anyhow::Result<Self> {
        let mut task_state = task_store
            .load()
            .context("could not load Goddard task database")?;
        migrate_projectless_state(&task_store, &mut task_state)?;
        let composer_drafts = ComposerDraftStore::for_state_path(task_store.path());
        let attachments = AttachmentStore::new(
            task_store
                .path()
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("attachments"),
        );
        let data_dir = task_store
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_owned();
        let share_dir = data_dir.clone().join("share");
        let settings = Arc::new(settings);
        let integrations =
            crate::integrations::IntegrationService::new(settings.clone(), data_dir.clone())
                .context("could not start the integrations service")?;
        let our_name = std::env::var("USER")
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Goddard".to_owned());
        let usage_rates_dir = data_dir.clone();
        let automations = Arc::new(
            AutomationService::open(data_dir.join("automations.json"))
                .context("could not load Goddard automations")?,
        );
        let auto_prompts = Arc::new(
            AutoPromptService::open(data_dir.join("auto-prompts.json"))
                .context("could not load Goddard auto prompt history")?,
        );
        let task_state = Arc::new(Mutex::new(task_state));
        let task_store = Arc::new(task_store);
        let backend = Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            repo_maps: Arc::new((Mutex::new(RepoMaps::default()), Condvar::new())),
            terminals: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(all(test, unix))]
            terminal_shell: None,
            memory: crate::memory::MemoryService::new(
                settings.clone(),
                task_state.clone(),
                task_store.clone(),
            ),
            settings,
            wake: Mutex::new(None),
            integrations,
            task_store,
            task_state,
            removed_session_ids: Mutex::new(HashSet::new()),
            removed_project_ids: Mutex::new(HashSet::new()),
            composer_drafts,
            attachments,
            usage_scan_cache: Mutex::new(HashMap::new()),
            codex_reset_credit_lock: Mutex::new(()),
            checkpoint_capture_locks: Mutex::new(HashMap::new()),
            agent: Arc::new(crate::agent::AgentState::default()),
            runtime_start_locks: Mutex::new(HashMap::new()),
            idle_reaper_started: std::sync::atomic::AtomicBool::new(false),
            daemon_address: Arc::new(Mutex::new(None)),
            exposed_port: Arc::new(Mutex::new(None)),
            stats: crate::stats::DaemonStats::open(&data_dir),
            usage_rates_dir,
            default_cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            share: Arc::new(crate::share::ShareService::new(
                share_dir.clone(),
                our_name.clone(),
            )),
            pairing: Arc::new(crate::pairing::PairingService::new(
                &data_dir,
                our_name.clone(),
            )),
            data_dir,
            our_name,
            automations,
            auto_prompts,
        };
        backend.purge_expired_archived_sessions();
        backend.install_link_handlers();
        {
            let task_state = backend.task_state.clone();
            let task_store = backend.task_store.clone();
            let share_dir = share_dir.clone();
            backend.share.set_transfer_hook(Arc::new(
                move |transfer| match create_transfer_session(
                    &task_state,
                    &task_store,
                    &share_dir,
                    transfer,
                ) {
                    Ok(session_id) => Some(session_id),
                    Err(error) => {
                        eprintln!("could not create transfer session: {error:#}");
                        None
                    }
                },
            ));
        }
        {
            let task_state = backend.task_state.clone();
            let task_store = backend.task_store.clone();
            let share_dir = share_dir.clone();
            backend.share.set_chat_hook(Arc::new(
                move |peer_name, text| match create_chat_session(
                    &task_state,
                    &task_store,
                    &share_dir,
                    &peer_name,
                    &text,
                ) {
                    Ok(session_id) => Some(session_id),
                    Err(error) => {
                        eprintln!("could not create chat session: {error:#}");
                        None
                    }
                },
            ));
        }
        {
            // The share layer's view of local projects — paths, names,
            // and `origin` URLs for share matching. Resolving every
            // remote is a git call per project, so cache briefly.
            let task_state = backend.task_state.clone();
            let cache = Arc::new(Mutex::new(
                None::<(std::time::Instant, Vec<crate::share::RepoInfo>)>,
            ));
            backend.share.set_repo_resolver(Arc::new(move || {
                let mut cache = cache.lock();
                let stale = cache
                    .as_ref()
                    .is_none_or(|(at, _)| at.elapsed() > std::time::Duration::from_secs(60));
                if stale {
                    let projects = task_state.lock().projects.clone();
                    let repos = projects
                        .iter()
                        .map(|project| crate::share::RepoInfo {
                            path: project.path.clone(),
                            name: project.name.clone(),
                            origin_url: crate::git_branch::remote_url(&project.path, "origin")
                                .ok()
                                .flatten(),
                        })
                        .collect();
                    *cache = Some((std::time::Instant::now(), repos));
                }
                cache
                    .as_ref()
                    .map(|(_, repos)| repos.to_vec())
                    .unwrap_or_default()
            }));
        }
        {
            // The share layer's view of sessions — what friends may list
            // and watch on projects shared with session sharing on.
            // Archived sessions and side chats stay hidden, matching
            // task-list semantics.
            let task_state = backend.task_state.clone();
            backend
                .share
                .set_session_source(crate::share::SessionSource {
                    list: Arc::new({
                        let task_state = task_state.clone();
                        move |repo_path| {
                            let state = task_state.lock();
                            let Some(project) = state
                                .projects
                                .iter()
                                .find(|project| project.path == repo_path)
                            else {
                                return Vec::new();
                            };
                            state
                                .sessions
                                .iter()
                                .filter(|session| {
                                    session.project_id == project.id
                                        && session.archived_at.is_none()
                                        && session.side_chat_of.is_none()
                                })
                                .map(|session| waku_protocol::friends::SharedSessionSummary {
                                    session_id: session.id,
                                    title: session.title.clone(),
                                    auto_title: session.auto_title.clone(),
                                    status: session.status,
                                    created_at: session.created_at,
                                    last_reply_at: session.last_reply_at,
                                })
                                .collect()
                        }
                    }),
                    snapshot: Arc::new(move |session_id| {
                        task_state
                            .lock()
                            .sessions
                            .iter()
                            .find(|session| session.id == session_id)
                            .cloned()
                    }),
                });
        }
        backend.apply_wake_setting();
        Ok(backend)
    }

    /// Acquire or release the host sleep assertion to match the stored
    /// `keep_awake` flag. Runs at construction and after every settings
    /// write that can carry the flag.
    fn apply_wake_setting(&self) {
        let enabled = self.settings.get().keep_awake;
        let mut wake = self.wake.lock();
        if enabled != wake.is_some() {
            *wake = enabled.then(|| {
                crate::power::SleepAssertion::acquire(
                    "Goddard keeps this host awake so connected devices stay reachable",
                )
            });
        }
    }

    /// Record the daemon's bound address for `GODDARD_DAEMON_ADDRESS`
    /// injection. Called once by the daemon executable before it starts
    /// serving; providers launched while it is unset get no agent surface.
    pub fn set_daemon_address(&self, address: String) {
        *self.daemon_address.lock() = Some(address);
    }

    /// Append the clean-exit marker to `daemon-stats.jsonl` — the next boot
    /// reads its absence as an abnormal death. Called by the daemon
    /// executable after `serve` returns on an orderly shutdown.
    pub fn mark_clean_shutdown(&self) {
        self.stats.mark_clean_shutdown();
    }

    /// Start the automation scheduler: reconcile runs a previous daemon
    /// left open, then tick. Called once by the daemon executable before it
    /// starts serving — the service needs the backend's `Arc` for dispatch.
    pub fn start_automations(self: &Arc<Self>) {
        self.automations.start(self);
        self.auto_prompts.start(self);
    }

    /// Point the `waku-link` ALPN at the daemon's metadata and pairing
    /// service. The handlers are installed once; whichever share runtime
    /// spawns later picks them up.
    fn install_link_handlers(&self) {
        let name = self.our_name.clone();
        let share_dir = self.share_dir();
        let daemon_address = self.daemon_address.clone();
        let exposed_port = self.exposed_port.clone();
        let pairing = self.pairing.clone();
        let info: waku_share::link::InfoHandler = Arc::new(move || {
            // Only report a ws port when the daemon is actually bound
            // beyond loopback — otherwise discovery would send clients to
            // an address they cannot reach. A runtime-opened listener wins
            // over the startup bind.
            let ws_port = exposed_port.lock().or_else(|| {
                daemon_address
                    .lock()
                    .as_deref()
                    .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
                    .filter(|address| !address.ip().is_loopback())
                    .map(|address| address.port())
            });
            let endpoint_id = waku_share::identity::load_or_create(&share_dir)
                .map(|secret| secret.public().to_string())
                .unwrap_or_default();
            waku_share::link::DaemonInfo {
                name: name.clone(),
                ws_port,
                protocol_version: waku_protocol::PROTOCOL_VERSION,
                endpoint_id,
            }
        });
        let pair: waku_share::link::PairHandler = Arc::new(move |_id, device_name| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let pairing = pairing.clone();
            // The pairing service is synchronous; park its wait on a
            // blocking thread so the share runtime's executor stays free.
            std::thread::spawn(move || {
                let decision = match pairing.request_blocking(&device_name, "link") {
                    crate::pairing::PairReply::Granted { token } => {
                        waku_share::link::PairDecision::Grant { token }
                    }
                    crate::pairing::PairReply::Declined { .. }
                    | crate::pairing::PairReply::Busy { .. } => {
                        waku_share::link::PairDecision::Decline
                    }
                };
                let _ = tx.send(decision);
            });
            rx
        });
        self.share.set_link_handlers(info, pair);
    }

    fn share_dir(&self) -> std::path::PathBuf {
        self.task_store
            .path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("share")
    }

    #[cfg(all(test, unix))]
    pub(crate) fn with_terminal_shell(mut self, shell: alacritty_terminal::tty::Shell) -> Self {
        self.terminal_shell = Some(shell);
        self
    }

    fn open_terminal(
        &self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        events: EventSink,
    ) -> anyhow::Result<crate::terminal::DaemonTerminal> {
        #[cfg(all(test, unix))]
        if let Some(shell) = &self.terminal_shell {
            return crate::terminal::DaemonTerminal::open_with_shell(
                cwd,
                cols,
                rows,
                events,
                shell.clone(),
            );
        }
        ensure_shell_environment();
        crate::terminal::DaemonTerminal::open(cwd, cols, rows, events)
    }

    /// Capture and persist one ending checkpoint exactly once per daemon.
    /// Desktop and Web may observe the same turn completion concurrently; a
    /// per-worktree lock prevents both clients — and adjacent turns — from
    /// running the expensive Git snapshot over the same worktree at once
    /// while leaving unrelated worktrees independent.
    fn capture_turn_checkpoint(
        &self,
        cwd: PathBuf,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<Checkpoint> {
        let capture_lock = self
            .checkpoint_capture_locks
            .lock()
            .entry(cwd.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _capture = capture_lock.lock();

        {
            let mut state = self.task_state.lock();
            if let Some(index) = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
            {
                self.task_store.hydrate(&mut state.sessions[index])?;
                if let Some(checkpoint) = state.sessions[index]
                    .turns
                    .iter()
                    .find(|turn| turn.turn_count == turn_count)
                    .and_then(|turn| turn.checkpoint.as_ref())
                    .filter(|checkpoint| {
                        matches!(
                            checkpoint.status,
                            CheckpointStatus::Ready | CheckpointStatus::Unavailable
                        )
                    })
                {
                    return Ok(checkpoint.clone());
                }
            }
        }

        let capture_started = std::time::Instant::now();
        let checkpoint = crate::checkpoint::capture_turn(&cwd, session_id, turn_count)?;
        // The transcript holds a "checking for changes" card open for this
        // round trip — the elapsed line is the only record of how long the
        // worktree snapshot actually took.
        eprintln!(
            "turn checkpoint for session {session_id} turn {turn_count} captured in {:?}",
            capture_started.elapsed()
        );
        let mut state = self.task_state.lock();
        if let Some(index) = state
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        {
            self.task_store.hydrate(&mut state.sessions[index])?;
            if let Some(turn) = state.sessions[index]
                .turns
                .iter_mut()
                .find(|turn| turn.turn_count == turn_count)
            {
                turn.checkpoint = Some(checkpoint.clone());
                state.mark_session_dirty(session_id);
                self.task_store.save(&mut state)?;
            }
        }
        Ok(checkpoint)
    }

    /// Drop terminals whose owning task is gone or whose cwd sat under a
    /// removed workspace. Remote clients send `CloseTerminal` when their
    /// surfaces unmount, but a disconnect — or a deletion another client
    /// made — skips that, and the PTY plus its shell would otherwise run
    /// until daemon exit.
    fn sweep_orphaned_terminals(
        &self,
        removed_sessions: &[Uuid],
        workspace_roots: &[PathBuf],
    ) -> Vec<crate::terminal::DaemonTerminal> {
        let mut terminals = self.terminals.lock();
        let orphaned = terminals
            .iter()
            .filter(|(_, entry)| {
                entry
                    .owner
                    .is_some_and(|owner| removed_sessions.contains(&owner))
                    || workspace_roots
                        .iter()
                        .any(|root| entry.cwd.starts_with(root))
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        orphaned
            .into_iter()
            .filter_map(|id| terminals.remove(&id).map(|entry| entry.terminal))
            .collect()
    }

    /// Removes one task from daemon state and storage. The id is remembered
    /// so a stale client `SaveTaskState` cannot restore the row, and any live
    /// runtime is dropped with it. When the departed task was the last one in
    /// a projectless workspace, that workspace leaves with it — its live
    /// directory and any archive zip — since no session can reach it again.
    fn remove_session(&self, session_id: Uuid) -> anyhow::Result<()> {
        let mut removed_workspace = None;
        let mut removed_ids;
        let mut workspace_roots: Vec<PathBuf>;
        {
            let mut state = self.task_state.lock();
            // Side chats die with their parent: walk the descendant set
            // first so a removal can never leave orphans behind.
            removed_ids = vec![session_id];
            let mut cursor = 0;
            while cursor < removed_ids.len() {
                let parent = removed_ids[cursor];
                cursor += 1;
                removed_ids.extend(
                    state
                        .sessions
                        .iter()
                        .filter(|session| session.side_chat_of == Some(parent))
                        .map(|session| session.id),
                );
            }
            {
                let mut removed = self.removed_session_ids.lock();
                for id in &removed_ids {
                    removed.insert(*id);
                }
            }
            // Terminals can't be attributed to a session row once it's gone,
            // so gather each removed task's dedicated workspace first. The
            // shared project root is deliberately absent — sibling tasks
            // keep their terminals.
            workspace_roots = state
                .sessions
                .iter()
                .filter(|session| removed_ids.contains(&session.id))
                .filter_map(|session| session.workspace.path().map(Path::to_path_buf))
                .collect();
            let project_id = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.project_id);
            state
                .sessions
                .retain(|session| !removed_ids.contains(&session.id));
            if let Some(project_id) = project_id {
                let remove_project = state
                    .projects
                    .iter()
                    .find(|project| project.id == project_id)
                    .is_some_and(Project::is_projectless)
                    && !state
                        .sessions
                        .iter()
                        .any(|session| session.project_id == project_id);
                if remove_project {
                    removed_workspace = state
                        .projects
                        .iter()
                        .find(|project| project.id == project_id)
                        .map(|project| project.path.clone());
                    state.projects.retain(|project| project.id != project_id);
                }
            }
            self.task_store.save(&mut state)?;
        }
        if let Some(path) = removed_workspace {
            workspace_roots.push(path.clone());
            // A failed removal leaves files behind — safe — so the task's
            // removal does not hinge on it.
            let _ = crate::projectless::remove_workspace(&path);
        }
        let removed_terminals = self.sweep_orphaned_terminals(&removed_ids, &workspace_roots);
        drop_detached(removed_terminals);
        let removed_runtimes = removed_ids
            .iter()
            .filter_map(|id| self.sessions.lock().remove(id))
            .collect::<Vec<_>>();
        for runtime in &removed_runtimes {
            runtime.driver.begin_shutdown();
        }
        drop_detached(removed_runtimes);
        for id in removed_ids {
            self.agent.clear_session(id);
        }
        Ok(())
    }

    /// Removes a project and every task under it. The ids are remembered for
    /// the same reason `remove_session` remembers tasks: a stale client save
    /// must not restore the catalog row another client just deleted.
    fn remove_project(&self, project_id: Uuid) -> anyhow::Result<()> {
        let mut removed_workspace = None;
        let mut removed_ids;
        let workspace_roots: Vec<PathBuf>;
        {
            let mut state = self.task_state.lock();
            let Some(index) = state
                .projects
                .iter()
                .position(|project| project.id == project_id)
            else {
                return Ok(());
            };
            if state.projects[index].is_projectless() {
                removed_workspace = Some(state.projects[index].path.clone());
            }
            let mut removed_set = state
                .sessions
                .iter()
                .filter(|session| session.project_id == project_id)
                .map(|session| session.id)
                .collect::<HashSet<_>>();
            let mut cursor = 0;
            removed_ids = removed_set.iter().copied().collect::<Vec<_>>();
            while cursor < removed_ids.len() {
                let parent = removed_ids[cursor];
                cursor += 1;
                for child in state
                    .sessions
                    .iter()
                    .filter(|session| session.side_chat_of == Some(parent))
                    .map(|session| session.id)
                {
                    if removed_set.insert(child) {
                        removed_ids.push(child);
                    }
                }
            }
            self.removed_project_ids.lock().insert(project_id);
            {
                let mut removed = self.removed_session_ids.lock();
                for id in &removed_ids {
                    removed.insert(*id);
                }
            }
            // The project row is leaving the catalog entirely: its checkout
            // and every removed task's worktree both end as sweep roots.
            workspace_roots = std::iter::once(state.projects[index].path.clone())
                .chain(
                    state
                        .sessions
                        .iter()
                        .filter(|session| removed_ids.contains(&session.id))
                        .filter_map(|session| session.workspace.path().map(Path::to_path_buf)),
                )
                .collect();
            state
                .sessions
                .retain(|session| !removed_ids.contains(&session.id));
            state.projects.remove(index);
            self.task_store.save(&mut state)?;
        }
        if let Some(path) = removed_workspace {
            let _ = crate::projectless::remove_workspace(&path);
        }
        let removed_terminals = self.sweep_orphaned_terminals(&removed_ids, &workspace_roots);
        drop_detached(removed_terminals);
        let removed_runtimes = removed_ids
            .iter()
            .filter_map(|id| self.sessions.lock().remove(id))
            .collect::<Vec<_>>();
        for runtime in &removed_runtimes {
            runtime.driver.begin_shutdown();
        }
        drop_detached(removed_runtimes);
        for id in removed_ids {
            self.agent.clear_session(id);
        }
        Ok(())
    }

    /// Deletes archived tasks whose archive has outlived the retention
    /// window.
    ///
    /// Runs whenever task state loads — daemon startup and every client
    /// `LoadTaskState` — so retention does not depend on a timer inside a
    /// daemon clients may keep alive for weeks. As with ordinary removal, the
    /// task's Git worktree is deliberately left on disk; only its checkpoint
    /// refs are deleted. A projectless workspace does leave with its last
    /// task — `remove_session` drops the live directory and any archive zip.
    fn purge_expired_archived_sessions(&self) {
        let cutoff = crate::model::unix_time().saturating_sub(ARCHIVED_SESSION_RETENTION_SECONDS);
        let expired = {
            let state = self.task_state.lock();
            let mut expired = Vec::new();
            for index in 0..state.sessions.len() {
                if state.sessions[index]
                    .archived_at
                    .is_none_or(|archived_at| archived_at > cutoff)
                {
                    continue;
                }
                let session_id = state.sessions[index].id;
                // Checkpoint refs live in the repository's shared
                // namespace, so delete them from the project checkout —
                // the task's worktree may already be gone, and a missing
                // cwd would silently leave the refs behind.
                let workspace = state
                    .projects
                    .iter()
                    .find(|project| project.id == state.sessions[index].project_id)
                    .map(|project| project.path.clone())
                    .or_else(|| {
                        state.sessions[index]
                            .workspace
                            .path()
                            .map(Path::to_path_buf)
                    });
                expired.push((session_id, workspace));
            }
            expired
        };
        for (session_id, workspace) in expired {
            if let Some(cwd) = workspace {
                let _ = crate::checkpoint::delete_all_session_refs(&cwd, session_id);
            }
            let _ = self.remove_session(session_id);
        }
    }
}

/// Storage-layout migrations belong to the daemon because both the database
/// rows and the directories name paths on its host. Persist after each move
/// so a later failure cannot leave an earlier project pointing at its old
/// location in SQLite.
fn migrate_projectless_state(
    task_store: &StateStore,
    task_state: &mut PersistedState,
) -> anyhow::Result<()> {
    let indices = task_state
        .projects
        .iter()
        .enumerate()
        .filter_map(|(index, project)| {
            crate::projectless::needs_migration(&project.path).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in indices {
        let old_path = task_state.projects[index].path.clone();
        let workspace = crate::projectless::migrate_workspace(&old_path).with_context(|| {
            format!(
                "could not move projectless workspace {} under ~/.goddard/projects",
                old_path.display()
            )
        })?;
        task_state.projects[index].name = crate::model::Project::PROJECTLESS_NAME.to_owned();
        task_state.projects[index].path = workspace.cwd;
        task_store
            .save(task_state)
            .context("could not persist migrated projectless workspace")?;
    }
    Ok(())
}

/// Materialize a transfer's agent session: a task under the synthetic
/// "Friends" project whose first messages are the sender's note and the
/// receipt — peer, title, and where the files landed — rendered as
/// assistant messages, not a sent bubble. The session stays quarantined
/// (idle, no provider turn started) until the user chooses to trust it.
fn create_transfer_session(
    task_state: &Arc<Mutex<PersistedState>>,
    task_store: &Arc<StateStore>,
    share_dir: &Path,
    transfer: &waku_protocol::friends::TransferInfo,
) -> anyhow::Result<Uuid> {
    let Some(dest_dir) = &transfer.dest_dir else {
        bail!("incoming transfer completed without a destination");
    };
    let mut state = task_state.lock();
    let project_id = friends_project_id(&mut state, share_dir);
    // Transfer sessions are unconditionally sandboxed (received files never
    // reach the host fs), so the provider must be one the sandbox can run —
    // fall back to Claude rather than minting a session that can never start.
    let provider = if state.last_provider.supports_sandbox() {
        state.last_provider
    } else {
        ProviderKind::Claude
    };
    let mut session = state.new_session(project_id, provider);
    session.title = format!("{} from {}", transfer.title, transfer.peer_name);
    // The receipt is a notification, not a prompt awaiting a reply — a
    // provider turn holds the assistant messages so they render like an
    // agent reply (bot style), not a sent bubble. The sender's note rides
    // above the delivery details.
    session.begin_provider_turn();
    if let Some(note) = transfer.note.as_deref().filter(|note| !note.is_empty()) {
        session.push_message(crate::model::MessageRole::Assistant, note);
    }
    session.push_message(
        crate::model::MessageRole::Assistant,
        format!(
            "{} sent you \"{}\".\n\nFiles are in {}\n\nThe files have not been opened or executed — decide whether you trust them before asking me to work with them.",
            transfer.peer_name,
            transfer.title,
            dest_dir.display()
        ),
    );
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;
    session.quarantined = true;
    // Received files stay in the sandbox VM even once trusted — the agent
    // never works on them with this Mac's filesystem in reach.
    session.environment = crate::model::SessionEnvironment::Sandbox;
    let session_id = session.id;
    state.push_session(session);
    task_store.save(&mut state)?;
    Ok(session_id)
}

/// The "Friends" project transfer and chat sessions live in — found by
/// path or registered on first delivery.
fn friends_project_id(state: &mut PersistedState, share_dir: &Path) -> Uuid {
    match state
        .projects
        .iter()
        .find(|project| project.path == share_dir)
        .map(|project| project.id)
    {
        Some(id) => id,
        None => {
            let mut project = Project::from_path(share_dir.to_path_buf());
            project.name = "Friends".to_owned();
            let id = project.id;
            state.projects.push(project);
            id
        }
    }
}

/// An incoming chat message materializes like a delivered transfer — a
/// session in the "Friends" project with the text rendered as an agent
/// reply. There is nothing to trust or hand off, so unlike a transfer it
/// is neither quarantined nor sandboxed — just an idle chat.
fn create_chat_session(
    task_state: &Arc<Mutex<PersistedState>>,
    task_store: &Arc<StateStore>,
    share_dir: &Path,
    peer_name: &str,
    text: &str,
) -> anyhow::Result<Uuid> {
    let mut state = task_state.lock();
    let project_id = friends_project_id(&mut state, share_dir);
    let mut session = state.new_session(project_id, state.last_provider);
    session.title = format!("Message from {peer_name}");
    session.begin_provider_turn();
    session.push_message(crate::model::MessageRole::Assistant, text);
    session.finish_active_turn(TurnStatus::Completed);
    session.status = SessionStatus::Idle;
    let session_id = session.id;
    state.push_session(session);
    task_store.save(&mut state)?;
    Ok(session_id)
}

impl Backend for WakuBackend {
    fn authenticate_agent(&self, token: &str) -> Option<Uuid> {
        self.agent.resolve(token)
    }

    fn authenticate_paired(&self, token: &str) -> bool {
        self.pairing.authenticate(token)
    }

    fn request_pair(&self, device_name: &str, transport: &str) -> crate::pairing::PairReply {
        self.pairing.request_blocking(device_name, transport)
    }

    fn set_pairing_sink(&self, sink: crate::pairing::PairingSink) {
        self.pairing.set_sink(sink);
    }

    fn daemon_name(&self) -> String {
        self.our_name.clone()
    }

    fn lan_advertisement(&self) -> Option<(String, String)> {
        // The EndpointId doubles as the daemon's LAN identity — the same
        // string friend codes and iroh discovery carry.
        let id = waku_share::identity::load_or_create(&self.share_dir())
            .map(|secret| secret.public().to_string())
            .ok()?;
        Some((self.our_name.clone(), id))
    }

    fn note_exposed_port(&self, port: Option<u16>) {
        *self.exposed_port.lock() = port;
    }

    fn kickstart_reachability(&self) {
        self.share.kickstart();
    }

    fn set_friends_sink(&self, sink: crate::share::FriendsSink) {
        self.share.set_sink(sink);
    }

    fn set_task_state_sink(&self, sink: crate::share::TaskNotifier) {
        self.share.set_task_notifier(sink.clone());
        self.automations.set_task_notifier(sink);
    }

    fn set_automations_sink(&self, sink: crate::automations::AutomationsSink) {
        self.automations.set_sink(sink);
    }

    fn set_event_source(&self, events: EventSink) {
        self.automations.set_event_source(events.clone());
        self.auto_prompts.set_event_source(events.clone());
        // The reaper starts with the event hub: retiring a runtime needs a
        // sink, and before serve() installs one there is nothing to retire.
        if self
            .idle_reaper_started
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        // The stats sampler shares the guard: it owns a file and a thread,
        // so a second event-source install must not duplicate it either.
        // The probe locks each map in turn rather than nesting — per-minute
        // cadence means a consistent-as-of snapshot per map is enough.
        let probe_sessions = self.sessions.clone();
        let probe_terminals = self.terminals.clone();
        let probe_state = self.task_state.clone();
        self.stats.spawn_sampler(move || {
            let (runtimes, runtime_dirs, running) = {
                let sessions = probe_sessions.lock();
                let running: HashSet<Uuid> = sessions.keys().copied().collect();
                let dirs = sessions
                    .iter()
                    .map(|(session_id, entry)| crate::stats::RuntimeDir {
                        session_id: *session_id,
                        provider: entry.provider,
                        cwd: std::fs::canonicalize(&entry.cwd)
                            .unwrap_or_else(|_| entry.cwd.clone()),
                    })
                    .collect();
                (sessions.len() as u32, dirs, running)
            };
            let (terminals, terminal_roots) = {
                let terminals = probe_terminals.lock();
                let roots = terminals
                    .iter()
                    .map(|(_, entry)| (entry.terminal.child_pid(), entry.owner))
                    .collect();
                (terminals.len() as u32, roots)
            };
            let state = probe_state.lock();
            crate::stats::StatsProbe {
                runtimes,
                terminals,
                runtime_dirs,
                terminal_roots,
                sessions_total: state.sessions.len() as u32,
                sessions: state
                    .sessions
                    .iter()
                    .filter_map(|session| session_stats_sample(session, running.contains(&session.id)))
                    .collect(),
            }
        });
        let sessions = self.sessions.clone();
        let task_state = self.task_state.clone();
        let settings = self.settings.clone();
        let agent = self.agent.clone();
        let repo_maps = self.repo_maps.clone();
        let _ = std::thread::Builder::new()
            .name("waku-idle-reaper".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(IDLE_REAPER_INTERVAL);
                    for (session_id, runtime_id, driver) in
                        reap_idle_runtimes(&sessions, &task_state, &settings, &agent)
                    {
                        evict_idle_runtime(
                            session_id,
                            runtime_id,
                            driver,
                            &agent,
                            &repo_maps,
                            &events,
                        );
                    }
                }
            });
    }

    fn set_review_notifier(&self, notifier: crate::share::ReviewNotifier) {
        self.share.set_review_notifier(notifier);
    }

    fn set_session_streamer(&self, streamer: crate::share::SessionStreamer) {
        self.share.set_session_streamer(streamer);
    }

    fn set_friend_session_sink(&self, sink: crate::share::FriendSessionSink) {
        self.share.set_friend_session_sink(sink);
    }

    fn trigger_automation_webhook(
        &self,
        automation_id: Uuid,
        key: &str,
    ) -> anyhow::Result<Option<waku_protocol::automations::AutomationRun>> {
        self.automations.trigger_webhook(automation_id, key)
    }

    fn handle(
        &self,
        request: Request,
        events: EventSink,
        agent: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload> {
        let session_id = request.session_id;
        let runtime_id = request.runtime_id;
        match request.command {
            Command::AttachSession => {
                let sessions = self.sessions.lock();
                let Some(entry) = sessions.get(&session_id) else {
                    return Ok(ResponsePayload::SessionRuntime {
                        runtime_id: None,
                        supports_steer: false,
                        supports_user_input_actions: false,
                    });
                };
                Ok(ResponsePayload::SessionRuntime {
                    runtime_id: Some(entry.runtime_id),
                    supports_steer: entry.driver.supports_steer(),
                    supports_user_input_actions: entry.driver.supports_user_input_actions(),
                })
            }
            Command::GetSettings => Ok(ResponsePayload::Settings {
                settings: self.settings.get(),
            }),
            Command::GetDaemonStats => {
                let (current, previous_boot, previous_boot_clean) = self.stats.snapshot();
                Ok(ResponsePayload::DaemonStats {
                    current,
                    previous_boot,
                    previous_boot_clean,
                })
            }
            Command::GetFriends => {
                // Reading friends state means this install wants to be
                // reachable — incoming requests and offers can only
                // arrive while the endpoint is up.
                self.share.kickstart();
                Ok(ResponsePayload::Friends {
                    state: self.share.state(),
                })
            }
            Command::GetPairing => Ok(ResponsePayload::Pairing {
                state: self.pairing.state(),
            }),
            Command::RespondPairRequest { request_id, accept } => {
                self.pairing.respond(request_id, accept)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RevokePairedClient { client_id } => {
                self.pairing.revoke(client_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::GetAutomations => Ok(ResponsePayload::Automations {
                state: self.automations.document(),
            }),
            Command::UpsertAutomation { input } => Ok(ResponsePayload::Automation {
                automation: self.automations.upsert(input)?,
            }),
            Command::RemoveAutomation { automation_id } => {
                self.automations.remove(automation_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RunAutomationNow { automation_id } => Ok(ResponsePayload::AutomationRun {
                run: self.automations.run_now(automation_id)?,
            }),
            Command::SendFriendRequest { code, name } => {
                self.share.send_friend_request(code, name)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RespondFriendRequest { node_id, accept } => {
                self.share.respond_friend_request(node_id, accept)?;
                Ok(ResponsePayload::Ack)
            }
            Command::WithdrawFriendRequest { node_id } => {
                self.share.withdraw_friend_request(node_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RemoveFriend { node_id } => {
                self.share.remove_friend(node_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SendFileToFriend {
                node_id,
                path,
                note,
            } => {
                self.share.send_file(node_id, path, note)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SendMessageToFriend { node_id, text } => {
                self.share.send_chat(node_id, text)?;
                Ok(ResponsePayload::Ack)
            }
            Command::CancelTransfer { transfer_id } => {
                self.share.cancel_transfer(transfer_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ProbeFriend { node_id } => {
                self.share.probe_friend(node_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SetFriendDisplayName { name } => {
                self.share.set_display_name(name)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SetFriendNickname { node_id, nickname } => {
                self.share.set_friend_nickname(node_id, nickname)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ShareProjectWithFriend {
                node_id,
                project_path,
            } => {
                self.share.share_project(node_id, project_path)?;
                Ok(ResponsePayload::Ack)
            }
            Command::UnshareProjectWithFriend {
                node_id,
                origin_url,
            } => {
                self.share.unshare_project(node_id, origin_url)?;
                Ok(ResponsePayload::Ack)
            }
            Command::EnableFriendSync {
                node_id,
                origin_url,
            } => {
                self.share.enable_sync(node_id, origin_url)?;
                Ok(ResponsePayload::Ack)
            }
            Command::DisableFriendSync { link_id } => {
                self.share.disable_sync(link_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::SetFriendSyncConfig {
                link_id,
                auto_push,
                enabled_branches,
            } => {
                self.share
                    .set_sync_config(link_id, auto_push, enabled_branches)?;
                Ok(ResponsePayload::Ack)
            }
            Command::FriendSyncNow { link_id, branch } => {
                self.share.sync_now(link_id, branch)?;
                Ok(ResponsePayload::Ack)
            }
            Command::FriendSyncAlertAction { alert_id, action } => {
                self.share.sync_alert_action(alert_id, action)?;
                Ok(ResponsePayload::Ack)
            }
            Command::GetFriendSyncBranches { link_id } => {
                let (branches, default_branch) = self.share.get_sync_branches(link_id.clone())?;
                Ok(ResponsePayload::FriendSyncBranches {
                    link_id,
                    branches,
                    default_branch,
                })
            }
            Command::SetFriendSessionSharing {
                node_id,
                origin_url,
                enabled,
            } => {
                self.share
                    .set_session_sharing(node_id, origin_url, enabled)?;
                Ok(ResponsePayload::Ack)
            }
            Command::GetFriendSessions {
                node_id,
                origin_url,
            } => {
                let sessions = self.share.friend_sessions(node_id, origin_url)?;
                Ok(ResponsePayload::FriendSessions { sessions })
            }
            Command::WatchFriendSession {
                node_id,
                origin_url,
                session_id,
            } => {
                let session = self
                    .share
                    .watch_friend_session(node_id, origin_url, session_id)?;
                Ok(ResponsePayload::FriendSession {
                    session: Box::new(session),
                })
            }
            Command::UnwatchFriendSession { session_id } => {
                self.share.unwatch_friend_session(session_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::UpdateSettings { settings } => {
                self.settings.replace(settings)?;
                self.apply_wake_setting();
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::Ack)
            }
            Command::UpsertCustomCommand { command } => {
                self.require_agent_settings(agent)?;
                let commands = self.upsert_custom_command(agent, command)?;
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::CustomCommands { commands })
            }
            Command::RemoveCustomCommand { id, name } => {
                self.require_agent_settings(agent)?;
                let commands = self.remove_custom_command(id, name)?;
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::CustomCommands { commands })
            }
            Command::ListCustomCommands => {
                self.require_agent_settings(agent)?;
                Ok(ResponsePayload::CustomCommands {
                    commands: self.settings.get().custom_commands,
                })
            }
            Command::ListIntegrations => Ok(ResponsePayload::Integrations {
                snapshots: self.integrations.snapshots(),
            }),
            Command::ConnectIntegration {
                id,
                variant_id,
                providers,
                api_key,
            } => {
                self.integrations
                    .connect(&id, &variant_id, providers, api_key, &events)?;
                crate::integrations::deliver::sync_file_providers(
                    &self.settings.get(),
                    &self.integrations,
                );
                Ok(ResponsePayload::Ack)
            }
            Command::SetIntegrationProviders { id, providers } => {
                self.integrations.set_providers(&id, providers)?;
                crate::integrations::deliver::sync_file_providers(
                    &self.settings.get(),
                    &self.integrations,
                );
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::Ack)
            }
            Command::DisconnectIntegration { id } => {
                self.integrations.disconnect(&id)?;
                crate::integrations::deliver::sync_file_providers(
                    &self.settings.get(),
                    &self.integrations,
                );
                events.settings_changed(self.settings.get());
                Ok(ResponsePayload::Ack)
            }
            Command::StartIntegrationAuth { id } => {
                self.integrations.begin_auth(&id, &events)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ProbeProvider {
                provider,
                binary_override,
                discover_models,
                probe_version,
            } => {
                // Bare detection probes double as the manual-refresh path, so
                // they may re-capture the shell environment; model discovery
                // and version probes only ensure it exists.
                if discover_models || probe_version {
                    ensure_shell_environment();
                } else {
                    crate::command_env::refresh_shell_environment_if_stale();
                }
                let mut probe = match binary_override.as_deref() {
                    override_value if discover_models || probe_version => {
                        crate::model::provider_probe(provider, override_value)
                    }
                    override_value => crate::model::cached_provider_probe(provider, override_value),
                };
                let version = probe_version
                    .then(|| {
                        probe
                            .path
                            .as_deref()
                            .and_then(crate::model::probe_provider_version)
                    })
                    .flatten();
                if discover_models {
                    probe = crate::model::discover_provider_models(probe);
                }
                Ok(ResponsePayload::ProviderProbe { probe, version })
            }
            Command::SandboxSignIn { provider } => {
                // The sign-in VM needs the provider checkpoint — build it if
                // this is the provider's first sandboxed touch.
                crate::sandbox::ensure_sign_in_image(provider)?;
                let (program, args, cwd) =
                    crate::sandbox::sign_in_invocation(provider, &self.data_dir)?;
                Ok(ResponsePayload::SandboxSignIn { program, args, cwd })
            }
            Command::SandboxAuthStatus { provider } => Ok(ResponsePayload::SandboxAuthStatus {
                signed_in: crate::sandbox::sandbox_signed_in(provider, &self.data_dir),
            }),
            Command::FetchPlanUsage {
                provider,
                binary_override,
                cli_version,
            } => {
                let usage = match provider {
                    crate::model::ProviderKind::Claude => Some(
                        crate::usage::fetch_claude_plan_usage(cli_version.as_deref())?,
                    ),
                    crate::model::ProviderKind::Codex => {
                        Some(crate::usage::fetch_codex_plan_usage()?)
                    }
                    crate::model::ProviderKind::OpenCode => {
                        crate::usage::fetch_opencode_go_plan_usage()?
                    }
                    crate::model::ProviderKind::Grok => {
                        ensure_shell_environment();
                        let probe = match binary_override.as_deref() {
                            override_value => {
                                crate::model::provider_probe(provider, override_value)
                            }
                        };
                        let binary = probe.path.ok_or_else(|| anyhow!("grok is not installed"))?;
                        Some(crate::usage::fetch_grok_plan_usage(&binary)?)
                    }
                    _ => bail!("provider has no plan usage fetcher"),
                };
                Ok(ResponsePayload::PlanUsage { usage })
            }
            Command::ConsumeCodexResetCredit { redeem_request_id } => {
                let Some(_redeem) = self.codex_reset_credit_lock.try_lock() else {
                    bail!("a Codex reset credit redemption is already in flight");
                };
                let outcome = crate::usage::consume_codex_reset_credit(&redeem_request_id)?;
                // Whatever the verdict, the account view may have moved —
                // a spent, missing, or stale credit all surface in the same
                // re-read, which is also how a successful spend confirms.
                let usage = crate::usage::fetch_codex_plan_usage().ok();
                Ok(ResponsePayload::CodexResetCredit { outcome, usage })
            }
            Command::ProbeComputerPermissions { prompt } => {
                // Probing installs and launches the helper app, so it obeys
                // the same experiment opt-in as starting a runtime.
                if !self.settings.get().computer_use_experiment_enabled {
                    bail!("Goddard Computer Use is not enabled in this daemon's settings");
                }
                Ok(ResponsePayload::ComputerPermissions {
                    permissions: crate::computer_use::probe_permissions(prompt)?,
                })
            }
            Command::Evaluate {
                state,
                questions,
                feature,
                timeout_secs,
            } => {
                let settings = self
                    .settings
                    .get()
                    .eval
                    .ok_or_else(|| anyhow!("no evaluation backend is configured"))?;
                let started = std::time::Instant::now();
                let result = crate::eval::evaluate_with_timeout(
                    &settings,
                    &state,
                    &questions,
                    timeout_secs.unwrap_or(crate::eval::EVAL_TIMEOUT_SECS),
                );
                let mut record = crate::eval::EvalDecisionRecord::empty("evaluate");
                if let Some(feature) = feature {
                    record.feature = feature;
                }
                record.backend = Some(settings.backend);
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
                Ok(ResponsePayload::Evaluation {
                    evaluation: result?,
                })
            }
            Command::TestEvalConnection { settings } => Ok(ResponsePayload::Evaluation {
                evaluation: crate::eval::probe(&settings)?,
            }),
            Command::RouteTask {
                prompt,
                project,
                candidates,
                last_used,
            } => {
                let settings = self.settings.get();
                let run = crate::routing::route_task(
                    settings.eval.as_ref(),
                    &settings.route_classes,
                    &prompt,
                    project.as_deref(),
                    &candidates,
                    last_used.as_ref(),
                );
                crate::eval::append_decision_log(&crate::eval::default_log_path(), &run.record);
                Ok(ResponsePayload::RouteDecision {
                    decision: run.decision,
                })
            }
            Command::RecordRouteOverride { session_id, target } => {
                let mut record = crate::eval::EvalDecisionRecord::empty("route-override");
                record.session_id = Some(session_id);
                record.resolved_provider = Some(target.provider);
                record.resolved_model = target.model;
                record.reason = Some("user-override".to_owned());
                crate::eval::append_decision_log(&crate::eval::default_log_path(), &record);
                Ok(ResponsePayload::Ack)
            }
            Command::LoadEvalUsage => Ok(ResponsePayload::EvalUsage {
                stats: crate::eval::usage_stats(&crate::eval::default_log_path()),
            }),
            Command::LoadUsageHistory {
                window,
                project_roots,
            } => {
                let rates = crate::usage_history::load_rate_table(&self.usage_rates_dir);
                let history = crate::usage_history::scan(
                    &mut self.usage_scan_cache.lock(),
                    &rates,
                    window,
                    &project_roots,
                );
                Ok(ResponsePayload::UsageHistory { history })
            }
            Command::LoadSkills { projects } => {
                let locations = crate::skills::skill_locations(&projects);
                Ok(ResponsePayload::SkillsCatalog {
                    catalog: crate::skills::scan_skills(&locations),
                })
            }
            Command::SetSkillsEnabled { dirs, enabled } => {
                for dir in dirs {
                    crate::skills::set_skill_enabled(&dir, enabled)
                        .map_err(|error| anyhow!(error))?;
                }
                Ok(ResponsePayload::Ack)
            }
            Command::TrashSkills { dirs } => {
                crate::skills::trash_skills(&dirs).map_err(|error| anyhow!(error))?;
                Ok(ResponsePayload::Ack)
            }
            Command::LoadTaskState => {
                self.purge_expired_archived_sessions();
                let state = self.task_state.lock();
                Ok(ResponsePayload::TaskState {
                    projects: state.projects.clone(),
                    sessions: state
                        .sessions
                        .iter()
                        .filter(|session| session.has_started())
                        .map(AgentSession::list_projection)
                        .collect(),
                    default_cwd: self.default_cwd.clone(),
                    projectless_root: crate::projectless::workspace_root(),
                })
            }
            Command::SaveTaskState {
                projects,
                live_session_ids: _,
                sessions,
            } => {
                let active_runtimes = self
                    .sessions
                    .lock()
                    .iter()
                    .map(|(session_id, entry)| (*session_id, entry.runtime_id))
                    .collect::<HashMap<_, _>>();
                let mut state = self.task_state.lock();
                let removed_project_ids = self.removed_project_ids.lock();
                for project in projects {
                    if removed_project_ids.contains(&project.id) {
                        continue;
                    }
                    if let Some(existing) = state
                        .projects
                        .iter_mut()
                        .find(|existing| existing.id == project.id)
                    {
                        *existing = project;
                    } else {
                        state.projects.push(project);
                    }
                }
                drop(removed_project_ids);
                let removed_session_ids = self.removed_session_ids.lock();
                let sessions = sessions
                    .into_iter()
                    .filter(|session| !removed_session_ids.contains(&session.id))
                    .collect::<Vec<_>>();
                drop(removed_session_ids);
                let mut saved_ids = Vec::with_capacity(sessions.len());
                for mut session in sessions {
                    let session_id = session.id;
                    let applied = if let Some(existing) = state
                        .sessions
                        .iter_mut()
                        .find(|existing| existing.id == session_id)
                    {
                        if !session.detail_loaded {
                            merge_session_list_columns(
                                existing,
                                session,
                                active_runtimes.contains_key(&session_id),
                            )
                        } else if existing.has_started() && !session.has_started() {
                            // A session that has started can never become a
                            // draft again; an "empty" loaded projection is a
                            // skeleton that lost its marker, not a cleared
                            // transcript.
                            false
                        } else if session_projection_precedes(
                            existing,
                            &session,
                            active_runtimes.get(&session_id).copied(),
                        ) {
                            merge_stale_session_metadata(existing, session);
                            true
                        } else {
                            preserve_daemon_checkpoints(existing, &mut session);
                            preserve_daemon_queued_messages(existing, &mut session);
                            *existing = session;
                            true
                        }
                    } else if session.detail_loaded && session.has_started() {
                        state.sessions.push(session);
                        true
                    } else {
                        // A skeleton can update a known row but never create
                        // one — none of its detail is real. An unstarted draft
                        // owns no row either: cataloguing it would project it
                        // back to every client as a phantom "New task" skeleton.
                        false
                    };
                    if applied {
                        saved_ids.push(session_id);
                    }
                }
                let used_project_ids = state
                    .sessions
                    .iter()
                    .map(|session| session.project_id)
                    .collect::<std::collections::HashSet<_>>();
                let now = crate::model::unix_time();
                state.projects.retain(|project| {
                    !project.is_projectless()
                        || used_project_ids.contains(&project.id)
                        || now.saturating_sub(project.created_at) < UNUSED_PROJECTLESS_GRACE_SECONDS
                });
                for session_id in &saved_ids {
                    state.mark_session_dirty(*session_id);
                }
                // Archiving a task deletes its side chats — the same rule the
                // native app applies. A client that only flips `archived_at`
                // must not leave them in the catalog.
                let mut cascaded = Vec::new();
                let mut queue: Vec<Uuid> = state
                    .sessions
                    .iter()
                    .filter(|session| session.archived_at.is_some())
                    .map(|session| session.id)
                    .collect();
                while let Some(parent) = queue.pop() {
                    for session in state
                        .sessions
                        .iter()
                        .filter(|session| session.side_chat_of == Some(parent))
                    {
                        cascaded.push(session.id);
                        queue.push(session.id);
                    }
                }
                let cascaded_roots = state
                    .sessions
                    .iter()
                    .filter(|session| cascaded.contains(&session.id))
                    .filter_map(|session| session.workspace.path().map(Path::to_path_buf))
                    .collect::<Vec<_>>();
                if !cascaded.is_empty() {
                    state
                        .sessions
                        .retain(|session| !cascaded.contains(&session.id));
                    let mut removed = self.removed_session_ids.lock();
                    for id in &cascaded {
                        removed.insert(*id);
                    }
                }
                self.task_store.save(&mut state)?;
                let removed_terminals = self.sweep_orphaned_terminals(&cascaded, &cascaded_roots);
                drop_detached(removed_terminals);
                let removed_runtimes = cascaded
                    .iter()
                    .filter_map(|id| self.sessions.lock().remove(id))
                    .collect::<Vec<_>>();
                for runtime in &removed_runtimes {
                    runtime.driver.begin_shutdown();
                }
                drop_detached(removed_runtimes);
                for id in cascaded {
                    self.agent.clear_session(id);
                }
                let sessions = saved_ids
                    .into_iter()
                    .filter_map(|session_id| {
                        state
                            .sessions
                            .iter()
                            .find(|session| session.id == session_id)
                            .cloned()
                    })
                    .collect::<Vec<_>>();
                self.auto_prompts.consider(&sessions);
                // The save above can adopt full transcripts for every session
                // the client touched. Keep only the recent window resident;
                // the echoed clones above still carry the saved detail.
                trim_resident_transcripts(&mut state, &active_runtimes.keys().copied().collect());
                Ok(ResponsePayload::TaskStateSaved { sessions })
            }
            Command::RemoveSession => {
                self.remove_session(session_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::RemoveProject { project_id } => {
                self.remove_project(project_id)?;
                Ok(ResponsePayload::Ack)
            }
            Command::HydrateSession { session_id } => {
                // Live runtimes stay resident; everything else is trimmed to
                // the recency window once the response is built.
                let pinned = self.sessions.lock().keys().copied().collect();
                let mut state = self.task_state.lock();
                let session = if let Some(session) = state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    self.task_store.hydrate(session)?;
                    Some(session.clone())
                } else {
                    None
                };
                trim_resident_transcripts(&mut state, &pinned);
                Ok(ResponsePayload::Session { session })
            }
            Command::SearchSessionMessages {
                query,
                limit,
                scope,
            } => {
                let matches = self.search_session_messages(&query, limit, scope, None, None)?;
                Ok(ResponsePayload::SessionMessageMatches { matches })
            }
            Command::ListProviderSessions { provider, limit } => {
                const MAX_PROVIDER_SESSIONS: usize = 500;
                let limit = limit.min(MAX_PROVIDER_SESSIONS);
                if limit == 0 {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
                        status: Default::default(),
                    });
                }
                ensure_shell_environment();
                let settings = self.settings.get();
                if settings.disabled_providers.contains(&provider) {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
                        status: Default::default(),
                    });
                }
                let binary_override = settings
                    .provider_binary_overrides
                    .get(&provider)
                    .map(String::as_str);
                let Some(binary) = crate::model::provider_probe(provider, binary_override).path
                else {
                    return Ok(ResponsePayload::ProviderSessions {
                        sessions: Vec::new(),
                        status: ProviderSessionCatalogStatus::BinaryMissing,
                    });
                };
                // Discovery is deliberately provider-scoped. Opening Resume
                // must not start every installed agent CLI, and another
                // provider is queried only after the user explicitly picks it.
                let mut catalog: crate::acp_session::ProviderSessionCatalog = match provider {
                    // Antigravity conversations live in its own TUI; there is
                    // no Goddard transcript to import.
                    ProviderKind::Antigravity => {
                        crate::acp_session::ProviderSessionCatalog::unsupported()
                    }
                    ProviderKind::Amp => {
                        crate::amp_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::Claude => {
                        crate::claude_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Codex => {
                        crate::codex_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::Copilot => {
                        crate::copilot_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Cursor
                    | ProviderKind::Devin
                    | ProviderKind::Fx
                    | ProviderKind::Droid
                    | ProviderKind::Goose => {
                        crate::acp_session::list_provider_sessions(provider, &binary, &[], limit)?
                    }
                    ProviderKind::OpenCode => {
                        crate::opencode_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::OpenCode2 => {
                        crate::opencode2_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::DeepSeek => {
                        crate::deepseek_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::Grok => {
                        crate::grok_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Kimi => {
                        crate::kimi_session::list_provider_sessions(limit)?.into()
                    }
                    ProviderKind::Muse => {
                        crate::muse_session::list_provider_sessions(&binary, limit)?.into()
                    }
                    ProviderKind::OhMyPi | ProviderKind::Pi => {
                        crate::pi_session::list_provider_sessions(provider, limit)?.into()
                    }
                };
                catalog.sessions.sort_by(|a, b| {
                    b.updated_at
                        .cmp(&a.updated_at)
                        .then_with(|| a.title.cmp(&b.title))
                });
                let imported = {
                    let state = self.task_state.lock();
                    state
                        .sessions
                        .iter()
                        .filter_map(|session| session.provider_cursor.as_ref())
                        .map(|cursor| (cursor.provider(), cursor.native_id().to_owned()))
                        .collect::<HashSet<_>>()
                };
                catalog.sessions.retain(|session| {
                    !imported.contains(&(session.provider(), session.cursor.native_id().to_owned()))
                });
                catalog.sessions.truncate(limit);
                for session in &mut catalog.sessions {
                    session.cwd_missing = !session.cwd.is_dir();
                }
                Ok(ResponsePayload::ProviderSessions {
                    sessions: catalog.sessions,
                    status: catalog.status,
                })
            }
            Command::LoadProviderSession { cursor, cwd } => {
                // Preserve every native turn shell for exact provider turn
                // numbering, but bound imported display text to recent turns.
                const VISIBLE_TURN_LIMIT: usize = 100;
                // A `cwd_missing` session's recorded folder is gone; launch
                // and load in the nearest surviving ancestor instead.
                let cwd = crate::acp_session::resume_working_directory(&cwd);
                let history = match &cursor {
                    ProviderResumeCursor::Amp { thread_id, .. } => {
                        let binary = self.provider_binary(ProviderKind::Amp)?;
                        crate::amp_session::provider_session_history(
                            &binary,
                            &cwd,
                            thread_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Claude { session_id, .. } => {
                        self.provider_binary(ProviderKind::Claude)?;
                        crate::claude_session::provider_session_history(
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Codex { thread_id } => {
                        let binary = self.provider_binary(ProviderKind::Codex)?;
                        crate::codex_session::provider_session_history(
                            &binary,
                            thread_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Copilot { session_id } => {
                        crate::copilot_session::provider_session_history(
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    // OpenCode 2 is not an ACP provider: its history comes
                    // from the adopted v2 service's own export route.
                    ProviderResumeCursor::OpenCode2 { session_id, .. } => {
                        let binary = self.provider_binary(ProviderKind::OpenCode2)?;
                        crate::opencode2_session::provider_session_history(
                            &binary,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Cursor { session_id, .. }
                    | ProviderResumeCursor::Devin { session_id }
                    | ProviderResumeCursor::Fx { session_id }
                    | ProviderResumeCursor::OpenCode { session_id }
                    | ProviderResumeCursor::Goose { session_id }
                    | ProviderResumeCursor::Grok { session_id }
                    | ProviderResumeCursor::Kimi { session_id }
                    | ProviderResumeCursor::Droid { session_id } => {
                        let provider = cursor.provider();
                        let binary = self.provider_binary(provider)?;
                        crate::acp_session::provider_session_history(
                            provider,
                            &binary,
                            &cwd,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Muse { session_id, .. } => {
                        let binary = self.provider_binary(ProviderKind::Muse)?;
                        crate::muse_session::provider_session_history(
                            &binary,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::DeepSeek { session_id } => {
                        let binary = self.provider_binary(ProviderKind::DeepSeek)?;
                        crate::deepseek_session::provider_session_history(
                            &binary,
                            session_id,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::OhMyPi {
                        session_id,
                        session_file,
                    }
                    | ProviderResumeCursor::Pi {
                        session_id,
                        session_file,
                    } => {
                        self.provider_binary(cursor.provider())?;
                        let session_file = session_file.as_deref().ok_or_else(|| {
                            anyhow!(
                                "{} did not report its native session file",
                                cursor.provider().display_name()
                            )
                        })?;
                        crate::pi_session::provider_session_history(
                            cursor.provider(),
                            session_id,
                            session_file,
                            VISIBLE_TURN_LIMIT,
                        )?
                    }
                    ProviderResumeCursor::Antigravity { .. } => {
                        bail!(
                            "Antigravity conversations live in its own TUI; there is no transcript to import"
                        )
                    }
                };
                Ok(ResponsePayload::ProviderSessionHistory {
                    history,
                    resolved_cwd: Some(cwd),
                })
            }
            Command::LoadComposerDrafts => Ok(ResponsePayload::ComposerDrafts {
                drafts: self.composer_drafts.load()?,
            }),
            Command::SaveComposerDrafts { drafts, generation } => {
                self.composer_drafts.save(drafts, generation)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ApplyComposerDraftChanges { changes } => {
                self.composer_drafts.apply_changes(changes)?;
                Ok(ResponsePayload::Ack)
            }
            Command::StoreBlob { mime_type, bytes } => {
                let reference = self
                    .task_store
                    .blobs()
                    .store_image_bytes(&mime_type, &bytes)?;
                let path = self
                    .task_store
                    .blobs()
                    .path_for(&reference)
                    .ok_or_else(|| anyhow!("stored blob has no daemon path"))?;
                Ok(ResponsePayload::BlobStored { reference, path })
            }
            Command::ImportAttachment { name, upload } => Ok(ResponsePayload::AttachmentStored {
                attachment: self.attachments.import(&name, upload)?,
            }),
            Command::ImportPathAttachment { path } => Ok(ResponsePayload::AttachmentStored {
                attachment: self.attachments.import_path(&path)?,
            }),
            Command::ReadBlob { reference } => {
                let path = self
                    .task_store
                    .blobs()
                    .path_for(&reference)
                    .ok_or_else(|| anyhow!("invalid blob reference"))?;
                Ok(ResponsePayload::BlobData {
                    bytes: std::fs::read(path)?,
                })
            }
            Command::ReadAttachment { reference, path } => Ok(ResponsePayload::BlobData {
                bytes: self.attachments.read_file(&reference, &path)?,
            }),
            Command::SweepBlobs => {
                self.task_store.blob_sweep()();
                Ok(ResponsePayload::Ack)
            }
            Command::ForkSessionFromResponse { turn_count } => {
                let (session, checkpoint_warning) =
                    self.fork_session_from_response(session_id, turn_count)?;
                Ok(ResponsePayload::SessionForked {
                    session,
                    checkpoint_warning,
                })
            }
            Command::RewindSessionToMessage { turn_count } => {
                let (session, cleanup_warning) =
                    self.rewind_session_to_message(session_id, turn_count)?;
                Ok(ResponsePayload::SessionRewound {
                    session,
                    cleanup_warning,
                })
            }
            Command::ForkProviderSession { request } => {
                Ok(ResponsePayload::ProviderSessionForked {
                    result: fork_provider_session(request)?,
                })
            }
            Command::Workspace {
                operation:
                    WorkspaceOperation::CaptureTurn {
                        cwd,
                        session_id,
                        turn_count,
                    },
            } => Ok(ResponsePayload::Workspace {
                result: WorkspaceResult::Checkpoint {
                    checkpoint: self.capture_turn_checkpoint(cwd, session_id, turn_count)?,
                },
            }),
            Command::Workspace { operation } => {
                // Commit/push/land move local refs — prompt the share
                // poll so synced branches push and notify promptly
                // rather than waiting out the interval.
                let kick = matches!(
                    operation,
                    WorkspaceOperation::Commit { .. }
                        | WorkspaceOperation::Push { .. }
                        | WorkspaceOperation::PushBase { .. }
                        | WorkspaceOperation::Land { .. }
                        | WorkspaceOperation::RebaseOnto { .. }
                );
                let review_move = match &operation {
                    WorkspaceOperation::ReviewApprove { cwd, .. } => {
                        Some((cwd.clone(), ReviewMove::Approved))
                    }
                    WorkspaceOperation::ReviewReject { cwd, .. } => {
                        Some((cwd.clone(), ReviewMove::Rejected))
                    }
                    WorkspaceOperation::ReviewPromote { cwd } => {
                        Some((cwd.clone(), ReviewMove::Promoted))
                    }
                    _ => None,
                };
                let qa_branch = self.settings.get().qa_branch;
                let result = crate::workspace::execute(operation, &qa_branch)?;
                if kick {
                    self.share.note_repo_activity();
                }
                if let Some((cwd, review_move)) = review_move {
                    self.notify_review_moved(&cwd, review_move, &result);
                }
                Ok(ResponsePayload::Workspace { result })
            }
            Command::OpenTerminal {
                cwd,
                cols,
                rows,
                owner,
            } => {
                let terminal = self.open_terminal(&cwd, cols, rows, events)?;
                let previous = self.terminals.lock().insert(
                    session_id,
                    TerminalEntry {
                        runtime_id,
                        owner,
                        cwd,
                        terminal,
                    },
                );
                drop_detached(previous);
                Ok(ResponsePayload::Ack)
            }
            Command::WriteTerminal { data } => {
                let terminals = self.terminals.lock();
                let entry = terminals
                    .get(&session_id)
                    .ok_or_else(|| anyhow!("daemon terminal {session_id} is not running"))?;
                if entry.runtime_id != runtime_id {
                    bail!(
                        "daemon terminal {session_id} belongs to runtime {}, not {runtime_id}",
                        entry.runtime_id
                    );
                }
                entry.terminal.write(data)?;
                Ok(ResponsePayload::Ack)
            }
            Command::ResizeTerminal { cols, rows } => {
                let terminals = self.terminals.lock();
                let entry = terminals
                    .get(&session_id)
                    .ok_or_else(|| anyhow!("daemon terminal {session_id} is not running"))?;
                if entry.runtime_id != runtime_id {
                    bail!(
                        "daemon terminal {session_id} belongs to runtime {}, not {runtime_id}",
                        entry.runtime_id
                    );
                }
                entry.terminal.resize(cols, rows);
                Ok(ResponsePayload::Ack)
            }
            Command::CloseTerminal => {
                let removed = {
                    let mut terminals = self.terminals.lock();
                    if let Some(entry) = terminals.get(&session_id)
                        && entry.runtime_id != runtime_id
                    {
                        bail!(
                            "daemon terminal {session_id} belongs to runtime {}, not {runtime_id}",
                            entry.runtime_id
                        );
                    }
                    terminals.remove(&session_id)
                };
                drop_detached(removed.map(|entry| entry.terminal));
                Ok(ResponsePayload::Ack)
            }
            Command::Start { options } => {
                let previous = self.sessions.lock().remove(&session_id);
                if let Some(previous) = &previous {
                    previous.driver.begin_shutdown();
                }
                drop_detached(previous);
                // The replaced runtime's scoped credential dies with it; the
                // new runtime mints its own inside `spawn_runtime`.
                self.agent.revoke_session(session_id);
                let provider = decode_enum(&options.provider)?;
                let options = DriverStartOptions {
                    binary: options.binary,
                    cwd: options.cwd,
                    mode: decode_enum(&options.mode)?,
                    model: options.model,
                    reasoning_effort: options.reasoning_effort,
                    service_tier: options.service_tier,
                    context_window: options.context_window,
                    agent_preset: options.agent_preset,
                    computer_use_enabled: options.computer_use_enabled,
                    agent: None,
                    read_own_transcript: options.read_own_transcript,
                    subagents: None,
                    // Filled in by `spawn_runtime` — the daemon owns the
                    // catalog, never the wire.
                    integrations: Vec::new(),
                    provider_cursor: options
                        .provider_cursor
                        .map(serde_json::from_value)
                        .transpose()
                        .context("daemon received an invalid provider cursor")?,
                    eval: None,
                    sandbox: None,
                    allow_model_fallback: false,
                };
                let resumable = options.provider_cursor.is_some();
                let cwd = options.cwd.clone();
                let handle =
                    self.spawn_runtime(session_id, runtime_id, provider, options, events)?;
                let supports_steer = handle.supports_steer();
                let supports_user_input_actions = handle.supports_user_input_actions();
                self.sessions.lock().insert(
                    session_id,
                    RuntimeEntry {
                        runtime_id,
                        driver: handle,
                        last_active: std::time::Instant::now(),
                        resumable,
                        provider,
                        cwd,
                    },
                );
                Ok(ResponsePayload::Started {
                    supports_steer,
                    supports_user_input_actions,
                })
            }
            Command::CloseSession => {
                let removed = {
                    let mut sessions = self.sessions.lock();
                    sessions
                        .get(&session_id)
                        .is_some_and(|entry| entry.runtime_id == runtime_id)
                        .then(|| sessions.remove(&session_id))
                        .flatten()
                };
                if let Some(removed) = &removed {
                    removed.driver.begin_shutdown();
                }
                drop_detached(removed);
                self.agent.revoke_session(session_id);
                Ok(ResponsePayload::Ack)
            }
            Command::AgentCreateSession {
                provider,
                model,
                project,
                workspace,
                base_branch,
                prompt,
                reasoning_effort,
                service_tier,
                context_window,
            } => {
                // A scoped credential names its owning session; a master-token
                // request may attribute the prompt to `request.session_id`
                // when it is a known task.
                let sender = agent.or_else(|| {
                    (!session_id.is_nil() && self.known_session(session_id)).then_some(session_id)
                });
                self.agent_create_session(
                    sender,
                    AgentCreateSelection {
                        provider,
                        model,
                        reasoning_effort,
                        service_tier,
                        context_window,
                    },
                    project,
                    workspace,
                    base_branch,
                    prompt,
                    events,
                )
            }
            Command::AgentPrompt {
                task_id,
                thread_id,
                provider,
                prompt,
                delivery,
            } => {
                let sender = agent.or_else(|| {
                    (!session_id.is_nil() && self.known_session(session_id)).then_some(session_id)
                });
                self.agent_prompt(
                    sender, task_id, thread_id, provider, prompt, delivery, events,
                )
            }
            Command::AgentRenameSelf { title } => self.agent_rename_self(agent, &title),
            Command::AgentReadSession {
                task_id,
                thread_id,
                provider,
                turn,
            } => self.agent_read_session(agent, task_id, thread_id, provider, turn),
            Command::AgentSearchSessions { query, last_turns } => {
                self.agent_search_sessions(agent, session_id, &query, last_turns)
            }
            Command::CancelQueuedPrompt { queued_message_id } => {
                self.cancel_queued_prompt(session_id, queued_message_id, &events)
            }
            command => {
                // Quarantined transfer sessions still take interactive
                // prompts — the sandbox is the boundary, and the quarantine
                // flag only keeps unattended senders (agent prompts,
                // automations) out until the user trusts the transfer.
                let driver = {
                    let mut sessions = self.sessions.lock();
                    let entry = sessions
                        .get_mut(&session_id)
                        .ok_or_else(|| anyhow!("daemon session {session_id} is not running"))?;
                    if entry.runtime_id != runtime_id {
                        bail!(
                            "daemon session {session_id} belongs to runtime {}, not {runtime_id}",
                            entry.runtime_id
                        );
                    }
                    // Serving a request is activity — the idle reaper must
                    // not reclaim a runtime a client just talked to.
                    entry.last_active = std::time::Instant::now();
                    entry.driver.clone()
                };
                if let Command::Prompt {
                    prompt,
                    turn_id,
                    message_id,
                    hidden,
                    ..
                } = &command
                {
                    // Publish the submission into the runtime's event stream
                    // before the provider can start the turn. Every attached
                    // client mirrors the user message and its turn from this
                    // event, so the submitting client's own save is no longer
                    // the only record of the prompt — a follower that only
                    // knew the provider's `turnStarted` used to persist a
                    // projection without it, erasing the message for everyone.
                    events.send(event_to_wire(DriverEvent::PromptSubmitted {
                        message: prompt.clone(),
                        turn_id: turn_id.unwrap_or_else(Uuid::new_v4),
                        message_id: message_id.unwrap_or_else(Uuid::new_v4),
                        sent_by_task: None,
                        hidden: *hidden,
                    })?)?;
                }
                let mut command = command;
                if let Command::Prompt { prompt, hidden, .. } = &mut command
                    && !*hidden
                {
                    if driver.supports_steer() {
                        // The prompt reaches the provider exactly as typed —
                        // title generation and first-prompt echoes stay
                        // clean — and the session's context blocks follow as
                        // a hidden steer.
                        let task = prompt.clone();
                        let result = handle_driver_command(&driver, command);
                        self.steer_first_prompt_context(session_id, &task, &driver, &events);
                        return result;
                    }
                    // The agent-surface note, a side chat's parent index,
                    // project memory, and the project map ride the first
                    // visible prompt: the wire event above already
                    // published the user's text, so the injected blocks
                    // reach the provider without entering the transcript as
                    // a user message.
                    *prompt =
                        self.prepend_agent_surface(session_id, &driver, std::mem::take(prompt));
                    if let Some(index) = self.side_chat_parent_block(session_id) {
                        *prompt = format!("{index}\n\n{}", std::mem::take(prompt));
                        // The prompt carries it — delivered once sent, no
                        // accept echo to wait on.
                        self.agent.mark_parent_index_prepended(session_id);
                    }
                    *prompt = self.memory.prompt_with_memory(session_id, prompt);
                    let (mapped, status) = self.inject_repo_map(session_id, std::mem::take(prompt));
                    *prompt = mapped;
                    if let Some(status) = status
                        && let Ok(wire) = event_to_wire(DriverEvent::ProjectMap(status))
                    {
                        let _ = events.send(wire);
                    }
                }
                handle_driver_command(&driver, command)
            }
        }
    }

    fn shutdown(&self) {
        let sessions = std::mem::take(&mut *self.sessions.lock());
        drop(sessions);
        self.agent.clear();
        let terminals = std::mem::take(&mut *self.terminals.lock());
        drop(terminals);
        self.share.shutdown();
        self.automations.stop();
    }
}

fn session_projection_precedes(
    existing: &AgentSession,
    incoming: &AgentSession,
    active_runtime_id: Option<Uuid>,
) -> bool {
    let existing_cursor = existing.runtime_event_cursor;
    let incoming_cursor = incoming.runtime_event_cursor;
    if let Some(active_runtime_id) = active_runtime_id {
        let existing_is_active =
            existing_cursor.is_some_and(|cursor| cursor.runtime_id == active_runtime_id);
        let incoming_is_active =
            incoming_cursor.is_some_and(|cursor| cursor.runtime_id == active_runtime_id);
        if existing_is_active != incoming_is_active {
            return existing_is_active;
        }
    }
    match (existing_cursor, incoming_cursor) {
        (Some(existing), Some(incoming))
            if existing.runtime_id == incoming.runtime_id && existing.epoch == incoming.epoch =>
        {
            incoming.sequence < existing.sequence
        }
        (Some(_), None) if existing.status.is_busy() => true,
        _ => incoming.updated_at < existing.updated_at,
    }
}

/// Applies the fields a list projection legitimately carries.
///
/// A skeleton's transcript and cursors are placeholders — only its column
/// values are real, and only while the projection is at least as new as what
/// is stored. `workspace` is a column too, but it is not merged here: a
/// client's copy may predate a move the daemon already recorded, and the
/// stored row stays authoritative either way. `status` is skipped while the
/// daemon owns a live runtime for the session: busy state belongs to that
/// runtime, not to a client's possibly-stale row. Returns whether anything
/// was applied.
fn merge_session_list_columns(
    existing: &mut AgentSession,
    incoming: AgentSession,
    has_active_runtime: bool,
) -> bool {
    if incoming.updated_at < existing.updated_at {
        return false;
    }
    existing.title = incoming.title;
    existing.auto_title = incoming.auto_title;
    existing.agent_rename_allowed = incoming.agent_rename_allowed;
    existing.project_id = incoming.project_id;
    existing.provider = incoming.provider;
    existing.model = incoming.model;
    if !has_active_runtime {
        existing.status = incoming.status;
    }
    existing.created_at = incoming.created_at;
    existing.updated_at = incoming.updated_at;
    existing.last_reply_at = existing.last_reply_at.max(incoming.last_reply_at);
    existing.archived_at = incoming.archived_at;
    existing.pinned_at = incoming.pinned_at;
    existing.dormant_at = incoming.dormant_at;
    existing.dormant_exempt_until = incoming.dormant_exempt_until;
    existing.landed_at = incoming.landed_at;
    // Set once at creation and never mutated, but a skeleton merge should
    // still carry it: the daemon's cascade reads it without hydrating.
    existing.side_chat_of = incoming.side_chat_of;
    true
}

fn merge_stale_session_metadata(existing: &mut AgentSession, incoming: AgentSession) {
    if incoming.updated_at >= existing.updated_at {
        existing.title = incoming.title;
        existing.agent_rename_allowed = incoming.agent_rename_allowed;
        existing.project_id = incoming.project_id;
        existing.provider = incoming.provider;
        existing.model = incoming.model;
        existing.runtime_mode = incoming.runtime_mode;
        existing.reasoning_effort = incoming.reasoning_effort;
        existing.service_tier = incoming.service_tier;
        existing.context_window = incoming.context_window;
        existing.agent_preset = incoming.agent_preset;
        existing.updated_at = incoming.updated_at;
        existing.last_reply_at = incoming.last_reply_at.or(existing.last_reply_at);
        existing.archived_at = incoming.archived_at;
        existing.pinned_at = incoming.pinned_at;
        existing.dormant_at = incoming.dormant_at;
        existing.dormant_exempt_until = incoming.dormant_exempt_until;
        existing.landed_at = incoming.landed_at;
    }
    for queued in incoming.queued_messages {
        // Client saves never create daemon-owned entries: a mirrored agent
        // prompt absent from the daemon's copy was delivered or cancelled,
        // and re-adding it would resurrect — then re-deliver — the prompt.
        if queued.is_agent_owned() {
            continue;
        }
        if !existing
            .queued_messages
            .iter()
            .any(|candidate| candidate.id == queued.id)
        {
            existing.queued_messages.push(queued);
        }
    }
}

/// Client saves never mutate the daemon-owned slice of a follow-up queue:
/// `existing` entries are the truth, `incoming` agent entries are only
/// echoes of them. Union them back before the wholesale replace so a
/// projection written before a mirror arrived cannot erase a parked prompt.
fn preserve_daemon_queued_messages(existing: &AgentSession, incoming: &mut AgentSession) {
    incoming
        .queued_messages
        .retain(|queued| !queued.is_agent_owned());
    incoming.queued_messages.extend(
        existing
            .queued_messages
            .iter()
            .filter(|queued| queued.is_agent_owned())
            .cloned(),
    );
    incoming
        .queued_messages
        .sort_by_key(|queued| queued.created_at);
}

/// Ending checkpoints are produced and stored by the daemon. A second client
/// may still save a projection created just before capture completed; never
/// let that stale projection erase the canonical Git snapshot.
fn preserve_daemon_checkpoints(existing: &AgentSession, incoming: &mut AgentSession) {
    for turn in &mut incoming.turns {
        let Some(checkpoint) = existing
            .turns
            .iter()
            .find(|candidate| candidate.turn_count == turn.turn_count)
            .and_then(|candidate| candidate.checkpoint.as_ref())
            .filter(|checkpoint| {
                matches!(
                    checkpoint.status,
                    CheckpointStatus::Ready | CheckpointStatus::Unavailable
                )
            })
        else {
            continue;
        };
        turn.checkpoint = Some(checkpoint.clone());
    }
}

/// Which review op moved shared state — decides which notices go out.
enum ReviewMove {
    Approved,
    Rejected,
    Promoted,
}

impl WakuBackend {
    /// A review op moved `refs/notes/qa`, the QA branch, or the base
    /// branch on a shared origin — tell friends sharing it and bump review
    /// surfaces locally. Best-effort: no notices go out without an
    /// `origin`.
    fn notify_review_moved(&self, cwd: &Path, review_move: ReviewMove, result: &WorkspaceResult) {
        let Ok(Some(origin_url)) = crate::git_branch::remote_url(cwd, "origin") else {
            return;
        };
        match review_move {
            ReviewMove::Approved => {
                self.share.notify_refs(
                    origin_url.clone(),
                    vec![crate::review::NOTES_REF.to_owned()],
                );
            }
            ReviewMove::Rejected => {
                self.share.notify_refs(
                    origin_url.clone(),
                    vec![crate::review::NOTES_REF.to_owned()],
                );
                self.share.notify_push(
                    origin_url.clone(),
                    vec![crate::review::qa_branch_name(&self.settings.get().qa_branch)],
                );
            }
            ReviewMove::Promoted => {
                let base = match result {
                    WorkspaceResult::ReviewQueue { queue: Some(queue) } => {
                        queue.base_branch.clone()
                    }
                    _ => None,
                };
                self.share.notify_push(
                    origin_url.clone(),
                    vec![base.unwrap_or_else(|| "main".to_owned())],
                );
            }
        }
        self.share.review_changed(origin_url);
    }

    /// Fork a response using only daemon-host state.
    ///
    /// A browser must never reconstruct or persist this operation itself:
    /// provider-native sessions, checkpoint refs, and the task database all
    /// belong to the daemon and may be on another machine.
    fn fork_session_from_response(
        &self,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<(AgentSession, Option<String>)> {
        let (source, cwd, fork_title) = {
            let mut state = self.task_state.lock();
            let source_index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("the source task is unavailable"))?;
            self.task_store
                .hydrate(&mut state.sessions[source_index])
                .context("could not load the source task")?;
            let source = state.sessions[source_index].clone();
            let project = state
                .projects
                .iter()
                .find(|project| project.id == source.project_id)
                .ok_or_else(|| anyhow!("the source task project is unavailable"))?;
            let cwd = source.workspace.path().unwrap_or(&project.path).to_owned();
            let fork_title = next_response_fork_title(
                source.display_title(),
                state
                    .sessions
                    .iter()
                    .filter(|session| session.project_id == source.project_id)
                    .map(AgentSession::display_title),
            );
            (source, cwd, fork_title)
        };

        validate_response_fork(&source, turn_count)?;
        let provider_turn_count = source
            .turns
            .iter()
            .take(turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let turns_to_remove = source.provider_turns_after(turn_count);
        let (provider_cursor, message_ids) = self.fork_provider_response(
            &source,
            &cwd,
            &fork_title,
            turn_count,
            provider_turn_count,
            turns_to_remove,
        )?;
        let mut forked = source
            .fork_through_turn(turn_count, provider_cursor, &fork_title)
            .ok_or_else(|| anyhow!("the selected response cannot be copied"))?;
        if !message_ids.is_empty() {
            for turn in &mut forked.turns {
                if let Some(message_id) = turn.provider_resume_at.as_mut()
                    && let Some(remapped) = message_ids.get(message_id)
                {
                    *message_id = remapped.clone();
                }
            }
        }

        let fork_id = forked.id;
        for turn in &mut forked.turns {
            if let Some(checkpoint) = turn.checkpoint.as_mut() {
                checkpoint.git_ref =
                    crate::checkpoint::checkpoint_ref(fork_id, checkpoint.turn_count);
            }
        }
        let checkpoint_warning =
            crate::checkpoint::copy_session_refs(&cwd, source.id, fork_id, turn_count)
                .err()
                .map(|error| error.to_string());

        let pinned = self.sessions.lock().keys().copied().collect();
        let mut state = self.task_state.lock();
        state.push_session(forked.clone());
        if let Err(error) = self.task_store.save(&mut state) {
            state.sessions.retain(|session| session.id != fork_id);
            let _ = crate::checkpoint::delete_all_session_refs(&cwd, fork_id);
            return Err(error).context("could not save the forked task");
        }
        trim_resident_transcripts(&mut state, &pinned);
        Ok((forked, checkpoint_warning))
    }

    /// Restore the daemon-host worktree, provider conversation, and stored
    /// transcript to immediately before one user turn.
    fn rewind_session_to_message(
        &self,
        session_id: Uuid,
        turn_count: usize,
    ) -> anyhow::Result<(AgentSession, Option<String>)> {
        let (source, cwd) = {
            let mut state = self.task_state.lock();
            let source_index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("the task is unavailable"))?;
            self.task_store
                .hydrate(&mut state.sessions[source_index])
                .context("could not load the task")?;
            let source = state.sessions[source_index].clone();
            let project = state
                .projects
                .iter()
                .find(|project| project.id == source.project_id)
                .ok_or_else(|| anyhow!("the task project is unavailable"))?;
            let cwd = source.workspace.path().unwrap_or(&project.path).to_owned();
            (source, cwd)
        };
        validate_message_rewind(&source, turn_count)?;

        // Resolve the executable before touching the worktree. Even native
        // transcript operations are immediately followed by a replacement
        // prompt, so accepting a rewind that cannot resume would strand the
        // user at a provider state the UI cannot continue.
        let binary = self.provider_binary(source.provider)?;
        let retained_turn_count = turn_count.saturating_sub(1);
        let previous_turn_count = source.turns.len();
        let rollback_turns = source.provider_turns_after(retained_turn_count);
        let provider_turn_count = source
            .turns
            .iter()
            .take(retained_turn_count)
            .filter(|turn| turn.provider_turn_started)
            .count();
        let provider_resume_at = retained_turn_count
            .checked_sub(1)
            .and_then(|index| source.turns.get(index))
            .and_then(|turn| turn.provider_resume_at.clone());

        let turn_start_ref = crate::checkpoint::turn_start_ref(session_id, turn_count);
        let retained_ref = crate::checkpoint::checkpoint_ref(session_id, retained_turn_count);
        let restore_ref = if crate::checkpoint::has_ref(&cwd, &turn_start_ref) {
            turn_start_ref
        } else {
            retained_ref
        };
        if !crate::checkpoint::has_ref(&cwd, &restore_ref) {
            bail!("the checkpoint before this message is unavailable");
        }

        let safety_ref = format!("refs/waku/revert-backup-{session_id}-{}", Uuid::new_v4());
        crate::checkpoint::capture_ref(&cwd, &safety_ref)
            .context("could not create a rewind safety snapshot")?;
        if let Err(error) = crate::checkpoint::restore_ref(&cwd, &restore_ref) {
            return Err(restore_rewind_safety(
                &cwd,
                &safety_ref,
                "could not restore the selected checkpoint",
                error,
            ));
        }

        let provider_rewind = self.rewind_provider_response(
            &source,
            &cwd,
            &binary,
            retained_turn_count,
            rollback_turns,
            provider_turn_count,
            provider_resume_at,
        );
        let (provider_cursor, message_ids, reset_native_session) = match provider_rewind {
            Ok(result) => result,
            Err(error) => {
                return Err(restore_rewind_safety(
                    &cwd,
                    &safety_ref,
                    "the provider rejected the rewind",
                    error,
                ));
            }
        };

        let _ = crate::checkpoint::delete_ref(&cwd, &safety_ref);
        let cleanup_warning = crate::checkpoint::delete_turn_refs_after(
            &cwd,
            session_id,
            retained_turn_count,
            previous_turn_count,
        )
        .err()
        .map(|error| error.to_string());

        // Every provider resumes from the newly stored cursor on the next
        // prompt. Removing the resident source driver also prevents its late
        // events from racing the rewound transcript: the hub retires the
        // runtime id, so anything the detached teardown still emits dies
        // with it.
        let removed = self.sessions.lock().remove(&session_id);
        if let Some(removed) = &removed {
            removed.driver.begin_shutdown();
        }
        drop_detached(removed);

        let mut rewound = source.clone();
        if !message_ids.is_empty() {
            for turn in rewound.turns.iter_mut().take(retained_turn_count) {
                if let Some(remapped) = turn
                    .provider_resume_at
                    .as_ref()
                    .and_then(|message_id| message_ids.get(message_id))
                    .cloned()
                {
                    turn.provider_resume_at = Some(remapped);
                }
            }
        }
        if reset_native_session {
            rewound.provider_cursor = None;
        } else if let Some(cursor) = provider_cursor {
            rewound.provider_cursor = Some(cursor);
        }
        rewound.truncate_after_turn(retained_turn_count);
        rewound.status = SessionStatus::Idle;

        let pinned = self.sessions.lock().keys().copied().collect();
        let mut state = self.task_state.lock();
        let existing = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| anyhow!("the task was removed while it was being rewound"))?;
        *existing = rewound.clone();
        state.mark_session_dirty(session_id);
        self.task_store
            .save(&mut state)
            .context("could not save the rewound task")?;
        trim_resident_transcripts(&mut state, &pinned);
        Ok((rewound, cleanup_warning))
    }

    fn fork_provider_response(
        &self,
        source: &AgentSession,
        cwd: &Path,
        fork_title: &str,
        turn_count: usize,
        provider_turn_count: usize,
        turns_to_remove: usize,
    ) -> anyhow::Result<(ProviderResumeCursor, HashMap<String, String>)> {
        match source.provider {
            ProviderKind::Claude => {
                let Some(ProviderResumeCursor::Claude { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Claude's native session is unavailable");
                };
                let resume_at = source
                    .turns
                    .get(turn_count.saturating_sub(1))
                    .and_then(|turn| turn.provider_resume_at.clone());
                let fork = fork_provider_session(ProviderSessionForkRequest::Claude {
                    session_id: session_id.clone(),
                    resume_at,
                    turn_count: provider_turn_count,
                    title: fork_title.to_owned(),
                })?;
                Ok((fork.cursor, fork.message_ids))
            }
            ProviderKind::Codex
            | ProviderKind::DeepSeek
            | ProviderKind::Muse
            | ProviderKind::OhMyPi
            | ProviderKind::Pi => Ok((
                self.fork_response_with_driver(source, cwd, turns_to_remove)?,
                HashMap::new(),
            )),
            ProviderKind::Cursor => {
                let fork = fork_provider_session(ProviderSessionForkRequest::Cursor {
                    source: source.clone(),
                    turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Amp => {
                let Some(ProviderResumeCursor::Amp {
                    thread_id,
                    fork_context,
                }) = source.provider_cursor.as_ref()
                else {
                    bail!("Amp's native thread is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Amp {
                    binary: self.provider_binary(ProviderKind::Amp)?,
                    cwd: cwd.to_owned(),
                    thread_id: thread_id.clone(),
                    fork_context: fork_context.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::OpenCode => {
                let Some(ProviderResumeCursor::OpenCode { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("OpenCode's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::OpenCode {
                    binary: self.provider_binary(ProviderKind::OpenCode)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::OpenCode2 => {
                let Some(ProviderResumeCursor::OpenCode2 { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("OpenCode 2's native session is unavailable");
                };
                // No cwd: a v2 session carries its own `location`, so there is
                // no server working directory to fork against.
                let fork = fork_provider_session(ProviderSessionForkRequest::OpenCode2 {
                    binary: self.provider_binary(ProviderKind::OpenCode2)?,
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Grok => {
                let Some(ProviderResumeCursor::Grok { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Grok Build's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Grok {
                    binary: self.provider_binary(ProviderKind::Grok)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            ProviderKind::Copilot => {
                let Some(ProviderResumeCursor::Copilot { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("GitHub Copilot's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Copilot {
                    binary: self.provider_binary(ProviderKind::Copilot)?,
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                    title: fork_title.to_owned(),
                })?;
                Ok((fork.cursor, HashMap::new()))
            }
            // Unreachable through the UI, which hides branching for providers
            // that answer `supports_conversation_fork` with false.
            ProviderKind::Antigravity
            | ProviderKind::Devin
            | ProviderKind::Droid
            | ProviderKind::Fx
            | ProviderKind::Goose
            | ProviderKind::Kimi => {
                bail!(
                    "{} cannot branch a conversation at a turn",
                    source.provider.display_name()
                )
            }
        }
    }

    fn fork_response_with_driver(
        &self,
        source: &AgentSession,
        cwd: &Path,
        turns_to_remove: usize,
    ) -> anyhow::Result<ProviderResumeCursor> {
        if let Some(driver) = self
            .sessions
            .lock()
            .get(&source.id)
            .map(|entry| entry.driver.clone())
        {
            return driver.fork(turns_to_remove);
        }

        match source.provider {
            ProviderKind::Codex
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Codex { .. })
                ) =>
            {
                bail!("Codex's native thread is unavailable");
            }
            ProviderKind::DeepSeek
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::DeepSeek { .. })
                ) =>
            {
                bail!("DeepSeek Harness's native session is unavailable");
            }
            ProviderKind::Pi
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Pi {
                        session_file: Some(_),
                        ..
                    })
                ) =>
            {
                bail!("Pi's native session file is unavailable");
            }
            ProviderKind::OhMyPi
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::OhMyPi {
                        session_file: Some(_),
                        ..
                    })
                ) =>
            {
                bail!("Oh My Pi's native session file is unavailable");
            }
            ProviderKind::Muse
                if !matches!(
                    source.provider_cursor.as_ref(),
                    Some(ProviderResumeCursor::Muse { .. })
                ) =>
            {
                bail!("Muse Code's native session is unavailable");
            }
            _ => {}
        }

        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, _event_receiver) = driver::event_channel(wake);
        let driver = driver::start_local(
            source.provider,
            DriverStartOptions {
                binary: self.provider_binary(source.provider)?,
                cwd: cwd.to_owned(),
                mode: source.runtime_mode,
                model: source.model.clone(),
                reasoning_effort: source.reasoning_effort.clone(),
                service_tier: source.service_tier.clone(),
                context_window: source.context_window.clone(),
                agent_preset: source.agent_preset.clone(),
                computer_use_enabled: false,
                // A fork/rollback driver is a one-shot process, not the
                // task's live runtime; it never receives a scoped token.
                agent: None,
                read_own_transcript: false,
                subagents: None,
                integrations: Vec::new(),
                provider_cursor: source.provider_cursor.clone(),
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
            },
            event_sender,
        )?;
        driver.fork(turns_to_remove)
    }

    #[allow(clippy::too_many_arguments)]
    fn rewind_provider_response(
        &self,
        source: &AgentSession,
        cwd: &Path,
        binary: &Path,
        retained_turn_count: usize,
        rollback_turns: usize,
        provider_turn_count: usize,
        provider_resume_at: Option<String>,
    ) -> anyhow::Result<(Option<ProviderResumeCursor>, HashMap<String, String>, bool)> {
        if rollback_turns == 0 {
            return Ok((None, HashMap::new(), false));
        }
        let reset_native_session = retained_turn_count == 0
            && matches!(
                source.provider,
                ProviderKind::Claude
                    | ProviderKind::Copilot
                    | ProviderKind::Cursor
                    | ProviderKind::Grok
            );
        if reset_native_session {
            return Ok((None, HashMap::new(), true));
        }

        match source.provider {
            ProviderKind::Claude => {
                let Some(ProviderResumeCursor::Claude { session_id, .. }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Claude's native session is unavailable");
                };
                let fork = fork_provider_session(ProviderSessionForkRequest::Claude {
                    session_id: session_id.clone(),
                    resume_at: provider_resume_at,
                    turn_count: provider_turn_count,
                    title: format!("{} (rewind)", source.display_title()),
                })?;
                Ok((Some(fork.cursor), fork.message_ids, false))
            }
            ProviderKind::OpenCode => {
                let cursor = if let Some(driver) = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|entry| entry.driver.clone())
                {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("OpenCode returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::OpenCode { session_id }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("OpenCode's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::OpenCode {
                        binary: binary.to_owned(),
                        cwd: cwd.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::OpenCode2 => {
                let cursor = if let Some(driver) = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|entry| entry.driver.clone())
                {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("OpenCode 2 returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::OpenCode2 { session_id, .. }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("OpenCode 2's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::OpenCode2 {
                        binary: binary.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Amp => {
                let Some(ProviderResumeCursor::Amp {
                    thread_id,
                    fork_context,
                }) = source.provider_cursor.as_ref()
                else {
                    bail!("Amp's native thread is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Amp {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    thread_id: thread_id.clone(),
                    fork_context: fork_context.clone(),
                    turn_count: provider_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Cursor => {
                let cursor = fork_provider_session(ProviderSessionForkRequest::Cursor {
                    source: source.clone(),
                    turn_count: retained_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Grok => {
                let Some(ProviderResumeCursor::Grok { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("Grok Build's native session is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Grok {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Copilot => {
                let Some(ProviderResumeCursor::Copilot { session_id }) =
                    source.provider_cursor.as_ref()
                else {
                    bail!("GitHub Copilot's native session is unavailable");
                };
                let cursor = fork_provider_session(ProviderSessionForkRequest::Copilot {
                    binary: binary.to_owned(),
                    cwd: cwd.to_owned(),
                    session_id: session_id.clone(),
                    turn_count: provider_turn_count,
                    title: format!("{} (rewind)", source.display_title()),
                })?
                .cursor;
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Muse => {
                let cursor = if let Some(driver) = self
                    .sessions
                    .lock()
                    .get(&source.id)
                    .map(|entry| entry.driver.clone())
                {
                    driver
                        .rollback(rollback_turns)?
                        .ok_or_else(|| anyhow!("Muse Code returned no rewound-session cursor"))?
                } else {
                    let Some(ProviderResumeCursor::Muse { session_id, .. }) =
                        source.provider_cursor.as_ref()
                    else {
                        bail!("Muse Code's native session is unavailable");
                    };
                    fork_provider_session(ProviderSessionForkRequest::Muse {
                        binary: binary.to_owned(),
                        session_id: session_id.clone(),
                        turn_count: provider_turn_count,
                    })?
                    .cursor
                };
                Ok((Some(cursor), HashMap::new(), false))
            }
            ProviderKind::Codex
            | ProviderKind::DeepSeek
            | ProviderKind::OhMyPi
            | ProviderKind::Pi => Ok((
                self.rollback_response_with_driver(source, cwd, binary, rollback_turns)?,
                HashMap::new(),
                false,
            )),
            // Unreachable through the UI, which hides rewinding for providers
            // that answer `supports_conversation_rollback` with false.
            ProviderKind::Antigravity
            | ProviderKind::Devin
            | ProviderKind::Droid
            | ProviderKind::Fx
            | ProviderKind::Goose
            | ProviderKind::Kimi => {
                bail!(
                    "{} cannot rewind a conversation to a turn",
                    source.provider.display_name()
                )
            }
        }
    }

    fn rollback_response_with_driver(
        &self,
        source: &AgentSession,
        cwd: &Path,
        binary: &Path,
        rollback_turns: usize,
    ) -> anyhow::Result<Option<ProviderResumeCursor>> {
        if let Some(driver) = self
            .sessions
            .lock()
            .get(&source.id)
            .map(|entry| entry.driver.clone())
        {
            return driver.rollback(rollback_turns);
        }

        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, _event_receiver) = driver::event_channel(wake);
        let driver = driver::start_local(
            source.provider,
            DriverStartOptions {
                binary: binary.to_owned(),
                cwd: cwd.to_owned(),
                mode: source.runtime_mode,
                model: source.model.clone(),
                reasoning_effort: source.reasoning_effort.clone(),
                service_tier: source.service_tier.clone(),
                context_window: source.context_window.clone(),
                agent_preset: source.agent_preset.clone(),
                computer_use_enabled: false,
                agent: None,
                read_own_transcript: false,
                subagents: None,
                integrations: Vec::new(),
                provider_cursor: source.provider_cursor.clone(),
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
            },
            event_sender,
        )?;
        driver.rollback(rollback_turns)
    }

    fn provider_binary(&self, provider: ProviderKind) -> anyhow::Result<PathBuf> {
        ensure_shell_environment();
        let settings = self.settings.get();
        let binary_override = settings
            .provider_binary_overrides
            .get(&provider)
            .map(String::as_str);
        crate::model::provider_probe(provider, binary_override)
            .path
            .ok_or_else(|| anyhow!("{} is not installed on the daemon", provider.display_name()))
    }

    /// Whether `session_id` names a task the daemon knows.
    pub(crate) fn known_session(&self, session_id: Uuid) -> bool {
        self.task_state
            .lock()
            .sessions
            .iter()
            .any(|session| session.id == session_id)
    }

    /// The whole agent surface sits behind this daemon-level setting. While
    /// it is off the commands are rejected for every caller — scoped
    /// credentials included — and nothing is minted or injected.
    fn require_agent_tools(&self) -> anyhow::Result<()> {
        if !self.settings.get().agent_tools_enabled {
            bail!("agent session commands are disabled on this daemon");
        }
        Ok(())
    }

    fn agent_rename_self(
        &self,
        agent: Option<Uuid>,
        title: &str,
    ) -> anyhow::Result<ResponsePayload> {
        let caller =
            agent.ok_or_else(|| anyhow!("agent rename requires a scoped task credential"))?;
        let mut state = self.task_state.lock();
        let session = state
            .session_mut(caller)
            .ok_or_else(|| anyhow!("task {caller} is unknown to the daemon"))?;
        if !session.agent_rename_allowed {
            bail!("this task has not granted its agent permission to rename it");
        }
        if title.trim().is_empty() {
            bail!("a task title cannot be empty");
        }
        if session.set_title(title) {
            self.task_store.save(&mut state)?;
        }
        Ok(ResponsePayload::Ack)
    }

    /// The settings surface is gated separately from task creation and only
    /// for scoped credentials — a client holding the master token already has
    /// full `updateSettings` access, so the flag must not gate it.
    fn require_agent_settings(&self, agent: Option<Uuid>) -> anyhow::Result<()> {
        if agent.is_some() && !self.settings.get().agent_settings_enabled {
            bail!("agent settings commands are disabled on this daemon");
        }
        Ok(())
    }

    /// Apply `command` to the daemon-owned list. An upsert keys on the id
    /// first and the exact name second, so an agent can assert "this command
    /// exists" without tracking list state; a nil id always mints a new
    /// command. Agent writes stamp `created_by_task` so clients can show
    /// where the entry came from.
    fn upsert_custom_command(
        &self,
        agent: Option<Uuid>,
        mut command: CustomCommand,
    ) -> anyhow::Result<Vec<CustomCommand>> {
        if command.script.trim().is_empty() {
            bail!("custom commands require a script");
        }
        // Agent writes are stamped with their task; a client keeps whatever
        // attribution the command already carries.
        if agent.is_some() {
            command.created_by_task = agent;
        }
        if command.id.is_nil() {
            command.id = Uuid::new_v4();
        }
        let mut settings = self.settings.get();
        let existing = settings.custom_commands.iter().position(|existing| {
            existing.id == command.id
                || command
                    .name
                    .as_deref()
                    .is_some_and(|name| existing.name.as_deref() == Some(name))
        });
        match existing {
            Some(index) => settings.custom_commands[index] = command,
            None => settings.custom_commands.push(command),
        }
        self.settings.replace(settings)?;
        Ok(self.settings.get().custom_commands)
    }

    fn remove_custom_command(
        &self,
        id: Option<Uuid>,
        name: Option<String>,
    ) -> anyhow::Result<Vec<CustomCommand>> {
        let name = name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        if id.is_none() && name.is_none() {
            bail!("custom command removal needs an id or a name");
        }
        let mut settings = self.settings.get();
        let before = settings.custom_commands.len();
        settings.custom_commands.retain(|command| {
            !(id.is_some_and(|id| command.id == id)
                || name
                    .as_deref()
                    .is_some_and(|name| command.name.as_deref() == Some(name)))
        });
        if settings.custom_commands.len() == before {
            bail!("no custom command matches");
        }
        self.settings.replace(settings)?;
        Ok(self.settings.get().custom_commands)
    }

    /// Start a provider runtime for `session_id` and forward its events into
    /// `events`, which must already target the new `runtime_id`.
    ///
    /// Shared by client `Start` requests and the agent commands' cold start:
    /// both mint the session's scoped credential (when the daemon's agent
    /// tools are enabled) and both run the forwarder that tracks turns,
    /// drains queued agent prompts, and attributes accepted steers.
    fn spawn_runtime(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        provider: ProviderKind,
        options: DriverStartOptions,
        events: EventSink,
    ) -> anyhow::Result<DriverHandle> {
        let (wake, _wake_events) = smol::channel::bounded(1);
        let (event_sender, event_receiver) = driver::event_channel(wake);
        let mut options = options;
        // A projectless workspace is daemon-owned scratch state: its
        // directory can vanish between draft creation and the first prompt —
        // emptied trash, an archive sweep, a client that provisioned it on a
        // stale listing. Restore unzips its archive or recreates it empty
        // instead of failing the launch on a path the user never manages.
        // Any other missing cwd is a real problem and fails here with the
        // path named, not inside the provider's spawn error.
        if !options.cwd.is_dir() && crate::projectless::is_projectless_path(&options.cwd) {
            crate::projectless::restore_workspace(&options.cwd).with_context(|| {
                format!(
                    "could not recreate the task's projectless workspace {}",
                    options.cwd.display()
                )
            })?;
        }
        if !options.cwd.is_dir() {
            bail!(
                "the task's working directory does not exist: {}",
                options.cwd.display()
            );
        }
        // The credential exists before the process does so it can travel
        // with the runtime's launch environment. A missing CLI or unset
        // daemon address disables injection for this launch only. Either
        // agent surface — task tools or settings writes — gets it injected,
        // as does a session whose own transcript is meant to be read back
        // (a provider-switch handoff or a side chat): its scoped read works
        // without the cross-task surface.
        let daemon_settings = self.settings.get();
        // Computer Use is experimental: the enable flag only counts while the
        // experiment opt-in is on, whatever a client or a hand-edited settings
        // document sent over the wire.
        options.computer_use_enabled = crate::computer_use::resolve_enabled(
            options.computer_use_enabled,
            daemon_settings.computer_use_experiment_enabled,
        );
        let (environment, rename_allowed) = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| (session.environment(), session.agent_rename_allowed))
            .unwrap_or_default();
        // A cloud task runs on the provider's hosted environment — nothing
        // local executes, so the launch injections below (agent surface,
        // subagents, project map, integrations) would all point at a host
        // the remote side cannot see. They stay off for cloud launches.
        let cloud_launch = environment.is_cloud();
        if !cloud_launch
            && (daemon_settings.agent_tools_enabled
                || daemon_settings.agent_settings_enabled
                || rename_allowed
                || options.read_own_transcript)
        {
            match self.agent_launch_env(session_id) {
                Ok(launch) => options.agent = Some(launch),
                Err(error) => eprintln!(
                    "goddard-daemon: agent surface unavailable for session {session_id}: {error:#}"
                ),
            }
        }
        // Named subagents ride the launch with the runtime: the fixed roster
        // resolves its models through the same class map routing uses, so
        // routing and subagents share one user-editable map. Drivers without
        // an injection channel simply ignore it. Still experimental —
        // injected only when the opt-in is on.
        if !cloud_launch && daemon_settings.subagents_enabled {
            options.subagents = Some(crate::subagents::spec_for(
                provider,
                &daemon_settings.route_classes,
            ));
        }
        // Auto-mode permission review rides the same BYOK evaluation backend
        // as routing. Unconfigured leaves each driver's ask-the-user path —
        // a backend missing its credential would only fail closed there too,
        // so it is withheld the same way rather than spending a doomed call
        // and a decision-log row on every request.
        options.eval = daemon_settings
            .eval
            .clone()
            .filter(|eval| !eval.credential_missing());
        // The project map is experimental the same way: record the session's
        // workspace for first-prompt injection and warm the index in the
        // background so it can beat the provider's launch.
        if !cloud_launch && daemon_settings.project_map_enabled {
            let ready = {
                let mut maps = self.repo_maps.0.lock();
                maps.sessions
                    .insert(session_id, (options.cwd.clone(), runtime_id));
                if options.provider_cursor.is_none() {
                    maps.pending.insert(session_id);
                }
                maps.indexes
                    .get(&options.cwd)
                    .map(|index| index.indexed_files())
            };
            let status = match ready {
                Some(indexed_files) => ProjectMapStatus::Ready { indexed_files },
                None => {
                    spawn_repo_map_refresh(
                        &self.repo_maps,
                        options.cwd.clone(),
                        Some(events.clone()),
                    );
                    ProjectMapStatus::Building
                }
            };
            if let Ok(wire) = event_to_wire(DriverEvent::ProjectMap(status)) {
                let _ = events.send_ephemeral(wire);
            }
        }
        // Connected integrations ride the launch too; drivers that take file
        // delivery instead see nothing here because their entries were
        // written at connect time.
        if !cloud_launch {
            options.integrations = self.integrations.launch_integrations(provider);
        }
        // A sandboxed session runs its provider inside a shuru VM — never on
        // the host. Every setup failure fails the task rather than silently
        // falling back to a local process.
        if environment.is_sandbox() {
            if !daemon_settings.sandbox_experiment_enabled {
                bail!(
                    "this task was created with the Sandbox VM environment, but the sandbox experiment is off"
                );
            }
            let launch = crate::sandbox::launch_for_provider(
                provider,
                &options.cwd,
                &self.data_dir,
                |status| {
                    // Launch progress is ephemeral — it exists to name the
                    // Connecting phase, never to enter the transcript.
                    if let Ok(wire) = event_to_wire(DriverEvent::SandboxSetup(status)) {
                        let _ = events.send_ephemeral(wire);
                    }
                },
            )
            .context("could not prepare the sandbox VM")?;
            options.binary = launch.binary;
            options.cwd = launch.cwd;
            options.sandbox = Some(launch.vm);
            // The agent surface's daemon address is this host's loopback —
            // unreachable from inside the guest. The headless computer-use
            // bridge is host-side too, so it is off in the sandbox.
            options.agent = None;
            options.computer_use_enabled = false;
        }
        // The agent-surface instruction reaches the session through whichever
        // channel its provider offers; first-prompt context needs the launch's
        // scopes for drivers without a native channel. A failed start revokes
        // the credential and this record together.
        if let Some(agent_env) = &options.agent {
            self.agent.note_surface(session_id, agent_env.scope());
        }
        // A launch that never came up keeps no credential.
        let sandboxed_launch = options.sandbox.is_some();
        let handle = match if cloud_launch {
            driver::start_cloud(provider, options, event_sender)
        } else {
            driver::start_local(provider, options, event_sender)
        } {
            Ok(handle) => handle,
            Err(error) => {
                self.agent.revoke_session(session_id);
                return Err(error);
            }
        };
        if sandboxed_launch {
            // The provider process is up — clear the launch phase so the
            // transcript's indicator falls back to its ordinary working state.
            if let Ok(wire) = event_to_wire(DriverEvent::SandboxSetup(
                waku_protocol::model::SandboxSetupStatus::Ready,
            )) {
                let _ = events.send_ephemeral(wire);
            }
        }
        let forwarder_handle = handle.clone();
        let agent = self.agent.clone();
        let task_state = self.task_state.clone();
        let task_store = self.task_store.clone();
        let sessions = self.sessions.clone();
        let automations = self.automations.clone();
        let auto_prompts = self.auto_prompts.clone();
        let memory = self.memory.clone();
        let repo_maps = self.repo_maps.clone();
        std::thread::Builder::new()
            .name(format!("goddard-daemon-events-{session_id}"))
            .spawn(move || {
                forward_driver_events(
                    session_id,
                    runtime_id,
                    event_receiver,
                    events,
                    forwarder_handle,
                    agent,
                    task_state,
                    task_store,
                    sessions,
                    automations,
                    auto_prompts,
                    memory,
                    repo_maps,
                );
            })
            .context("could not start daemon event forwarding thread")?;
        Ok(handle)
    }

    /// Mint the runtime's scoped credential and assemble the environment the
    /// provider launch receives. The shim directory lives beside the daemon
    /// database so shared-service providers have somewhere private to put a
    /// per-session launcher.
    fn agent_launch_env(&self, session_id: Uuid) -> anyhow::Result<crate::agent::AgentLaunchEnv> {
        let daemon_address = self
            .daemon_address
            .lock()
            .clone()
            .ok_or_else(|| anyhow!("the daemon's bound address is unknown"))?;
        let cli_path = crate::agent::agent_cli_path()?;
        let shim_directory = self
            .task_store
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("agent")
            .join(session_id.to_string());
        let settings = self.settings.get();
        // `side_chat_of` is a list column, so a skeleton answers this
        // without a hydrate.
        let parent_task_id = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| session.side_chat_of);
        Ok(crate::agent::AgentLaunchEnv {
            token: self.agent.mint(session_id),
            task_id: session_id,
            parent_task_id,
            daemon_address,
            cli_path,
            shim_directory,
            task_tools: settings.agent_tools_enabled,
            settings_writes: settings.agent_settings_enabled,
        })
    }

    /// Return the live driver for `session_id`, cold-starting it from the
    /// stored task's provider cursor and saved options when no runtime is
    /// running.
    fn ensure_agent_runtime(
        &self,
        session_id: Uuid,
        events: &EventSink,
    ) -> anyhow::Result<(Uuid, DriverHandle)> {
        if let Some(entry) = self.sessions.lock().get_mut(&session_id) {
            entry.last_active = std::time::Instant::now();
            return Ok((entry.runtime_id, entry.driver.clone()));
        }
        // A per-session lock keeps two simultaneous agent prompts from
        // cold-starting the same stored task twice.
        let start_lock = self
            .runtime_start_locks
            .lock()
            .entry(session_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _start_guard = start_lock.lock();
        if let Some(entry) = self.sessions.lock().get_mut(&session_id) {
            entry.last_active = std::time::Instant::now();
            return Ok((entry.runtime_id, entry.driver.clone()));
        }
        let (provider, options) = {
            let mut state = self.task_state.lock();
            let index = state
                .sessions
                .iter()
                .position(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("task {session_id} is unknown to the daemon"))?;
            self.task_store
                .hydrate(&mut state.sessions[index])
                .context("could not load the task's stored state")?;
            let session = &state.sessions[index];
            let cwd = session
                .workspace
                .path()
                .map(Path::to_path_buf)
                .or_else(|| {
                    state
                        .projects
                        .iter()
                        .find(|project| project.id == session.project_id)
                        .map(|project| project.path.clone())
                })
                .ok_or_else(|| anyhow!("task {session_id} has no project to run in"))?;
            let provider = session.provider;
            let options = DriverStartOptions {
                binary: self.provider_binary(provider)?,
                cwd,
                mode: session.runtime_mode,
                model: session.model.clone(),
                reasoning_effort: session.reasoning_effort.clone(),
                service_tier: session.service_tier.clone(),
                context_window: session.context_window.clone(),
                agent_preset: session.agent_preset.clone(),
                computer_use_enabled: self.settings.get().computer_use_enabled,
                agent: None,
                // A switched or side-chat task cold-started this way still
                // gets the read surface — the session is hydrated here.
                read_own_transcript: session.side_chat_of.is_some()
                    || !session.suspended_provider_sessions.is_empty()
                    || session.pending_provider_context.is_some(),
                subagents: None,
                integrations: Vec::new(),
                provider_cursor: session.provider_cursor.clone(),
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
            };
            (provider, options)
        };
        let runtime_id = Uuid::new_v4();
        let sink = events.begin_session_runtime(session_id, runtime_id);
        let resumable = options.provider_cursor.is_some();
        let cwd = options.cwd.clone();
        let handle =
            match self.spawn_runtime(session_id, runtime_id, provider, options, sink.clone()) {
                Ok(handle) => handle,
                Err(error) => {
                    sink.end_session_runtime();
                    return Err(error);
                }
            };
        let driver = handle.clone();
        self.sessions.lock().insert(
            session_id,
            RuntimeEntry {
                runtime_id,
                driver: handle,
                last_active: std::time::Instant::now(),
                resumable,
                provider,
                cwd,
            },
        );
        Ok((runtime_id, driver))
    }

    /// `agent create`: build a fully configured task and start its first
    /// prompt. Mirrors the app's New Task defaults — the project must be an
    /// absolute path; an existing project resolves by it, and an unknown
    /// path registers only as a primary checkout.
    fn agent_create_session(
        &self,
        sender: Option<Uuid>,
        selection: AgentCreateSelection,
        project: PathBuf,
        workspace: AgentWorkspace,
        base_branch: Option<String>,
        prompt: String,
        events: EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        self.require_agent_tools()?;
        let session_id = self.create_agent_task(
            sender,
            selection,
            project,
            workspace,
            base_branch,
            prompt,
            &events,
        )?;
        Ok(ResponsePayload::AgentSessionCreated { session_id })
    }

    /// The task-creation half of `agent create`, shared with the automation
    /// scheduler: the agent-tools credential gate is the only difference —
    /// the daemon's own scheduler needs no scoped token.
    pub(crate) fn create_agent_task(
        &self,
        sender: Option<Uuid>,
        selection: AgentCreateSelection,
        project: PathBuf,
        workspace: AgentWorkspace,
        base_branch: Option<String>,
        prompt: String,
        events: &EventSink,
    ) -> anyhow::Result<Uuid> {
        if prompt.trim().is_empty() {
            bail!("agent sessions require a prompt");
        }
        if !project.is_absolute() {
            bail!("the project path must be absolute");
        }
        // Fields the payload omits inherit the sending task's configuration,
        // but only while it runs the resolved provider — a different
        // provider's model and trait vocabularies may not carry over.
        let sender_config = sender.and_then(|id| {
            self.task_state
                .lock()
                .sessions
                .iter()
                .find(|session| session.id == id)
                .map(|session| {
                    (
                        session.provider,
                        session.model.clone(),
                        session.reasoning_effort.clone(),
                        session.service_tier.clone(),
                        session.context_window.clone(),
                    )
                })
        });
        let provider = selection
            .provider
            .or(sender_config.as_ref().map(|config| config.0))
            .ok_or_else(|| {
                anyhow!("`provider` is required when no sending task is known to inherit from")
            })?;
        let sender_config = sender_config.filter(|config| config.0 == provider);
        let model = match selection.model.as_deref().map(str::trim) {
            Some("" | "default") => None,
            Some(model) => Some(model.to_owned()),
            None => sender_config.as_ref().and_then(|config| config.1.clone()),
        };
        let (inherited_effort, inherited_tier, inherited_window) = sender_config
            .map(|config| (config.2, config.3, config.4))
            .unwrap_or_default();
        // The resolved model's catalog entry bounds which inherited traits
        // still apply; an empty or missing entry cannot constrain them.
        let catalog = crate::model_catalog::cached_models(provider)
            .unwrap_or_else(|| crate::model_catalog::fallback_models(provider));
        let catalog_model = match model.as_deref() {
            Some(requested) => {
                waku_protocol::model_catalog::packed_catalog_model(&catalog, requested, provider)
                    .map(|matched| matched.model)
            }
            None => catalog
                .iter()
                .find(|entry| entry.is_default)
                .or_else(|| catalog.first()),
        };
        let reasoning_effort = resolve_agent_trait(
            selection.reasoning_effort,
            inherited_effort
                .map(|value| waku_protocol::model_catalog::normalize_reasoning_effort(&value)),
            catalog_model.map(|model| model.reasoning_efforts.as_slice()),
            catalog_model.and_then(|model| model.default_reasoning_effort.as_deref()),
        );
        let service_tier = resolve_agent_trait(
            selection.service_tier,
            inherited_tier,
            catalog_model.map(|model| model.service_tiers.as_slice()),
            catalog_model.and_then(|model| model.default_service_tier.as_deref()),
        );
        let context_window = resolve_agent_trait(
            selection.context_window,
            inherited_window,
            catalog_model.map(|model| model.context_windows.as_slice()),
            catalog_model.and_then(|model| model.default_context_window.as_deref()),
        );
        if matches!(workspace, AgentWorkspace::Worktree)
            && base_branch
                .as_deref()
                .is_none_or(|branch| branch.trim().is_empty())
        {
            bail!("worktree sessions require a base branch");
        }
        let project = dunce::canonicalize(&project)
            .with_context(|| format!("project path {} does not exist", project.display()))?;
        let (project_id, project_path) = {
            let mut state = self.task_state.lock();
            match state
                .projects
                .iter()
                .find(|existing| {
                    dunce::canonicalize(&existing.path).is_ok_and(|path| path == project)
                })
                .map(|existing| (existing.id, existing.path.clone()))
            {
                Some(found) => found,
                None => {
                    if crate::worktree::is_linked_worktree(&project) {
                        bail!(
                            "{} is a Git worktree; only primary checkouts can be registered as projects",
                            project.display()
                        );
                    }
                    let registered = Project::from_path(project.clone());
                    let found = (registered.id, registered.path.clone());
                    state.projects.push(registered);
                    found
                }
            }
        };
        let mut session = AgentSession::new(project_id, provider);
        session.model = model;
        session.reasoning_effort = reasoning_effort;
        session.service_tier = service_tier;
        session.context_window = context_window;
        session.workspace = match workspace {
            AgentWorkspace::Local => SessionWorkspace::Local,
            AgentWorkspace::Worktree => {
                let created = crate::worktree::create(
                    &project_path,
                    None,
                    base_branch.as_deref(),
                    false,
                    &[],
                )?;
                SessionWorkspace::Worktree {
                    path: created.path,
                    name: created.name,
                    branch: None,
                    base_branch,
                }
            }
        };
        let session_id = session.id;
        let turn_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();
        session.adopt_submitted_prompt(&prompt, turn_id, message_id, sender, false);
        {
            let mut state = self.task_state.lock();
            state.push_session(session);
            self.task_store.save(&mut state)?;
        }
        // The adopted prompt above already persisted, so a launch failure
        // still leaves a normal task behind. Delivering it now starts the
        // first turn immediately.
        let (runtime_id, driver) = self.ensure_agent_runtime(session_id, events)?;
        let sink = events.for_session(session_id, runtime_id);
        sink.send(event_to_wire(DriverEvent::PromptSubmitted {
            message: prompt.clone(),
            turn_id,
            message_id,
            sent_by_task: sender,
            hidden: false,
        })?)?;
        if driver.supports_steer() {
            // The prompt reaches the provider exactly as typed — title
            // generation and first-prompt echoes stay clean — and the
            // session's context blocks follow as a hidden steer.
            driver.prompt(prompt.clone());
            self.steer_first_prompt_context(session_id, &prompt, &driver, &sink);
        } else {
            let prompt = self.prepend_agent_surface(session_id, &driver, prompt);
            let (prompt, status) = self.inject_repo_map(
                session_id,
                self.memory.prompt_with_memory(session_id, &prompt),
            );
            if let Some(status) = status
                && let Ok(wire) = event_to_wire(DriverEvent::ProjectMap(status))
            {
                let _ = sink.send(wire);
            }
            driver.prompt(prompt);
        }
        Ok(session_id)
    }

    /// The session's first prompt already went out clean; its context
    /// blocks — the project map, then project memory — follow as a hidden
    /// steer so provider title generation never sees them. Delivery
    /// confirms on the `steerAccepted` echo: memory's injected flag is set
    /// there, and a rejected steer leaves the session eligible so the next
    /// prompt retries.
    fn steer_first_prompt_context(
        &self,
        session_id: Uuid,
        task: &str,
        driver: &DriverHandle,
        sink: &EventSink,
    ) {
        if self.agent.context_steer_pending(session_id) {
            return;
        }
        let (map, status) = self.repo_map_block(session_id);
        if let Some(status) = status
            && let Ok(wire) = event_to_wire(DriverEvent::ProjectMap(status))
        {
            let _ = sink.send(wire);
        }
        let memory = self.memory.context_block(session_id, task);
        let parent_index = self.side_chat_parent_block(session_id);
        let block = [
            map,
            memory.clone(),
            parent_index.clone(),
            self.agent_surface_block(session_id, driver),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");
        if block.is_empty() {
            return;
        }
        // The index rides this steer — an accept settles it, a rejection
        // leaves the flag so the next prompt retries.
        if parent_index.is_some() {
            self.agent.note_index_steer(session_id);
        }
        // The steer lands as a user message mid-turn — frame the blocks as
        // context so the provider does not read them as a new instruction.
        let steer = format!(
            "Session context — background information only, not a new \
             instruction. Continue the task you are already working on.\n\n{block}"
        );
        self.agent.record_pending_steer(
            session_id,
            crate::agent::AgentPrompt {
                prompt: steer.clone(),
                transport: None,
                sender: None,
                queued_id: None,
                context: Some(if memory.is_some() {
                    crate::agent::ContextSteer::Memory
                } else {
                    crate::agent::ContextSteer::Blocks
                }),
            },
        );
        driver.steer(steer);
    }

    /// A side chat's context block: the parent task's user messages verbatim
    /// plus a per-turn cue index — extractive pointers, never a summary —
    /// snapshot at the side chat's first prompt. When the session's launch
    /// carried the `goddard-agent` surface the block names the `read`
    /// invocations that reach the parent's current text; without it the
    /// index degrades to plain context. `None` for ordinary sessions and
    /// parents with nothing to show.
    fn side_chat_parent_block(&self, session_id: Uuid) -> Option<String> {
        if !self.agent.parent_index_owed(session_id) {
            return None;
        }
        let parent = {
            let mut state = self.task_state.lock();
            let parent_id = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)?
                .side_chat_of?;
            let index = state
                .sessions
                .iter()
                .position(|session| session.id == parent_id)?;
            self.task_store.hydrate(&mut state.sessions[index]).ok()?;
            state.sessions[index].clone()
        };
        let groups = parent.transcript_index();
        if groups.is_empty() {
            return None;
        }
        let mut body = String::new();
        for (turn, lines) in &groups {
            if let Some(turn) = turn {
                body.push_str(&format!("turn {turn}\n"));
            }
            for line in lines {
                body.push_str("  ");
                body.push_str(line);
                body.push('\n');
            }
            body.push('\n');
        }
        let range = match (
            groups.iter().filter_map(|(turn, ..)| *turn).min(),
            groups.iter().filter_map(|(turn, ..)| *turn).max(),
        ) {
            (Some(low), Some(high)) if low == high => format!(" turns=\"{low}\""),
            (Some(low), Some(high)) => format!(" turns=\"{low}-{high}\""),
            _ => String::new(),
        };
        let read_note = if self.agent.has_surface(session_id) {
            format!(
                " — a snapshot taken now; `goddard-agent read \
                 '{{\"task_id\":\"{}\"}}'` always returns its current text, and \
                 `goddard-agent read '{{\"task_id\":\"{}\",\"turn\":N}}'` returns \
                 one turn's full messages and tool output.",
                parent.id, parent.id
            )
        } else {
            String::from(".")
        };
        Some(format!(
            "This session is a side chat of the task \"{}\"; its transcript is \
             indexed below{read_note}\n\n<goddard-session-context source=\"{}\" \
             kind=\"index\"{range}>\n{body}</goddard-session-context>",
            parent.display_title(),
            parent.provider.id(),
        ))
    }

    /// The launch-scoped `goddard-agent` instruction for a session whose
    /// provider delivered the surface without telling the model — `None`
    /// when the driver announced it natively or the launch env never
    /// arrived.
    fn agent_surface_block(&self, session_id: Uuid, driver: &DriverHandle) -> Option<String> {
        if driver.agent_surface_delivery() != crate::driver::AgentSurfaceDelivery::Silent {
            return None;
        }
        self.agent.surface_block(session_id)
    }

    /// Fold the owed agent-surface instruction into a first prompt — the
    /// non-steer path's single shot at telling the session about
    /// `goddard-agent`, so delivery is marked as the prompt goes out.
    fn prepend_agent_surface(
        &self,
        session_id: Uuid,
        driver: &DriverHandle,
        prompt: String,
    ) -> String {
        let Some(surface) = self.agent_surface_block(session_id, driver) else {
            return prompt;
        };
        self.agent.mark_surface_announced(session_id);
        format!("{surface}\n\n{prompt}")
    }

    /// Prefix `prompt` with the session's project map when the session is
    /// owed one — its first visible prompt. A cold index gets a bounded
    /// moment to finish building, then the prompt goes out unmapped rather
    /// than stalling the turn. The `Sent` status comes back for the caller
    /// to publish on the session's stream, so clients can show the
    /// provider-facing artifact.
    fn inject_repo_map(
        &self,
        session_id: Uuid,
        prompt: String,
    ) -> (String, Option<ProjectMapStatus>) {
        match self.repo_map_block(session_id) {
            (Some(map), status) => (format!("{map}\n\n{prompt}"), status),
            (None, status) => (prompt, status),
        }
    }

    /// Render the session's pending project map as a `<project-map>` block.
    /// The pending flag is consumed either way — a cold index gets a bounded
    /// moment to finish, then the session ships unmapped rather than
    /// stalling. The `Sent` status comes back for the caller to publish.
    fn repo_map_block(&self, session_id: Uuid) -> (Option<String>, Option<ProjectMapStatus>) {
        const WAIT_FOR_COLD_INDEX: std::time::Duration = std::time::Duration::from_millis(1_500);
        let (lock, cvar) = &*self.repo_maps;
        let mut maps = lock.lock();
        if !maps.pending.remove(&session_id) {
            return (None, None);
        }
        let Some(cwd) = maps.sessions.get(&session_id).map(|(cwd, _)| cwd.clone()) else {
            return (None, None);
        };
        let deadline = std::time::Instant::now() + WAIT_FOR_COLD_INDEX;
        while !maps.indexes.contains_key(&cwd) && maps.building.contains(&cwd) {
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                break;
            };
            if cvar.wait_for(&mut maps, remaining).timed_out() {
                break;
            }
        }
        let Some(index) = maps.indexes.get(&cwd) else {
            return (None, None);
        };
        let map = index.render(crate::repo_map::DEFAULT_TOKEN_BUDGET);
        drop(maps);
        if map.text.is_empty() {
            return (None, None);
        }
        let status = ProjectMapStatus::Sent {
            mapped_files: map.mapped_files,
            indexed_files: map.indexed_files,
            estimated_tokens: map.estimated_tokens,
            text: map.text.clone(),
        };
        (
            Some(format!(
                "<project-map>\n\
                 A structural map of this workspace, most-referenced files first. \
                 It is partial — open files to verify before relying on it. \
                 Paths are workspace-relative.\n\
                 {}</project-map>",
                map.text
            )),
            Some(status),
        )
    }

    /// Persisted quarantine flag — set on received-file sessions until the
    /// user trusts the transfer. Checked against `task_state`, not the
    /// running-driver map, so it holds for sessions that aren't running.
    pub(crate) fn session_quarantined(&self, session_id: Uuid) -> bool {
        let mut state = self.task_state.lock();
        let Some(index) = state
            .sessions
            .iter()
            .position(|session| session.id == session_id)
        else {
            return false;
        };
        // Quarantine lives in the session detail, not the list row — a
        // skeleton answers false until hydrated, so load before judging.
        if !state.sessions[index].detail_loaded {
            let _ = self.task_store.hydrate(&mut state.sessions[index]);
        }
        state.sessions[index].quarantined
    }

    /// `agent prompt`: deliver a message to an existing task, by Waku task
    /// id or provider-native thread id. Queue mode holds the prompt in a
    /// daemon-side per-session queue until the target is idle; steer mode
    /// injects it into the running turn.
    fn agent_prompt(
        &self,
        sender: Option<Uuid>,
        task_id: Option<Uuid>,
        thread_id: Option<String>,
        provider: Option<ProviderKind>,
        prompt: String,
        delivery: AgentPromptDelivery,
        events: EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        self.require_agent_tools()?;
        if prompt.trim().is_empty() {
            bail!("agent prompts require a prompt");
        }
        let target = self.resolve_agent_target(task_id, thread_id, provider)?;
        if self.session_quarantined(target) {
            bail!("received files are quarantined until trusted");
        }
        match delivery {
            AgentPromptDelivery::Steer => {
                let driver = self
                    .sessions
                    .lock()
                    .get(&target)
                    .map(|entry| entry.driver.clone())
                    .ok_or_else(|| anyhow!("task {target} has no running session to steer"))?;
                if !self.agent.has_open_turn(target) {
                    bail!("task {target} has no running turn to steer");
                }
                if !driver.supports_steer() {
                    bail!("the task's provider does not support steering");
                }
                let transport = agent_prompt_envelope(&self.task_state, target, sender, &prompt);
                self.agent.record_pending_steer(
                    target,
                    crate::agent::AgentPrompt {
                        prompt: prompt.clone(),
                        transport: transport.clone(),
                        sender,
                        // A direct steer never parks — no chip to mirror.
                        queued_id: None,
                        context: None,
                    },
                );
                driver.steer(transport.unwrap_or(prompt));
                Ok(ResponsePayload::Ack)
            }
            AgentPromptDelivery::Queue => {
                self.queue_agent_prompt(target, prompt, sender, &events)?;
                Ok(ResponsePayload::Ack)
            }
        }
    }

    /// Queue-mode delivery shared with the automation scheduler: the prompt
    /// waits behind any open turn and drains once the session goes idle.
    /// Enqueueing before the working check means a prompt can never slip
    /// between a finishing turn and its queue drain.
    pub(crate) fn queue_agent_prompt(
        &self,
        target: Uuid,
        prompt: String,
        sender: Option<Uuid>,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        let queued_id = Uuid::new_v4();
        self.agent.enqueue(
            target,
            crate::agent::AgentPrompt {
                prompt: prompt.clone(),
                // Wrapped at delivery so a renamed sender and a late
                // relationship read their current values.
                transport: None,
                sender,
                queued_id: Some(queued_id),
                context: None,
            },
        );
        if self.agent.is_working(target) {
            // The runtime event forwarder delivers queued prompts in
            // order once the provider finishes the turn. Mirror the wait
            // into the session document so every client renders the parked
            // prompt as a queued follow-up chip.
            mirror_agent_queued_prompt(
                &self.task_state,
                &self.task_store,
                target,
                queued_id,
                &prompt,
                sender,
            )?;
            if let Some(runtime_id) = self.runtime_id_for(target) {
                send_agent_queue_changed(
                    &self.task_state,
                    &events.for_session(target, runtime_id),
                    target,
                );
            }
            return Ok(());
        }
        let (runtime_id, driver) = self.ensure_agent_runtime(target, events)?;
        let sink = events.for_session(target, runtime_id);
        self.drain_agent_queue(target, &driver, &sink)
    }

    pub(crate) fn auto_prompt_settings(&self) -> crate::DaemonSettings {
        self.settings.get()
    }

    pub(crate) fn auto_prompt_turn_is_latest(&self, session_id: Uuid, turn_id: Uuid) -> bool {
        if self.agent.is_working(session_id) {
            return false;
        }
        self.task_state.lock().sessions.iter().any(|session| {
            session.id == session_id
                && session.archived_at.is_none()
                && session.turns.last().is_some_and(|turn| turn.id == turn_id)
                && session.queued_messages.is_empty()
        })
    }

    pub(crate) fn queue_auto_prompt(
        &self,
        session_id: Uuid,
        source_turn: Uuid,
        prompt: String,
        events: &EventSink,
    ) -> anyhow::Result<()> {
        if !self.auto_prompt_turn_is_latest(session_id, source_turn) {
            bail!("task moved on before auto prompt dispatch");
        }
        if self.session_quarantined(session_id) {
            bail!("task is quarantined");
        }
        if !self.sessions.lock().contains_key(&session_id) {
            bail!("task runtime is unavailable");
        }
        self.queue_agent_prompt(session_id, prompt, None, events)
    }

    /// Pop every queued agent prompt for the session, in submission order.
    /// A turn that started working mid-drain holds the remainder for the
    /// provider's finish event.
    fn drain_agent_queue(
        &self,
        session_id: Uuid,
        driver: &DriverHandle,
        sink: &EventSink,
    ) -> anyhow::Result<()> {
        rehydrate_agent_queue(&self.agent, &self.task_state, &self.task_store, session_id);
        while let Some(entry) = self.agent.pop_queued(session_id) {
            if self.agent.is_working(session_id) {
                self.agent.requeue_front(session_id, entry);
                break;
            }
            deliver_agent_prompt(
                session_id,
                driver,
                entry,
                sink,
                &self.agent,
                &self.auto_prompts,
                &self.task_state,
                &self.task_store,
            )?;
        }
        Ok(())
    }

    /// The running runtime's id for a session, when one is live. Queue
    /// change events go out on that runtime's stream so attached clients
    /// redraw the chip row without waiting for the next save cycle.
    fn runtime_id_for(&self, session_id: Uuid) -> Option<Uuid> {
        self.sessions
            .lock()
            .get(&session_id)
            .map(|entry| entry.runtime_id)
    }

    /// Cancel a parked agent prompt — the chip's own remove affordance.
    /// Works whether or not the session's runtime is up: the in-memory
    /// queue and the session document's mirrored entry are both cleared.
    fn cancel_queued_prompt(
        &self,
        session_id: Uuid,
        queued_message_id: Uuid,
        events: &EventSink,
    ) -> anyhow::Result<ResponsePayload> {
        self.agent.remove_queued(session_id, queued_message_id);
        {
            let mut state = self.task_state.lock();
            let session = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .ok_or_else(|| anyhow!("task {session_id} is unknown to the daemon"))?;
            self.task_store.hydrate(session)?;
            let index = session
                .queued_messages
                .iter()
                .position(|queued| queued.id == queued_message_id)
                .ok_or_else(|| {
                    anyhow!("task {session_id} has no queued prompt {queued_message_id}")
                })?;
            if !session.queued_messages[index].is_agent_owned() {
                bail!("queued message {queued_message_id} is owned by the client, not the daemon");
            }
            session.queued_messages.remove(index);
            session.updated_at = crate::model::unix_time();
            state.mark_session_dirty(session_id);
            self.task_store.save(&mut state)?;
        }
        if let Some(runtime_id) = self.runtime_id_for(session_id) {
            send_agent_queue_changed(
                &self.task_state,
                &events.for_session(session_id, runtime_id),
                session_id,
            );
        }
        Ok(ResponsePayload::Ack)
    }

    /// Resolve an agent read's target and return its transcript — the
    /// compact item view a scoped caller pulls a task's context from.
    /// Addressed like [`Self::agent_prompt`], except a scoped caller that
    /// names nothing reads its own task.
    ///
    /// A session's credential may always read its own transcript — and a
    /// side chat's parent — without the cross-task surface: the handoff and
    /// side-chat designs rely on the pull being available even when the
    /// user never opted into agent task tools. Anything broader still needs
    /// `agent_tools_enabled`.
    fn agent_read_session(
        &self,
        agent: Option<Uuid>,
        task_id: Option<Uuid>,
        thread_id: Option<String>,
        provider: Option<ProviderKind>,
        turn: Option<usize>,
    ) -> anyhow::Result<ResponsePayload> {
        let target = match (task_id, thread_id.as_ref()) {
            (None, None) => {
                agent.ok_or_else(|| anyhow!("exactly one of task_id and thread_id is required"))?
            }
            _ => self.resolve_agent_target(task_id, thread_id, provider)?,
        };
        let in_scope = agent.is_some_and(|caller| {
            caller == target
                || self
                    .task_state
                    .lock()
                    .sessions
                    .iter()
                    .find(|session| session.id == caller)
                    .and_then(|session| session.side_chat_of)
                    == Some(target)
        });
        if !in_scope {
            self.require_agent_tools()?;
        }
        let mut state = self.task_state.lock();
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == target)
            .ok_or_else(|| anyhow!("task {target} is unknown to the daemon"))?;
        self.task_store.hydrate(session)?;
        if let Some(turn) = turn {
            if !session.turns.iter().any(|entry| entry.turn_count == turn) {
                bail!("task {target} has no turn {turn}");
            }
        }
        Ok(ResponsePayload::AgentSessionTranscript {
            transcript: session.agent_transcript(turn),
        })
    }

    /// The scoped credential's transcript search: the same corpus and
    /// filters as `SearchSessionMessages`, confined to the calling task's
    /// project. A scoped caller may still write `project:` — it just has to
    /// name that project.
    fn agent_search_sessions(
        &self,
        agent: Option<Uuid>,
        session_id: Uuid,
        query: &str,
        last_turns: Option<usize>,
    ) -> anyhow::Result<ResponsePayload> {
        self.require_agent_tools()?;
        // A scoped token names its owning session; a master-token request may
        // scope the search to `session_id` when it is a known task.
        let caller = agent.or_else(|| {
            (!session_id.is_nil() && self.known_session(session_id)).then_some(session_id)
        });
        let Some(caller) = caller else {
            bail!("task search needs a calling task to scope to");
        };
        let project_id = self
            .task_state
            .lock()
            .sessions
            .iter()
            .find(|session| session.id == caller)
            .map(|session| session.project_id)
            .ok_or_else(|| anyhow!("task {caller} is unknown to the daemon"))?;
        let matches = self.search_session_messages(
            query,
            AGENT_SEARCH_DEFAULT_LIMIT,
            SessionMessageSearchScope::Active,
            Some(project_id),
            last_turns,
        )?;
        let state = self.task_state.lock();
        let hits = matches
            .into_iter()
            .filter_map(|matched| {
                state
                    .sessions
                    .iter()
                    .find(|session| session.id == matched.session_id)
                    .map(|session| AgentSessionSearchHit {
                        task_id: session.id,
                        title: session.display_title().to_owned(),
                        provider: session.provider,
                        status: session.status,
                        updated_at: session.updated_at,
                        source: matched.source,
                        snippet: matched.snippet,
                    })
            })
            .collect();
        Ok(ResponsePayload::AgentSessionSearch { hits })
    }

    /// Run a transcript search after lifting `field:value` filters out of
    /// `query` — the shared implementation behind the palette-facing
    /// `SearchSessionMessages` and the agent-scoped `AgentSearchSessions`.
    /// `project_scope` confines the search to one project (the agent
    /// credential's own); `None` lets `project:` tokens select any project.
    /// `last_turns` — agent search only — scans just each task's N most
    /// recent turns.
    fn search_session_messages(
        &self,
        query: &str,
        limit: usize,
        scope: SessionMessageSearchScope,
        project_scope: Option<Uuid>,
        last_turns: Option<usize>,
    ) -> anyhow::Result<Vec<SessionMessageMatch>> {
        let parsed = parse_session_message_search(query);
        if parsed.is_blank() {
            return Ok(Vec::new());
        }
        // `project:`/`status:` resolve against live task state into a
        // session-id allowlist; the store scan then only sees the survivors.
        let allowed = {
            let state = self.task_state.lock();
            let project_ids: Option<Vec<Uuid>> = match project_scope {
                Some(own) => {
                    for value in &parsed.projects {
                        if resolve_named_search_project(&state.projects, value) != Some(own) {
                            bail!("project `{value}` is not this task's project");
                        }
                    }
                    Some(vec![own])
                }
                None if parsed.projects.is_empty() => None,
                None => Some(
                    parsed
                        .projects
                        .iter()
                        .filter_map(|value| resolve_named_search_project(&state.projects, value))
                        .collect(),
                ),
            };
            if project_ids.as_ref().is_some_and(Vec::is_empty) {
                return Ok(Vec::new());
            }
            (project_ids.is_some() || !parsed.statuses.is_empty()).then(|| {
                state
                    .sessions
                    .iter()
                    .filter(|session| {
                        project_ids
                            .as_ref()
                            .is_none_or(|ids| ids.contains(&session.project_id))
                            && (parsed.statuses.is_empty()
                                || parsed.statuses.contains(&session.status))
                    })
                    .map(|session| session.id)
                    .collect::<Vec<_>>()
            })
        };
        let matches = self.task_store.session_message_search(
            parsed.text,
            parsed.limit.unwrap_or(limit),
            parsed.scope.unwrap_or(scope),
            allowed,
            last_turns,
        )()?;
        Ok(matches)
    }

    /// Resolve an agent prompt's target: an explicit Waku task id, or a
    /// provider-native Agent CLI thread id matched against every
    /// daemon-known task's stored resume cursor.
    fn resolve_agent_target(
        &self,
        task_id: Option<Uuid>,
        thread_id: Option<String>,
        provider: Option<ProviderKind>,
    ) -> anyhow::Result<Uuid> {
        match (task_id, thread_id) {
            (Some(task_id), None) => self
                .known_session(task_id)
                .then_some(task_id)
                .ok_or_else(|| anyhow!("task {task_id} is unknown to the daemon")),
            (None, Some(thread_id)) => {
                let thread_id = thread_id.trim().to_owned();
                if thread_id.is_empty() {
                    bail!("the thread id must not be empty");
                }
                let mut state = self.task_state.lock();
                let mut matches = Vec::new();
                for index in 0..state.sessions.len() {
                    // The resume cursor lives in the session detail blob, so
                    // skeletons need hydrating before they can answer.
                    self.task_store.hydrate(&mut state.sessions[index])?;
                    let session = &state.sessions[index];
                    let Some(cursor) = &session.provider_cursor else {
                        continue;
                    };
                    if cursor.native_id() == thread_id
                        && provider.is_none_or(|provider| cursor.provider() == provider)
                    {
                        matches.push(session.id);
                    }
                }
                match matches.len() {
                    0 => bail!("no daemon task uses agent thread {thread_id}"),
                    1 => Ok(matches[0]),
                    _ => bail!(
                        "agent thread {thread_id} matches {} tasks; pass provider to disambiguate",
                        matches.len()
                    ),
                }
            }
            _ => bail!("exactly one of task_id and thread_id is required"),
        }
    }
}

fn validate_message_rewind(source: &AgentSession, turn_count: usize) -> anyhow::Result<()> {
    if !matches!(source.status, SessionStatus::Idle | SessionStatus::Failed) {
        bail!("stop the task before editing a prior message");
    }
    let Some(turn) = source
        .turns
        .iter()
        .find(|turn| turn.turn_count == turn_count)
    else {
        bail!("the selected message is unavailable");
    };
    if !source.messages.iter().any(|message| {
        message.turn_id == Some(turn.id) && message.role == crate::model::MessageRole::User
    }) {
        bail!("the selected user message is unavailable");
    }
    let rollback_turns = source.provider_turns_after(turn_count.saturating_sub(1));
    if rollback_turns > 0 && source.provider_cursor.is_none() {
        bail!("the provider conversation is unavailable");
    }
    Ok(())
}

fn restore_rewind_safety(
    cwd: &Path,
    safety_ref: &str,
    context: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    match crate::checkpoint::restore_ref(cwd, safety_ref) {
        Ok(()) => {
            let _ = crate::checkpoint::delete_ref(cwd, safety_ref);
            anyhow!("{context}: {error}; the original worktree was restored")
        }
        Err(restore_error) => anyhow!(
            "{context}: {error}; restoring the safety snapshot also failed: {restore_error}; snapshot: {safety_ref}"
        ),
    }
}

fn validate_response_fork(source: &AgentSession, turn_count: usize) -> anyhow::Result<()> {
    if !matches!(source.status, SessionStatus::Idle | SessionStatus::Failed) {
        bail!("stop the task before forking a response");
    }
    let cursor = source
        .provider_cursor
        .as_ref()
        .ok_or_else(|| anyhow!("the provider conversation is unavailable"))?;
    if cursor.provider() != source.provider {
        bail!("the provider conversation does not match this task");
    }
    if source
        .turns
        .get(turn_count.saturating_sub(1))
        .is_none_or(|turn| turn.turn_count != turn_count || !turn.provider_turn_started)
    {
        bail!("the selected response cannot be forked");
    }
    Ok(())
}

fn numbered_title_suffix(title: &str) -> Option<(&str, usize)> {
    let (base, suffix) = title.rsplit_once(" (")?;
    let number = suffix.strip_suffix(')')?.parse().ok()?;
    (!base.is_empty() && number >= 2).then_some((base, number))
}

fn next_response_fork_title<'a>(
    source_title: &str,
    existing_titles: impl IntoIterator<Item = &'a str>,
) -> String {
    let existing_titles = existing_titles.into_iter().collect::<Vec<_>>();
    let base = numbered_title_suffix(source_title)
        .filter(|(base, _)| existing_titles.iter().any(|title| title == base))
        .map_or(source_title, |(base, _)| base);
    let highest_number = existing_titles
        .iter()
        .filter_map(|title| {
            if *title == base {
                Some(1)
            } else {
                numbered_title_suffix(title)
                    .filter(|(candidate_base, _)| *candidate_base == base)
                    .map(|(_, number)| number)
            }
        })
        .max()
        .unwrap_or(1);
    format!("{base} ({})", highest_number.saturating_add(1).max(2))
}

fn fork_provider_session(
    request: ProviderSessionForkRequest,
) -> anyhow::Result<ProviderSessionFork> {
    use crate::model::ProviderResumeCursor;

    let (cursor, message_ids, source_resume_at) = match request {
        ProviderSessionForkRequest::Claude {
            session_id,
            resume_at,
            turn_count,
            title,
        } => {
            let source_resume_at = resume_at.map(Ok).unwrap_or_else(|| {
                crate::claude_session::message_id_for_turn(&session_id, turn_count)
            })?;
            let fork =
                crate::claude_session::fork_session_at(&session_id, &source_resume_at, &title)?;
            let fork_resume_at = fork
                .message_ids
                .get(&source_resume_at)
                .cloned()
                .ok_or_else(|| anyhow!("Claude fork did not include its target message"))?;
            (
                ProviderResumeCursor::Claude {
                    session_id: fork.session_id,
                    resume_at: Some(fork_resume_at),
                },
                fork.message_ids,
                Some(source_resume_at),
            )
        }
        ProviderSessionForkRequest::Amp {
            binary,
            cwd,
            thread_id,
            fork_context,
            turn_count,
        } => (
            crate::amp_session::fork_session_at_turn(
                &binary,
                &cwd,
                &thread_id,
                fork_context.as_deref(),
                turn_count,
            )?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Cursor { source, turn_count } => (
            crate::cursor_session::fork_session_at_turn(&source, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::OpenCode {
            binary,
            cwd,
            session_id,
            turn_count,
        } => (
            crate::opencode_session::fork_session_at_turn(&binary, &cwd, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::OpenCode2 {
            binary,
            session_id,
            turn_count,
        } => (
            crate::opencode2_session::fork_session_at_turn(&binary, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Grok {
            binary,
            cwd,
            session_id,
            turn_count,
        } => (
            crate::grok_session::fork_session_at_turn(&binary, &cwd, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Copilot {
            binary,
            cwd,
            session_id,
            turn_count,
            title,
        } => (
            crate::copilot_session::fork_session_at_turn(
                &binary,
                &cwd,
                &session_id,
                turn_count,
                &title,
            )?,
            HashMap::new(),
            None,
        ),
        ProviderSessionForkRequest::Muse {
            binary,
            session_id,
            turn_count,
        } => (
            crate::muse_session::fork_session_at_turn(&binary, &session_id, turn_count)?,
            HashMap::new(),
            None,
        ),
    };
    Ok(ProviderSessionFork {
        cursor,
        message_ids,
        source_resume_at,
    })
}

fn handle_driver_command(
    driver: &DriverHandle,
    command: Command,
) -> anyhow::Result<ResponsePayload> {
    match command {
        Command::Prompt {
            prompt,
            attachments,
            ..
        } => driver.prompt_with_attachments(prompt, attachments),
        Command::Steer { prompt } => driver.steer(prompt),
        Command::ClarifyUserInput {
            request_id,
            content,
        } => driver.clarify_user_input(request_id, content),
        Command::CancelUserInput { request_id } => driver.cancel_user_input(request_id),
        Command::Cancel => driver.cancel(),
        Command::CancelComputerUse => driver.cancel_computer_use(),
        Command::RefreshBackgroundWork => driver.refresh_background_work(),
        Command::StopBackgroundWork { key, control_id } => {
            driver.stop_background_work(
                serde_json::from_value(key).context("invalid background-work key")?,
                control_id,
            );
        }
        Command::Respond {
            request_id,
            option_id,
        } => driver.respond(request_id, option_id),
        Command::RespondUserInput {
            request_id,
            answers,
        } => driver.respond_user_input(request_id, answers),
        Command::Goal { operation } => driver.goal(operation),
        // Fire-and-forget like Goal: admission and the outcome arrive as
        // driver events, so the caller never waits on a response.
        Command::Compact => driver.compact(),
        Command::RunComputerTool { request } => {
            driver.run_computer_tool(crate::computer_use::ComputerToolRequest {
                call_id: request.call_id,
                tool: request.tool,
                arguments: request.arguments,
            });
        }
        Command::RejectComputerTool { request, reason } => {
            driver.reject_computer_tool(
                crate::computer_use::ComputerToolRequest {
                    call_id: request.call_id,
                    tool: request.tool,
                    arguments: request.arguments,
                },
                reason,
            );
        }
        Command::ApplyOptions { options } => {
            return Ok(ResponsePayload::OptionsApplied {
                applied: driver.apply_options(SessionOptions {
                    mode: decode_enum(&options.mode)?,
                    model: options.model,
                    reasoning_effort: options.reasoning_effort,
                    service_tier: options.service_tier,
                    context_window: options.context_window,
                }),
            });
        }
        Command::Rollback { turns } => {
            let cursor = driver
                .rollback(turns)?
                .map(serde_json::to_value)
                .transpose()?;
            return Ok(ResponsePayload::Cursor { cursor });
        }
        Command::Fork { turns_to_remove } => {
            let cursor = Some(serde_json::to_value(driver.fork(turns_to_remove)?)?);
            return Ok(ResponsePayload::Cursor { cursor });
        }
        Command::AttachSession
        | Command::Start { .. }
        | Command::GetSettings
        | Command::UpdateSettings { .. }
        | Command::SetDaemonExposure { .. }
        | Command::ProbeProvider { .. }
        | Command::SandboxSignIn { .. }
        | Command::SandboxAuthStatus { .. }
        | Command::FetchPlanUsage { .. }
        | Command::ConsumeCodexResetCredit { .. }
        | Command::ProbeComputerPermissions { .. }
        | Command::Evaluate { .. }
        | Command::TestEvalConnection { .. }
        | Command::RouteTask { .. }
        | Command::RecordRouteOverride { .. }
        | Command::LoadEvalUsage
        | Command::LoadUsageHistory { .. }
        | Command::LoadSkills { .. }
        | Command::SetSkillsEnabled { .. }
        | Command::TrashSkills { .. }
        | Command::LoadTaskState
        | Command::SaveTaskState { .. }
        | Command::RemoveSession
        | Command::RemoveProject { .. }
        | Command::HydrateSession { .. }
        | Command::SearchSessionMessages { .. }
        | Command::ListProviderSessions { .. }
        | Command::LoadProviderSession { .. }
        | Command::LoadComposerDrafts
        | Command::SaveComposerDrafts { .. }
        | Command::ApplyComposerDraftChanges { .. }
        | Command::StoreBlob { .. }
        | Command::ImportAttachment { .. }
        | Command::ImportPathAttachment { .. }
        | Command::ReadBlob { .. }
        | Command::ReadAttachment { .. }
        | Command::SweepBlobs
        | Command::ForkSessionFromResponse { .. }
        | Command::RewindSessionToMessage { .. }
        | Command::ForkProviderSession { .. }
        | Command::Workspace { .. }
        | Command::OpenTerminal { .. }
        | Command::WriteTerminal { .. }
        | Command::ResizeTerminal { .. }
        | Command::CloseTerminal
        | Command::CloseSession
        | Command::AgentCreateSession { .. }
        | Command::AgentPrompt { .. }
        | Command::AgentRenameSelf { .. }
        | Command::AgentReadSession { .. }
        | Command::AgentSearchSessions { .. }
        | Command::CancelQueuedPrompt { .. }
        | Command::UpsertCustomCommand { .. }
        | Command::RemoveCustomCommand { .. }
        | Command::ListCustomCommands
        | Command::ListIntegrations
        | Command::ConnectIntegration { .. }
        | Command::SetIntegrationProviders { .. }
        | Command::DisconnectIntegration { .. }
        | Command::StartIntegrationAuth { .. }
        | Command::GetFriends
        | Command::GetPairing
        | Command::RespondPairRequest { .. }
        | Command::RevokePairedClient { .. }
        | Command::SendFriendRequest { .. }
        | Command::RespondFriendRequest { .. }
        | Command::WithdrawFriendRequest { .. }
        | Command::RemoveFriend { .. }
        | Command::SendFileToFriend { .. }
        | Command::SendMessageToFriend { .. }
        | Command::CancelTransfer { .. }
        | Command::ProbeFriend { .. }
        | Command::SetFriendDisplayName { .. }
        | Command::SetFriendNickname { .. }
        | Command::GetAutomations
        | Command::GetDaemonStats
        | Command::UpsertAutomation { .. }
        | Command::RemoveAutomation { .. }
        | Command::RunAutomationNow { .. }
        | Command::ShareProjectWithFriend { .. }
        | Command::UnshareProjectWithFriend { .. }
        | Command::EnableFriendSync { .. }
        | Command::DisableFriendSync { .. }
        | Command::SetFriendSyncConfig { .. }
        | Command::FriendSyncNow { .. }
        | Command::FriendSyncAlertAction { .. }
        | Command::GetFriendSyncBranches { .. }
        | Command::SetFriendSessionSharing { .. }
        | Command::GetFriendSessions { .. }
        | Command::WatchFriendSession { .. }
        | Command::UnwatchFriendSession { .. } => {
            bail!("daemon received a command in the wrong dispatch path")
        }
    }
    Ok(ResponsePayload::Ack)
}

fn ensure_shell_environment() {
    static REFRESHED: OnceLock<()> = OnceLock::new();
    REFRESHED.get_or_init(|| {
        crate::command_env::refresh_from_default_shell();
    });
}

/// Kick a build or refresh of one workspace's project-map index on a
/// background thread. Already-building roots are skipped; the index steps
/// out of the map while it parses so readers never wait on a refresh — a
/// missing entry just means "send unmapped" that turn. Returns whether a
/// build actually started; `sink`, when given, hears the `Ready` update.
fn spawn_repo_map_refresh(
    repo_maps: &Arc<(Mutex<RepoMaps>, Condvar)>,
    root: PathBuf,
    sink: Option<EventSink>,
) -> bool {
    {
        let mut maps = repo_maps.0.lock();
        if !maps.building.insert(root.clone()) {
            return false;
        }
    }
    let repo_maps = repo_maps.clone();
    let _ = std::thread::Builder::new()
        .name("goddard-repo-map".to_owned())
        .spawn(move || {
            let mut index = repo_maps.0.lock().indexes.remove(&root);
            let result = match index.as_mut() {
                Some(existing) => existing.refresh().map(|_| ()),
                None => crate::repo_map::RepoMapIndex::scan(&root).map(|built| index = Some(built)),
            };
            let mut maps = repo_maps.0.lock();
            match result {
                Ok(()) => {
                    if let Some(index) = index {
                        let indexed_files = index.indexed_files();
                        maps.indexes.insert(root.clone(), index);
                        if let Some(sink) = &sink
                            && let Ok(wire) =
                                event_to_wire(DriverEvent::ProjectMap(ProjectMapStatus::Ready {
                                    indexed_files,
                                }))
                        {
                            // Every session in this root moves to ready —
                            // including one that joined the build late and
                            // never got its own thread.
                            for (session_id, (_, runtime_id)) in
                                maps.sessions.iter().filter(|(_, (cwd, _))| *cwd == root)
                            {
                                let _ = sink
                                    .for_session(*session_id, *runtime_id)
                                    .send_ephemeral(wire.clone());
                            }
                        }
                    }
                }
                Err(error) => {
                    eprintln!(
                        "goddard-daemon: project map scan failed for {}: {error:#}",
                        root.display()
                    );
                    if let Some(prior) = index {
                        maps.indexes.insert(root.clone(), prior);
                    }
                }
            }
            maps.building.remove(&root);
            repo_maps.1.notify_all();
        });
    true
}

/// Pump provider events for one runtime into the client's event stream.
///
/// Besides serialization this maintains the agent-surface bookkeeping: turn
/// state decides where queue-mode prompts wait, a finished turn drains that
/// queue in submission order, a `steerAccepted` echo is annotated with the
/// sending task, and a dead runtime drops its credential and registry entry
/// with it.
fn forward_driver_events(
    session_id: Uuid,
    runtime_id: Uuid,
    event_receiver: crossbeam_channel::Receiver<DriverEvent>,
    events: EventSink,
    driver: DriverHandle,
    agent: Arc<crate::agent::AgentState>,
    task_state: Arc<Mutex<PersistedState>>,
    task_store: Arc<StateStore>,
    sessions: Arc<Mutex<HashMap<Uuid, RuntimeEntry>>>,
    automations: Arc<AutomationService>,
    auto_prompts: Arc<AutoPromptService>,
    memory: Arc<crate::memory::MemoryService>,
    repo_maps: Arc<(Mutex<RepoMaps>, Condvar)>,
) {
    while let Ok(event) = event_receiver.recv() {
        // Every forwarded event is activity the idle reaper counts.
        if let Some(entry) = sessions.lock().get_mut(&session_id)
            && entry.runtime_id == runtime_id
        {
            entry.last_active = std::time::Instant::now();
        }
        let rejected_steer = agent.note_driver_event(session_id, &event);
        automations.note_driver_event(session_id, &event);
        if let DriverEvent::TurnFinished { success, .. } = &event {
            auto_prompts.note_turn_finished(session_id, *success);
        }
        let event = match event {
            DriverEvent::Connected { provider_cursor } => {
                // The daemon keeps its own copy of the resume cursor so
                // thread-id resolution and cold starts work even when no
                // client ever saves the task.
                record_provider_cursor(&task_state, &task_store, session_id, &provider_cursor);
                // A reported cursor makes this runtime evictable — the next
                // prompt can rebuild it even if the catalog row has since
                // been skeletonized by the transcript window.
                if provider_cursor.is_some()
                    && let Some(entry) = sessions.lock().get_mut(&session_id)
                    && entry.runtime_id == runtime_id
                {
                    entry.resumable = true;
                }
                DriverEvent::Connected { provider_cursor }
            }
            DriverEvent::SteerAccepted { message, .. } => {
                let steer = agent.take_pending_steer(session_id, &message);
                // An enveloped steer echoes the envelope; the transcript and
                // attached clients show the sender's own words.
                let message = steer
                    .as_ref()
                    .filter(|steer| steer.transport.is_some())
                    .map(|steer| steer.prompt.clone())
                    .unwrap_or(message);
                let sent_by_task = steer.as_ref().and_then(|steer| steer.sender);
                let hidden = steer.as_ref().is_some_and(|steer| steer.context.is_some());
                if let Some(sender) = sent_by_task {
                    record_agent_steer(&task_state, &task_store, session_id, &message, sender);
                }
                // A context steer carrying project memory settled the
                // session's injection — later prompts stay untouched. A
                // rejected steer leaves the flag unset so the next prompt
                // retries.
                if steer
                    .as_ref()
                    .is_some_and(|steer| steer.context == Some(crate::agent::ContextSteer::Memory))
                {
                    memory.mark_injected(session_id);
                }
                // The agent-surface instruction composes into every context
                // steer while it is owed, so any accepted context steer
                // delivered it; a rejection leaves it pending to retry.
                if steer.as_ref().is_some_and(|steer| steer.context.is_some()) {
                    agent.mark_surface_announced(session_id);
                    // A pending parent-index carry settles with the steer
                    // that shipped it.
                    agent.mark_parent_index_delivered(session_id);
                }
                // A queue-drained prompt folded into the parked turn: its
                // mirrored chip's wait is over even when the steer carried
                // no sender (an automation run) to attribute.
                if let Some(queued_id) = steer.and_then(|steer| steer.queued_id)
                    && unmirror_agent_queued_prompt(&task_state, &task_store, session_id, queued_id)
                        .is_ok()
                {
                    send_agent_queue_changed(&task_state, &events, session_id);
                }
                DriverEvent::SteerAccepted {
                    message,
                    sent_by_task,
                    hidden,
                }
            }
            DriverEvent::SteerRejected {
                message,
                reason,
                reason_i18n,
                ..
            } => {
                // A daemon-injected context steer is the daemon's own
                // delivery — swallow the rejection so no client surfaces it;
                // the session stays eligible and the next prompt retries.
                if rejected_steer
                    .as_ref()
                    .is_some_and(|steer| steer.context.is_some())
                {
                    continue;
                }
                // An enveloped steer echoes the envelope; a surfaced
                // rejection names the sender's own words.
                let message = rejected_steer
                    .as_ref()
                    .filter(|steer| steer.transport.is_some())
                    .map(|steer| steer.prompt.clone())
                    .unwrap_or(message);
                DriverEvent::SteerRejected {
                    message,
                    reason,
                    reason_i18n,
                    hidden: false,
                }
            }
            event => event,
        };
        // A finished turn frees the session for the next queued prompt; a
        // `connected` greeting means a freshly (re)started runtime is idle,
        // so prompts queued while it was down deliver now.
        let drains_queue = matches!(
            &event,
            DriverEvent::TurnFinished { .. } | DriverEvent::Connected { .. }
        );
        // A settled turn is the freshness trigger for the workspace map:
        // whatever the turn edited lands in the index before the next
        // session's first prompt reads it.
        if matches!(&event, DriverEvent::TurnFinished { .. }) {
            let cwd = repo_maps
                .0
                .lock()
                .sessions
                .get(&session_id)
                .map(|(cwd, _)| cwd.clone());
            if let Some(cwd) = cwd
                && spawn_repo_map_refresh(&repo_maps, cwd, Some(events.clone()))
                && let Ok(wire) =
                    event_to_wire(DriverEvent::ProjectMap(ProjectMapStatus::Refreshing))
            {
                let _ = events.send_ephemeral(wire);
            }
        }
        let process_exited = matches!(&event, DriverEvent::ProcessExited);
        // A finished turn or a dead runtime is a memory-worthy boundary: mark
        // the project for distillation. The service decides cheaply whether
        // enough new transcript exists to spend a provider call on.
        if matches!(
            &event,
            DriverEvent::TurnFinished { .. } | DriverEvent::ProcessExited
        ) {
            memory.note_session_activity(session_id);
        }
        let wire = event_to_wire(event).unwrap_or_else(|error| {
            WireDriverEvent::new(
                "error",
                Value::String(format!("could not encode daemon event: {error}")),
            )
        });
        if events.send(wire).is_err() {
            break;
        }
        if drains_queue {
            // A restarted daemon rebuilt no in-memory queue — the session
            // document's mirrored entries are the surviving record.
            rehydrate_agent_queue(&agent, &task_state, &task_store, session_id);
            while let Some(entry) = agent.pop_queued(session_id) {
                if agent.is_working(session_id) {
                    // A turn started while the queue drained — a human
                    // prompt, or a provider-side wake. Queue-mode messages
                    // wait for the finish rather than steer mid-turn.
                    agent.requeue_front(session_id, entry);
                    break;
                }
                if let Err(error) = deliver_agent_prompt(
                    session_id,
                    &driver,
                    entry,
                    &events,
                    &agent,
                    &auto_prompts,
                    &task_state,
                    &task_store,
                ) {
                    eprintln!(
                        "goddard-daemon could not deliver a queued agent prompt for task {session_id}: {error:#}"
                    );
                    break;
                }
            }
        }
        if process_exited {
            agent.clear_session(session_id);
            {
                let mut maps = repo_maps.0.lock();
                maps.sessions.remove(&session_id);
                maps.pending.remove(&session_id);
            }
            // The hub retires a runtime on the request path — CloseSession,
            // a failed Start — but a provider that exits on its own never
            // produces one. Without this the dead runtime's replay journal,
            // sequence counter, and active-runtime entry sit in the hub for
            // the rest of the daemon's life.
            events
                .for_session(session_id, runtime_id)
                .end_session_runtime();
            let removed = {
                let mut sessions = sessions.lock();
                sessions
                    .get(&session_id)
                    .is_some_and(|entry| entry.runtime_id == runtime_id)
                    .then(|| sessions.remove(&session_id))
                    .flatten()
            };
            drop_detached(removed);
            break;
        }
    }
}

/// Reclaim provider runtimes idle past the configured timeout. A runtime is
/// only reclaimable when its task can come back: nothing mid-turn, parked,
/// or queued for delivery, and the session either never produced provider
/// state or holds a resume cursor to rebuild it. Entries are removed here;
/// hub retirement and process teardown are the caller's job, off this lock.
fn reap_idle_runtimes(
    sessions: &Mutex<HashMap<Uuid, RuntimeEntry>>,
    task_state: &Mutex<PersistedState>,
    settings: &DaemonSettingsStore,
    agent: &crate::agent::AgentState,
) -> Vec<(Uuid, Uuid, DriverHandle)> {
    let timeout = match settings.get().runtime_idle_timeout_secs {
        Some(0) => return Vec::new(),
        Some(secs) => std::time::Duration::from_secs(secs),
        None => DEFAULT_RUNTIME_IDLE_TIMEOUT,
    };
    let cutoff = std::time::Instant::now() - timeout;
    let state = task_state.lock();
    let mut sessions = sessions.lock();
    let evictable = sessions
        .iter()
        .filter(|(session_id, entry)| {
            entry.last_active <= cutoff && runtime_evictable(&state, **session_id, entry, agent)
        })
        .map(|(session_id, _)| *session_id)
        .collect::<Vec<_>>();
    evictable
        .into_iter()
        .filter_map(|session_id| {
            sessions
                .remove(&session_id)
                .map(|entry| (session_id, entry.runtime_id, entry.driver))
        })
        .collect()
}

/// Retire one runtime the reaper claimed: shut the provider down, release
/// its agent bookkeeping and project-map state, then tell attached
/// clients the runtime ended. The notification must run before
/// `end_session_runtime` clears the hub's routing — afterwards the event
/// would be dropped as stale — and without it a client keeps its driver
/// handle, so the next prompt vanishes into a runtime the daemon no
/// longer has: runtime commands are fire-and-forget and carry no response.
fn evict_idle_runtime(
    session_id: Uuid,
    runtime_id: Uuid,
    driver: DriverHandle,
    agent: &crate::agent::AgentState,
    repo_maps: &Arc<(Mutex<RepoMaps>, Condvar)>,
    events: &EventSink,
) {
    driver.begin_shutdown();
    // The scoped credential was valid only while the provider process
    // carrying it lived; the turn bookkeeping and pending steers die with
    // it too.
    agent.clear_session(session_id);
    {
        let mut maps = repo_maps.0.lock();
        maps.sessions.remove(&session_id);
        maps.pending.remove(&session_id);
    }
    let sink = events.for_session(session_id, runtime_id);
    sink.notify_runtime_ended();
    sink.end_session_runtime();
    drop_detached(driver);
}

/// Whether killing this task's provider process loses nothing the next
/// prompt cannot rebuild. Busy is checked twice — the persisted status and
/// the forwarder's turn bookkeeping — because either can be the fresher
/// signal when they disagree. Resumability comes from the runtime entry,
/// not the catalog row: a skeletonized session reads `provider_cursor:
/// None` until its next hydrate.
fn runtime_evictable(
    state: &PersistedState,
    session_id: Uuid,
    entry: &RuntimeEntry,
    agent: &crate::agent::AgentState,
) -> bool {
    let Some(session) = state.sessions.iter().find(|s| s.id == session_id) else {
        // No catalog row claims this runtime — eviction only helps.
        return true;
    };
    if session.status.is_busy()
        || agent.has_open_turn(session_id)
        || agent.has_queued(session_id)
        || (session.detail_loaded && !session.queued_messages.is_empty())
    {
        return false;
    }
    entry.resumable || !session.has_started()
}

/// The provider-facing envelope for a task-to-task prompt: names the
/// sending task so the receiving agent knows another of the user's agents —
/// not the user — is talking, and has the id it needs to answer through
/// `goddard-agent`. The transcript keeps `prompt` verbatim; `None` means
/// send it unwrapped (unattributed sender, or a task messaging itself).
fn agent_prompt_envelope(
    task_state: &Mutex<PersistedState>,
    target: Uuid,
    sender: Option<Uuid>,
    prompt: &str,
) -> Option<String> {
    let sender_id = sender.filter(|sender| *sender != target)?;
    let state = task_state.lock();
    let sender_session = state
        .sessions
        .iter()
        .find(|session| session.id == sender_id);
    let relation = if sender_session.is_some_and(|sender| sender.side_chat_of == Some(target)) {
        "your side chat"
    } else {
        "the agent of another Goddard task"
    };
    let title = sender_session
        .map(|session| {
            session
                .display_title()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|title| !title.is_empty());
    let origin = match title {
        Some(title) => format!("{relation} \"{title}\" (task id {sender_id})"),
        None => format!("{relation} (task id {sender_id})"),
    };
    Some(format!(
        "The message below is from {origin}, sent through `goddard-agent` on \
         the user's behalf — the user can see this exchange. To send a \
         message back, run \
         `goddard-agent prompt '{{\"task_id\":\"{sender_id}\",\"prompt\":\"<reply>\"}}'`.\n\n{prompt}"
    ))
}

/// Deliver one queued agent prompt to a live session. A session with an
/// open but parked turn is messaged through the provider's steer path so
/// the prompt folds into the waiting turn; anything else begins a normal
/// new turn whose `promptSubmitted` broadcast carries the sender's
/// provenance.
fn deliver_agent_prompt(
    session_id: Uuid,
    driver: &DriverHandle,
    entry: crate::agent::AgentPrompt,
    sink: &EventSink,
    agent: &crate::agent::AgentState,
    auto_prompts: &AutoPromptService,
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
) -> anyhow::Result<()> {
    if agent.has_parked_turn(session_id) && driver.supports_steer() {
        let mut entry = entry;
        entry.transport =
            agent_prompt_envelope(task_state, session_id, entry.sender, &entry.prompt);
        let prompt = entry
            .transport
            .clone()
            .unwrap_or_else(|| entry.prompt.clone());
        agent.record_pending_steer(session_id, entry);
        driver.steer(prompt);
        return Ok(());
    }
    let turn_id = Uuid::new_v4();
    auto_prompts.note_nonhuman_turn(session_id, turn_id);
    // A parked prompt reuses its mirrored chip's id as the delivered
    // message's id: clients folding `promptSubmitted` drop the chip and
    // adopt the transcript row in one move.
    let message_id = entry.queued_id.unwrap_or_else(Uuid::new_v4);
    persist_agent_prompt(
        task_state,
        task_store,
        session_id,
        &entry.prompt,
        turn_id,
        message_id,
        entry.sender,
        entry.queued_id,
    )?;
    sink.send(event_to_wire(DriverEvent::PromptSubmitted {
        message: entry.prompt.clone(),
        turn_id,
        message_id,
        sent_by_task: entry.sender,
        hidden: false,
    })?)?;
    send_agent_queue_changed(task_state, sink, session_id);
    driver.prompt(
        agent_prompt_envelope(task_state, session_id, entry.sender, &entry.prompt)
            .unwrap_or(entry.prompt),
    );
    Ok(())
}

/// Mirror an accepted agent prompt into the daemon's stored copy of the
/// task, so the message and its sender provenance persist even when no
/// client is attached to adopt it. `queued_id` names the parked chip the
/// prompt is delivering out of — it leaves the queue in the same write.
fn persist_agent_prompt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    message: &str,
    turn_id: Uuid,
    message_id: Uuid,
    sent_by_task: Option<Uuid>,
    queued_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        bail!("task {session_id} is unknown to the daemon");
    };
    task_store.hydrate(session)?;
    let dequeued = queued_id.is_some_and(|queued_id| {
        let before = session.queued_messages.len();
        session
            .queued_messages
            .retain(|queued| queued.id != queued_id);
        session.queued_messages.len() != before
    });
    if session.adopt_submitted_prompt(message, turn_id, message_id, sent_by_task, false) || dequeued
    {
        state.mark_session_dirty(session_id);
        task_store.save(&mut state)?;
    }
    Ok(())
}

/// Park an agent prompt in the session document's follow-up queue so every
/// client renders the wait as a queued chip. The entry's id doubles as the
/// eventual transcript message id — delivery removes it.
fn mirror_agent_queued_prompt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    queued_id: Uuid,
    prompt: &str,
    sent_by: Option<Uuid>,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        bail!("task {session_id} is unknown to the daemon");
    };
    task_store.hydrate(session)?;
    if session
        .queued_messages
        .iter()
        .any(|queued| queued.id == queued_id)
    {
        return Ok(());
    }
    let mut entry = crate::model::QueuedMessage::agent(prompt, sent_by);
    entry.id = queued_id;
    session.queued_messages.push(entry);
    session.updated_at = crate::model::unix_time();
    state.mark_session_dirty(session_id);
    task_store.save(&mut state)?;
    Ok(())
}

/// Drop the mirrored chip a parked-steer prompt delivered out of, once the
/// provider accepted it into the waiting turn.
fn unmirror_agent_queued_prompt(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    queued_id: Uuid,
) -> anyhow::Result<()> {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return Ok(());
    };
    task_store.hydrate(session)?;
    let before = session.queued_messages.len();
    session
        .queued_messages
        .retain(|queued| queued.id != queued_id);
    if session.queued_messages.len() == before {
        return Ok(());
    }
    session.updated_at = crate::model::unix_time();
    state.mark_session_dirty(session_id);
    task_store.save(&mut state)?;
    Ok(())
}

/// Refill the in-memory prompt queue from agent entries the session
/// document still mirrors as parked. The mirror survives restarts the
/// `AgentState` map does not, so a drained or never-started runtime finds
/// its backlog here instead of losing it.
fn rehydrate_agent_queue(
    agent: &crate::agent::AgentState,
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
) {
    let seeded = {
        let mut state = task_state.lock();
        let Some(session) = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        else {
            return;
        };
        if task_store.hydrate(session).is_err() {
            return;
        }
        session
            .queued_messages
            .iter()
            .filter_map(|queued| match queued.source {
                crate::model::QueuedMessageSource::Agent { sent_by } => {
                    Some(crate::agent::AgentPrompt {
                        prompt: queued.content.clone(),
                        transport: None,
                        sender: sent_by,
                        queued_id: Some(queued.id),
                        context: None,
                    })
                }
                crate::model::QueuedMessageSource::User => None,
            })
            .collect::<Vec<_>>()
    };
    if !seeded.is_empty() {
        agent.seed_queue(session_id, seeded);
    }
}

/// Publish the daemon-owned follow-up queue for a session so attached
/// clients redraw the chip row. Best-effort: a dead subscriber just misses
/// the frame and picks the state up on its next hydrate.
fn send_agent_queue_changed(
    task_state: &Mutex<PersistedState>,
    sink: &EventSink,
    session_id: Uuid,
) {
    let messages = task_state
        .lock()
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .map(|session| {
            session
                .queued_messages
                .iter()
                .filter(|queued| queued.is_agent_owned())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Ok(wire) = event_to_wire(DriverEvent::QueuedMessagesChanged { messages }) {
        let _ = sink.send(wire);
    }
}

/// Mirror a provider-accepted agent steer into the stored task the way
/// [`persist_agent_prompt`] mirrors a queued prompt.
fn record_agent_steer(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    message: &str,
    sent_by_task: Uuid,
) {
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };
    if task_store.hydrate(session).is_err() {
        return;
    }
    session.push_user_message_with_presentation(message, None, Vec::new(), Some(sent_by_task));
    state.mark_session_dirty(session_id);
    if let Err(error) = task_store.save(&mut state) {
        eprintln!(
            "goddard-daemon could not persist an agent steer for task {session_id}: {error:#}"
        );
    }
}

/// Keep the daemon's stored copy of a task's provider cursor current so
/// cold starts and thread-id resolution work without a client ever saving.
fn record_provider_cursor(
    task_state: &Mutex<PersistedState>,
    task_store: &StateStore,
    session_id: Uuid,
    provider_cursor: &Option<ProviderResumeCursor>,
) {
    let Some(cursor) = provider_cursor else {
        return;
    };
    let mut state = task_state.lock();
    let Some(session) = state
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };
    if task_store.hydrate(session).is_err() || session.provider_cursor.as_ref() == Some(cursor) {
        return;
    }
    session.provider_cursor = Some(cursor.clone());
    state.mark_session_dirty(session_id);
    if let Err(error) = task_store.save(&mut state) {
        eprintln!(
            "goddard-daemon could not persist a provider cursor for task {session_id}: {error:#}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The auth-status command reads the per-provider shared home — a
    /// credential file under `sandbox-homes/<id>` flips the answer.
    #[test]
    fn sandbox_auth_status_reflects_the_provider_home() {
        let root = std::env::temp_dir().join(format!("waku-sandbox-auth-{}", Uuid::new_v4()));
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let request = |command| waku_protocol::Request {
            request_id: Uuid::new_v4(),
            session_id: Uuid::nil(),
            runtime_id: Uuid::nil(),
            command,
        };

        let status = backend
            .handle(
                request(Command::SandboxAuthStatus {
                    provider: ProviderKind::Devin,
                }),
                EventSink::detached(),
                None,
            )
            .unwrap();
        assert!(matches!(
            status,
            ResponsePayload::SandboxAuthStatus { signed_in: false }
        ));

        let creds = root.join("sandbox-homes/devin/.local/share/devin");
        std::fs::create_dir_all(&creds).unwrap();
        std::fs::write(creds.join("credentials.toml"), "[auth]\n").unwrap();
        let status = backend
            .handle(
                request(Command::SandboxAuthStatus {
                    provider: ProviderKind::Devin,
                }),
                EventSink::detached(),
                None,
            )
            .unwrap();
        assert!(matches!(
            status,
            ResponsePayload::SandboxAuthStatus { signed_in: true }
        ));
        std::fs::remove_dir_all(&root).ok();
    }

    /// Sign-in resolves to the host-side `shuru run` argv the sign-in
    /// terminal executes. Skipped where shuru is not installed.
    #[test]
    fn sandbox_sign_in_returns_the_guest_invocation() {
        if crate::sandbox::shuru_binary().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("waku-sandbox-signin-{}", Uuid::new_v4()));
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let response = backend
            .handle(
                waku_protocol::Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command: Command::SandboxSignIn {
                        provider: ProviderKind::Devin,
                    },
                },
                EventSink::detached(),
                None,
            )
            .unwrap();
        let ResponsePayload::SandboxSignIn { program, args, cwd } = response else {
            panic!("expected SandboxSignIn, got {response:?}");
        };
        assert_eq!(program, crate::sandbox::shuru_binary().unwrap());
        assert_eq!(cwd, root.join("sandbox-homes"));
        assert!(args.contains(&"HOME=/root".to_owned()));
        assert!(args.contains(&"--allow-host-writes".to_owned()));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn agent_create_trait_resolution() {
        let options = vec![
            ProviderModelOption::new("medium", "Medium"),
            ProviderModelOption::new("high", "High"),
        ];

        // An explicit id wins, even one the catalog does not list.
        assert_eq!(
            resolve_agent_trait(
                Some("xhigh".into()),
                Some("medium".into()),
                Some(&options),
                Some("medium"),
            ),
            Some("xhigh".into())
        );
        // An explicit "default" (or empty) selects the provider's own
        // default rather than inheriting.
        for value in ["default", ""] {
            assert_eq!(
                resolve_agent_trait(
                    Some(value.into()),
                    Some("high".into()),
                    Some(&options),
                    Some("medium"),
                ),
                None
            );
        }
        // An omitted field inherits a value the resolved model still lists.
        assert_eq!(
            resolve_agent_trait(None, Some("high".into()), Some(&options), Some("medium")),
            Some("high".into())
        );
        // An inherited value the model no longer lists falls back to the
        // model's own default, or nothing when it declares none.
        assert_eq!(
            resolve_agent_trait(None, Some("ultra".into()), Some(&options), Some("medium")),
            Some("medium".into())
        );
        assert_eq!(
            resolve_agent_trait(None, Some("ultra".into()), Some(&options), None),
            None
        );
        // A catalog entry without options cannot constrain inheritance.
        assert_eq!(
            resolve_agent_trait(None, Some("ultra".into()), Some(&[]), None),
            Some("ultra".into())
        );
        // No catalog entry at all behaves the same.
        assert_eq!(
            resolve_agent_trait(None, Some("ultra".into()), None, None),
            Some("ultra".into())
        );
    }

    #[test]
    fn stale_runtime_projection_keeps_newer_transcript_cursor() {
        let runtime_id = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        existing.status = SessionStatus::Working;
        existing.runtime_event_cursor = Some(crate::model::RuntimeEventCursor {
            runtime_id,
            epoch,
            sequence: 10,
        });
        existing.push_message(crate::model::MessageRole::Assistant, "complete so far");

        let mut stale = existing.clone();
        stale.title = "Renamed elsewhere".into();
        stale.messages.clear();
        stale.runtime_event_cursor = Some(crate::model::RuntimeEventCursor {
            runtime_id,
            epoch,
            sequence: 7,
        });

        assert!(session_projection_precedes(
            &existing,
            &stale,
            Some(runtime_id)
        ));
        merge_stale_session_metadata(&mut existing, stale);
        assert_eq!(existing.title, "Renamed elsewhere");
        assert_eq!(existing.messages.len(), 1);
        assert_eq!(existing.runtime_event_cursor.unwrap().sequence, 10);
    }

    #[test]
    fn client_projection_cannot_replace_a_daemon_checkpoint() {
        let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        existing.begin_turn("change it");
        existing.finish_active_turn(crate::model::TurnStatus::Completed);
        let checkpoint = Checkpoint {
            turn_count: 1,
            git_ref: "refs/waku/canonical".into(),
            status: CheckpointStatus::Ready,
            files: Vec::new(),
            additions: 0,
            deletions: 0,
            created_at: 1,
        };
        existing.turns[0].checkpoint = Some(checkpoint.clone());

        let mut incoming = existing.clone();
        incoming.turns[0].checkpoint = Some(Checkpoint {
            git_ref: "refs/waku/stale-client".into(),
            ..checkpoint.clone()
        });
        preserve_daemon_checkpoints(&existing, &mut incoming);

        assert_eq!(incoming.turns[0].checkpoint.as_ref(), Some(&checkpoint));
    }

    #[test]
    fn a_parked_agent_prompt_mirrors_a_chip_then_cancels_cleanly() {
        let root = std::env::temp_dir().join(format!("waku-queue-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        // Unstarted drafts own no row; the session must exist on disk for
        // the backend to know it.
        state.sessions[0].begin_turn("seed");
        state.sessions[0].finish_active_turn(TurnStatus::Completed);
        store.save(&mut state).unwrap();
        let session_id = state.sessions[0].id;
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();

        // A working session parks the prompt — no runtime runs in the test,
        // so the queuedMessagesChanged broadcast is skipped but the mirror
        // still lands in the session document.
        let _ = backend
            .agent
            .note_driver_event(session_id, &DriverEvent::TurnStarted);
        backend
            .queue_agent_prompt(
                session_id,
                "agent follow-up".into(),
                None,
                &EventSink::detached(),
            )
            .unwrap();

        {
            let mut locked = backend.task_state.lock();
            let session = locked
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .unwrap();
            backend.task_store.hydrate(session).unwrap();
            assert_eq!(session.queued_messages.len(), 1);
            assert!(session.queued_messages[0].is_agent_owned());
            assert_eq!(session.queued_messages[0].content, "agent follow-up");
        }

        // A fresh daemon's memory holds nothing; the document rebuilds the
        // parked prompt.
        backend.agent.clear_session(session_id);
        rehydrate_agent_queue(
            &backend.agent,
            &backend.task_state,
            &backend.task_store,
            session_id,
        );
        let restored = backend.agent.pop_queued(session_id).unwrap();
        assert_eq!(restored.prompt, "agent follow-up");
        assert!(restored.queued_id.is_some());

        // Cancel drops the memory entry and the mirrored chip.
        backend.agent.clear_session(session_id);
        let queued_id = restored.queued_id.unwrap();
        rehydrate_agent_queue(
            &backend.agent,
            &backend.task_state,
            &backend.task_store,
            session_id,
        );
        let result = backend
            .cancel_queued_prompt(session_id, queued_id, &EventSink::detached())
            .unwrap();
        assert!(matches!(result, ResponsePayload::Ack));
        assert!(backend.agent.pop_queued(session_id).is_none());
        {
            let mut locked = backend.task_state.lock();
            let session = locked
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .unwrap();
            backend.task_store.hydrate(session).unwrap();
            assert!(session.queued_messages.is_empty());
        }

        // Cancelling a client-owned entry is refused.
        let mut locked = backend.task_state.lock();
        let session = locked
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .unwrap();
        session
            .queued_messages
            .push(crate::model::QueuedMessage::new("mine"));
        let user_id = session.queued_messages[0].id;
        drop(locked);
        assert!(
            backend
                .cancel_queued_prompt(session_id, user_id, &EventSink::detached())
                .is_err()
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn client_saves_cannot_resurrect_or_erase_daemon_queue_entries() {
        let mut existing = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let mut agent_entry = crate::model::QueuedMessage::agent("parked", None);
        agent_entry.created_at = 20;
        let mut user_entry = crate::model::QueuedMessage::new("mine");
        user_entry.created_at = 10;
        existing.queued_messages = vec![user_entry.clone(), agent_entry.clone()];

        // Stale path: a client projection still holding a delivered agent
        // chip must not re-add it — rehydration would deliver it twice.
        let daemon_copy_without_agent = {
            let mut copy = existing.clone();
            copy.queued_messages
                .retain(|queued| !queued.is_agent_owned());
            copy
        };
        let mut resurrecting = daemon_copy_without_agent.clone();
        merge_stale_session_metadata(&mut resurrecting, existing.clone());
        assert!(
            resurrecting
                .queued_messages
                .iter()
                .all(|queued| !queued.is_agent_owned()),
            "a stale client save must not resurrect a daemon-owned entry"
        );

        // Fresh path: a projection written before the mirror arrived keeps
        // the daemon's parked entry instead of erasing it wholesale.
        let mut fresh = daemon_copy_without_agent.clone();
        preserve_daemon_queued_messages(&existing, &mut fresh);
        assert_eq!(
            fresh.queued_messages,
            vec![user_entry, agent_entry],
            "the daemon's agent slice survives a fresh client save"
        );
    }

    #[test]
    fn expired_archives_are_purged_when_state_loads() {
        let root = std::env::temp_dir().join(format!("waku-archive-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        let expired_id = state.sessions[0].id;
        state.sessions[0].begin_turn("expired archive");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        state.sessions[0].archived_at =
            Some(crate::model::unix_time() - ARCHIVED_SESSION_RETENTION_SECONDS - 1);

        let project_id = state.projects[0].id;
        let mut recent = AgentSession::new(project_id, ProviderKind::Codex);
        recent.begin_turn("recent archive");
        recent.finish_active_turn(crate::model::TurnStatus::Completed);
        recent.archived_at = Some(crate::model::unix_time());
        let recent_id = recent.id;
        state.push_session(recent);

        let mut active = AgentSession::new(project_id, ProviderKind::Codex);
        active.begin_turn("still active");
        active.finish_active_turn(crate::model::TurnStatus::Completed);
        let active_id = active.id;
        state.push_session(active);
        store.save(&mut state).unwrap();

        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        let remaining = backend.task_state.lock().sessions.clone();
        assert!(
            !remaining.iter().any(|session| session.id == expired_id),
            "an archive past the retention window is removed on load"
        );
        assert!(
            remaining
                .iter()
                .any(|session| session.id == recent_id && session.archived_at.is_some()),
            "a recent archive survives the sweep and stays archived"
        );
        assert!(remaining.iter().any(|session| session.id == active_id));

        // The row is gone from storage too, so it cannot come back.
        let reloaded = StateStore::daemon(root.join("app.db")).load().unwrap();
        assert!(
            !reloaded
                .sessions
                .iter()
                .any(|session| session.id == expired_id)
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn incoming_transfer_materializes_a_quarantined_session() {
        let root = std::env::temp_dir().join(format!("waku-transfer-{}", Uuid::new_v4()));
        let share_dir = root.join("share");
        let store = Arc::new(StateStore::daemon(root.join("app.db")));
        let task_state = Arc::new(Mutex::new(PersistedState::fresh(root.join("repo"))));
        {
            let mut state = task_state.lock();
            state.last_provider = ProviderKind::Grok;
            state.last_model = Some("grok-code-fast-1".into());
        }
        let transfer = waku_protocol::friends::TransferInfo {
            id: Uuid::new_v4(),
            direction: waku_protocol::friends::TransferDirection::Incoming,
            peer_id: "peer".into(),
            peer_name: "maya".into(),
            title: "design.pdf".into(),
            note: Some("here's the new mockups".into()),
            status: waku_protocol::friends::TransferStatus::Done,
            bytes_done: 12,
            bytes_total: 12,
            dest_dir: Some(share_dir.join("transfers/x")),
            session_id: None,
        };

        let session_id =
            create_transfer_session(&task_state, &store, &share_dir, &transfer).unwrap();

        {
            let state = task_state.lock();
            let project = state
                .projects
                .iter()
                .find(|project| project.path == share_dir)
                .expect("a Friends project at the share dir");
            assert_eq!(project.name, "Friends");
            let session = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .expect("the transfer's session");
            assert!(session.quarantined, "received files start untrusted");
            assert!(
                session.environment().is_sandbox(),
                "received files run in the sandbox VM once trusted"
            );
            assert_eq!(session.status, SessionStatus::Idle);
            assert_eq!(session.provider, ProviderKind::Grok);
            assert_eq!(session.model.as_deref(), Some("grok-code-fast-1"));
            assert!(!session.provider_locked());
            assert!(session.can_choose_model(ProviderKind::Claude));
            assert_eq!(session.project_id, project.id);
            assert_eq!(session.title, "design.pdf from maya");
            let receipt = &session.turns[0];
            assert_eq!(receipt.status, crate::model::TurnStatus::Completed);
            // Note and receipt are assistant messages — bot-style blocks,
            // not a sent bubble — with the note above the delivery details.
            assert!(
                session
                    .messages
                    .iter()
                    .all(|message| message.role == crate::model::MessageRole::Assistant)
            );
            let note_index = session
                .messages
                .iter()
                .position(|message| message.content == "here's the new mockups")
                .expect("the sender's note message");
            let receipt_index = session
                .messages
                .iter()
                .position(|message| message.content.contains("transfers/x"))
                .expect("the delivery receipt message");
            assert!(note_index < receipt_index, "the note lands above the file");
        }

        // A second transfer reuses the same Friends project.
        let second = waku_protocol::friends::TransferInfo {
            id: Uuid::new_v4(),
            dest_dir: Some(share_dir.join("transfers/y")),
            ..transfer.clone()
        };
        create_transfer_session(&task_state, &store, &share_dir, &second).unwrap();
        assert_eq!(
            task_state
                .lock()
                .projects
                .iter()
                .filter(|project| project.path == share_dir)
                .count(),
            1
        );

        // The flag survives a reload — quarantine isn't a runtime accident.
        // It lives in the session detail, so the list skeleton reads false
        // and hydrate restores the persisted value.
        let reload_store = StateStore::daemon(root.join("app.db"));
        let mut reloaded = reload_store.load().unwrap();
        let index = reloaded
            .sessions
            .iter()
            .position(|session| session.id == session_id)
            .unwrap();
        assert!(!reloaded.sessions[index].quarantined);
        assert!(!reloaded.sessions[index].environment().is_sandbox());
        reload_store.hydrate(&mut reloaded.sessions[index]).unwrap();
        assert!(reloaded.sessions[index].quarantined);
        assert!(reloaded.sessions[index].environment().is_sandbox());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn incoming_chat_materializes_a_session() {
        let root = std::env::temp_dir().join(format!("waku-chat-{}", Uuid::new_v4()));
        let share_dir = root.join("share");
        let store = Arc::new(StateStore::daemon(root.join("app.db")));
        let task_state = Arc::new(Mutex::new(PersistedState::fresh(root.join("repo"))));

        let session_id = create_chat_session(
            &task_state,
            &store,
            &share_dir,
            "maya",
            "shipping the update tonight — changelog attached",
        )
        .unwrap();

        let state = task_state.lock();
        let session = state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .expect("the message's session");
        assert_eq!(
            session.project_id,
            state
                .projects
                .iter()
                .find(|project| project.path == share_dir)
                .expect("a Friends project at the share dir")
                .id
        );
        assert_eq!(session.title, "Message from maya");
        assert_eq!(session.status, SessionStatus::Idle);
        // A message has nothing to quarantine or sandbox — it is just an
        // idle chat holding the friend's text as an agent reply.
        assert!(!session.quarantined);
        assert!(!session.sandboxed);
        assert_eq!(session.messages.len(), 1);
        assert_eq!(
            session.messages[0].role,
            crate::model::MessageRole::Assistant
        );
        assert_eq!(
            session.messages[0].content,
            "shipping the update tonight — changelog attached"
        );
        drop(state);

        // A second message reuses the same Friends project.
        create_chat_session(&task_state, &store, &share_dir, "maya", "and the pdf").unwrap();
        assert_eq!(
            task_state
                .lock()
                .projects
                .iter()
                .filter(|project| project.path == share_dir)
                .count(),
            1
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn removing_a_parent_removes_its_side_chats() {
        let root = std::env::temp_dir().join(format!("waku-side-chats-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        let parent_id = state.sessions[0].id;
        state.sessions[0].begin_turn("parent");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);

        let project_id = state.projects[0].id;
        let mut side_chat = AgentSession::new(project_id, ProviderKind::Codex);
        side_chat.side_chat_of = Some(parent_id);
        side_chat.begin_turn("side prompt");
        side_chat.finish_active_turn(crate::model::TurnStatus::Completed);
        let side_chat_id = side_chat.id;
        state.push_session(side_chat);
        // A sibling in the same project stays — the cascade follows
        // `side_chat_of`, not shared lineage.
        let mut sibling = AgentSession::new(project_id, ProviderKind::Codex);
        sibling.begin_turn("sibling");
        sibling.finish_active_turn(crate::model::TurnStatus::Completed);
        let sibling_id = sibling.id;
        state.push_session(sibling);
        store.save(&mut state).unwrap();

        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        backend.remove_session(parent_id).unwrap();
        let remaining = backend
            .task_state
            .lock()
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();
        assert_eq!(remaining, vec![sibling_id]);

        // The cascade is durable: the tombstone also keeps a stale client
        // save from resurrecting the child.
        let reloaded = StateStore::daemon(root.join("app.db")).load().unwrap();
        assert!(
            !reloaded
                .sessions
                .iter()
                .any(|session| session.id == side_chat_id)
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn removing_a_project_removes_its_tasks_and_blocks_stale_saves() {
        let root = std::env::temp_dir().join(format!("waku-remove-project-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        let project = state.projects[0].clone();
        state.sessions[0].begin_turn("remove me");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let parent = state.sessions[0].clone();
        let mut side_chat = AgentSession::new(project.id, ProviderKind::Codex);
        side_chat.side_chat_of = Some(parent.id);
        side_chat.begin_turn("side prompt");
        side_chat.finish_active_turn(crate::model::TurnStatus::Completed);
        state.push_session(side_chat);
        let other_project = Project::from_path(root.join("other"));
        let mut other_session = AgentSession::new(other_project.id, ProviderKind::Codex);
        other_session.begin_turn("keep me");
        other_session.finish_active_turn(crate::model::TurnStatus::Completed);
        state.projects.push(other_project.clone());
        state.push_session(other_session.clone());
        store.save(&mut state).unwrap();

        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        backend.remove_project(project.id).unwrap();
        {
            let state = backend.task_state.lock();
            assert_eq!(
                state
                    .projects
                    .iter()
                    .map(|project| project.id)
                    .collect::<Vec<_>>(),
                vec![other_project.id]
            );
            assert_eq!(
                state
                    .sessions
                    .iter()
                    .map(|session| session.id)
                    .collect::<Vec<_>>(),
                vec![other_session.id]
            );
        }

        let stale = backend
            .handle(
                waku_protocol::Request {
                    request_id: Uuid::new_v4(),
                    session_id: Uuid::nil(),
                    runtime_id: Uuid::nil(),
                    command: Command::SaveTaskState {
                        projects: vec![project.clone(), other_project.clone()],
                        live_session_ids: vec![parent.id, other_session.id],
                        sessions: vec![parent.clone(), other_session.clone()],
                    },
                },
                EventSink::detached(),
                None,
            )
            .unwrap();
        assert!(matches!(stale, ResponsePayload::TaskStateSaved { .. }));
        {
            let state = backend.task_state.lock();
            assert_eq!(
                state
                    .projects
                    .iter()
                    .map(|project| project.id)
                    .collect::<Vec<_>>(),
                vec![other_project.id]
            );
            assert_eq!(
                state
                    .sessions
                    .iter()
                    .map(|session| session.id)
                    .collect::<Vec<_>>(),
                vec![other_session.id]
            );
        }

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn response_fork_titles_follow_one_numbered_sequence() {
        assert_eq!(
            next_response_fork_title("Fix the bug", ["Fix the bug"]),
            "Fix the bug (2)"
        );
        assert_eq!(
            next_response_fork_title(
                "Fix the bug (2)",
                ["Fix the bug", "Fix the bug (2)", "Fix the bug (4)"]
            ),
            "Fix the bug (5)"
        );
        assert_eq!(
            next_response_fork_title("Plan (2026)", ["Plan (2026)"]),
            "Plan (2026) (2)"
        );
    }

    #[test]
    fn message_rewind_requires_a_settled_user_turn_and_provider_cursor() {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("change it");
        session.mark_active_turn_provider_started();
        session.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "thread".into(),
        });
        session.finish_active_turn(crate::model::TurnStatus::Completed);

        assert!(validate_message_rewind(&session, 1).is_ok());

        let mut busy = session.clone();
        busy.status = SessionStatus::Working;
        assert!(validate_message_rewind(&busy, 1).is_err());

        let mut missing_cursor = session.clone();
        missing_cursor.provider_cursor = None;
        assert!(validate_message_rewind(&missing_cursor, 1).is_err());

        let mut missing_message = session;
        missing_message.messages.clear();
        assert!(validate_message_rewind(&missing_message, 1).is_err());
    }

    #[test]
    fn wire_event_round_trip_preserves_ordered_delta_payload() {
        let wire = event_to_wire(DriverEvent::TextDelta("hello".into())).unwrap();
        assert_eq!(wire.kind, "textDelta");
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::TextDelta(text) if text == "hello"
        ));
    }

    #[test]
    fn wire_event_round_trip_preserves_prompt_submission_identity() {
        let turn_id = Uuid::new_v4();
        let message_id = Uuid::new_v4();
        let wire = event_to_wire(DriverEvent::PromptSubmitted {
            message: "ship it".into(),
            turn_id,
            message_id,
            sent_by_task: None,
            hidden: false,
        })
        .unwrap();
        assert_eq!(wire.kind, "promptSubmitted");
        assert_eq!(wire.payload["message"], "ship it");
        assert_eq!(wire.payload["turnId"], turn_id.to_string());
        assert_eq!(wire.payload["messageId"], message_id.to_string());
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::PromptSubmitted { message, turn_id: decoded_turn, message_id: decoded_message, .. }
                if message == "ship it" && decoded_turn == turn_id && decoded_message == message_id
        ));
    }

    #[test]
    fn wire_event_round_trip_preserves_agent_provenance() {
        let sender = Uuid::new_v4();
        let wire = event_to_wire(DriverEvent::PromptSubmitted {
            message: "from another task".into(),
            turn_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            sent_by_task: Some(sender),
            hidden: false,
        })
        .unwrap();
        assert_eq!(wire.payload["sentByTask"], sender.to_string());
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::PromptSubmitted { sent_by_task: Some(decoded), .. } if decoded == sender
        ));

        // An older daemon's payload lacks the field and still decodes.
        let wire = WireDriverEvent::new(
            "promptSubmitted",
            json!({
                "message": "old",
                "turnId": Uuid::new_v4(),
                "messageId": Uuid::new_v4(),
            }),
        );
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::PromptSubmitted {
                sent_by_task: None,
                ..
            }
        ));
        let wire = WireDriverEvent::new("steerAccepted", json!({ "message": "old" }));
        assert!(matches!(
            event_from_wire(wire).unwrap(),
            DriverEvent::SteerAccepted {
                sent_by_task: None,
                ..
            }
        ));
    }

    #[test]
    fn the_first_prompt_of_a_fresh_session_carries_the_project_map() {
        let root = std::env::temp_dir().join(format!("waku-repo-map-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let session_id = Uuid::new_v4();

        // What spawn_runtime records for a fresh session, plus a built index.
        {
            let mut maps = backend.repo_maps.0.lock();
            maps.sessions
                .insert(session_id, (root.clone(), Uuid::new_v4()));
            maps.pending.insert(session_id);
            maps.indexes.insert(
                root.clone(),
                crate::repo_map::RepoMapIndex::scan(&root).unwrap(),
            );
        }

        // The payload the driver receives leads with the map block; the
        // user's own text follows it untouched.
        let (prompt, status) = backend.inject_repo_map(session_id, "fix the bug".to_owned());
        assert!(prompt.starts_with("<project-map>"));
        assert!(prompt.contains("src/lib.rs:"));
        assert!(prompt.contains("pub fn entry() {}"));
        assert!(prompt.ends_with("</project-map>\n\nfix the bug"));
        assert!(matches!(
            status,
            Some(ProjectMapStatus::Sent {
                mapped_files: 1,
                indexed_files: 1,
                ..
            })
        ));

        // Consumed once — the next prompt goes out as typed, and a session
        // that was never registered is untouched too.
        let (follow_up, status) = backend.inject_repo_map(session_id, "follow up".to_owned());
        assert_eq!(follow_up, "follow up");
        assert!(status.is_none());
        assert_eq!(
            backend.inject_repo_map(Uuid::new_v4(), "hi".to_owned()),
            ("hi".to_owned(), None)
        );
    }

    /// Records the commands a session's driver receives — the steer path's
    /// only observable effect before the provider echoes.
    #[derive(Default)]
    struct CaptureDriver {
        prompts: Mutex<Vec<String>>,
        steers: Mutex<Vec<String>>,
        surface_delivery: crate::driver::AgentSurfaceDelivery,
    }

    impl crate::driver::DriverControl for CaptureDriver {
        fn prompt(&self, prompt: String) {
            self.prompts.lock().push(prompt);
        }
        fn supports_steer(&self) -> bool {
            true
        }
        fn agent_surface_delivery(&self) -> crate::driver::AgentSurfaceDelivery {
            self.surface_delivery
        }
        fn steer(&self, prompt: String) {
            self.steers.lock().push(prompt);
        }
        fn respond(&self, _request_id: String, _option_id: String) {}
        fn rollback(
            &self,
            _turns: usize,
        ) -> anyhow::Result<Option<waku_protocol::model::ProviderResumeCursor>> {
            Ok(None)
        }
        fn cancel(&self) {}
    }

    #[test]
    fn the_first_prompt_context_rides_a_hidden_steer() {
        let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn entry() {}\n").unwrap();
        let memory_store = repo.join(".goddard/memory");
        std::fs::create_dir_all(&memory_store).unwrap();
        std::fs::write(
            memory_store.join("MEMORY.md"),
            "The release freeze lands on Fridays.\n",
        )
        .unwrap();
        std::fs::write(memory_store.join("LOG.txt"), "one durable note\n").unwrap();

        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(repo.clone());
        state.sessions[0].begin_turn("seed");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let session_id = state.sessions[0].id;
        store.save(&mut state).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        // What spawn_runtime records for a fresh session, plus a built index.
        {
            let mut maps = backend.repo_maps.0.lock();
            maps.sessions
                .insert(session_id, (repo.clone(), Uuid::new_v4()));
            maps.pending.insert(session_id);
            maps.indexes.insert(
                repo.clone(),
                crate::repo_map::RepoMapIndex::scan(&repo).unwrap(),
            );
        }

        let capture = Arc::new(CaptureDriver::default());
        let driver = crate::driver::DriverHandle::from_control(capture.clone());
        backend.steer_first_prompt_context(
            session_id,
            "fix the bug",
            &driver,
            &EventSink::detached(),
        );

        // The clean prompt went out untouched; the context blocks follow in
        // one framed steer — map first, then memory.
        assert!(capture.prompts.lock().is_empty());
        let steers = capture.steers.lock().clone();
        assert_eq!(steers.len(), 1);
        let steer = &steers[0];
        assert!(steer.starts_with("Session context — background information only"));
        assert!(steer.find("<project-map>").unwrap() < steer.find("<project-memory>").unwrap());
        assert!(steer.contains("src/lib.rs:"));
        assert!(steer.contains("The release freeze lands on Fridays."));
        assert!(!steer.contains("fix the bug"));

        // The injection is unsettled until the provider echoes the steer;
        // accepting it marks the session's memory delivered.
        assert!(backend.agent.context_steer_pending(session_id));
        let taken = backend.agent.take_pending_steer(session_id, steer).unwrap();
        assert_eq!(taken.context, Some(crate::agent::ContextSteer::Memory));
        backend.memory.mark_injected(session_id);

        backend.steer_first_prompt_context(
            session_id,
            "follow up",
            &driver,
            &EventSink::detached(),
        );
        assert_eq!(capture.steers.lock().len(), 1, "nothing re-injects");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A session plus its side chat in the test store; returns
    /// (backend, session_id, side_chat_id). The default test settings keep
    /// `agent_tools_enabled` off, so reads exercise the scoped exemption.
    fn read_scope_test_backend(root: &Path) -> (WakuBackend, Uuid, Uuid) {
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(repo);
        state.sessions[0].begin_turn("seed");
        state.sessions[0].push_message(crate::model::MessageRole::Assistant, "seeded answer");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let session_id = state.sessions[0].id;
        let mut side = crate::model::AgentSession::new(
            state.sessions[0].project_id,
            crate::model::ProviderKind::Codex,
        );
        side.side_chat_of = Some(session_id);
        side.begin_turn("side question");
        side.finish_active_turn(crate::model::TurnStatus::Completed);
        let side_id = side.id;
        state.sessions.push(side);
        store.save(&mut state).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        (backend, session_id, side_id)
    }

    #[test]
    fn agent_rename_requires_own_task_grant_and_persists_title() {
        let root = std::env::temp_dir().join(format!("waku-agent-rename-{}", Uuid::new_v4()));
        let (backend, session_id, side_id) = read_scope_test_backend(&root);
        assert!(backend.agent_rename_self(None, "No caller").is_err());
        assert!(
            backend
                .agent_rename_self(Some(session_id), "Denied")
                .is_err()
        );
        {
            let mut state = backend.task_state.lock();
            state.session_mut(session_id).unwrap().agent_rename_allowed = true;
            backend.task_store.save(&mut state).unwrap();
        }
        assert!(backend.agent_rename_self(Some(session_id), " ").is_err());
        backend
            .agent_rename_self(Some(session_id), "  My title  ")
            .unwrap();
        let state = backend.task_state.lock();
        assert_eq!(
            state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .unwrap()
                .title,
            "My title"
        );
        assert_eq!(
            state
                .sessions
                .iter()
                .find(|session| session.id == side_id)
                .unwrap()
                .title,
            AgentSession::DEFAULT_TITLE
        );
        drop(state);
        let restored = StateStore::daemon(root.join("app.db")).load().unwrap();
        let own = restored
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .unwrap();
        assert_eq!(own.title, "My title");
        assert!(own.agent_rename_allowed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_scoped_read_reaches_self_and_a_side_chats_parent() {
        let root = std::env::temp_dir().join(format!("waku-read-scope-{}", Uuid::new_v4()));
        let (backend, session_id, side_id) = read_scope_test_backend(&root);

        // Self-read works with the task-tools flag off, addressed or bare.
        for (task_id, thread_id) in [(Some(session_id), None), (None, None)] {
            let read = backend
                .agent_read_session(Some(session_id), task_id, thread_id, None, None)
                .expect("a self read is always in scope");
            let ResponsePayload::AgentSessionTranscript { transcript } = read else {
                panic!("expected a transcript");
            };
            assert_eq!(transcript.task_id, session_id);
            assert_eq!(transcript.items.len(), 2);
        }

        // The side chat reads its parent; the reverse direction is not in
        // scope while the flag is off.
        assert!(
            backend
                .agent_read_session(Some(side_id), Some(session_id), None, None, None)
                .is_ok()
        );
        assert!(
            backend
                .agent_read_session(Some(session_id), Some(side_id), None, None, None)
                .is_err()
        );
        // A credential cannot read an unrelated task either.
        assert!(
            backend
                .agent_read_session(Some(session_id), Some(Uuid::new_v4()), None, None, None)
                .is_err()
        );

        // A turn filter returns that turn's entries; a missing turn is a
        // clean error, not an empty transcript.
        let read = backend
            .agent_read_session(Some(session_id), None, None, None, Some(1))
            .expect("turn 1 exists");
        let ResponsePayload::AgentSessionTranscript { transcript } = read else {
            panic!("expected a transcript");
        };
        assert_eq!(transcript.items.len(), 2);
        assert!(transcript.items.iter().all(|item| item.turn == Some(1)));
        let error = backend
            .agent_read_session(Some(session_id), None, None, None, Some(9))
            .unwrap_err();
        assert!(error.to_string().contains("has no turn 9"));

        let _ = std::fs::remove_dir_all(&root);
    }

    fn surface_test_backend(root: &Path) -> (WakuBackend, Uuid) {
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(repo);
        state.sessions[0].begin_turn("seed");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let session_id = state.sessions[0].id;
        store.save(&mut state).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        backend.agent.note_surface(
            session_id,
            crate::agent::AgentSurfaceScope {
                task_tools: true,
                settings_writes: true,
                parent_task_id: None,
            },
        );
        (backend, session_id)
    }

    #[test]
    fn the_agent_surface_instruction_rides_the_context_steer_once() {
        let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
        let (backend, session_id) = surface_test_backend(&root);
        let capture = Arc::new(CaptureDriver::default());
        let driver = crate::driver::DriverHandle::from_control(capture.clone());

        backend.steer_first_prompt_context(
            session_id,
            "create a task for this",
            &driver,
            &EventSink::detached(),
        );

        let steers = capture.steers.lock().clone();
        assert_eq!(steers.len(), 1);
        assert!(steers[0].contains("`goddard-agent`"));
        assert!(steers[0].contains("create, start, or spawn"));

        // The accepted echo marks the surface delivered; the next prompt
        // owes no block, so nothing steers at all.
        assert!(
            backend
                .agent
                .take_pending_steer(session_id, &steers[0])
                .is_some()
        );
        backend.agent.mark_surface_announced(session_id);
        backend.steer_first_prompt_context(
            session_id,
            "follow up",
            &driver,
            &EventSink::detached(),
        );
        assert_eq!(capture.steers.lock().len(), 1, "nothing re-injects");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_side_chats_parent_index_rides_the_context_steer_once() {
        let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
        let (backend, session_id, side_id) = read_scope_test_backend(&root);
        // The launch minted the side chat's env — the index names the read
        // surface the credential actually carries.
        backend.agent.note_surface(
            side_id,
            crate::agent::AgentSurfaceScope {
                task_tools: false,
                settings_writes: false,
                parent_task_id: Some(session_id),
            },
        );
        let capture = Arc::new(CaptureDriver::default());
        let driver = crate::driver::DriverHandle::from_control(capture.clone());

        backend.steer_first_prompt_context(
            side_id,
            "side question",
            &driver,
            &EventSink::detached(),
        );

        let steers = capture.steers.lock().clone();
        assert_eq!(steers.len(), 1);
        let steer = &steers[0];
        assert!(steer.contains("kind=\"index\""));
        // The parent's user text verbatim, its reply as a cue line, and the
        // read invocations pointed at the parent's task id.
        assert!(steer.contains("User: seed"));
        assert!(steer.contains("— Assistant: seeded answer"));
        assert!(steer.contains(&format!("\"task_id\":\"{session_id}\"")));
        assert!(steer.contains("\"turn\":N"));

        // An accepted carry settles the index — the next prompt's steer adds
        // nothing once every other block has also delivered.
        backend.agent.take_pending_steer(side_id, steer).unwrap();
        backend.agent.mark_parent_index_delivered(side_id);
        backend.agent.mark_surface_announced(side_id);
        backend.steer_first_prompt_context(side_id, "follow up", &driver, &EventSink::detached());
        assert_eq!(
            capture.steers.lock().len(),
            1,
            "the index does not re-steer"
        );

        // A session with no parent owes no index at all.
        assert!(backend.side_chat_parent_block(session_id).is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_parent_index_without_the_cli_names_no_read_surface() {
        let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
        let (backend, _session_id, side_id) = read_scope_test_backend(&root);
        // No surface was noted — a daemon that never minted the env — so the
        // index is plain context, not a pointer to a missing command.
        let block = backend.side_chat_parent_block(side_id).unwrap();
        assert!(block.contains("kind=\"index\""));
        assert!(block.contains("User: seed"));
        assert!(!block.contains("goddard-agent read"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_natively_announced_surface_stays_out_of_the_context_steer() {
        let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
        let (backend, session_id) = surface_test_backend(&root);
        let capture = Arc::new(CaptureDriver {
            surface_delivery: crate::driver::AgentSurfaceDelivery::Announced,
            ..Default::default()
        });
        let driver = crate::driver::DriverHandle::from_control(capture.clone());

        backend.steer_first_prompt_context(
            session_id,
            "create a task for this",
            &driver,
            &EventSink::detached(),
        );

        // The driver already told the session — no other block is owed, so
        // no steer goes out at all.
        assert!(capture.steers.lock().is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_agent_surface_prepends_once_for_non_steer_drivers() {
        let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
        let (backend, session_id) = surface_test_backend(&root);
        let capture = Arc::new(CaptureDriver::default());
        let driver = crate::driver::DriverHandle::from_control(capture.clone());

        let prompt = backend.prepend_agent_surface(session_id, &driver, "create a task".to_owned());
        assert!(prompt.starts_with("<goddard-agent>"));
        assert!(prompt.ends_with("create a task"));

        let prompt = backend.prepend_agent_surface(session_id, &driver, "next".to_owned());
        assert_eq!(prompt, "next");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_rejected_context_steer_leaves_the_session_eligible() {
        let root = std::env::temp_dir().join(format!("waku-context-steer-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let memory_store = repo.join(".goddard/memory");
        std::fs::create_dir_all(&memory_store).unwrap();
        std::fs::write(memory_store.join("MEMORY.md"), "one fact\n").unwrap();

        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(repo.clone());
        state.sessions[0].begin_turn("seed");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        let session_id = state.sessions[0].id;
        store.save(&mut state).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        let capture = Arc::new(CaptureDriver::default());
        let driver = crate::driver::DriverHandle::from_control(capture.clone());
        backend.steer_first_prompt_context(
            session_id,
            "first task",
            &driver,
            &EventSink::detached(),
        );
        assert_eq!(capture.steers.lock().len(), 1);

        // The steer missed the turn: the pending record drops but the
        // memory flag stays unset, so the next prompt injects again.
        let rejected = backend.agent.note_driver_event(
            session_id,
            &DriverEvent::SteerRejected {
                message: capture.steers.lock()[0].clone(),
                reason: "turn ended".into(),
                reason_i18n: None,
                hidden: false,
            },
        );
        assert!(rejected.is_some_and(|steer| steer.context.is_some()));
        assert!(!backend.agent.context_steer_pending(session_id));

        backend.steer_first_prompt_context(
            session_id,
            "next task",
            &driver,
            &EventSink::detached(),
        );
        assert_eq!(
            capture.steers.lock().len(),
            2,
            "the next prompt retries the context steer"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A driver that answers nothing; the forwarder only reaches for it
    /// while draining a queued prompt, which these tests never do.
    struct IdleDriver;

    impl driver::DriverControl for IdleDriver {
        fn prompt(&self, _prompt: String) {}
        fn cancel(&self) {}
        fn respond(&self, _request_id: String, _option_id: String) {}
        fn rollback(&self, _turns: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
            Ok(None)
        }
    }

    #[test]
    fn an_exited_runtime_releases_its_replay_journal() {
        let root = std::env::temp_dir().join(format!("waku-exit-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        // The sink a runtime-starting request carries: bound to the session
        // and registered with the hub so emitted events journal.
        let events = EventSink::detached().begin_session_runtime(session_id, runtime_id);
        let (wake, _wakes) = smol::channel::bounded(1);
        let (driver_events, event_receiver) = driver::event_channel(wake);
        let driver = DriverHandle::from_control(Arc::new(IdleDriver));
        let sessions = Arc::new(Mutex::new(HashMap::from([(
            session_id,
            RuntimeEntry {
                runtime_id,
                driver: driver.clone(),
                last_active: std::time::Instant::now(),
                resumable: true,
                provider: ProviderKind::Claude,
                cwd: PathBuf::new(),
            },
        )])));
        driver_events
            .send(DriverEvent::TextDelta("streamed".into()))
            .unwrap();
        driver_events.send(DriverEvent::ProcessExited).unwrap();

        let task_state = Arc::new(Mutex::new(PersistedState::empty()));
        let task_store = Arc::new(StateStore::daemon(root.join("app.db")));
        forward_driver_events(
            session_id,
            runtime_id,
            event_receiver,
            events.clone(),
            driver,
            Arc::new(crate::agent::AgentState::default()),
            task_state.clone(),
            task_store.clone(),
            sessions.clone(),
            Arc::new(AutomationService::open(root.join("automations.json")).unwrap()),
            Arc::new(AutoPromptService::open(root.join("auto-prompts.json")).unwrap()),
            crate::memory::MemoryService::new(
                Arc::new(DaemonSettingsStore::open(root.join("settings.json")).unwrap()),
                task_state,
                task_store,
            ),
            Arc::new((Mutex::new(RepoMaps::default()), Condvar::new())),
        );

        assert!(sessions.lock().is_empty());
        assert_eq!(events.journaled_event_count(session_id), 0);
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_task_sweeps_its_daemon_terminals() {
        let root = std::env::temp_dir().join(format!("waku-sweep-{}", Uuid::new_v4()));
        let project_dir = root.join("repo");
        let worktree_dir = root.join("worktree");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&worktree_dir).unwrap();
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(project_dir.clone());
        let project_id = state.projects[0].id;
        let local_id = state.sessions[0].id;
        // Blank sessions never reach the store; both fixtures need a turn.
        state.sessions[0].begin_turn("seed");
        state.sessions[0].finish_active_turn(TurnStatus::Completed);
        let mut worktree_session = state.new_session(project_id, ProviderKind::Codex);
        worktree_session.workspace = SessionWorkspace::Worktree {
            path: worktree_dir.clone(),
            name: "worktree".into(),
            branch: None,
            base_branch: None,
        };
        worktree_session.begin_turn("seed");
        worktree_session.finish_active_turn(TurnStatus::Completed);
        let worktree_id = worktree_session.id;
        state.push_session(worktree_session);
        store.save(&mut state).unwrap();

        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap()
        .with_terminal_shell(alacritty_terminal::tty::Shell::new(
            "/bin/sh".into(),
            vec!["-c".into(), "while IFS= read -r line; do :; done".into()],
        ));
        let open = |terminal_id, cwd: PathBuf, owner| {
            backend
                .handle(
                    Request {
                        request_id: Uuid::new_v4(),
                        session_id: terminal_id,
                        runtime_id: terminal_id,
                        command: Command::OpenTerminal {
                            cwd,
                            cols: 80,
                            rows: 24,
                            owner,
                        },
                    },
                    EventSink::detached(),
                    None,
                )
                .unwrap();
        };
        // One terminal owned by the local task at the shared project root,
        // one anonymous terminal inside the worktree, one outside both.
        let owned = Uuid::new_v4();
        let worktree_bound = Uuid::new_v4();
        let unrelated = Uuid::new_v4();
        open(owned, project_dir.clone(), Some(local_id));
        open(worktree_bound, worktree_dir.clone(), None);
        open(unrelated, root.clone(), None);
        assert_eq!(backend.terminals.lock().len(), 3);

        // Ownership sweeps even at the shared root, but the anonymous
        // worktree terminal survives — its workspace belongs to a live task.
        backend.remove_session(local_id).unwrap();
        assert!(!backend.terminals.lock().contains_key(&owned));
        assert!(backend.terminals.lock().contains_key(&worktree_bound));
        assert!(backend.terminals.lock().contains_key(&unrelated));

        // Removing the worktree task takes every terminal under its path,
        // tagged or not; the outside terminal is untouched.
        backend.remove_session(worktree_id).unwrap();
        assert!(!backend.terminals.lock().contains_key(&worktree_bound));
        assert!(backend.terminals.lock().contains_key(&unrelated));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_idle_reaper_only_takes_runtimes_that_can_come_back() {
        let root = std::env::temp_dir().join(format!("waku-reaper-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        let project_id = state.projects[0].id;
        let seed = |session: &mut AgentSession| {
            session.begin_turn("seed");
            session.mark_active_turn_provider_started();
            session.provider_cursor = Some(ProviderResumeCursor::Codex {
                thread_id: "thread".into(),
            });
            session.finish_active_turn(TurnStatus::Completed);
        };
        // Resumable and settled — the only runtime the reaper may take.
        let idle_id = state.sessions[0].id;
        seed(&mut state.sessions[0]);
        // Settled but holding no resume cursor — killing its provider would
        // lose the session context a restart cannot rebuild.
        let mut unresumable = state.new_session(project_id, ProviderKind::Codex);
        unresumable.begin_turn("seed");
        unresumable.finish_active_turn(TurnStatus::Completed);
        let unresumable_id = unresumable.id;
        state.push_session(unresumable);
        // Resumable but mid-turn.
        let mut working = state.new_session(project_id, ProviderKind::Codex);
        seed(&mut working);
        working.begin_turn("go");
        working.mark_active_turn_provider_started();
        working.status = SessionStatus::Working;
        let working_id = working.id;
        state.push_session(working);
        // Resumable and settled, with a prompt still parked for delivery.
        let mut queued = state.new_session(project_id, ProviderKind::Codex);
        seed(&mut queued);
        let queued_id = queued.id;
        state.push_session(queued);
        store.save(&mut state).unwrap();

        let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
        let mut doc = settings.get();
        doc.runtime_idle_timeout_secs = Some(60);
        settings.replace(doc).unwrap();
        let backend = WakuBackend::new(settings, store).unwrap();
        backend.agent.enqueue(
            queued_id,
            crate::agent::AgentPrompt {
                prompt: "parked".into(),
                transport: None,
                sender: None,
                queued_id: Some(Uuid::new_v4()),
                context: None,
            },
        );

        // A two-minute-old stamp clears the configured minute. Resumability
        // rides on the runtime entry — the catalog rows could be skeletons.
        let driver = DriverHandle::from_control(Arc::new(IdleDriver));
        let sessions = Arc::new(Mutex::new(HashMap::from_iter(
            [idle_id, unresumable_id, working_id, queued_id]
                .into_iter()
                .map(|id| {
                    (
                        id,
                        RuntimeEntry {
                            runtime_id: Uuid::new_v4(),
                            driver: driver.clone(),
                            last_active: std::time::Instant::now()
                                - std::time::Duration::from_secs(120),
                            resumable: id != unresumable_id,
                            provider: ProviderKind::Claude,
                            cwd: PathBuf::new(),
                        },
                    )
                }),
        )));

        let evicted = reap_idle_runtimes(
            &sessions,
            &backend.task_state,
            &backend.settings,
            &backend.agent,
        );
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, idle_id);
        let remaining = sessions.lock();
        assert!(!remaining.contains_key(&idle_id));
        assert!(remaining.contains_key(&unresumable_id));
        assert!(remaining.contains_key(&working_id));
        assert!(remaining.contains_key(&queued_id));
        drop(remaining);

        // A fresh stamp keeps even a resumable runtime.
        sessions.lock().insert(
            idle_id,
            RuntimeEntry {
                runtime_id: Uuid::new_v4(),
                driver,
                last_active: std::time::Instant::now(),
                resumable: true,
                provider: ProviderKind::Claude,
                cwd: PathBuf::new(),
            },
        );
        assert!(
            reap_idle_runtimes(
                &sessions,
                &backend.task_state,
                &backend.settings,
                &backend.agent
            )
            .is_empty()
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_agent_prompt_envelope_names_its_sending_task() {
        let mut state = PersistedState::empty();
        let target = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let target_id = target.id;
        let mut sender = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        sender.set_title("Fix the flaky test");
        let sender_id = sender.id;
        state.sessions.extend([target, sender]);
        let state = Mutex::new(state);

        // A task-to-task prompt names the sender and ends in the verbatim
        // prompt.
        let wrapped = agent_prompt_envelope(&state, target_id, Some(sender_id), "how is it going?")
            .expect("an attributed prompt wraps");
        assert!(wrapped.contains("the agent of another Goddard task"));
        assert!(wrapped.contains("\"Fix the flaky test\""));
        assert!(wrapped.contains(&sender_id.to_string()));
        assert!(wrapped.ends_with("\n\nhow is it going?"));

        // A side chat of the target gets the warmer relation.
        state
            .lock()
            .sessions
            .iter_mut()
            .find(|session| session.id == sender_id)
            .unwrap()
            .side_chat_of = Some(target_id);
        let wrapped = agent_prompt_envelope(&state, target_id, Some(sender_id), "hi")
            .expect("a side chat's prompt wraps");
        assert!(wrapped.contains("your side chat"));

        // Unattributed, self-addressed, and unknown senders.
        assert!(agent_prompt_envelope(&state, target_id, None, "hi").is_none());
        assert!(agent_prompt_envelope(&state, target_id, Some(target_id), "hi").is_none());
        let unknown = Uuid::new_v4();
        let wrapped = agent_prompt_envelope(&state, target_id, Some(unknown), "hi")
            .expect("a known sender id still wraps without its record");
        assert!(wrapped.contains(&unknown.to_string()));
        assert!(wrapped.contains("the agent of another Goddard task"));
    }

    #[test]
    fn agent_search_stays_inside_the_callers_project() {
        let root = std::env::temp_dir().join(format!("waku-agent-search-{}", Uuid::new_v4()));
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = PersistedState::fresh(root.join("repo"));
        // The caller's own task — its prompt does not carry the needle.
        let caller_id = state.sessions[0].id;
        let project_id = state.projects[0].id;
        let project_name = state.projects[0].name.clone();
        state.sessions[0].begin_turn("caller prompt");
        state.sessions[0].finish_active_turn(crate::model::TurnStatus::Completed);
        // A sibling in the same project carries it.
        let mut sibling = AgentSession::new(project_id, ProviderKind::Codex);
        sibling.begin_turn("the rare needle phrase");
        sibling.finish_active_turn(crate::model::TurnStatus::Completed);
        let sibling_id = sibling.id;
        state.push_session(sibling);
        // A task in another project matches the text but is out of scope.
        let other_project = Project::from_path(root.join("other"));
        let mut other = AgentSession::new(other_project.id, ProviderKind::Codex);
        other.begin_turn("the rare needle phrase");
        other.finish_active_turn(crate::model::TurnStatus::Completed);
        let other_id = other.id;
        state.projects.push(other_project.clone());
        state.push_session(other);
        store.save(&mut state).unwrap();

        let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
        let mut daemon_settings = settings.get();
        daemon_settings.agent_tools_enabled = true;
        settings.replace(daemon_settings).unwrap();
        let backend = WakuBackend::new(settings, store).unwrap();

        let hits = |query: &str, agent: Uuid| match backend
            .agent_search_sessions(Some(agent), Uuid::nil(), query, None)
            .unwrap()
        {
            ResponsePayload::AgentSessionSearch { hits } => hits,
            other => panic!("unexpected payload {other:?}"),
        };

        // The needle only surfaces the sibling, never the foreign project.
        assert_eq!(
            hits("rare needle", caller_id)
                .iter()
                .map(|hit| hit.task_id)
                .collect::<Vec<_>>(),
            vec![sibling_id]
        );
        // The same query scoped to the other project's task finds its own
        // sibling instead — the confinement follows the caller.
        assert_eq!(
            hits("rare needle", other_id)
                .iter()
                .map(|hit| hit.task_id)
                .collect::<Vec<_>>(),
            vec![other_id]
        );
        // Naming the caller's own project is accepted; naming a foreign
        // one is an error, not a silent empty result.
        assert_eq!(
            hits(&format!("project:{project_name} rare needle"), caller_id)
                .iter()
                .map(|hit| hit.task_id)
                .collect::<Vec<_>>(),
            vec![sibling_id]
        );
        assert!(
            backend
                .agent_search_sessions(
                    Some(caller_id),
                    Uuid::nil(),
                    "project:other rare needle",
                    None
                )
                .is_err()
        );
        // `status:` intersects the project allowlist: every task here is
        // idle, so `busy` finds nothing and an idle-only query lists both.
        assert!(hits("status:busy rare needle", caller_id).is_empty());
        let mut listed = hits("status:idle", caller_id)
            .iter()
            .map(|hit| hit.task_id)
            .collect::<Vec<_>>();
        listed.sort();
        let mut expected = vec![caller_id, sibling_id];
        expected.sort();
        assert_eq!(listed, expected);
        // An anonymous request has nothing to scope to.
        assert!(
            backend
                .agent_search_sessions(None, Uuid::nil(), "rare needle", None)
                .is_err()
        );

        std::fs::remove_dir_all(root).ok();
    }
}
