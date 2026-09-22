//! Daemon-owned friend-to-friend sharing. Owns the `waku-share` iroh
//! endpoint on a dedicated tokio thread; the threaded daemon talks to it
//! over crossbeam channels and every state change broadcasts a whole
//! `FriendsState` document to subscribers (the `FriendsChanged` message).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, bail};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use uuid::Uuid;

use waku_protocol::friends::{
    FriendInfo, FriendRequestInfo, FriendSyncAlertAction, FriendsState, IncomingShareInfo,
    SharedProjectInfo, SharedSessionSummary, SyncAlertInfo, SyncAlertKind, SyncLinkInfo,
    TransferDirection, TransferInfo, TransferStatus,
};
use waku_protocol::git::SyncInProgress;
use waku_protocol::model::AgentSession;
use waku_protocol::{ReplayCursor, SequencedEvent, ServerMessage};
use waku_share::friends::{
    self, Friend, FriendStore, FriendsMessage, FriendsProtocol, OfferInfo, PendingRequest,
    RequestDecision, SessionFeed,
};
use waku_share::projects::{
    IncomingShare, Integration, OutgoingShare, ShareStore, SharedRepo, SyncLink, normalize_origin,
};
use waku_share::{EndpointId, RelayMode, ShareNode, TempTag};

/// How long a probe verdict stays fresh before a surface should re-dial.
const PROBE_CACHE_MS: u64 = 30_000;
/// A probe gives the dial this long before declaring the peer unreachable.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Daemon→runtime command timeout. Transfers can legitimately take longer —
/// those complete asynchronously and the command returns after the offer is
/// accepted, not after the bytes land.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// Bound on dialing a friend and delivering an offer — iroh's internal
/// retries can otherwise pin the command loop far past the caller's wait.
const OFFER_TIMEOUT: Duration = Duration::from_secs(60);
/// Bound on waiting for a relay address before minting a ticket — the send
/// must still proceed for LAN/discovery-only peers when no relay answers.
const ONLINE_WAIT: Duration = Duration::from_secs(10);
/// Bound on the receiver's done receipt — it is courtesy bookkeeping, not
/// part of the verified download.
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(30);
/// An outgoing transfer's only completion signal is the receiver's
/// TransferDone receipt; past this it is declared stalled. A late receipt
/// still flips the row back to Done.
const TRANSFER_STALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Local ref poll cadence — detects commits landing on synced branches
/// (auto-push) and manual pushes completing (push notices). Pure local
/// reads, so it can stay quick.
const SYNC_POLL_INTERVAL: Duration = Duration::from_secs(15);
/// Backstop fetch cadence — catches pushes the notice path missed
/// (friend's other machines, other collaborators) and retries unacked
/// `SyncEnabled` handshakes.
const SYNC_FETCH_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// The daemon's view of a local project for share matching — its path,
/// display name, and `origin` fetch URL.
#[derive(Clone)]
pub struct RepoInfo {
    pub path: PathBuf,
    pub name: String,
    pub origin_url: Option<String>,
}

/// Resolves the local projects the share layer can match incoming
/// shares against and offer for sharing. Installed by the daemon, which
/// owns the project list; called only on the worker thread.
pub type RepoResolver = Arc<dyn Fn() -> Vec<RepoInfo> + Send + Sync>;

/// Work for the share worker thread — every job is blocking git or
/// store bookkeeping, serialized so sync never races itself on a repo.
enum SyncJob {
    /// Detect new commits on synced branches (auto-push) and completed
    /// manual pushes (notices); reconcile stale alerts.
    Poll,
    /// Fetch + integrate every enabled branch of one link — on creation
    /// and on the slow backstop cadence (`None` → all links).
    Fetch { link_id: Option<String> },
    /// A friend's push notice: fetch + integrate the named branches.
    Integrate {
        link_id: String,
        branches: Vec<String>,
    },
    /// Manual sync: unpause the branch, fetch, integrate.
    SyncNow { link_id: String, branch: String },
    /// Alert decisions that touch git state. Dismiss is store-level.
    AlertAction {
        alert_id: String,
        merge_instead: bool,
    },
    /// Create the local side of a link (enable-sync opt-in or the
    /// SyncEnabled handshake) — resolves the default branch on the
    /// worker, not the command path.
    CreateLink {
        peer: EndpointId,
        origin_url: String,
        repo_path: PathBuf,
        peer_initiated: bool,
        reply: Sender<anyhow::Result<String>>,
    },
    /// A friend's `SyncEnabled`/`SyncDisabled` arrived — mark flags,
    /// create or tear down the link.
    PeerSyncState {
        peer: EndpointId,
        origin_url: String,
        enabled: bool,
    },
    /// Share a project with a friend — resolves name/origin on the
    /// worker, then re-sends them the full set.
    ShareProject {
        peer: EndpointId,
        project_path: PathBuf,
        reply: Sender<anyhow::Result<()>>,
    },
    /// Stop sharing an origin with a friend — re-sends the set, then
    /// tears the link down when no incoming share justifies it.
    UnshareProject {
        peer: EndpointId,
        origin_url: String,
        reply: Sender<anyhow::Result<()>>,
    },
    /// Tear down one link — user disable (`notify` sends `SyncDisabled`)
    /// or the peer's own message (`notify` false).
    TearDownLink { link_id: String, notify: bool },
    /// A friend's ref notice: fetch the moved refs into every checkout we
    /// can resolve for this origin, then bump review surfaces.
    RefNotice {
        origin_url: String,
        refs: Vec<String>,
    },
    /// A friend's full shared set arrived — reconcile incoming shares
    /// and the links they fed.
    UpdateIncoming {
        peer: EndpointId,
        projects: Vec<SharedRepo>,
    },
    /// Local branches + default for the link config UI.
    QueryBranches {
        link_id: String,
        reply: Sender<anyhow::Result<(Vec<String>, Option<String>)>>,
    },
    /// Flip `share_sessions` on an outgoing share — re-sends the shared
    /// set so the friend's incoming row picks up the flag.
    SetSessionSharing {
        peer: EndpointId,
        origin_url: String,
        enabled: bool,
        reply: Sender<anyhow::Result<()>>,
    },
}

/// Peer messages the worker wants sent — it owns no async context, so
/// the runtime drains these and dials.
enum RuntimeEffect {
    ShareProjects {
        peer: EndpointId,
        projects: Vec<SharedRepo>,
    },
    SyncState {
        peer: EndpointId,
        origin_url: String,
        enabled: bool,
    },
    PushNotice {
        peer: EndpointId,
        origin_url: String,
        branches: Vec<String>,
    },
}

/// Installed by the server so async share events (incoming request, offer,
/// progress) reach every subscribed client.
pub type FriendsSink = Arc<dyn Fn(FriendsState) + Send + Sync>;

/// Fired when an incoming transfer finishes and its files are on disk.
/// The daemon creates the transfer's agent session from this hook and
/// returns its id, which is recorded on the transfer for clients.
pub type TransferHook = Arc<dyn Fn(&TransferInfo) -> Option<Uuid> + Send + Sync>;

/// Fired when an incoming chat message arrives — `(peer name, text)`.
/// The daemon creates the message's agent session from this hook and
/// returns its id, recorded on the row like a transfer's.
pub type ChatHook = Arc<dyn Fn(String, String) -> Option<Uuid> + Send + Sync>;

/// Fired when the share layer changed session/project state — the hub
/// translates it into a `TaskStateChanged` bump for every client.
pub type TaskNotifier = Arc<dyn Fn() + Send + Sync>;

/// Fired when `refs/notes/qa` (or a promoted base branch) moved for an
/// origin — locally or on a friend's machine. The hub broadcasts a
/// `ReviewChanged` with the origin URL so review surfaces re-read.
pub type ReviewNotifier = Arc<dyn Fn(String) + Send + Sync>;

/// The daemon's session catalog as the share layer needs it — installed
/// by `WakuBackend` like `RepoResolver`. Called on the share runtime and
/// worker threads only.
#[derive(Clone)]
pub struct SessionSource {
    /// Sessions belonging to the project at `repo_path`, slimmed to the
    /// rows a friend's session list renders. Archived sessions and side
    /// chats stay hidden, matching task-list semantics.
    pub list: Arc<dyn Fn(&std::path::Path) -> Vec<SharedSessionSummary> + Send + Sync>,
    /// Full snapshot for a subscription's first frame.
    pub snapshot: Arc<dyn Fn(Uuid) -> Option<AgentSession> + Send + Sync>,
}

/// A hub-provided live event stream for one session — what the share
/// layer pumps onto a friend's `SessionSubscribe` stream.
pub type SessionStreamer =
    Arc<dyn Fn(Uuid, Option<ReplayCursor>) -> crate::server::SessionStream + Send + Sync>;

/// What a peer subscription delivers back to this daemon — the hub sink
/// in `serve` turns these into client broadcasts.
pub enum FriendSessionUpdate {
    Event(SequencedEvent),
    /// The stream ended: `revoked` when the friend turned sharing off or
    /// unshared the project, `false` for disconnects and gone sessions.
    Closed {
        session_id: Uuid,
        revoked: bool,
    },
}

/// Installed by the server so peer session events reach subscribed
/// clients, like `FriendsSink` for the friends document.
pub type FriendSessionSink = Arc<dyn Fn(FriendSessionUpdate) + Send + Sync>;

/// An open `SessionSubscribe` stream we serve to a peer — the worker
/// revokes by pushing `SharingRevoked` then dropping the sender.
struct SessionFeedHandle {
    origin_url: String,
    sender: tokio::sync::mpsc::Sender<FriendsMessage>,
}

enum ShareCommand {
    SendRequest {
        code: String,
        name: String,
        reply: Sender<anyhow::Result<()>>,
    },
    Respond {
        node_id: String,
        accept: bool,
        reply: Sender<anyhow::Result<()>>,
    },
    SendFile {
        node_id: String,
        path: PathBuf,
        note: Option<String>,
        reply: Sender<anyhow::Result<()>>,
    },
    SendChat {
        node_id: String,
        text: String,
        reply: Sender<anyhow::Result<()>>,
    },
    CancelTransfer {
        id: Uuid,
        reply: Sender<anyhow::Result<()>>,
    },
    Probe {
        node_id: String,
        reply: Sender<anyhow::Result<()>>,
    },
    ShareProject {
        node_id: String,
        project_path: PathBuf,
        reply: Sender<anyhow::Result<()>>,
    },
    UnshareProject {
        node_id: String,
        origin_url: String,
        reply: Sender<anyhow::Result<()>>,
    },
    EnableSync {
        node_id: String,
        origin_url: String,
        reply: Sender<anyhow::Result<()>>,
    },
    DisableSync {
        link_id: String,
        reply: Sender<anyhow::Result<()>>,
    },
    SetSyncConfig {
        link_id: String,
        auto_push: bool,
        enabled_branches: Vec<String>,
        reply: Sender<anyhow::Result<()>>,
    },
    SyncNow {
        link_id: String,
        branch: String,
        reply: Sender<anyhow::Result<()>>,
    },
    AlertAction {
        alert_id: String,
        action: FriendSyncAlertAction,
        reply: Sender<anyhow::Result<()>>,
    },
    GetSyncBranches {
        link_id: String,
        reply: Sender<anyhow::Result<(Vec<String>, Option<String>)>>,
    },
    /// Toggle session sharing on a shared project — the worker owns the
    /// flag and the re-send.
    SetSessionSharing {
        node_id: String,
        origin_url: String,
        enabled: bool,
        reply: Sender<anyhow::Result<()>>,
    },
    /// List the sessions a friend exposes on their shared project.
    FriendSessions {
        node_id: String,
        origin_url: String,
        reply: Sender<anyhow::Result<Vec<SharedSessionSummary>>>,
    },
    /// Open a live, read-only tail of a friend's session — the reply
    /// carries the snapshot; events then arrive on the session sink.
    WatchFriendSession {
        node_id: String,
        origin_url: String,
        session_id: Uuid,
        reply: Sender<anyhow::Result<AgentSession>>,
    },
    /// Close a watch's pump task.
    UnwatchFriendSession {
        session_id: Uuid,
        reply: Sender<anyhow::Result<()>>,
    },
    /// A workspace op landed commits or moved refs — poll promptly
    /// instead of waiting out the interval.
    KickPoll,
    /// A workspace op pushed branches on a shared origin — send push
    /// notices to every friend sharing it.
    NotifyPush {
        origin_url: String,
        branches: Vec<String>,
    },
    /// A workspace op moved non-branch refs on a shared origin — send
    /// ref notices to every friend sharing it.
    NotifyRefs {
        origin_url: String,
        refs: Vec<String>,
    },
    Shutdown,
}

