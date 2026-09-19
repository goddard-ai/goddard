//! Friend list persistence and the friend-request handshake.
//!
//! Friends are mutual: a request only becomes a friend when the remote side
//! accepts. The handshake runs on [`ALPN_FRIENDS`] over the same QUIC
//! connection security as file transfer — the peer's `EndpointId` is
//! authenticated by the transport itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, bail};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use serde::{Deserialize, Serialize};
use parking_lot::Mutex;

/// ALPN for the friends control channel (friend requests, transfer offers,
/// project shares). Bumped on breaking wire changes; v1 adds
/// `ShareProjects`/`SyncEnabled`/`SyncDisabled`/`PushNotice`.
pub const ALPN_FRIENDS: &[u8] = b"goddard/friends/1";

const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Friend {
    pub name: String,
    pub node_id: EndpointId,
    pub added_at_ms: u64,
    /// Last successful contact (probe, request, offer, or done). Presence is
    /// derived from this — there is no heartbeat.
    #[serde(default)]
    pub last_seen_ms: Option<u64>,
    /// Local-only override for `name` — never leaves this install. The
    /// friend still reports their own display name; the nickname is what
    /// this side renders and names transfer links with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

impl Friend {
    /// What the local user sees: nickname override, else the self-reported
    /// name.
    pub fn display_name(&self) -> &str {
        self.nickname
            .as_deref()
            .filter(|n| !n.is_empty())
            .unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    /// Who we asked, or who asked us.
    pub name: String,
    pub node_id: EndpointId,
    pub incoming: bool,
    pub at_ms: u64,
}

/// Persisted friend state — `friends.json` under the Goddard data dir,
/// written atomically like the rest of the app's JSON state.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FriendStore {
    /// The name friends see on our requests and offers.
    #[serde(default)]
    pub display_name: String,
    pub friends: BTreeMap<EndpointId, Friend>,
    pub requests: Vec<PendingRequest>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl FriendStore {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join("friends.json");
        let mut store = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        store.path = Some(path);
        Ok(store)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn is_friend(&self, node: &EndpointId) -> bool {
        self.friends.contains_key(node)
    }

    /// Record a successful contact — probes, offers, done notifications.
    /// Presence renders from this, never from a heartbeat.
    pub fn mark_seen(&mut self, node: &EndpointId) {
        if let Some(friend) = self.friends.get_mut(node) {
            friend.last_seen_ms = Some(now_ms());
        }
    }

    /// Store a local nickname override; `None` or blank clears it.
    pub fn set_nickname(&mut self, node: &EndpointId, nickname: Option<String>) {
        if let Some(friend) = self.friends.get_mut(node) {
            friend.nickname = nickname
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty());
        }
    }

    /// The name this install shows for `node`: nickname, else the
    /// self-reported name, else `fallback` (or "friend" when that's empty).
    pub fn resolved_name(&self, node: &EndpointId, fallback: &str) -> String {
        self.friends
            .get(node)
            .map(|f| f.display_name().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                if fallback.is_empty() {
                    "friend".to_string()
                } else {
                    fallback.to_string()
                }
            })
    }
}

