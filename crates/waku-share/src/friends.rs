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
use tokio::sync::Mutex;

/// ALPN for the friends control channel (friend requests, transfer offers).
/// Bumped on breaking wire changes; v0.
pub const ALPN_FRIENDS: &[u8] = b"goddard/friends/0";

const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Friend {
    pub name: String,
    pub node_id: EndpointId,
    pub added_at_ms: u64,
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
        let data = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn is_friend(&self, node: &EndpointId) -> bool {
        self.friends.contains_key(node)
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
        name: String,
        note: Option<String>,
        ticket: String,
    },
    OfferDecline,
}

/// What the local user decided about an incoming request.
pub enum RequestDecision {
    Accept { our_name: String },
    Decline,
}

/// Callback the host app installs to decide friend requests. Returning
/// `None` means "no decision yet" — the connection is answered with a
/// pending-style decline for now; v0 keeps this simple and the requester
/// retries after re-prompting.
pub type RequestHandler =
    Arc<dyn Fn(EndpointId, String) -> RequestDecision + Send + Sync>;

/// Acceptor for [`ALPN_FRIENDS`]: reads one request, applies the decision
/// callback, replies. Transfer offers get handed to `offer_handler` once the
/// transfer step lands — until then they're declined.
#[derive(Clone)]
pub struct FriendsProtocol {
    on_request: RequestHandler,
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
    pub fn new(on_request: RequestHandler, store: Arc<Mutex<FriendStore>>) -> Self {
        Self { on_request, store }
    }
}

impl ProtocolHandler for FriendsProtocol {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = conn.remote_id();
        let (mut send, mut recv) = conn.accept_bi().await?;
        let msg = read_message(&mut recv).await.map_err(accept_err)?;
        match msg {
            FriendsMessage::FriendRequest { name } => {
                let reply = match (self.on_request)(remote, name.clone()) {
                    RequestDecision::Accept { our_name } => {
                        {
                            let mut store = self.store.lock().await;
                            store.friends.insert(
                                remote,
                                Friend {
                                    name: name.clone(),
                                    node_id: remote,
                                    added_at_ms: now_ms(),
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
            // Offers arrive in the transfer step; refuse politely for now.
            FriendsMessage::Offer { .. } => {
                write_message(&mut send, &FriendsMessage::OfferDecline)
                    .await
                    .map_err(accept_err)?;
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
            let mut store = store.lock().await;
            store.friends.insert(
                remote,
                Friend {
                    name: name.clone(),
                    node_id: remote,
                    added_at_ms: now_ms(),
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