/// Mutable state shared between the command loop and the protocol
/// callbacks; every mutation re-publishes the wire snapshot.
struct ShareInner {
    store: Arc<Mutex<FriendStore>>,
    /// Shared repos, sync links, and pending alerts — `share.json`.
    share_store: Arc<Mutex<ShareStore>>,
    /// The daemon's project list for origin matching, installed once.
    resolver: Option<RepoResolver>,
    transfers: Vec<TransferInfo>,
    /// node id string → (last probe verdict, when taken). Presence is lazy.
    probes: HashMap<String, (bool, u64)>,
    /// Friend-request decisions the UI hasn't answered yet.
    pending: HashMap<String, tokio::sync::oneshot::Sender<RequestDecision>>,
    /// ticket string → outgoing transfer id, so `TransferDone` can match.
    outgoing_tickets: HashMap<String, Uuid>,
    /// transfer id → temp tag keeping the offered blob pinned against GC
    /// for the transfer's lifetime.
    outgoing_tags: HashMap<Uuid, TempTag>,
    transfer_hook: Option<TransferHook>,
    chat_hook: Option<ChatHook>,
    task_notifier: Option<TaskNotifier>,
    review_notifier: Option<ReviewNotifier>,
    /// Session summaries/snapshots for friends we share with.
    session_source: Option<SessionSource>,
    /// Per-session event streams from the hub — the serving side of a
    /// friend's subscription.
    session_streamer: Option<SessionStreamer>,
    /// Where inbound friend-session events get published — installed by
    /// the server once the hub exists.
    session_sink: Option<FriendSessionSink>,
    /// Open session feeds we serve to peers, keyed `(peer, session)`.
    session_feeds: HashMap<(EndpointId, Uuid), SessionFeedHandle>,
    /// Daemon metadata + pairing handlers for the `waku-link` ALPN. The
    /// daemon installs both once; the runtime turns them into a
    /// `LinkProtocol` at spawn.
    link_info: Option<waku_share::link::InfoHandler>,
    link_pair: Option<waku_share::link::PairHandler>,
    friend_code: String,
}

impl ShareInner {
    fn snapshot(&self) -> FriendsState {
        let store = self.store.lock();
        let share_store = self.share_store.lock();
        FriendsState {
            friend_code: self.friend_code.clone(),
            display_name: store.display_name.clone(),
            friends: store
                .friends
                .values()
                .map(|f| {
                    let id = f.node_id.to_string();
                    let online = self.probes.get(&id).is_some_and(|(ok, at)| {
                        *ok && now_ms().saturating_sub(*at) < PROBE_CACHE_MS
                    });
                    FriendInfo {
                        node_id: id,
                        name: f.name.clone(),
                        nickname: f.nickname.clone(),
                        last_seen_ms: f.last_seen_ms,
                        online,
                    }
                })
                .collect(),
            incoming_requests: store
                .requests
                .iter()
                .filter(|r| r.incoming)
                .map(request_info)
                .collect(),
            outgoing_requests: store
                .requests
                .iter()
                .filter(|r| !r.incoming)
                .map(request_info)
                .collect(),
            transfers: self.transfers.clone(),
            shared_projects: share_store
                .outgoing
                .iter()
                .map(|share| SharedProjectInfo {
                    peer_id: share.peer.to_string(),
                    peer_name: store.resolved_name(&share.peer, ""),
                    project_name: share.name.clone(),
                    origin_url: share.origin_url.clone(),
                    repo_path: share.repo_path.clone(),
                    peer_sync_enabled: share.peer_sync_enabled,
                    share_sessions: share.share_sessions,
                    shared_at_ms: share.shared_at_ms,
                })
                .collect(),
            incoming_shares: share_store
                .incoming
                .iter()
                .map(|share| IncomingShareInfo {
                    peer_id: share.peer.to_string(),
                    peer_name: store.resolved_name(&share.peer, &share.peer_name),
                    project_name: share.name.clone(),
                    origin_url: share.origin_url.clone(),
                    matched_path: share.matched_path.clone(),
                    matched_project_name: share.matched_name.clone(),
                    sync_enabled: share_store
                        .link_for(&share.peer, &share.origin_url)
                        .is_some(),
                    share_sessions: share.share_sessions,
                    received_at_ms: share.received_at_ms,
                })
                .collect(),
            sync_links: share_store
                .links
                .iter()
                .map(|link| SyncLinkInfo {
                    id: link.id.clone(),
                    peer_id: link.peer.to_string(),
                    peer_name: store.resolved_name(&link.peer, ""),
                    origin_url: link.origin_url.clone(),
                    repo_path: link.repo_path.clone(),
                    auto_push: link.auto_push,
                    enabled_branches: link.enabled_branches.iter().cloned().collect(),
                    paused_branches: link.paused_branches.iter().cloned().collect(),
                    peer_sync_enabled: link.peer_sync_enabled,
                    created_at_ms: link.created_at_ms,
                })
                .collect(),
            sync_alerts: share_store
                .alerts
                .iter()
                .filter_map(|alert| {
                    let link = share_store.links.iter().find(|l| l.id == alert.link_id)?;
                    Some(SyncAlertInfo {
                        id: alert.id.clone(),
                        link_id: alert.link_id.clone(),
                        peer_name: store.resolved_name(&link.peer, ""),
                        branch: alert.branch.clone(),
                        repo_path: link.repo_path.clone(),
                        kind: match alert.kind {
                            waku_share::projects::SyncAlertKind::Conflict => {
                                SyncAlertKind::Conflict
                            }
                            waku_share::projects::SyncAlertKind::RefusedDirtyWorktree => {
                                SyncAlertKind::RefusedDirtyWorktree
                            }
                        },
                        in_progress: alert.in_progress.map(|i| match i {
                            Integration::Rebase => SyncInProgress::Rebase,
                            Integration::Merge => SyncInProgress::Merge,
                        }),
                        files: alert.files.clone(),
                        worktree_path: alert.worktree_path.clone(),
                        temp_worktree: alert.temp_worktree,
                        at_ms: alert.at_ms,
                    })
                })
                .collect(),
        }
    }
}

fn request_info(r: &PendingRequest) -> FriendRequestInfo {
    FriendRequestInfo {
        node_id: r.node_id.to_string(),
        name: r.name.clone(),
        at_ms: r.at_ms,
    }
}

/// Sync facade the threaded daemon talks to.
pub struct ShareService {
    dir: PathBuf,
    our_name: String,
    cmd: Mutex<Option<Sender<ShareCommand>>>,
    state: Arc<Mutex<ShareInner>>,
    sink: Arc<Mutex<Option<FriendsSink>>>,
}

impl ShareService {
    /// Does not bind sockets — the runtime starts lazily on the first
    /// command so tests and headless runs pay nothing.
    pub fn new(dir: PathBuf, our_name: String) -> Self {
        // Load eagerly so store-level commands (nicknames, display name)
        // work before the endpoint has ever started, and so pre-runtime
        // mutations land in the file the runtime will reload.
        let mut store = FriendStore::load(&dir).unwrap_or_default();
        if store.display_name.is_empty() {
            store.display_name = our_name.clone();
        }
        Self {
            dir: dir.clone(),
            our_name,
            cmd: Mutex::new(None),
            state: Arc::new(Mutex::new(ShareInner {
                store: Arc::new(Mutex::new(store)),
                share_store: Arc::new(Mutex::new(ShareStore::load(&dir).unwrap_or_default())),
                resolver: None,
                transfers: Vec::new(),
                probes: HashMap::new(),
                pending: HashMap::new(),
                outgoing_tickets: HashMap::new(),
                outgoing_tags: HashMap::new(),
                transfer_hook: None,
                chat_hook: None,
                task_notifier: None,
                review_notifier: None,
                session_source: None,
                session_streamer: None,
                session_sink: None,
                session_feeds: HashMap::new(),
                link_info: None,
                link_pair: None,
                friend_code: String::new(),
            })),
            sink: Arc::new(Mutex::new(None)),
        }
    }

    /// Where the server installs the `FriendsChanged` broadcast.
    pub fn set_sink(&self, sink: FriendsSink) {
        *self.sink.lock() = Some(sink);
    }

    /// Where the daemon installs session creation for finished incoming
    /// transfers.
    pub fn set_transfer_hook(&self, hook: TransferHook) {
        self.state.lock().transfer_hook = Some(hook);
    }

    /// Where the daemon installs session creation for incoming chat
    /// messages.
    pub fn set_chat_hook(&self, hook: ChatHook) {
        self.state.lock().chat_hook = Some(hook);
    }

    /// Where the server installs the `TaskStateChanged` bump.
    pub fn set_task_notifier(&self, notifier: TaskNotifier) {
        self.state.lock().task_notifier = Some(notifier);
    }

    /// Where the server installs the `ReviewChanged` broadcast.
    pub fn set_review_notifier(&self, notifier: ReviewNotifier) {
        self.state.lock().review_notifier = Some(notifier);
    }

    /// Where the daemon installs the session catalog friends may list and
    /// watch — installed once at construction like `set_repo_resolver`.
    pub fn set_session_source(&self, source: SessionSource) {
        self.state.lock().session_source = Some(source);
    }

    /// Where the server installs hub-backed event streams for the serving
    /// side of friend subscriptions.
    pub fn set_session_streamer(&self, streamer: SessionStreamer) {
        self.state.lock().session_streamer = Some(streamer);
    }

    /// Where the server installs the broadcast path for events arriving
    /// from peer session subscriptions.
    pub fn set_friend_session_sink(&self, sink: FriendSessionSink) {
        self.state.lock().session_sink = Some(sink);
    }

    /// Where the daemon installs the `waku-link` handlers — what metadata
    /// LAN-discovered daemons report and how their pair requests resolve.
    /// Installed before the runtime starts; a runtime already running
    /// keeps whatever it spawned with.
    pub fn set_link_handlers(
        &self,
        info: waku_share::link::InfoHandler,
        pair: waku_share::link::PairHandler,
    ) {
        let mut inner = self.state.lock();
        inner.link_info = Some(info);
        inner.link_pair = Some(pair);
    }

    /// Latest wire snapshot for `GetFriends`. Cheap — no runtime required,
    /// though the friend code is resolved lazily from the identity file so
    /// it displays before the endpoint has ever started.
    pub fn state(&self) -> FriendsState {
        let mut inner = self.state.lock();
        if inner.friend_code.is_empty() {
            if let Ok(secret) = waku_share::identity::load_or_create(&self.dir) {
                inner.friend_code = waku_share::identity::friend_code(secret.public());
            }
        }
        inner.snapshot()
    }

    /// Begin binding the endpoint without blocking the caller on
    /// readiness — surfaces that make this install reachable (opening
    /// Friends, connecting a client) kick this so requests and offers
    /// can arrive, while their own reply returns immediately.
    pub fn kickstart(self: &Arc<Self>) {
        let this = self.clone();
        std::thread::spawn(move || {
            let _ = this.ensure_started();
        });
    }