/// Wire messages on the friends channel. JSON + u32 length prefix; the
/// version lives in the ALPN so old peers fail fast on connect.
#[derive(Debug, Serialize, Deserialize)]
pub enum FriendsMessage {
    /// "Add me as a friend." `name` is self-reported display text.
    FriendRequest { name: String },
    FriendAccept { name: String },
    FriendDecline,
    /// File offer — filled in by the transfer step. `ticket` is a
    /// sendme-compatible blob ticket string; `note` is the sender's message.
    Offer {
        /// Sender's display name.
        name: String,
        /// File or folder name being offered.
        file_name: String,
        /// Declared byte size — the receiver renders progress against it.
        size: u64,
        note: Option<String>,
        ticket: String,
    },
    /// Receiver started the download. Carries the manifest so the sender's
    /// UI can show what was accepted.
    OfferAccept,
    OfferDecline,
    /// Download finished (hash verified). Sent on a second connection after
    /// the blobs fetch completes.
    TransferDone { ticket: String },
    /// Generic acknowledgement for one-way messages; needed because closing
    /// the connection right after `finish()` can drop unflushed stream data.
    Ack,
    /// Liveness probe — presence is lazy (dial on demand), never heartbeated.
    Ping,
    /// The sender's full shared-repo set for us — replaces what we
    /// recorded, so removing a repo from the list unshares it.
    ShareProjects {
        projects: Vec<crate::projects::SharedRepo>,
    },
    /// Sender enabled automatic sync for `origin_url` on a repo we shared
    /// with them — the link becomes mutual.
    SyncEnabled { origin_url: String },
    /// Sender disabled sync for `origin_url` — both sides tear the link
    /// down.
    SyncDisabled { origin_url: String },
    /// Sender pushed commits to the shared origin on these branches —
    /// the recipient fetches and integrates. A hint: the receiver still
    /// verifies against origin.
    PushNotice {
        origin_url: String,
        branches: Vec<String>,
    },
    /// Sender moved refs that aren't branches — `refs/notes/qa` for QA
    /// review state. The recipient fetches them so review surfaces stay
    /// fresh; a hint, same as `PushNotice`.
    RefNotice {
        origin_url: String,
        refs: Vec<String>,
    },
}

/// What the local user decided about an incoming request.
pub enum RequestDecision {
    Accept { our_name: String },
    Decline,
}

/// Callback the host app installs to decide friend requests. It returns a
/// oneshot the acceptor awaits — the UI can take minutes to answer without
/// blocking the protocol task. Timeout defaults to decline.
pub type RequestHandler = Arc<
    dyn Fn(EndpointId, String) -> tokio::sync::oneshot::Receiver<RequestDecision>
        + Send
        + Sync,
>;

/// An incoming transfer offer, already parsed and sender-authenticated.
pub struct OfferInfo {
    pub from: EndpointId,
    pub name: String,
    pub file_name: String,
    pub size: u64,
    pub note: Option<String>,
    pub ticket: String,
}

/// Fired when a friend offers a file. The handler owns spawning the actual
/// blobs download — this callback must return fast.
pub type OfferHandler = Arc<dyn Fn(OfferInfo) + Send + Sync>;

/// Fired when the receiver reports a finished, verified download.
pub type DoneHandler = Arc<dyn Fn(EndpointId, String) + Send + Sync>;

/// Fired when a friend sends their full shared-repo set — replaces what
/// we recorded for that peer.
pub type ShareListHandler =
    Arc<dyn Fn(EndpointId, Vec<crate::projects::SharedRepo>) + Send + Sync>;

/// Fired when a friend enables or disables sync on a repo we shared
/// with them. `enabled` distinguishes `SyncEnabled` from `SyncDisabled`.
pub type SyncStateHandler = Arc<dyn Fn(EndpointId, String, bool) + Send + Sync>;

/// Fired when a friend reports pushing to a shared origin — the receiver
/// fetches and integrates its enabled branches.
pub type PushHandler = Arc<dyn Fn(EndpointId, String, Vec<String>) + Send + Sync>;

/// Fired when a friend reports moving non-branch refs on a shared
/// origin — e.g. `refs/notes/qa` review records. The receiver fetches
/// them and refreshes any surface that reads them.
pub type RefHandler = Arc<dyn Fn(EndpointId, String, Vec<String>) + Send + Sync>;

/// Acceptor for [`ALPN_FRIENDS`]: reads one message, dispatches to the
/// installed handler, replies. Offers from non-friends are declined;
/// offers from friends are auto-accepted (per the product decision) and the
/// fetch is kicked off by `on_offer`.
#[derive(Clone)]
pub struct FriendsProtocol {
    on_request: RequestHandler,
    on_offer: OfferHandler,
    on_done: DoneHandler,
    on_share: Option<ShareListHandler>,
    on_sync_state: Option<SyncStateHandler>,
    on_push: Option<PushHandler>,
    on_refs: Option<RefHandler>,
    store: Arc<Mutex<FriendStore>>,
}

impl std::fmt::Debug for FriendsProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FriendsProtocol").finish_non_exhaustive()
    }
}

fn accept_err(e: anyhow::Error) -> AcceptError {
    AcceptError::from(std::io::Error::other(e))
}

