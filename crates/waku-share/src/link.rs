//! Daemon link protocol — the "what are you and may I pair" channel on the
//! shared iroh endpoint. LAN discovery (see `crate::discover`) finds the
//! endpoint by `EndpointId`; this ALPN answers what the endpoint is (daemon
//! name, reachable WebSocket port) and brokers pairing: an approved request
//! returns a daemon token the caller can then use on the normal WebSocket
//! protocol. Both run inside iroh's endpoint-to-endpoint encryption, so no
//! bearer material crosses the LAN in cleartext.

use std::sync::Arc;

use anyhow::{Context as _, bail};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};

/// The control ALPN a client dials after discovering a waku endpoint.
pub const ALPN_WAKU_LINK: &[u8] = b"goddard/link/0";

const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// How long a pending pair request may sit unanswered before the protocol
/// declines it — bounds the stream, not the user's decision latency.
const PAIR_DECISION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// What a waku endpoint reports about itself. `ws_port` is `Some` only when
/// the daemon is reachable off-loopback — discovered endpoints that share
/// files but expose no daemon surface report `None`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DaemonInfo {
    pub name: String,
    pub ws_port: Option<u16>,
    pub protocol_version: u32,
    /// The pairing daemon's stable identity — the endpoint's own id,
    /// carried so a client that later sees this daemon over plain mDNS can
    /// confirm it found the same install.
    pub endpoint_id: String,
}

/// What the local user decided about an incoming pair request.
pub enum PairDecision {
    /// Grant a full client token. The daemon mints and records it; the
    /// caller uses it as the `hello` bearer on the WebSocket protocol.
    Grant { token: String },
    Decline,
}

/// Callback the host installs to answer pair requests. Returning a oneshot
/// keeps the UI free to take minutes — timeout declines.
pub type PairHandler =
    Arc<dyn Fn(EndpointId, String) -> tokio::sync::oneshot::Receiver<PairDecision> + Send + Sync>;

/// Supplies the [`DaemonInfo`] payload fresh on every `Info` query — the
/// daemon's exposed port changes with its exposure setting.
pub type InfoHandler = Arc<dyn Fn() -> DaemonInfo + Send + Sync>;

#[derive(Clone, Debug, Deserialize, Serialize)]
enum LinkMessage {
    Info,
    InfoReply(Box<DaemonInfo>),
    Pair { device_name: String },
    PairGranted { token: String },
    PairDeclined,
}

/// Acceptor for [`ALPN_WAKU_LINK`]: `Info` answers from the installed
/// handler; `Pair` registers a pending request and awaits the user's
/// decision on a oneshot, mirroring the friend-request pattern.
#[derive(Clone)]
pub struct LinkProtocol {
    info: InfoHandler,
    on_pair: PairHandler,
}

impl std::fmt::Debug for LinkProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkProtocol").finish_non_exhaustive()
    }
}

impl LinkProtocol {
    pub fn new(info: InfoHandler, on_pair: PairHandler) -> Self {
        Self { info, on_pair }
    }
}

fn accept_err(e: anyhow::Error) -> AcceptError {
    AcceptError::from(std::io::Error::other(e))
}

impl ProtocolHandler for LinkProtocol {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = conn.remote_id();
        let (mut send, mut recv) = conn.accept_bi().await?;
        let msg = read_message(&mut recv).await.map_err(accept_err)?;
        match msg {
            LinkMessage::Info => {
                let reply = LinkMessage::InfoReply(Box::new((self.info)()));
                write_message(&mut send, &reply).await.map_err(accept_err)?;
                send.finish()?;
            }
            LinkMessage::Pair { device_name } => {
                let decision = match tokio::time::timeout(
                    PAIR_DECISION_TIMEOUT,
                    (self.on_pair)(remote, device_name),
                )
                .await
                {
                    Ok(Ok(decision)) => decision,
                    _ => PairDecision::Decline,
                };
                let reply = match decision {
                    PairDecision::Grant { token } => LinkMessage::PairGranted { token },
                    PairDecision::Decline => LinkMessage::PairDeclined,
                };
                write_message(&mut send, &reply).await.map_err(accept_err)?;
                send.finish()?;
            }
            _ => send.finish()?,
        }
        // Mirror the friends acceptor: wait for the peer to close so our
        // reply isn't discarded with the connection.
        conn.closed().await;
        Ok(())
    }

    async fn shutdown(&self) {}
}

/// Ask a discovered endpoint what it is. `None` for non-waku endpoints and
/// unreachable peers — callers treat them as "not a daemon".
pub async fn fetch_info(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
) -> anyhow::Result<DaemonInfo> {
    let conn = endpoint.connect(addr.into(), ALPN_WAKU_LINK).await?;
    let (mut send, mut recv) = conn.open_bi().await?;
    write_message(&mut send, &LinkMessage::Info).await?;
    send.finish()?;
    let reply = read_message(&mut recv).await;
    conn.close(0u32.into(), b"done");
    match reply? {
        LinkMessage::InfoReply(info) => Ok(*info),
        _ => bail!("unexpected reply to Info"),
    }
}

/// The outcome of a pair request as the caller sees it.
#[derive(Clone, Debug)]
pub enum PairOutcome {
    /// The remote user approved; `token` is a bearer for the daemon's
    /// WebSocket `hello`.
    Granted { token: String },
    Declined,
}

/// Send a pair request naming `device_name` and await the remote user's
/// decision. The stream stays open until they answer, so `timeout` bounds
/// the whole exchange.
pub async fn request_pair(
    endpoint: &Endpoint,
    addr: impl Into<EndpointAddr>,
    device_name: &str,
    timeout: std::time::Duration,
) -> anyhow::Result<PairOutcome> {
    let fut = async {
        let conn = endpoint.connect(addr.into(), ALPN_WAKU_LINK).await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        write_message(
            &mut send,
            &LinkMessage::Pair {
                device_name: device_name.to_string(),
            },
        )
        .await?;
        send.finish()?;
        let reply = read_message(&mut recv).await;
        conn.close(0u32.into(), b"done");
        anyhow::Ok(match reply? {
            LinkMessage::PairGranted { token } => PairOutcome::Granted { token },
            LinkMessage::PairDeclined => PairOutcome::Declined,
            _ => bail!("unexpected reply to Pair"),
        })
    };
    tokio::time::timeout(timeout, fut)
        .await
        .context("pair request timed out")?
}

async fn write_message(send: &mut SendStream, msg: &LinkMessage) -> anyhow::Result<()> {
    let data = serde_json::to_vec(msg)?;
    send.write_all(&(data.len() as u32).to_be_bytes()).await?;
    send.write_all(&data).await?;
    Ok(())
}

async fn read_message(recv: &mut RecvStream) -> anyhow::Result<LinkMessage> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_MESSAGE_BYTES {
        bail!("link message too large: {len}");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf)?)
}