    fn ensure_started(&self) -> anyhow::Result<Sender<ShareCommand>> {
        let mut guard = self.cmd.lock();
        if let Some(tx) = guard.as_ref() {
            return Ok(tx.clone());
        }
        let dir = self.dir.clone();
        let our_name = self.our_name.clone();
        let state = self.state.clone();
        let sink = self.sink.clone();
        let (ready_tx, ready_rx) = bounded(1);
        let (cmd_tx, cmd_rx) = unbounded();
        std::thread::Builder::new()
            .name("goddard-share".into())
            .spawn(move || {
                if let Err(e) = run_runtime(dir, our_name, state, sink, cmd_rx, ready_tx) {
                    eprintln!("goddard share runtime failed: {e:#}");
                }
            })?;
        // The runtime signals readiness (or failure) before serving commands.
        match ready_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(result) => result?,
            Err(_) => bail!("share runtime did not start"),
        }
        *guard = Some(cmd_tx.clone());
        Ok(cmd_tx)
    }

    fn call(
        &self,
        mk: impl FnOnce(Sender<anyhow::Result<()>>) -> ShareCommand,
    ) -> anyhow::Result<()> {
        let tx = self.ensure_started()?;
        let (reply_tx, reply_rx) = bounded(1);
        tx.send(mk(reply_tx))?;
        reply_rx.recv_timeout(COMMAND_TIMEOUT)??;
        Ok(())
    }

    // -- daemon command handlers -------------------------------------------

    pub fn send_friend_request(&self, code: String, name: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::SendRequest { code, name, reply })
    }

    pub fn respond_friend_request(&self, node_id: String, accept: bool) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::Respond {
            node_id,
            accept,
            reply,
        })
    }

    /// Drop a pending outgoing request. Store mutation only, so it works
    /// without the endpoint and before the runtime has ever started.
    pub fn withdraw_friend_request(&self, node_id: String) -> anyhow::Result<()> {
        let id = node_id.parse::<EndpointId>()?;
        let inner = self.state.lock();
        let mut store = inner.store.lock();
        let before = store.requests.len();
        store.requests.retain(|r| !(r.node_id == id && !r.incoming));
        if store.requests.len() == before {
            anyhow::bail!("no outgoing request to that code");
        }
        let _ = store.save();
        drop(store);
        drop(inner);
        publish(&self.state, &self.sink);
        Ok(())
    }

    /// Store-level mutation like `withdraw_friend_request` — no endpoint
    /// needed. A blank name resets to the default the service was built
    /// with.
    pub fn set_display_name(&self, name: String) -> anyhow::Result<()> {
        {
            let inner = self.state.lock();
            let mut store = inner.store.lock();
            store.display_name = name.trim().to_string();
            if store.display_name.is_empty() {
                store.display_name = self.our_name.clone();
            }
            let _ = store.save();
        }
        publish(&self.state, &self.sink);
        Ok(())
    }

    /// Store a local nickname override for a friend and re-point any
    /// transfer links that carried the old name.
    pub fn set_friend_nickname(
        &self,
        node_id: String,
        nickname: Option<String>,
    ) -> anyhow::Result<()> {
        let id = node_id.parse::<EndpointId>()?;
        {
            let inner = self.state.lock();
            let mut store = inner.store.lock();
            if !store.friends.contains_key(&id) {
                anyhow::bail!("no friend with that code");
            }
            store.set_nickname(&id, nickname);
            let _ = store.save();
        }
        publish(&self.state, &self.sink);
        relink_peer_transfers(&self.state, &id);
        Ok(())
    }

    /// Store-level mutation like `withdraw_friend_request` — no endpoint
    /// needed, so it must not queue behind in-flight probes or sends.
    pub fn remove_friend(&self, node_id: String) -> anyhow::Result<()> {
        let id = node_id.parse::<EndpointId>()?;
        {
            let inner = self.state.lock();
            let mut store = inner.store.lock();
            if store.friends.remove(&id).is_none() {
                anyhow::bail!("no friend with that code");
            }
            let _ = store.save();
        }
        // Their session feeds die with the friendship.
        revoke_session_feeds(&self.state, &id, None);
        publish(&self.state, &self.sink);
        Ok(())
    }

    pub fn send_file(
        &self,
        node_id: String,
        path: PathBuf,
        note: Option<String>,
    ) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::SendFile {
            node_id,
            path,
            note,
            reply,
        })
    }

    pub fn send_chat(&self, node_id: String, text: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::SendChat {
            node_id,
            text,
            reply,
        })
    }

    pub fn cancel_transfer(&self, id: Uuid) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::CancelTransfer { id, reply })
    }

    pub fn probe_friend(&self, node_id: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::Probe { node_id, reply })
    }

    /// Where the daemon installs the project list used for origin
    /// matching and share offers.
    pub fn set_repo_resolver(&self, resolver: RepoResolver) {
        self.state.lock().resolver = Some(resolver);
    }

    pub fn share_project(&self, node_id: String, project_path: PathBuf) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::ShareProject {
            node_id,
            project_path,
            reply,
        })
    }

    pub fn unshare_project(&self, node_id: String, origin_url: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::UnshareProject {
            node_id,
            origin_url,
            reply,
        })
    }

    pub fn enable_sync(&self, node_id: String, origin_url: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::EnableSync {
            node_id,
            origin_url,
            reply,
        })
    }

    pub fn disable_sync(&self, link_id: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::DisableSync { link_id, reply })
    }

    pub fn set_sync_config(
        &self,
        link_id: String,
        auto_push: bool,
        enabled_branches: Vec<String>,
    ) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::SetSyncConfig {
            link_id,
            auto_push,
            enabled_branches,
            reply,
        })
    }

    pub fn sync_now(&self, link_id: String, branch: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::SyncNow {
            link_id,
            branch,
            reply,
        })
    }

    pub fn sync_alert_action(
        &self,
        alert_id: String,
        action: FriendSyncAlertAction,
    ) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::AlertAction {
            alert_id,
            action,
            reply,
        })
    }

    pub fn get_sync_branches(
        &self,
        link_id: String,
    ) -> anyhow::Result<(Vec<String>, Option<String>)> {
        let tx = self.ensure_started()?;
        let (reply_tx, reply_rx) = bounded(1);
        tx.send(ShareCommand::GetSyncBranches {
            link_id,
            reply: reply_tx,
        })?;
        reply_rx.recv_timeout(COMMAND_TIMEOUT)?
    }

    /// Toggle whether `node_id` may watch sessions in the shared project
    /// `origin_url`. Independent of sync — a friend can watch sessions
    /// without syncing the repo.
    pub fn set_session_sharing(
        &self,
        node_id: String,
        origin_url: String,
        enabled: bool,
    ) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::SetSessionSharing {
            node_id,
            origin_url,
            enabled,
            reply,
        })
    }

    /// The sessions a friend exposes on their shared project — dials the
    /// friend over the friends channel.
    pub fn friend_sessions(
        &self,
        node_id: String,
        origin_url: String,
    ) -> anyhow::Result<Vec<SharedSessionSummary>> {
        let tx = self.ensure_started()?;
        let (reply_tx, reply_rx) = bounded(1);
        tx.send(ShareCommand::FriendSessions {
            node_id,
            origin_url,
            reply: reply_tx,
        })?;
        reply_rx.recv_timeout(COMMAND_TIMEOUT)?
    }

    /// Open a live read-only tail of a friend's session. The returned
    /// snapshot is the current state; updates arrive on the session sink
    /// as `ServerMessage::Event`s under the friend's session/runtime ids.
    pub fn watch_friend_session(
        &self,
        node_id: String,
        origin_url: String,
        session_id: Uuid,
    ) -> anyhow::Result<AgentSession> {
        let tx = self.ensure_started()?;
        let (reply_tx, reply_rx) = bounded(1);
        tx.send(ShareCommand::WatchFriendSession {
            node_id,
            origin_url,
            session_id,
            reply: reply_tx,
        })?;
        reply_rx.recv_timeout(COMMAND_TIMEOUT)?
    }

    /// Stop watching a friend's session — closes the peer subscription.
    pub fn unwatch_friend_session(&self, session_id: Uuid) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::UnwatchFriendSession { session_id, reply })
    }

    /// A workspace op landed commits or moved refs — prompt the poll
    /// instead of waiting out the interval. No-op before the runtime
    /// starts.
    pub fn note_repo_activity(&self) {
        if let Some(tx) = self.cmd.lock().as_ref() {
            let _ = tx.send(ShareCommand::KickPoll);
        }
    }

    /// A workspace op pushed `branches` on a shared origin — tell every
    /// friend sharing it. No-op until the endpoint is running; offline
    /// peers catch up on their next fetch.
    pub fn notify_push(&self, origin_url: String, branches: Vec<String>) {
        if let Some(tx) = self.cmd.lock().as_ref() {
            let _ = tx.send(ShareCommand::NotifyPush {
                origin_url,
                branches,
            });
        }
    }

    /// A workspace op moved `refs` on a shared origin — tell every friend
    /// sharing it. Same laziness as `notify_push`.
    pub fn notify_refs(&self, origin_url: String, refs: Vec<String>) {
        if let Some(tx) = self.cmd.lock().as_ref() {
            let _ = tx.send(ShareCommand::NotifyRefs { origin_url, refs });
        }
    }

    /// A local review action moved state — bump review surfaces through
    /// the installed notifier.
    pub fn review_changed(&self, origin_url: String) {
        if let Some(notifier) = self.state.lock().review_notifier.clone() {
            notifier(origin_url);
        }
    }

    pub fn shutdown(&self) {
        if let Some(tx) = self.cmd.lock().take() {
            let _ = tx.send(ShareCommand::Shutdown);
        }
    }
}

/// Friends who share `origin_url` — through a sync link or a project
/// share either direction. Notices go to all of them: a reviewer who
/// never enabled sync still wants their queue fresh.
fn peers_for_origin(store: &ShareStore, origin_url: &str) -> Vec<EndpointId> {
    let key = normalize_origin(origin_url);
    let mut peers = std::collections::BTreeSet::new();
    for link in &store.links {
        if normalize_origin(&link.origin_url) == key {
            peers.insert(link.peer);
        }
    }
    for share in &store.incoming {
        if normalize_origin(&share.origin_url) == key {
            peers.insert(share.peer);
        }
    }
    for share in &store.outgoing {
        if normalize_origin(&share.origin_url) == key {
            peers.insert(share.peer);
        }
    }
    peers.into_iter().collect()
}

/// The authorization check both session handlers share: `peer` must hold
/// an outgoing share on `origin_url` with `share_sessions` on. Returns
/// the share's local checkout and the session catalog.
fn shared_repo_for_sessions(
    state: &Arc<Mutex<ShareInner>>,
    peer: &EndpointId,
    origin_url: &str,
) -> Option<(PathBuf, SessionSource)> {
    let inner = state.lock();
    let share_store = inner.share_store.lock();
    let share = share_store.outgoing.iter().find(|s| {
        s.peer == *peer
            && s.share_sessions
            && normalize_origin(&s.origin_url) == normalize_origin(origin_url)
    })?;
    let repo_path = share.repo_path.clone();
    let source = inner.session_source.clone()?;
    Some((repo_path, source))
}

/// Close session feeds we serve to `peer` — `None` origin revokes all of
/// them (friend removed), `Some` just that project's (unshare or the
/// session-sharing toggle going off). `SharingRevoked` lands as the
/// terminal frame when the feed's queue has room; either way dropping the
/// sender ends the stream.
fn revoke_session_feeds(
    state: &Arc<Mutex<ShareInner>>,
    peer: &EndpointId,
    origin_url: Option<&str>,
) {
    let mut inner = state.lock();
    let normalized = origin_url.map(normalize_origin);
    let keys: Vec<(EndpointId, Uuid)> = inner
        .session_feeds
        .iter()
        .filter(|((p, _), feed)| {
            *p == *peer
                && normalized
                    .as_ref()
                    .is_none_or(|n| normalize_origin(&feed.origin_url) == *n)
        })
        .map(|(key, _)| *key)
        .collect();
    for key in keys {
        if let Some(feed) = inner.session_feeds.remove(&key) {
            let _ = feed.sender.try_send(FriendsMessage::SharingRevoked);
        }
    }
}