impl FriendsProtocol {
    pub fn new(
        on_request: RequestHandler,
        on_offer: OfferHandler,
        on_done: DoneHandler,
        store: Arc<Mutex<FriendStore>>,
    ) -> Self {
        Self {
            on_request,
            on_offer,
            on_done,
            on_share: None,
            on_sync_state: None,
            on_push: None,
            on_refs: None,
            store,
        }
    }

    /// Install the project-sharing handlers. Separate from `new` so the
    /// spike and tests can run the request/offer flow without them.
    pub fn with_project_handlers(
        mut self,
        on_share: ShareListHandler,
        on_sync_state: SyncStateHandler,
        on_push: PushHandler,
        on_refs: RefHandler,
    ) -> Self {
        self.on_share = Some(on_share);
        self.on_sync_state = Some(on_sync_state);
        self.on_push = Some(on_push);
        self.on_refs = Some(on_refs);
        self
    }

    /// Friend-gate + mark-seen shared by the one-way messages; returns
    /// false for non-friends (whose messages get no reply and a closed
    /// stream).
    fn touch_friend(&self, remote: &EndpointId) -> bool {
        let mut store = self.store.lock();
        let is_friend = store.is_friend(remote);
        if is_friend {
            store.mark_seen(remote);
            let _ = store.save();
        }
        is_friend
    }
}

impl ProtocolHandler for FriendsProtocol {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = conn.remote_id();
        let (mut send, mut recv) = conn.accept_bi().await?;
        let msg = read_message(&mut recv).await.map_err(accept_err)?;
        match msg {
            FriendsMessage::FriendRequest { name } => {
                let decision_rx = (self.on_request)(remote, name.clone());
                let decision = match tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    decision_rx,
                )
                .await
                {
                    Ok(Ok(decision)) => decision,
                    // Timed out or the request was dropped — decline.
                    _ => RequestDecision::Decline,
                };
                let reply = match decision {
                    RequestDecision::Accept { our_name } => {
                        {
                            let mut store = self.store.lock();
                            store.friends.insert(
                                remote,
                                Friend {
                                    name: name.clone(),
                                    node_id: remote,
                                    added_at_ms: now_ms(),
                                    last_seen_ms: Some(now_ms()),
                                    nickname: None,
                                },
                            );
                            store.requests.retain(|r| r.node_id != remote);
                            let _ = store.save();
                        }
                        FriendsMessage::FriendAccept { name: our_name }
                    }
                    RequestDecision::Decline => FriendsMessage::FriendDecline,
                };
                write_message(&mut send, &reply).await.map_err(accept_err)?;
                send.finish()?;
            }
            FriendsMessage::Offer { name, file_name, size, note, ticket } => {
                let is_friend = {
                    let mut store = self.store.lock();
                    let is_friend = store.is_friend(&remote);
                    if is_friend {
                        store.mark_seen(&remote);
                        let _ = store.save();
                    }
                    is_friend
                };
                if is_friend {
                    (self.on_offer)(OfferInfo {
                        from: remote,
                        name,
                        file_name,
                        size,
                        note,
                        ticket,
                    });
                    write_message(&mut send, &FriendsMessage::OfferAccept)
                        .await
                        .map_err(accept_err)?;
                } else {
                    write_message(&mut send, &FriendsMessage::OfferDecline)
                        .await
                        .map_err(accept_err)?;
                }
                send.finish()?;
            }
            FriendsMessage::Ping => {
                write_message(&mut send, &FriendsMessage::Ack)
                    .await
                    .map_err(accept_err)?;
                send.finish()?;
            }
            FriendsMessage::TransferDone { ticket } => {
                {
                    let mut store = self.store.lock();
                    store.mark_seen(&remote);
                    let _ = store.save();
                }
                (self.on_done)(remote, ticket);
                write_message(&mut send, &FriendsMessage::Ack)
                    .await
                    .map_err(accept_err)?;
                send.finish()?;
            }
            FriendsMessage::ShareProjects { projects } => {
                if self.touch_friend(&remote) {
                    if let Some(on_share) = &self.on_share {
                        on_share(remote, projects);
                    }
                    write_message(&mut send, &FriendsMessage::Ack)
                        .await
                        .map_err(accept_err)?;
                }
                send.finish()?;
            }
            FriendsMessage::SyncEnabled { origin_url } => {
                if self.touch_friend(&remote) {
                    if let Some(on_sync_state) = &self.on_sync_state {
                        on_sync_state(remote, origin_url, true);
                    }
                    write_message(&mut send, &FriendsMessage::Ack)
                        .await
                        .map_err(accept_err)?;
                }
                send.finish()?;
            }
            FriendsMessage::SyncDisabled { origin_url } => {
                if self.touch_friend(&remote) {
                    if let Some(on_sync_state) = &self.on_sync_state {
                        on_sync_state(remote, origin_url, false);
                    }
                    write_message(&mut send, &FriendsMessage::Ack)
                        .await
                        .map_err(accept_err)?;
                }
                send.finish()?;
            }
            FriendsMessage::PushNotice {
                origin_url,
                branches,
            } => {
                if self.touch_friend(&remote) {
                    if let Some(on_push) = &self.on_push {
                        on_push(remote, origin_url, branches);
                    }
                    write_message(&mut send, &FriendsMessage::Ack)
                        .await
                        .map_err(accept_err)?;
                }
                send.finish()?;
            }
            FriendsMessage::RefNotice { origin_url, refs } => {
                if self.touch_friend(&remote) {
                    if let Some(on_refs) = &self.on_refs {
                        on_refs(remote, origin_url, refs);
                    }
                    write_message(&mut send, &FriendsMessage::Ack)
                        .await
                        .map_err(accept_err)?;
                }
                send.finish()?;
            }
            _ => send.finish()?,
        }
        // Returning from accept lets the router close the connection, which
        // would discard our reply before the requester reads it. Wait for the
        // peer to close instead.
        conn.closed().await;
        Ok(())
    }

    async fn shutdown(&self) {}
}

