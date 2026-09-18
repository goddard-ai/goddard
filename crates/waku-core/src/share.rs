//! Daemon-owned friend-to-friend sharing. Owns the `waku-share` iroh
//! endpoint on a dedicated tokio thread; the threaded daemon talks to it
//! over crossbeam channels and every state change broadcasts a whole
//! `FriendsState` document to subscribers (the `FriendsChanged` message).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use uuid::Uuid;

use waku_protocol::friends::{
    FriendInfo, FriendRequestInfo, FriendsState, TransferDirection, TransferInfo, TransferStatus,
};
use waku_share::friends::{
    self, Friend, FriendStore, FriendsProtocol, OfferInfo, PendingRequest, RequestDecision,
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

/// Installed by the server so async share events (incoming request, offer,
/// progress) reach every subscribed client.
pub type FriendsSink = Arc<dyn Fn(FriendsState) + Send + Sync>;

/// Fired when an incoming transfer finishes and its files are on disk.
/// The daemon creates the transfer's agent session from this hook and
/// returns its id, which is recorded on the transfer for clients.
pub type TransferHook = Arc<dyn Fn(&TransferInfo) -> Option<Uuid> + Send + Sync>;

/// Fired when the share layer changed session/project state — the hub
/// translates it into a `TaskStateChanged` bump for every client.
pub type TaskNotifier = Arc<dyn Fn() + Send + Sync>;

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
    Remove {
        node_id: String,
        reply: Sender<anyhow::Result<()>>,
    },
    SendFile {
        node_id: String,
        path: PathBuf,
        note: Option<String>,
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
    Shutdown,
}

/// Mutable state shared between the command loop and the protocol
/// callbacks; every mutation re-publishes the wire snapshot.
struct ShareInner {
    store: Arc<Mutex<FriendStore>>,
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
    task_notifier: Option<TaskNotifier>,
    friend_code: String,
}

impl ShareInner {
    fn snapshot(&self) -> FriendsState {
        let store = self.store.lock();
        FriendsState {
            friend_code: self.friend_code.clone(),
            friends: store
                .friends
                .values()
                .map(|f| {
                    let id = f.node_id.to_string();
                    let online = self
                        .probes
                        .get(&id)
                        .is_some_and(|(ok, at)| {
                            *ok && now_ms().saturating_sub(*at) < PROBE_CACHE_MS
                        });
                    FriendInfo {
                        node_id: id,
                        name: f.name.clone(),
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
        Self {
            dir,
            our_name,
            cmd: Mutex::new(None),
            state: Arc::new(Mutex::new(ShareInner {
                store: Arc::new(Mutex::new(FriendStore::default())),
                transfers: Vec::new(),
                probes: HashMap::new(),
                pending: HashMap::new(),
                outgoing_tickets: HashMap::new(),
                outgoing_tags: HashMap::new(),
                transfer_hook: None,
                task_notifier: None,
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

    /// Where the server installs the `TaskStateChanged` bump.
    pub fn set_task_notifier(&self, notifier: TaskNotifier) {
        self.state.lock().task_notifier = Some(notifier);
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
        store
            .requests
            .retain(|r| !(r.node_id == id && !r.incoming));
        if store.requests.len() == before {
            anyhow::bail!("no outgoing request to that code");
        }
        let _ = store.save();
        drop(store);
        drop(inner);
        publish(&self.state, &self.sink);
        Ok(())
    }

    pub fn remove_friend(&self, node_id: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::Remove { node_id, reply })
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

    pub fn cancel_transfer(&self, id: Uuid) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::CancelTransfer { id, reply })
    }

    pub fn probe_friend(&self, node_id: String) -> anyhow::Result<()> {
        self.call(|reply| ShareCommand::Probe { node_id, reply })
    }

    pub fn shutdown(&self) {
        if let Some(tx) = self.cmd.lock().take() {
            let _ = tx.send(ShareCommand::Shutdown);
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
        state.lock().friend_code = friend_code;
        publish(&state, &sink);

        // The ShareNode Arc is filled after spawn; offer tasks wait on it.
        let node: Arc<tokio::sync::Mutex<Option<Arc<ShareNode>>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        let proto = FriendsProtocol::new(
            // on_request: register pending, broadcast, hand back a oneshot.
            {
                let state = state.clone();
                let sink = sink.clone();
                let our_name = our_name.clone();
                Arc::new(move |id: EndpointId, name: String| {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    {
                        let mut s = state.lock();
                        if s.store.lock().is_friend(&id) {
                            // Re-adding an existing friend: accept silently.
                            let _ = tx.send(RequestDecision::Accept {
                                our_name: our_name.clone(),
                            });
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
        );

        let share_node = match ShareNode::spawn(&dir, secret, RelayMode::Default, proto).await {
            Ok(n) => Arc::new(n),
            Err(e) => {
                drop(ready_tx.send(Err(e)));
                return Ok(());
            }
        };
        drop(ready_tx.send(Ok(())));
        *node.lock().await = Some(share_node.clone());

        while let Some(cmd) = async_rx.recv().await {
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
                ShareCommand::Remove { node_id, reply } => {
                    {
                        let s = state.lock();
                        if let Ok(id) = node_id.parse::<EndpointId>() {
                            s.store.lock().friends.remove(&id);
                            let _ = s.store.lock().save();
                        }
                    }
                    publish(&state, &sink);
                    let _ = reply.send(Ok(()));
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
            }
        }
        Ok(())
    })
}