/// Publish the current snapshot to every subscriber.
fn publish(state: &Arc<Mutex<ShareInner>>, sink: &Arc<Mutex<Option<FriendsSink>>>) {
    let snapshot = state.lock().snapshot();
    if let Some(sink) = sink.lock().as_ref() {
        sink(snapshot);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Transfer rows title a chat message by its first non-blank line — the
/// full text rides the row's `note` and lands in the materialized session.
fn chat_title(text: &str) -> String {
    const MAX: usize = 60;
    let first = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("message");
    if first.chars().count() > MAX {
        let mut title: String = first.chars().take(MAX).collect();
        title.push('…');
        title
    } else {
        first.to_string()
    }
}

/// `~/Documents/Goddard/From Friends` — the human-readable mirror of the
/// real `transfers/<uuid>` folders. Links are cosmetic: the canonical path
/// stays on the transfer, a missing or stale link costs nothing.
fn friends_links_dir() -> Option<PathBuf> {
    dirs::document_dir().map(|d| d.join("Goddard").join("From Friends"))
}

/// Make a wire-supplied name safe as a single path component: no
/// separators, control characters, leading dots, or unbounded length.
fn sanitize_link_component(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c == '/' || c.is_control() { '-' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end();
    let cleaned: String = cleaned.chars().take(60).collect();
    let cleaned = cleaned.trim_end();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

/// Ensure `base` has a `<peer>-<title>` symlink to the received file —
/// `dest_dir` is the per-transfer `transfers/<uuid>` folder, but the link
/// should open the payload itself. Falls back to the folder when the
/// title doesn't resolve to an entry inside it. Suffixes the transfer id
/// on a name collision and removes stale links left by an older name.
fn sync_transfer_link(base: &std::path::Path, peer: &str, transfer: &TransferInfo) {
    let Some(dest_dir) = &transfer.dest_dir else {
        return;
    };
    let payload = dest_dir.join(&transfer.title);
    let dest = if payload.symlink_metadata().is_ok() {
        payload
    } else {
        dest_dir.clone()
    };
    let peer = sanitize_link_component(peer, "friend");
    let title = sanitize_link_component(&transfer.title, "files");
    let mut name = format!("{peer}-{title}");
    // A link is "ours" when it points at the payload or its containing
    // folder — links written before the link target moved inside
    // `dest_dir` still name the folder.
    let ours = |target: &std::path::Path| target == dest || target == *dest_dir;
    // True when `name` is already taken by something that isn't this
    // transfer's destination — a real file or a link to another folder.
    let occupied = |name: &str| {
        let path = base.join(name);
        match std::fs::read_link(&path) {
            Ok(target) => !ours(&target),
            Err(_) => path.symlink_metadata().is_ok(),
        }
    };
    if occupied(&name) {
        let short: String = transfer.id.simple().to_string().chars().take(4).collect();
        name = format!("{peer}-{title}-{short}");
        if occupied(&name) {
            return;
        }
    }
    // Drop stale links pointing at this destination under other names —
    // a rename leaves the old label behind otherwise.
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name() == name.as_str() {
                continue;
            }
            if std::fs::read_link(&path).is_ok_and(|t| ours(&t)) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let link = base.join(&name);
    match std::fs::read_link(&link) {
        Ok(target) if target == dest => return,
        // Re-point links that still name the folder (or anything stale).
        Ok(_) => {
            let _ = std::fs::remove_file(&link);
        }
        Err(_) => {}
    }
    let _ = crate::fs_ext::symlink(&dest, &link);
}

/// The name this install renders for `peer_id`: nickname, else the
/// friend's self-reported name, else the transfer's recorded name.
fn resolved_peer_name(inner: &ShareInner, peer_id: &str, fallback: &str) -> String {
    let store = inner.store.lock();
    peer_id
        .parse::<EndpointId>()
        .map(|id| store.resolved_name(&id, fallback))
        .unwrap_or_else(|_| {
            if fallback.is_empty() {
                "friend".to_string()
            } else {
                fallback.to_string()
            }
        })
}

/// Publish a finished incoming transfer into `~/Documents/Goddard/From
/// Friends` so the files are findable by name, not just by UUID.
fn link_transfer(state: &Arc<Mutex<ShareInner>>, transfer: &TransferInfo) {
    let Some(base) = friends_links_dir() else {
        return;
    };
    if std::fs::create_dir_all(&base).is_err() {
        return;
    }
    let peer = {
        let inner = state.lock();
        resolved_peer_name(&inner, &transfer.peer_id, &transfer.peer_name)
    };
    sync_transfer_link(&base, &peer, transfer);
}

/// Re-point this friend's transfer links after a nickname change. Only
/// runs when the links directory already exists — no side effects for
/// users who never received a file.
fn relink_peer_transfers(state: &Arc<Mutex<ShareInner>>, peer: &EndpointId) {
    let Some(base) = friends_links_dir() else {
        return;
    };
    if std::fs::read_dir(&base).is_err() {
        return;
    }
    let inner = state.lock();
    let peer_id = peer.to_string();
    let peer_name = resolved_peer_name(&inner, &peer_id, "");
    for transfer in inner.transfers.iter().filter(|t| {
        t.direction == TransferDirection::Incoming
            && t.status == TransferStatus::Done
            && t.dest_dir.is_some()
            && t.peer_id == peer_id
    }) {
        sync_transfer_link(&base, &peer_name, transfer);
    }
}

/// The sync worker's context — everything a job needs that isn't the
/// job itself. One worker serializes all share git so syncs never race
/// each other on the same repository.
struct SyncWorker {
    state: Arc<Mutex<ShareInner>>,
    share_store: Arc<Mutex<ShareStore>>,
    sink: Arc<Mutex<Option<FriendsSink>>>,
    /// Temp worktrees for branches checked out nowhere — `dir/sync/`.
    scratch_dir: PathBuf,
    effect_tx: tokio::sync::mpsc::UnboundedSender<RuntimeEffect>,
    /// link id → branch → remembered tips, the manual-push detector.
    branch_states: HashMap<String, HashMap<String, crate::sync::BranchTips>>,
}

impl SyncWorker {
    fn publish(&self) {
        publish(&self.state, &self.sink);
    }

    fn store(&self) -> parking_lot::MutexGuard<'_, ShareStore> {
        self.share_store.lock()
    }

    /// Persist the share store after a mutation, then broadcast.
    fn save_and_publish(&self) {
        let _ = self.store().save();
        self.publish();
    }

    /// The peer's shared set changed — replace it, re-match origins
    /// against our projects, and drop links whose shares went away.
    fn update_incoming(&self, peer: EndpointId, projects: Vec<SharedRepo>) {
        let resolver = self.state.lock().resolver.clone();
        let repos = resolver.map(|r| r()).unwrap_or_default();
        let peer_name = self.state.lock().store.lock().resolved_name(&peer, "");
        let dead = {
            let mut store = self.store();
            store.incoming.retain(|s| s.peer != peer);
            for repo in projects {
                let normalized = normalize_origin(&repo.origin_url);
                let matched = repos.iter().find(|r| {
                    r.origin_url
                        .as_deref()
                        .is_some_and(|url| normalize_origin(url) == normalized)
                });
                store.incoming.push(IncomingShare {
                    peer,
                    peer_name: peer_name.clone(),
                    name: repo.name,
                    origin_url: repo.origin_url,
                    matched_path: matched.map(|r| r.path.clone()),
                    matched_name: matched.map(|r| r.name.clone()),
                    share_sessions: repo.share_sessions,
                    received_at_ms: now_ms(),
                });
            }
            // Links whose justifying share vanished come down. A link is
            // justified by either direction — our outgoing share to the
            // peer keeps it alive even when their share of the same repo
            // is revoked.
            let live: std::collections::BTreeSet<String> = store
                .incoming
                .iter()
                .filter(|s| s.peer == peer)
                .map(|s| normalize_origin(&s.origin_url))
                .collect();
            let dead: Vec<String> = store
                .links
                .iter()
                .filter(|l| {
                    l.peer == peer
                        && !live.contains(&normalize_origin(&l.origin_url))
                        && !store.outgoing.iter().any(|s| {
                            s.peer == peer
                                && normalize_origin(&s.origin_url)
                                    == normalize_origin(&l.origin_url)
                        })
                })
                .map(|l| l.id.clone())
                .collect();
            let _ = store.save();
            dead
        };
        for link_id in dead {
            // Notify tears their side down too — a revoked share ends
            // sync both ways.
            self.tear_down_link(&link_id, true);
        }
        self.publish();
    }

    /// The full shared-repo set we advertise to `peer`.
    fn shared_set(&self, peer: &EndpointId) -> Vec<SharedRepo> {
        self.store()
            .outgoing
            .iter()
            .filter(|s| s.peer == *peer)
            .map(|s| SharedRepo {
                name: s.name.clone(),
                origin_url: s.origin_url.clone(),
                share_sessions: s.share_sessions,
            })
            .collect()
    }

    fn share_project(
        &self,
        peer: EndpointId,
        project_path: PathBuf,
        reply: Sender<anyhow::Result<()>>,
    ) {
        let result = (|| {
            let resolver = self.state.lock().resolver.clone();
            let repo = resolver
                .and_then(|r| r().into_iter().find(|repo| repo.path == project_path))
                .context("no project at that path")?;
            let origin_url = repo.origin_url.context("project has no origin remote")?;
            {
                let mut store = self.store();
                if store.outgoing.iter().any(|s| {
                    s.peer == peer
                        && normalize_origin(&s.origin_url) == normalize_origin(&origin_url)
                }) {
                    anyhow::bail!("already shared with them");
                }
                store.outgoing.push(OutgoingShare {
                    peer,
                    name: repo.name,
                    origin_url,
                    repo_path: project_path,
                    peer_sync_enabled: false,
                    share_sessions: false,
                    shared_at_ms: now_ms(),
                });
                let _ = store.save();
            }
            Ok(())
        })();
        let ok = result.is_ok();
        let _ = reply.send(result);
        if ok {
            self.publish();
            let projects = self.shared_set(&peer);
            let _ = self
                .effect_tx
                .send(RuntimeEffect::ShareProjects { peer, projects });
        }
    }

    fn unshare_project(
        &self,
        peer: EndpointId,
        origin_url: String,
        reply: Sender<anyhow::Result<()>>,
    ) {
        let result = (|| {
            {
                let mut store = self.store();
                let before = store.outgoing.len();
                store.outgoing.retain(|s| {
                    !(s.peer == peer
                        && normalize_origin(&s.origin_url) == normalize_origin(&origin_url))
                });
                if store.outgoing.len() == before {
                    anyhow::bail!("not shared with them");
                }
                let _ = store.save();
            }
            Ok(())
        })();
        let ok = result.is_ok();
        let _ = reply.send(result);
        if !ok {
            return;
        }
        self.publish();
        self.revoke_session_feeds_for(&peer, &origin_url);
        let projects = self.shared_set(&peer);
        let _ = self
            .effect_tx
            .send(RuntimeEffect::ShareProjects { peer, projects });
        // The link survives only while an incoming share justifies it.
        let justified = self.store().incoming.iter().any(|s| {
            s.peer == peer && normalize_origin(&s.origin_url) == normalize_origin(&origin_url)
        });
        if !justified {
            let link_id = self
                .store()
                .link_for(&peer, &origin_url)
                .map(|l| l.id.clone());
            if let Some(link_id) = link_id {
                self.tear_down_link(&link_id, true);
            }
        }
    }

    fn revoke_session_feeds_for(&self, peer: &EndpointId, origin_url: &str) {
        revoke_session_feeds(&self.state, peer, Some(origin_url));
    }

    /// Toggle session sharing on an outgoing share. Independent of sync
    /// — the flag rides the shared set like `peer_sync_enabled` does.
    fn set_session_sharing(
        &self,
        peer: EndpointId,
        origin_url: String,
        enabled: bool,
        reply: Sender<anyhow::Result<()>>,
    ) {
        let result = (|| {
            let mut store = self.store();
            let Some(share) = store.outgoing.iter_mut().find(|s| {
                s.peer == peer && normalize_origin(&s.origin_url) == normalize_origin(&origin_url)
            }) else {
                anyhow::bail!("not shared with them");
            };
            share.share_sessions = enabled;
            let _ = store.save();
            Ok(())
        })();
        let ok = result.is_ok();
        let _ = reply.send(result);
        if !ok {
            return;
        }
        self.publish();
        if !enabled {
            self.revoke_session_feeds_for(&peer, &origin_url);
        }
        let projects = self.shared_set(&peer);
        let _ = self
            .effect_tx
            .send(RuntimeEffect::ShareProjects { peer, projects });
    }

    /// Fetch + integrate a link's branches, recording alerts on
    /// conflict/refusal and clearing them on success.
    fn integrate_link(&self, link_id: &str, branches: &[String], fetch_first: bool) {
        let link = self.store().links.iter().find(|l| l.id == link_id).cloned();
        let Some(link) = link else { return };
        if fetch_first && let Err(error) = crate::sync::fetch(&link.repo_path) {
            eprintln!(
                "share sync: fetch {} failed: {error:#}",
                link.repo_path.display()
            );
            return;
        }
        let mut changed = false;
        for branch in branches {
            if !link.enabled_branches.contains(branch) || link.paused_branches.contains(branch) {
                continue;
            }
            match crate::sync::integrate(&link.repo_path, &self.scratch_dir, &link.id, branch) {
                Ok(crate::sync::IntegrateOutcome::Conflict {
                    in_progress,
                    files,
                    worktree,
                    temp_worktree,
                }) => {
                    let mut store = self.store();
                    store
                        .alerts
                        .retain(|a| !(a.link_id == link.id && a.branch == *branch));
                    store.alerts.push(crate::sync::conflict_alert(
                        &link,
                        branch,
                        in_progress,
                        files,
                        worktree,
                        temp_worktree,
                    ));
                    changed = true;
                }
                Ok(crate::sync::IntegrateOutcome::RefusedDirty { worktree }) => {
                    let mut store = self.store();
                    if !store
                        .alerts
                        .iter()
                        .any(|a| a.link_id == link.id && a.branch == *branch)
                    {
                        store
                            .alerts
                            .push(crate::sync::refused_alert(&link, branch, worktree));
                        changed = true;
                    }
                }
                Ok(
                    crate::sync::IntegrateOutcome::FastForwarded
                    | crate::sync::IntegrateOutcome::Integrated
                    | crate::sync::IntegrateOutcome::UpToDate
                    | crate::sync::IntegrateOutcome::AheadOnly,
                ) => {
                    let mut store = self.store();
                    let before = store.alerts.len();
                    // Success clears a refused alert; a conflict alert is
                    // the reconcile pass's call — the stopped integration
                    // may still be on disk.
                    store.alerts.retain(|a| {
                        !(a.link_id == link.id
                            && a.branch == *branch
                            && a.kind == waku_share::projects::SyncAlertKind::RefusedDirtyWorktree)
                    });
                    changed |= store.alerts.len() != before;
                }
                Ok(crate::sync::IntegrateOutcome::Busy) => {}
                Err(error) => {
                    eprintln!(
                        "share sync: integrate {} {branch} failed: {error:#}",
                        link.repo_path.display()
                    );
                }
            }
        }
        if changed {
            self.save_and_publish();
        }
    }

    /// Reconcile persisted alerts against the repos — drops alerts
    /// resolved elsewhere and pauses branches aborted elsewhere.
    fn reconcile_alerts(&self) {
        let alerts = self.store().alerts.clone();
        let mut changed = false;
        for alert in alerts {
            let link = self
                .store()
                .links
                .iter()
                .find(|l| l.id == alert.link_id)
                .cloned();
            let Some(link) = link else {
                self.store().alerts.retain(|a| a.id != alert.id);
                changed = true;
                continue;
            };
            match crate::sync::reconcile_alert(&link.repo_path, &alert) {
                Ok(Some(paused)) => {
                    {
                        let mut store = self.store();
                        store.alerts.retain(|a| a.id != alert.id);
                        if paused
                            && let Some(l) = store.links.iter_mut().find(|l| l.id == alert.link_id)
                        {
                            l.paused_branches.insert(alert.branch.clone());
                        }
                        let _ = store.save();
                    }
                    // Whatever way the stop ended, the temp worktree we
                    // owned goes — abort is a no-op once nothing is in
                    // progress and drops the registration either way.
                    if alert.temp_worktree
                        && let Err(error) =
                            crate::sync::abort(&link.repo_path, &alert.worktree_path, true)
                    {
                        eprintln!("share sync: temp cleanup failed: {error:#}");
                    }
                    changed = true;
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("share sync: reconcile {} failed: {error:#}", alert.branch);
                }
            }
        }
        if changed {
            self.save_and_publish();
        }
    }

    /// Create the local side of a link. `peer_initiated` marks the
    /// handshake receiver — its `peer_sync_enabled` starts true and its
    /// ack goes back over the wire.
    fn create_link(
        &mut self,
        peer: EndpointId,
        origin_url: String,
        repo_path: PathBuf,
        peer_initiated: bool,
        reply: Option<Sender<anyhow::Result<String>>>,
    ) {
        let mut created_id = None;
        let result = (|| {
            if let Some(id) = self
                .store()
                .link_for(&peer, &origin_url)
                .map(|l| l.id.clone())
            {
                return Ok(id);
            }
            let default = crate::sync::default_branch(&repo_path)?;
            let mut enabled = std::collections::BTreeSet::new();
            if let Some(branch) = default {
                enabled.insert(branch);
            }
            let link = SyncLink {
                id: Uuid::new_v4().to_string(),
                peer,
                origin_url: origin_url.clone(),
                repo_path,
                auto_push: true,
                enabled_branches: enabled,
                paused_branches: Default::default(),
                peer_sync_enabled: peer_initiated,
                created_at_ms: now_ms(),
            };
            let id = link.id.clone();
            {
                let mut store = self.store();
                store.links.push(link);
                if peer_initiated
                    && let Some(share) = store.outgoing.iter_mut().find(|s| {
                        s.peer == peer
                            && normalize_origin(&s.origin_url) == normalize_origin(&origin_url)
                    })
                {
                    share.peer_sync_enabled = true;
                }
                let _ = store.save();
            }
            created_id = Some(id.clone());
            Ok(id)
        })();
        let ok = result.is_ok();
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
        if ok {
            self.publish();
            // Both creation paths send `SyncEnabled`: manual enable opens
            // the handshake, peer-initiated creation answers it.
            let _ = self.effect_tx.send(RuntimeEffect::SyncState {
                peer,
                origin_url: origin_url.clone(),
                enabled: true,
            });
        }
        if let Some(id) = created_id {
            // The new side catches up immediately — pushes that landed
            // before the link existed are already on origin.
            self.integrate_link(&id, &self.enabled_branches(&id), true);
        }
    }

    fn enabled_branches(&self, link_id: &str) -> Vec<String> {
        self.store()
            .links
            .iter()
            .find(|l| l.id == link_id)
            .map(|l| l.enabled_branches.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Drop a link and its alerts, aborting any temp worktrees the
    /// alerts still own. `notify` sends `SyncDisabled` — false when the
    /// teardown itself came from the peer's `SyncDisabled`.
    fn tear_down_link(&self, link_id: &str, notify: bool) {
        let link = {
            let mut store = self.store();
            let Some(index) = store.links.iter().position(|l| l.id == link_id) else {
                return;
            };
            let link = store.links.remove(index);
            // A stopped integration owns a real checkout state — abort
            // and clean temp worktrees before forgetting the alert.
            let alerts: Vec<waku_share::projects::SyncAlert> = store
                .alerts
                .iter()
                .filter(|a| a.link_id == link_id)
                .cloned()
                .collect();
            store.alerts.retain(|a| a.link_id != link_id);
            if let Some(share) = store.outgoing.iter_mut().find(|s| {
                s.peer == link.peer
                    && normalize_origin(&s.origin_url) == normalize_origin(&link.origin_url)
            }) {
                share.peer_sync_enabled = false;
            }
            let _ = store.save();
            (link, alerts)
        };
        for alert in link.1 {
            if let Err(error) =
                crate::sync::abort(&link.0.repo_path, &alert.worktree_path, alert.temp_worktree)
            {
                eprintln!("share sync: cleanup {} failed: {error:#}", alert.branch);
            }
        }
        let link = link.0;
        self.publish();
        if notify {
            let _ = self.effect_tx.send(RuntimeEffect::SyncState {
                peer: link.peer,
                origin_url: link.origin_url,
                enabled: false,
            });
        }
    }

    fn run(&mut self, job_rx: Receiver<SyncJob>) {
        while let Ok(job) = job_rx.recv() {
            match job {
                SyncJob::Poll => {
                    let links = self.store().links.clone();
                    let mut notices: Vec<(EndpointId, String, Vec<String>)> = Vec::new();
                    for link in &links {
                        let states = self.branch_states.entry(link.id.clone()).or_default();
                        match crate::sync::poll(&link.repo_path, link, states) {
                            Ok(report) => {
                                let branches: Vec<String> = report
                                    .pushed
                                    .iter()
                                    .chain(report.noticed.iter())
                                    .cloned()
                                    .collect();
                                if !branches.is_empty() {
                                    notices.push((link.peer, link.origin_url.clone(), branches));
                                }
                            }
                            Err(error) => eprintln!(
                                "share sync: poll {} failed: {error:#}",
                                link.repo_path.display()
                            ),
                        }
                    }
                    for (peer, origin_url, branches) in notices {
                        let _ = self.effect_tx.send(RuntimeEffect::PushNotice {
                            peer,
                            origin_url,
                            branches,
                        });
                    }
                    self.reconcile_alerts();
                }
                SyncJob::Fetch { link_id } => {
                    let links = self.store().links.clone();
                    // Slow cadence also re-sends unacked SyncEnabled
                    // handshakes — an offline peer learns the link when
                    // it comes back.
                    for link in &links {
                        if !link.peer_sync_enabled {
                            let _ = self.effect_tx.send(RuntimeEffect::SyncState {
                                peer: link.peer,
                                origin_url: link.origin_url.clone(),
                                enabled: true,
                            });
                        }
                    }
                    for link in links
                        .iter()
                        .filter(|l| link_id.as_ref().is_none_or(|id| &l.id == id))
                    {
                        let branches: Vec<String> = link.enabled_branches.iter().cloned().collect();
                        self.integrate_link(&link.id, &branches, true);
                    }
                }
                SyncJob::Integrate { link_id, branches } => {
                    self.integrate_link(&link_id, &branches, true);
                }
                SyncJob::SyncNow { link_id, branch } => {
                    {
                        let mut store = self.store();
                        if let Some(link) = store.links.iter_mut().find(|l| l.id == link_id) {
                            link.paused_branches.remove(&branch);
                            let _ = store.save();
                        }
                    }
                    self.publish();
                    self.integrate_link(&link_id, &[branch], true);
                }
                SyncJob::AlertAction {
                    alert_id,
                    merge_instead,
                } => {
                    let (alert, link) = {
                        let store = self.store();
                        let alert = store.alerts.iter().find(|a| a.id == alert_id).cloned();
                        let link = alert
                            .as_ref()
                            .and_then(|a| store.links.iter().find(|l| l.id == a.link_id).cloned());
                        (alert, link)
                    };
                    let (Some(alert), Some(link)) = (alert, link) else {
                        continue;
                    };
                    if merge_instead {
                        match crate::sync::merge_instead(
                            &link.repo_path,
                            &alert.worktree_path,
                            &alert.branch,
                            alert.temp_worktree,
                        ) {
                            Ok(crate::sync::IntegrateOutcome::Conflict {
                                in_progress,
                                files,
                                worktree,
                                temp_worktree,
                            }) => {
                                let mut store = self.store();
                                if let Some(a) = store.alerts.iter_mut().find(|a| a.id == alert_id)
                                {
                                    a.in_progress = Some(in_progress);
                                    a.files = files;
                                    a.worktree_path = worktree;
                                    a.temp_worktree = temp_worktree;
                                    a.at_ms = now_ms();
                                }
                            }
                            Ok(_) => {
                                self.store().alerts.retain(|a| a.id != alert_id);
                            }
                            Err(error) => {
                                eprintln!("share sync: merge-instead failed: {error:#}");
                            }
                        }
                    } else {
                        if let Err(error) = crate::sync::abort(
                            &link.repo_path,
                            &alert.worktree_path,
                            alert.temp_worktree,
                        ) {
                            eprintln!("share sync: abort failed: {error:#}");
                        }
                        {
                            let mut store = self.store();
                            store.alerts.retain(|a| a.id != alert_id);
                            // Aborting pauses the branch until a manual
                            // sync — spec.
                            if let Some(l) = store.links.iter_mut().find(|l| l.id == alert.link_id)
                            {
                                l.paused_branches.insert(alert.branch.clone());
                            }
                        }
                    }
                    self.save_and_publish();
                }
                SyncJob::CreateLink {
                    peer,
                    origin_url,
                    repo_path,
                    peer_initiated,
                    reply,
                } => {
                    self.create_link(peer, origin_url, repo_path, peer_initiated, Some(reply));
                }
                SyncJob::PeerSyncState {
                    peer,
                    origin_url,
                    enabled,
                } => {
                    if enabled {
                        let has_link = self.store().link_for(&peer, &origin_url).is_some();
                        let share = self
                            .store()
                            .outgoing
                            .iter()
                            .find(|s| {
                                s.peer == peer
                                    && normalize_origin(&s.origin_url)
                                        == normalize_origin(&origin_url)
                            })
                            .cloned();
                        if has_link {
                            // Their ack — mark both flags.
                            {
                                let mut store = self.store();
                                if let Some(link) = store.link_for_mut(&peer, &origin_url) {
                                    link.peer_sync_enabled = true;
                                }
                                if let Some(share) = store.outgoing.iter_mut().find(|s| {
                                    s.peer == peer
                                        && normalize_origin(&s.origin_url)
                                            == normalize_origin(&origin_url)
                                }) {
                                    share.peer_sync_enabled = true;
                                }
                                let _ = store.save();
                            }
                            self.publish();
                        } else if let Some(share) = share {
                            // They enabled sync on a repo we shared —
                            // create our side of the link.
                            self.create_link(peer, origin_url, share.repo_path, true, None);
                        }
                    } else {
                        // Their SyncDisabled — tear our side down without
                        // answering (they already know).
                        let link_id = self
                            .store()
                            .link_for(&peer, &origin_url)
                            .map(|l| l.id.clone());
                        if let Some(link_id) = link_id {
                            self.tear_down_link(&link_id, false);
                        } else {
                            // No link — still clear the share's flag.
                            {
                                let mut store = self.store();
                                if let Some(share) = store.outgoing.iter_mut().find(|s| {
                                    s.peer == peer
                                        && normalize_origin(&s.origin_url)
                                            == normalize_origin(&origin_url)
                                }) {
                                    share.peer_sync_enabled = false;
                                    let _ = store.save();
                                }
                            }
                            self.publish();
                        }
                    }
                }
                SyncJob::ShareProject {
                    peer,
                    project_path,
                    reply,
                } => {
                    self.share_project(peer, project_path, reply);
                }
                SyncJob::UnshareProject {
                    peer,
                    origin_url,
                    reply,
                } => {
                    self.unshare_project(peer, origin_url, reply);
                }
                SyncJob::TearDownLink { link_id, notify } => {
                    self.tear_down_link(&link_id, notify);
                }
                SyncJob::RefNotice { origin_url, refs } => {
                    // Fetch moved refs into every checkout we can resolve
                    // for this origin — sync links and matched incoming
                    // shares — then bump review surfaces. An open Review
                    // tab re-reads its queue; no checkout resolved means
                    // the bump is all that happens.
                    let key = normalize_origin(&origin_url);
                    let paths: Vec<PathBuf> = {
                        let store = self.share_store.lock();
                        store
                            .links
                            .iter()
                            .filter(|l| normalize_origin(&l.origin_url) == key)
                            .map(|l| l.repo_path.clone())
                            .chain(
                                store
                                    .incoming
                                    .iter()
                                    .filter(|s| normalize_origin(&s.origin_url) == key)
                                    .filter_map(|s| s.matched_path.clone()),
                            )
                            .collect()
                    };
                    for path in paths {
                        crate::review::fetch_refs(&path, &refs);
                    }
                    if let Some(notifier) = self.state.lock().review_notifier.clone() {
                        notifier(origin_url);
                    }
                }
                SyncJob::UpdateIncoming { peer, projects } => {
                    self.update_incoming(peer, projects);
                }
                SyncJob::SetSessionSharing {
                    peer,
                    origin_url,
                    enabled,
                    reply,
                } => {
                    self.set_session_sharing(peer, origin_url, enabled, reply);
                }
                SyncJob::QueryBranches { link_id, reply } => {
                    let link = self.store().links.iter().find(|l| l.id == link_id).cloned();
                    let result = match link {
                        Some(link) => crate::sync::local_branches(&link.repo_path).and_then(|b| {
                            crate::sync::default_branch(&link.repo_path).map(|d| (b, d))
                        }),
                        None => Err(anyhow::anyhow!("no sync link with that id")),
                    };
                    let _ = reply.send(result);
                }
            }
        }
    }
}

fn run_runtime(
    dir: PathBuf,
    our_name: String,
    state: Arc<Mutex<ShareInner>>,
    sink: Arc<Mutex<Option<FriendsSink>>>,
    cmd_rx: Receiver<ShareCommand>,
    ready_tx: Sender<anyhow::Result<()>>,
) -> anyhow::Result<()> {
    // A std thread forwards sync crossbeam commands into the async loop.
    let (async_tx, mut async_rx) = tokio::sync::mpsc::unbounded_channel::<ShareCommand>();
    std::thread::Builder::new()
        .name("goddard-share-cmds".into())
        .spawn(move || {
            while let Ok(cmd) = cmd_rx.recv() {
                if async_tx.send(cmd).is_err() {
                    break;
                }
            }
        })?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        std::fs::create_dir_all(&dir)?;
        let secret = waku_share::identity::load_or_create(&dir)?;
        let friend_code = waku_share::identity::friend_code(secret.public());
        let store = state.lock().store.clone();
        *store.lock() = FriendStore::load(&dir)?;
        let share_store = state.lock().share_store.clone();
        *share_store.lock() = ShareStore::load(&dir)?;
        {
            let mut store = store.lock();
            if store.display_name.is_empty() {
                store.display_name = our_name.clone();
                let _ = store.save();
            }
        }
        state.lock().friend_code = friend_code;
        publish(&state, &sink);

        // The ShareNode Arc is filled after spawn; offer tasks wait on it.
        let node: Arc<tokio::sync::Mutex<Option<Arc<ShareNode>>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        // The sync worker serializes all share git. Jobs arrive from
        // commands and the protocol handlers; peer messages it wants sent
        // come back over `effect_rx` so the async loop can dial.
        let (job_tx, job_rx) = unbounded::<SyncJob>();
        let (effect_tx, mut effect_rx) =
            tokio::sync::mpsc::unbounded_channel::<RuntimeEffect>();
        {
            let state = state.clone();
            let share_store = share_store.clone();
            let sink = sink.clone();
            let scratch_dir = dir.join("sync");
            std::thread::Builder::new()
                .name("goddard-share-sync".into())
                .spawn(move || {
                    let mut worker = SyncWorker {
                        state,
                        share_store,
                        sink,
                        scratch_dir,
                        effect_tx,
                        branch_states: HashMap::new(),
                    };
                    worker.run(job_rx);
                })?;
        }

        let proto = FriendsProtocol::new(
            // on_request: register pending, broadcast, hand back a oneshot.
            {
                let state = state.clone();
                let sink = sink.clone();
                Arc::new(move |id: EndpointId, name: String| {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    {
                        let mut s = state.lock();
                        if s.store.lock().is_friend(&id) {
                            // Re-adding an existing friend: accept silently.
                            let our_name = s.store.lock().display_name.clone();
                            let _ = tx.send(RequestDecision::Accept { our_name });
                            return rx;
                        }
                        s.store.lock().requests.push(PendingRequest {
                            name,
                            node_id: id,
                            incoming: true,
                            at_ms: now_ms(),
                        });
                        s.pending.insert(id.to_string(), tx);
                        let _ = s.store.lock().save();
                    }
                    publish(&state, &sink);
                    rx
                })
            },
            // on_offer: auto-accept from friends; spawn the fetch task.
            {
                let state = state.clone();
                let sink = sink.clone();
                let node = node.clone();
                let dir = dir.clone();
                Arc::new(move |offer: OfferInfo| {
                    let state = state.clone();
                    let sink = sink.clone();
                    let node = node.clone();
                    let dir = dir.clone();
                    tokio::spawn(async move {
                        let id = Uuid::new_v4();
                        {
                            let mut s = state.lock();
                            s.transfers.push(TransferInfo {
                                id,
                                direction: TransferDirection::Incoming,
                                peer_id: offer.from.to_string(),
                                peer_name: offer.name.clone(),
                                title: offer.file_name.clone(),
                                note: offer.note.clone(),
                                status: TransferStatus::Transferring,
                                bytes_done: 0,
                                bytes_total: offer.size,
                                dest_dir: None,
                                session_id: None,
                            });
                        }
                        publish(&state, &sink);
                        let node = match node.lock().await.clone() {
                            Some(node) => node,
                            None => {
                                // Runtime gone — fail the row instead of
                                // panicking the task and stranding it.
                                {
                                    let mut s = state.lock();
                                    if let Some(t) = s
                                        .transfers
                                        .iter_mut()
                                        .find(|t| t.id == id)
                                    {
                                        t.status = TransferStatus::Failed;
                                    }
                                }
                                publish(&state, &sink);
                                return;
                            }
                        };
                        let progress = {
                            let state = state.clone();
                            let sink = sink.clone();
                            let mut last_publish = std::time::Instant::now();
                            move |done: u64| {
                                {
                                    let mut s = state.lock();
                                    if let Some(t) =
                                        s.transfers.iter_mut().find(|t| t.id == id)
                                    {
                                        t.bytes_done = done;
                                    }
                                }
                                // Chunks can land faster than frames; cap the
                                // broadcast at ~4 Hz. Completion publishes
                                // unconditionally below.
                                if last_publish.elapsed() >= Duration::from_millis(250) {
                                    last_publish = std::time::Instant::now();
                                    publish(&state, &sink);
                                }
                            }
                        };
                        let result = async {
                            let ticket: waku_share::Ticket = offer.ticket.parse()?;
                            let hash = node.fetch(&ticket, progress).await?;
                            let dest_dir = dir.join("transfers").join(id.to_string());
                            std::fs::create_dir_all(&dest_dir)?;
                            let dest = dest_dir.join(&offer.file_name);
                            node.export(hash, &dest).await?;
                            // The done receipt is courtesy bookkeeping for
                            // the sender — the bytes are already verified on
                            // disk, so a missed callback must not fail the
                            // transfer or hide the file.
                            let notified = tokio::time::timeout(
                                NOTIFY_TIMEOUT,
                                friends::notify_transfer_done(
                                    node.endpoint(),
                                    ticket.addr().clone(),
                                    &offer.ticket,
                                ),
                            )
                            .await;
                            match notified {
                                Ok(Ok(())) => {}
                                Ok(Err(error)) => eprintln!(
                                    "share: done-notify for transfer {id} failed: {error:#}"
                                ),
                                Err(_) => eprintln!(
                                    "share: done-notify for transfer {id} timed out"
                                ),
                            }
                            anyhow::Ok(dest_dir)
                        }
                        .await;
                        let (hook, notifier) = {
                            let mut s = state.lock();
                            if let Some(t) = s.transfers.iter_mut().find(|t| t.id == id) {
                                match &result {
                                    Ok(dest) => {
                                        t.status = TransferStatus::Done;
                                        t.dest_dir = Some(dest.clone());
                                    }
                                    Err(error) => {
                                        eprintln!(
                                            "share: incoming transfer {id} from {} failed: {error:#}",
                                            offer.name
                                        );
                                        t.status = TransferStatus::Failed;
                                    }
                                }
                            }
                            let transfer = s.transfers.iter().find(|t| t.id == id).cloned();
                            (
                                transfer.filter(|t| t.status == TransferStatus::Done).zip(
                                    s.transfer_hook.clone(),
                                ),
                                s.task_notifier.clone(),
                            )
                        };
                        if let Some((transfer, _)) = &hook {
                            link_transfer(&state, transfer);
                        }
                        if let Some((transfer, hook)) = hook {
                            let session_id = hook(&transfer);
                            if let Some(session_id) = session_id {
                                let mut s = state.lock();
                                if let Some(t) =
                                    s.transfers.iter_mut().find(|t| t.id == id)
                                {
                                    t.session_id = Some(session_id);
                                }
                            }
                            if let Some(notifier) = notifier {
                                notifier();
                            }
                        }
                        publish(&state, &sink);
                    });
                })
            },
            // on_done: match the outgoing transfer by its ticket string.
            {
                let state = state.clone();
                let sink = sink.clone();
                Arc::new(move |from: EndpointId, ticket: String| {
                    {
                        let mut s = state.lock();
                        s.store.lock().mark_seen(&from);
                        let _ = s.store.lock().save();
                        if let Some(id) = s.outgoing_tickets.remove(&ticket) {
                            if let Some(t) = s.transfers.iter_mut().find(|t| t.id == id) {
                                t.status = TransferStatus::Done;
                                t.bytes_done = t.bytes_total;
                            }
                            s.outgoing_tags.remove(&id);
                        }
                    }
                    publish(&state, &sink);
                })
            },
            store.clone(),
        )
        .with_project_handlers(
            // on_share: the friend's full shared set — the worker
            // reconciles incoming shares and any links they fed.
            {
                let job_tx = job_tx.clone();
                Arc::new(move |peer: EndpointId, projects: Vec<SharedRepo>| {
                    let _ = job_tx.send(SyncJob::UpdateIncoming { peer, projects });
                })
            },
            // on_sync_state: SyncEnabled/SyncDisabled handshake.
            {
                let job_tx = job_tx.clone();
                Arc::new(move |peer: EndpointId, origin_url: String, enabled: bool| {
                    let _ = job_tx.send(SyncJob::PeerSyncState {
                        peer,
                        origin_url,
                        enabled,
                    });
                })
            },
            // on_push: a friend pushed to a shared origin — resolve the
            // link and integrate the named branches.
            {
                let job_tx = job_tx.clone();
                let state = state.clone();
                Arc::new(move |peer: EndpointId, origin_url: String, branches: Vec<String>| {
                    let link_id = state
                        .lock()
                        .share_store
                        .lock()
                        .link_for(&peer, &origin_url)
                        .map(|l| l.id.clone());
                    if let Some(link_id) = link_id {
                        let _ = job_tx.send(SyncJob::Integrate { link_id, branches });
                    }
                })
            },
            // on_refs: non-branch refs moved on a shared origin — the
            // worker fetches them into repos it can resolve, then bumps
            // review surfaces.
            {
                let job_tx = job_tx.clone();
                Arc::new(move |_peer: EndpointId, origin_url: String, refs: Vec<String>| {
                    let _ = job_tx.send(SyncJob::RefNotice { origin_url, refs });
                })
            },
        )
        .with_session_handlers(
            // on_session_list: the share itself authorizes — `None`
            // answers `SessionDenied`. The session catalog is the
            // daemon-installed `SessionSource`.
            {
                let state = state.clone();
                Arc::new(move |peer: EndpointId, origin_url: String| {
                    let (repo_path, source) = shared_repo_for_sessions(&state, &peer, &origin_url)?;
                    Some((source.list)(&repo_path))
                })
            },
            // on_session_subscribe: same authorization plus a membership
            // check — the session must be one `SessionList` would name.
            {
                let state = state.clone();
                Arc::new(
                    move |peer: EndpointId,
                          origin_url: String,
                          session_id: Uuid,
                          resume: Option<ReplayCursor>| {
                        let (repo_path, source, streamer) = {
                            let (repo_path, source) =
                                shared_repo_for_sessions(&state, &peer, &origin_url)?;
                            let streamer = state.lock().session_streamer.clone()?;
                            (repo_path, source, streamer)
                        };
                        if !(source.list)(&repo_path)
                            .iter()
                            .any(|s| s.session_id == session_id)
                        {
                            return None;
                        }
                        let session = (source.snapshot)(session_id)?;
                        let stream = streamer(session_id, resume);
                        let (tx, rx) = tokio::sync::mpsc::channel(256);
                        state.lock().session_feeds.insert(
                            (peer, session_id),
                            SessionFeedHandle {
                                origin_url: origin_url.clone(),
                                sender: tx.clone(),
                            },
                        );
                        // The hub's stream is a blocking crossbeam
                        // receiver; bridge it onto the feed's async
                        // channel on a dedicated thread so the runtime's
                        // acceptor never blocks on the journal.
                        let state = state.clone();
                        std::thread::Builder::new()
                            .name(format!("goddard-session-feed-{session_id}"))
                            .spawn(move || {
                                loop {
                                    crossbeam_channel::select! {
                                        recv(stream.events) -> msg => match msg {
                                            Ok(ServerMessage::Event(event)) => {
                                                if tx.blocking_send(FriendsMessage::SessionEvent {
                                                    event: Box::new(event),
                                                }).is_err() {
                                                    break;
                                                }
                                            }
                                            // A filtered stream carries
                                            // only events for its session.
                                            Ok(_) => {}
                                            Err(_) => break,
                                        },
                                        // The hub dropped us for lagging —
                                        // end the feed; the viewer
                                        // resubscribes with a fresh cursor.
                                        recv(stream.kicked) -> _ => break,
                                    }
                                }
                                state.lock().session_feeds.remove(&(peer, session_id));
                            })
                            .ok()?;
                        Some(SessionFeed {
                            session,
                            events: rx,
                        })
                    },
                )
            },
        )
        .with_chat_handler(
            // on_chat: record the message as a finished incoming row, then
            // materialize its session through the daemon-installed hook —
            // the same shape a completed transfer takes.
            {
                let state = state.clone();
                let sink = sink.clone();
                Arc::new(move |from: EndpointId, name: String, text: String| {
                    let id = Uuid::new_v4();
                    let (hook, notifier) = {
                        let mut s = state.lock();
                        s.transfers.push(TransferInfo {
                            id,
                            direction: TransferDirection::Incoming,
                            peer_id: from.to_string(),
                            peer_name: name.clone(),
                            title: chat_title(&text),
                            note: Some(text.clone()),
                            status: TransferStatus::Done,
                            bytes_done: 0,
                            bytes_total: 0,
                            dest_dir: None,
                            session_id: None,
                        });
                        (s.chat_hook.clone(), s.task_notifier.clone())
                    };
                    if let Some(hook) = hook {
                        if let Some(session_id) = hook(name, text) {
                            let mut s = state.lock();
                            if let Some(t) = s.transfers.iter_mut().find(|t| t.id == id) {
                                t.session_id = Some(session_id);
                            }
                        }
                        if let Some(notifier) = notifier {
                            notifier();
                        }
                    }
                    publish(&state, &sink);
                })
            },
        );

        let link = {
            let inner = state.lock();
            match (inner.link_info.clone(), inner.link_pair.clone()) {
                (Some(info), Some(pair)) => {
                    Some(waku_share::link::LinkProtocol::new(info, pair))
                }
                _ => None,
            }
        };
        let share_node = match ShareNode::spawn(&dir, secret, RelayMode::Default, proto, link).await
        {
            Ok(n) => Arc::new(n),
            Err(e) => {
                drop(ready_tx.send(Err(e)));
                return Ok(());
            }
        };
        drop(ready_tx.send(Ok(())));
        *node.lock().await = Some(share_node.clone());

        // Open friend watches: session id → the pump task's abort handle.
        // `UnwatchFriendSession` aborts; the task removes itself on exit.
        let watches: Arc<Mutex<HashMap<Uuid, tokio::task::AbortHandle>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let mut poll_tick = tokio::time::interval(SYNC_POLL_INTERVAL);
        poll_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut fetch_tick = tokio::time::interval(SYNC_FETCH_INTERVAL);
        fetch_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            let cmd = tokio::select! {
                cmd = async_rx.recv() => match cmd {
                    Some(cmd) => cmd,
                    None => break,
                },
                effect = effect_rx.recv() => {
                    match effect {
                        Some(RuntimeEffect::ShareProjects { peer, projects }) => {
                            let endpoint = share_node.endpoint().clone();
                            tokio::spawn(async move {
                                if let Err(error) =
                                    friends::send_share_projects(&endpoint, peer, &projects).await
                                {
                                    eprintln!("share: share-projects to {peer} failed: {error:#}");
                                }
                            });
                        }
                        Some(RuntimeEffect::SyncState { peer, origin_url, enabled }) => {
                            let endpoint = share_node.endpoint().clone();
                            tokio::spawn(async move {
                                if let Err(error) =
                                    friends::send_sync_state(&endpoint, peer, &origin_url, enabled)
                                        .await
                                {
                                    eprintln!("share: sync-state to {peer} failed: {error:#}");
                                }
                            });
                        }
                        Some(RuntimeEffect::PushNotice { peer, origin_url, branches }) => {
                            let endpoint = share_node.endpoint().clone();
                            tokio::spawn(async move {
                                if let Err(error) = friends::send_push_notice(
                                    &endpoint,
                                    peer,
                                    &origin_url,
                                    &branches,
                                )
                                .await
                                {
                                    eprintln!("share: push notice to {peer} failed: {error:#}");
                                }
                            });
                        }
                        None => break,
                    }
                    continue;
                }
                _ = poll_tick.tick() => {
                    let _ = job_tx.send(SyncJob::Poll);
                    continue;
                }
                _ = fetch_tick.tick() => {
                    let _ = job_tx.send(SyncJob::Fetch { link_id: None });
                    continue;
                }
            };
            match cmd {
                ShareCommand::Shutdown => break,
                ShareCommand::Probe { node_id, reply } => {
                    let fresh = state
                        .lock()
                        .probes
                        .get(&node_id)
                        .is_some_and(|(_, at)| now_ms().saturating_sub(*at) < PROBE_CACHE_MS);
                    if !fresh && let Ok(id) = node_id.parse::<EndpointId>() {
                        let ok = friends::probe(share_node.endpoint(), id, PROBE_TIMEOUT).await;
                        let mut s = state.lock();
                        s.probes.insert(node_id.clone(), (ok, now_ms()));
                        if ok {
                            s.store.lock().mark_seen(&id);
                            let _ = s.store.lock().save();
                        }
                    }
                    publish(&state, &sink);
                    let _ = reply.send(Ok(()));
                }
                ShareCommand::SendRequest { code, name, reply } => {
                    let result = async {
                        let id = waku_share::identity::parse_friend_code(&code)?;
                        if id == share_node.endpoint().id() {
                            anyhow::bail!("that's your own friend code");
                        }
                        // Record the outgoing request.
                        {
                            let s = state.lock();
                            let mut store = s.store.lock();
                            if !store
                                .requests
                                .iter()
                                .any(|r| r.node_id == id && !r.incoming)
                            {
                                store.requests.push(PendingRequest {
                                    name: name.clone(),
                                    node_id: id,
                                    incoming: false,
                                    at_ms: now_ms(),
                                });
                                let _ = store.save();
                            }
                        }
                        publish(&state, &sink);
                        if let Err(e) = friends::send_friend_request(
                            share_node.endpoint(),
                            id,
                            &name,
                            &store,
                        )
                        .await
                        {
                            // A failed send must not strand a pending row —
                            // there is no peer who could resolve it.
                            let s = state.lock();
                            s.store
                                .lock()
                                .requests
                                .retain(|r| !(r.node_id == id && !r.incoming));
                            let _ = s.store.lock().save();
                            return Err(e);
                        }
                        anyhow::Ok(())
                    }
                    .await;
                    publish(&state, &sink);
                    let _ = reply.send(result);
                }
                ShareCommand::Respond {
                    node_id,
                    accept,
                    reply,
                } => {
                    let decision = state.lock().pending.remove(&node_id);
                    let our_name = state.lock().store.lock().display_name.clone();
                    let result = match decision {
                        Some(tx) => {
                            let _ = tx.send(if accept {
                                RequestDecision::Accept {
                                    our_name: our_name.clone(),
                                }
                            } else {
                                RequestDecision::Decline
                            });
                            // Mirror the store mutation the protocol handler
                            // performs once it wakes on the decision — the
                            // publish below would otherwise snapshot a stale
                            // request card and no friend row, and nothing
                            // re-publishes when the handler later lands.
                            if let Ok(id) = node_id.parse::<EndpointId>() {
                                let s = state.lock();
                                let mut store = s.store.lock();
                                let name = accept.then(|| {
                                    store
                                        .requests
                                        .iter()
                                        .find(|r| r.node_id == id && r.incoming)
                                        .map(|r| r.name.clone())
                                });
                                if let Some(Some(name)) = name {
                                    store.friends.insert(
                                        id,
                                        Friend {
                                            name,
                                            node_id: id,
                                            added_at_ms: now_ms(),
                                            last_seen_ms: Some(now_ms()),
                                            nickname: None,
                                        },
                                    );
                                }
                                store.requests.retain(|r| r.node_id != id);
                                let _ = store.save();
                            }
                            Ok(())
                        }
                        None => {
                            // A repeat respond racing the broadcast resolves
                            // to a no-op — the first one already answered.
                            let resolved = node_id
                                .parse::<EndpointId>()
                                .ok()
                                .is_some_and(|id| {
                                    let s = state.lock();
                                    let store = s.store.lock();
                                    !store.requests.iter().any(|r| r.node_id == id)
                                        && (!accept || store.is_friend(&id))
                                });
                            if resolved {
                                Ok(())
                            } else {
                                Err(anyhow::anyhow!("no pending request from that code"))
                            }
                        }
                    };
                    publish(&state, &sink);
                    let _ = reply.send(result);
                }
                ShareCommand::SendFile {
                    node_id,
                    path,
                    note,
                    reply,
                } => {
                    let result = async {
                        let id: EndpointId = node_id.parse()?;
                        // Mint the ticket once the endpoint reports a
                        // dialable address — an early ticket can carry no
                        // relay path. Bounded so LAN/discovery-only sends
                        // still proceed when no relay answers.
                        let _ = tokio::time::timeout(
                            ONLINE_WAIT,
                            share_node.wait_online(),
                        )
                        .await;
                        let (ticket, tag) = share_node.provide(&path).await?;
                        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        let title = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "file".into());
                        let ticket_str = ticket.to_string();
                        let transfer_id = Uuid::new_v4();
                        {
                            let mut s = state.lock();
                            let peer_name = s
                                .store
                                .lock()
                                .friends
                                .get(&id)
                                .map(|f| f.name.clone())
                                .unwrap_or_default();
                            s.outgoing_tickets.insert(ticket_str.clone(), transfer_id);
                            s.outgoing_tags.insert(transfer_id, tag);
                            s.transfers.push(TransferInfo {
                                id: transfer_id,
                                direction: TransferDirection::Outgoing,
                                peer_id: node_id.clone(),
                                peer_name,
                                title: title.clone(),
                                note: note.clone(),
                                status: TransferStatus::Transferring,
                                bytes_done: 0,
                                bytes_total: size,
                                dest_dir: None,
                                session_id: None,
                            });
                        }
                        publish(&state, &sink);
                        let our_name = state.lock().store.lock().display_name.clone();
                        tokio::time::timeout(
                            OFFER_TIMEOUT,
                            friends::send_offer(
                                share_node.endpoint(),
                                id,
                                &our_name,
                                &title,
                                size,
                                note,
                                &ticket_str,
                            ),
                        )
                        .await
                        .map_err(|_| anyhow::anyhow!("offer timed out"))??;
                        // The receiver's TransferDone is the only completion
                        // signal an outgoing transfer gets — if it never
                        // lands, fail the row instead of leaving it at "0 B"
                        // forever. A late receipt still flips it to Done.
                        {
                            let state = state.clone();
                            let sink = sink.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(TRANSFER_STALL_TIMEOUT).await;
                                {
                                    let mut s = state.lock();
                                    if let Some(t) =
                                        s.transfers.iter_mut().find(|t| {
                                            t.id == transfer_id
                                                && t.status == TransferStatus::Transferring
                                        })
                                    {
                                        t.status = TransferStatus::Failed;
                                    }
                                }
                                publish(&state, &sink);
                            });
                        }
                        anyhow::Ok(())
                    }
                    .await;
                    if let Err(error) = &result {
                        eprintln!("share: send to {node_id} failed: {error:#}");
                        let mut s = state.lock();
                        let failed_id = s
                            .transfers
                            .iter_mut()
                            .rev()
                            .find(|t| {
                                t.direction == TransferDirection::Outgoing
                                    && t.peer_id == node_id
                                    && t.status == TransferStatus::Transferring
                            })
                            .map(|t| {
                                t.status = TransferStatus::Failed;
                                t.id
                            });
                        if let Some(failed_id) = failed_id {
                            s.outgoing_tags.remove(&failed_id);
                        }
                    }
                    publish(&state, &sink);
                    let _ = reply.send(result);
                }
                ShareCommand::SendChat {
                    node_id,
                    text,
                    reply,
                } => {
                    let result = async {
                        let id: EndpointId = node_id.parse()?;
                        if !state.lock().store.lock().is_friend(&id) {
                            anyhow::bail!("no friend with that code");
                        }
                        let transfer_id = Uuid::new_v4();
                        {
                            let mut s = state.lock();
                            let peer_name = s
                                .store
                                .lock()
                                .friends
                                .get(&id)
                                .map(|f| f.name.clone())
                                .unwrap_or_default();
                            s.transfers.push(TransferInfo {
                                id: transfer_id,
                                direction: TransferDirection::Outgoing,
                                peer_id: node_id.clone(),
                                peer_name,
                                title: chat_title(&text),
                                note: Some(text.clone()),
                                status: TransferStatus::Transferring,
                                bytes_done: 0,
                                bytes_total: 0,
                                dest_dir: None,
                                session_id: None,
                            });
                        }
                        publish(&state, &sink);
                        // Same dial path as offers — wait briefly for a
                        // reachable address, then bound the send.
                        let _ =
                            tokio::time::timeout(ONLINE_WAIT, share_node.wait_online()).await;
                        let our_name = state.lock().store.lock().display_name.clone();
                        let result = tokio::time::timeout(
                            OFFER_TIMEOUT,
                            friends::send_chat(share_node.endpoint(), id, &our_name, &text),
                        )
                        .await
                        .map_err(|_| anyhow::anyhow!("message send timed out"))
                        .and_then(|r| r);
                        {
                            let mut s = state.lock();
                            if let Some(t) =
                                s.transfers.iter_mut().find(|t| t.id == transfer_id)
                            {
                                t.status = if result.is_ok() {
                                    TransferStatus::Done
                                } else {
                                    TransferStatus::Failed
                                };
                            }
                        }
                        result
                    }
                    .await;
                    publish(&state, &sink);
                    let _ = reply.send(result);
                }
                ShareCommand::CancelTransfer { id, reply } => {
                    {
                        let mut s = state.lock();
                        if let Some(t) = s.transfers.iter_mut().find(|t| t.id == id) {
                            t.status = TransferStatus::Cancelled;
                        }
                        s.outgoing_tags.remove(&id);
                    }
                    publish(&state, &sink);
                    let _ = reply.send(Ok(()));
                }
                ShareCommand::ShareProject {
                    node_id,
                    project_path,
                    reply,
                } => {
                    let result = node_id
                        .parse::<EndpointId>()
                        .map_err(|e| anyhow::anyhow!("{e}"))
                        .and_then(|peer| {
                            if !state.lock().store.lock().is_friend(&peer) {
                                anyhow::bail!("no friend with that code");
                            }
                            job_tx
                                .send(SyncJob::ShareProject {
                                    peer,
                                    project_path,
                                    reply: reply.clone(),
                                })
                                .map_err(|e| anyhow::anyhow!("{e}"))
                        });
                    if let Err(error) = result {
                        let _ = reply.send(Err(error));
                    }
                }
                ShareCommand::UnshareProject {
                    node_id,
                    origin_url,
                    reply,
                } => {
                    let result = node_id
                        .parse::<EndpointId>()
                        .map_err(|e| anyhow::anyhow!("{e}"))
                        .and_then(|peer| {
                            job_tx
                                .send(SyncJob::UnshareProject {
                                    peer,
                                    origin_url,
                                    reply: reply.clone(),
                                })
                                .map_err(|e| anyhow::anyhow!("{e}"))
                        });
                    if let Err(error) = result {
                        let _ = reply.send(Err(error));
                    }
                }
                ShareCommand::EnableSync {
                    node_id,
                    origin_url,
                    reply,
                } => {
                    let result = (|| {
                        let peer: EndpointId = node_id.parse()?;
                        // The daemon-resolved match wins over the client's
                        // copy — the incoming share's `matched_path` is the
                        // authoritative local checkout for that origin.
                        let repo_path = {
                            let s = state.lock();
                            let share = s.share_store.lock();
                            share
                                .incoming
                                .iter()
                                .find(|i| {
                                    i.peer == peer
                                        && normalize_origin(&i.origin_url)
                                            == normalize_origin(&origin_url)
                                })
                                .and_then(|i| i.matched_path.clone())
                                .context("they don't share that repo with you")?
                        };
                        let (job_reply, job_rx) = bounded(1);
                        job_tx.send(SyncJob::CreateLink {
                            peer,
                            origin_url,
                            repo_path,
                            peer_initiated: false,
                            reply: job_reply,
                        })?;
                        job_rx.recv_timeout(COMMAND_TIMEOUT)??;
                        anyhow::Ok(())
                    })();
                    let _ = reply.send(result);
                }
                ShareCommand::DisableSync { link_id, reply } => {
                    let exists = {
                        let s = state.lock();
                        s.share_store.lock().links.iter().any(|l| l.id == link_id)
                    };
                    let result = if exists {
                        job_tx
                            .send(SyncJob::TearDownLink {
                                link_id,
                                notify: true,
                            })
                            .map_err(|e| anyhow::anyhow!("{e}"))
                    } else {
                        Err(anyhow::anyhow!("no sync link with that id"))
                    };
                    let _ = reply.send(result);
                }
                ShareCommand::SetSyncConfig {
                    link_id,
                    auto_push,
                    enabled_branches,
                    reply,
                } => {
                    let result = (|| {
                        {
                            let s = state.lock();
                            let mut share = s.share_store.lock();
                            let Some(link) =
                                share.links.iter_mut().find(|l| l.id == link_id)
                            else {
                                anyhow::bail!("no sync link with that id");
                            };
                            link.auto_push = auto_push;
                            link.enabled_branches =
                                enabled_branches.into_iter().collect();
                            // A disabled branch's pause flag is moot —
                            // keep the set tidy.
                            let enabled = link.enabled_branches.clone();
                            link.paused_branches.retain(|b| enabled.contains(b));
                            let _ = share.save();
                        }
                        anyhow::Ok(())
                    })();
                    publish(&state, &sink);
                    let _ = reply.send(result);
                }
                ShareCommand::SyncNow {
                    link_id,
                    branch,
                    reply,
                } => {
                    let exists = {
                        let s = state.lock();
                        s.share_store.lock().links.iter().any(|l| l.id == link_id)
                    };
                    let result = if exists {
                        job_tx
                            .send(SyncJob::SyncNow { link_id, branch })
                            .map_err(|e| anyhow::anyhow!("{e}"))
                    } else {
                        Err(anyhow::anyhow!("no sync link with that id"))
                    };
                    let _ = reply.send(result);
                }
                ShareCommand::AlertAction {
                    alert_id,
                    action,
                    reply,
                } => {
                    let result = match action {
                        FriendSyncAlertAction::Dismiss => {
                            {
                                let s = state.lock();
                                let mut share = s.share_store.lock();
                                share.alerts.retain(|a| a.id != alert_id);
                                let _ = share.save();
                            }
                            publish(&state, &sink);
                            Ok(())
                        }
                        _ => {
                            let exists = {
                                let s = state.lock();
                                s.share_store
                                    .lock()
                                    .alerts
                                    .iter()
                                    .any(|a| a.id == alert_id)
                            };
                            if exists {
                                job_tx
                                    .send(SyncJob::AlertAction {
                                        alert_id,
                                        merge_instead: matches!(
                                            action,
                                            FriendSyncAlertAction::MergeInstead
                                        ),
                                    })
                                    .map_err(|e| anyhow::anyhow!("{e}"))
                            } else {
                                Err(anyhow::anyhow!("no sync alert with that id"))
                            }
                        }
                    };
                    let _ = reply.send(result);
                }
                ShareCommand::GetSyncBranches { link_id, reply } => {
                    let (job_reply, job_rx) = bounded(1);
                    let result = job_tx
                        .send(SyncJob::QueryBranches {
                            link_id,
                            reply: job_reply,
                        })
                        .map_err(|e| anyhow::anyhow!("{e}"))
                        .and_then(|_| {
                            job_rx
                                .recv_timeout(COMMAND_TIMEOUT)
                                .map_err(|e| anyhow::anyhow!("{e}"))?
                        });
                    let _ = reply.send(result);
                }
                ShareCommand::SetSessionSharing {
                    node_id,
                    origin_url,
                    enabled,
                    reply,
                } => {
                    let result = node_id
                        .parse::<EndpointId>()
                        .map_err(|e| anyhow::anyhow!("{e}"))
                        .and_then(|peer| {
                            if !state.lock().store.lock().is_friend(&peer) {
                                anyhow::bail!("no friend with that code");
                            }
                            job_tx
                                .send(SyncJob::SetSessionSharing {
                                    peer,
                                    origin_url,
                                    enabled,
                                    reply: reply.clone(),
                                })
                                .map_err(|e| anyhow::anyhow!("{e}"))
                        });
                    if let Err(error) = result {
                        let _ = reply.send(Err(error));
                    }
                }
                ShareCommand::FriendSessions {
                    node_id,
                    origin_url,
                    reply,
                } => {
                    let result = node_id
                        .parse::<EndpointId>()
                        .map_err(|e| anyhow::anyhow!("{e}"));
                    match result {
                        Err(error) => {
                            let _ = reply.send(Err(error));
                        }
                        Ok(peer) => {
                            let endpoint = share_node.endpoint().clone();
                            tokio::spawn(async move {
                                let _ = reply.send(
                                    friends::fetch_session_list(&endpoint, peer, &origin_url)
                                        .await,
                                );
                            });
                        }
                    }
                }
                ShareCommand::WatchFriendSession {
                    node_id,
                    origin_url,
                    session_id,
                    reply,
                } => {
                    let result = node_id
                        .parse::<EndpointId>()
                        .map_err(|e| anyhow::anyhow!("{e}"))
                        .and_then(|peer| {
                            if watches.lock().contains_key(&session_id) {
                                anyhow::bail!("already watching that session");
                            }
                            Ok(peer)
                        });
                    match result {
                        Err(error) => {
                            let _ = reply.send(Err(error));
                        }
                        Ok(peer) => {
                            let endpoint = share_node.endpoint().clone();
                            let state = state.clone();
                            let task_watches = watches.clone();
                            let task = tokio::spawn(async move {
                                match friends::subscribe_session(
                                    &endpoint,
                                    peer,
                                    &origin_url,
                                    session_id,
                                    None,
                                )
                                .await
                                {
                                    Err(error) => {
                                        let _ = reply.send(Err(error));
                                    }
                                    Ok((session, conn, mut recv)) => {
                                        let _ = reply.send(Ok(session));
                                        let sink =
                                            state.lock().session_sink.clone();
                                        let mut revoked = false;
                                        loop {
                                            match friends::read_session_frame(&mut recv)
                                                .await
                                            {
                                                Ok(FriendsMessage::SessionEvent {
                                                    event,
                                                }) => {
                                                    if let Some(sink) = &sink {
                                                        sink(FriendSessionUpdate::Event(
                                                            *event,
                                                        ));
                                                    }
                                                }
                                                Ok(FriendsMessage::SharingRevoked) => {
                                                    revoked = true;
                                                    break;
                                                }
                                                _ => break,
                                            }
                                        }
                                        if let Some(sink) = &sink {
                                            sink(FriendSessionUpdate::Closed {
                                                session_id,
                                                revoked,
                                            });
                                        }
                                        drop(conn);
                                        task_watches.lock().remove(&session_id);
                                    }
                                }
                            });
                            watches
                                .lock()
                                .insert(session_id, task.abort_handle());
                        }
                    }
                }
                ShareCommand::UnwatchFriendSession { session_id, reply } => {
                    if let Some(handle) = watches.lock().remove(&session_id) {
                        handle.abort();
                    }
                    if let Some(sink) = state.lock().session_sink.clone() {
                        sink(FriendSessionUpdate::Closed {
                            session_id,
                            revoked: false,
                        });
                    }
                    let _ = reply.send(Ok(()));
                }
                ShareCommand::KickPoll => {
                    let _ = job_tx.send(SyncJob::Poll);
                }
                ShareCommand::NotifyPush {
                    origin_url,
                    branches,
                } => {
                    for peer in
                        peers_for_origin(&state.lock().share_store.lock(), &origin_url)
                    {
                        let endpoint = share_node.endpoint().clone();
                        let origin_url = origin_url.clone();
                        let branches = branches.clone();
                        tokio::spawn(async move {
                            if let Err(error) =
                                friends::send_push_notice(&endpoint, peer, &origin_url, &branches)
                                    .await
                            {
                                eprintln!("share: push notice to {peer} failed: {error:#}");
                            }
                        });
                    }
                }
                ShareCommand::NotifyRefs { origin_url, refs } => {
                    for peer in
                        peers_for_origin(&state.lock().share_store.lock(), &origin_url)
                    {
                        let endpoint = share_node.endpoint().clone();
                        let origin_url = origin_url.clone();
                        let refs = refs.clone();
                        tokio::spawn(async move {
                            if let Err(error) =
                                friends::send_ref_notice(&endpoint, peer, &origin_url, &refs).await
                            {
                                eprintln!("share: ref notice to {peer} failed: {error:#}");
                            }
                        });
                    }
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("goddard-share-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn new_peer(dir: &std::path::Path) -> EndpointId {
        waku_share::identity::load_or_create(dir).unwrap().public()
    }

    fn service(dir: &std::path::Path) -> ShareService {
        ShareService::new(dir.to_path_buf(), "tester".to_owned())
    }

    fn stub_session_source() -> SessionSource {
        SessionSource {
            list: Arc::new(|_| {
                vec![SharedSessionSummary {
                    session_id: Uuid::new_v4(),
                    title: "session".to_owned(),
                    auto_title: None,
                    status: waku_protocol::model::SessionStatus::Idle,
                    created_at: 0,
                    last_reply_at: None,
                }]
            }),
            snapshot: Arc::new(|_| None),
        }
    }

    fn outgoing(dir: &std::path::Path, share_sessions: bool) -> OutgoingShare {
        OutgoingShare {
            peer: new_peer(dir),
            name: "repo".to_owned(),
            origin_url: "git@github.com:org/repo.git".to_owned(),
            repo_path: dir.join("checkout"),
            peer_sync_enabled: false,
            share_sessions,
            shared_at_ms: 0,
        }
    }

    #[test]
    fn session_access_requires_opted_in_share() {
        let dir = temp_dir("authz");
        let peer_dir = temp_dir("authz-peer");
        let service = service(&dir);
        let share = OutgoingShare {
            peer: new_peer(&peer_dir),
            ..outgoing(&dir, false)
        };
        let peer = share.peer;
        service.state.lock().share_store.lock().outgoing.push(share);
        service.state.lock().session_source = Some(stub_session_source());

        // Off by default: the flag is the only gate.
        assert!(
            shared_repo_for_sessions(&service.state, &peer, "git@github.com:org/repo.git")
                .is_none()
        );
        // A stranger is denied even with the flag on.
        service
            .state
            .lock()
            .share_store
            .lock()
            .outgoing
            .iter_mut()
            .for_each(|s| s.share_sessions = true);
        assert!(
            shared_repo_for_sessions(
                &service.state,
                &new_peer(&temp_dir("stranger")),
                "git@github.com:org/repo.git"
            )
            .is_none()
        );
        // The real peer passes, with origin matching normalized.
        let (repo_path, source) =
            shared_repo_for_sessions(&service.state, &peer, "https://github.com/org/repo")
                .expect("authorized share");
        assert_eq!(repo_path, dir.join("checkout"));
        assert_eq!((source.list)(&repo_path).len(), 1);
    }

    #[test]
    fn revocation_closes_matching_feeds() {
        let dir = temp_dir("revoke");
        let service = service(&dir);
        let owner = new_peer(&temp_dir("owner"));
        let other = new_peer(&temp_dir("other"));
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();
        let session_c = Uuid::new_v4();

        let (tx_a, mut rx_a) = tokio::sync::mpsc::channel(4);
        let (tx_b, mut rx_b) = tokio::sync::mpsc::channel(4);
        let (tx_c, mut rx_c) = tokio::sync::mpsc::channel(4);
        {
            let mut inner = service.state.lock();
            inner.session_feeds.insert(
                (owner, session_a),
                SessionFeedHandle {
                    origin_url: "https://github.com/org/repo".to_owned(),
                    sender: tx_a,
                },
            );
            inner.session_feeds.insert(
                (owner, session_b),
                SessionFeedHandle {
                    origin_url: "https://github.com/org/other".to_owned(),
                    sender: tx_b,
                },
            );
            inner.session_feeds.insert(
                (other, session_c),
                SessionFeedHandle {
                    origin_url: "https://github.com/org/repo".to_owned(),
                    sender: tx_c,
                },
            );
        }

        // Revoking one project hits only that peer's feeds on that origin.
        revoke_session_feeds(&service.state, &owner, Some("git@github.com:org/repo.git"));
        assert!(matches!(
            rx_a.try_recv(),
            Ok(FriendsMessage::SharingRevoked)
        ));
        assert!(rx_a.try_recv().is_err());
        assert!(rx_b.try_recv().is_err());
        assert!(rx_c.try_recv().is_err());
        assert_eq!(service.state.lock().session_feeds.len(), 2);

        // Revoking the peer entirely hits their remaining feeds.
        revoke_session_feeds(&service.state, &owner, None);
        assert!(matches!(
            rx_b.try_recv(),
            Ok(FriendsMessage::SharingRevoked)
        ));
        assert!(rx_c.try_recv().is_err());
        assert!(service.state.lock().session_feeds.len() == 1);
    }

    #[test]
    fn persisted_shares_default_session_sharing_off() {
        // Data written before session sharing existed must deserialize
        // with the flag off — sharing is opt-in, never implied.
        let outgoing: OutgoingShare = serde_json::from_value(serde_json::json!({
            "peer": "0000000000000000000000000000000000000000000000000000000000000000",
            "name": "repo",
            "origin_url": "git@github.com:org/repo.git",
            "repo_path": "/tmp/repo",
            "shared_at_ms": 0
        }))
        .unwrap();
        assert!(!outgoing.share_sessions);
        assert!(!outgoing.peer_sync_enabled);

        let repo: SharedRepo = serde_json::from_value(serde_json::json!({
            "name": "repo",
            "origin_url": "git@github.com:org/repo.git"
        }))
        .unwrap();
        assert!(!repo.share_sessions);

        let incoming: IncomingShare = serde_json::from_value(serde_json::json!({
            "peer": "0000000000000000000000000000000000000000000000000000000000000000",
            "peer_name": "alice",
            "name": "repo",
            "origin_url": "git@github.com:org/repo.git",
            "received_at_ms": 0
        }))
        .unwrap();
        assert!(!incoming.share_sessions);
    }
}