/// Ask `addr` to add us. On accept, records them as a friend locally and
/// returns their self-reported name.
pub async fn send_friend_request(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    our_name: &str,
    store: &Arc<Mutex<FriendStore>>,
) -> anyhow::Result<String> {
    let conn = endpoint.connect(addr.into(), ALPN_FRIENDS).await?;
    let remote = conn.remote_id();
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(
        &mut send,
        &FriendsMessage::FriendRequest {
            name: our_name.to_string(),
        },
    )
    .await?;
    send.finish()?;
    let reply = read_message(&mut recv).await;
    // Let the acceptor's `conn.closed()` resolve promptly.
    conn.close(0u32.into(), b"done");
    match reply? {
        FriendsMessage::FriendAccept { name } => {
            let mut store = store.lock();
            store.friends.insert(
                remote,
                Friend {
                    name: name.clone(),
                    node_id: remote,
                    added_at_ms: now_ms(),
                    last_seen_ms: Some(now_ms()),
                    nickname: None,
                },
            );
            store.requests.retain(|r| r.node_id != remote);
            store.save()?;
            Ok(name)
        }
        FriendsMessage::FriendDecline => bail!("friend request declined"),
        _ => bail!("unexpected reply to friend request"),
    }
}

/// Offer `ticket` to a friend. Returns Ok on OfferAccept — the receiver has
/// started the fetch; completion arrives back via `TransferDone`.
pub async fn send_offer(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    our_name: &str,
    file_name: &str,
    size: u64,
    note: Option<String>,
    ticket: &str,
) -> anyhow::Result<()> {
    let conn = endpoint.connect(addr.into(), ALPN_FRIENDS).await?;
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(
        &mut send,
        &FriendsMessage::Offer {
            name: our_name.to_string(),
            file_name: file_name.to_string(),
            size,
            note,
            ticket: ticket.to_string(),
        },
    )
    .await?;
    send.finish()?;
    let reply = read_message(&mut recv).await;
    conn.close(0u32.into(), b"done");
    match reply? {
        FriendsMessage::OfferAccept => Ok(()),
        FriendsMessage::OfferDecline => bail!("offer declined"),
        _ => bail!("unexpected reply to offer"),
    }
}

/// Dial a friend to check liveness. This is the only presence mechanism —
/// callers decide when a surface needs it (opening the Friends tab, Send),
/// cache the result briefly, and render `last_seen` on miss.
pub async fn probe(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    timeout: std::time::Duration,
) -> bool {
    let fut = async {
        let conn = endpoint.connect(addr.into(), ALPN_FRIENDS).await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        write_message(&mut send, &FriendsMessage::Ping).await?;
        send.finish()?;
        let reply = read_message(&mut recv).await;
        conn.close(0u32.into(), b"done");
        anyhow::Ok(matches!(reply?, FriendsMessage::Ack))
    };
    matches!(tokio::time::timeout(timeout, fut).await, Ok(Ok(true)))
}

/// Tell the original sender the download finished and verified. Waits for
/// the ack — closing right after `finish()` can drop the unflushed write.
pub async fn notify_transfer_done(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    ticket: &str,
) -> anyhow::Result<()> {
    let conn = endpoint.connect(addr.into(), ALPN_FRIENDS).await?;
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(
        &mut send,
        &FriendsMessage::TransferDone {
            ticket: ticket.to_string(),
        },
    )
    .await?;
    send.finish()?;
    let reply = read_message(&mut recv).await;
    conn.close(0u32.into(), b"done");
    match reply? {
        FriendsMessage::Ack => Ok(()),
        _ => bail!("unexpected reply to TransferDone"),
    }
}

/// Send a one-way message that expects an `Ack` back.
async fn send_acked(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    msg: &FriendsMessage,
) -> anyhow::Result<()> {
    let conn = endpoint.connect(addr.into(), ALPN_FRIENDS).await?;
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(&mut send, msg).await?;
    send.finish()?;
    let reply = read_message(&mut recv).await;
    conn.close(0u32.into(), b"done");
    match reply? {
        FriendsMessage::Ack => Ok(()),
        _ => bail!("unexpected reply to share message"),
    }
}

/// Publish our full shared-repo set to a friend — the receiver replaces
/// what it recorded, so shrinking the list unshares what was removed.
pub async fn send_share_projects(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    projects: &[crate::projects::SharedRepo],
) -> anyhow::Result<()> {
    send_acked(
        endpoint,
        addr,
        &FriendsMessage::ShareProjects {
            projects: projects.to_vec(),
        },
    )
    .await
}

/// Tell `addr` we enabled (`true`) or disabled (`false`) sync on a repo
/// they shared with us. Enable creates their side of the link; disable
/// tears both down.
pub async fn send_sync_state(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    origin_url: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    let msg = if enabled {
        FriendsMessage::SyncEnabled {
            origin_url: origin_url.to_string(),
        }
    } else {
        FriendsMessage::SyncDisabled {
            origin_url: origin_url.to_string(),
        }
    };
    send_acked(endpoint, addr, &msg).await
}

/// Tell a friend we pushed to a shared origin — they fetch and integrate
/// their enabled branches.
pub async fn send_push_notice(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    origin_url: &str,
    branches: &[String],
) -> anyhow::Result<()> {
    send_acked(
        endpoint,
        addr,
        &FriendsMessage::PushNotice {
            origin_url: origin_url.to_string(),
            branches: branches.to_vec(),
        },
    )
    .await
}

/// Tell a friend non-branch refs moved on a shared origin — they fetch
/// them and refresh surfaces that read them.
pub async fn send_ref_notice(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    origin_url: &str,
    refs: &[String],
) -> anyhow::Result<()> {
    send_acked(
        endpoint,
        addr,
        &FriendsMessage::RefNotice {
            origin_url: origin_url.to_string(),
            refs: refs.to_vec(),
        },
    )
    .await
}

async fn write_message(
    send: &mut iroh::endpoint::SendStream,
    msg: &FriendsMessage,
) -> anyhow::Result<()> {
    let data = serde_json::to_vec(msg)?;
    send.write_all(&(data.len() as u32).to_be_bytes()).await?;
    send.write_all(&data).await?;
    Ok(())
}

async fn read_message(
    recv: &mut iroh::endpoint::RecvStream,
) -> anyhow::Result<FriendsMessage> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_MESSAGE_BYTES {
        bail!("friends message too large: {len}");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf)?)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
