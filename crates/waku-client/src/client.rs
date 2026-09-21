use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};
use uuid::Uuid;

use waku_protocol::MAX_WIRE_MESSAGE_BYTES;
use waku_protocol::{
    ClientMessage, Command, DaemonSettings, PROTOCOL_VERSION, ReplayCursor, Request,
    ResponseOutcome, ResponsePayload, RpcError, SequencedEvent, ServerMessage,
    WorkspaceOperation,
};

const READ_POLL_INTERVAL: Duration = Duration::from_millis(25);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Operations that walk or rebuild an entire worktree — snapshots,
/// checkouts, restores — legitimately take minutes on a large repository.
/// The daemon keeps working after the client stops waiting, so a tighter
/// bound only orphans a result that lands anyway.
const WORKTREE_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_BUFFERED_EVENTS_PER_RUNTIME: usize = 4096;

/// The daemon answers most requests quickly, but anything that snapshots or
/// rebuilds a worktree — checkpoint captures, worktree creation, a rewind's
/// safety snapshot and restore — is bounded by repository size, not latency.
fn request_timeout(command: &Command) -> Duration {
    match command {
        Command::Workspace { operation } => match operation {
            WorkspaceOperation::CaptureTurn { .. }
            | WorkspaceOperation::CaptureTurnStart { .. }
            | WorkspaceOperation::CreateWorktree { .. }
            | WorkspaceOperation::CreateWorktreeFromCheckout { .. }
            | WorkspaceOperation::EnsureWorktree { .. }
            | WorkspaceOperation::RemoveWorktree { .. }
            | WorkspaceOperation::PruneWorktrees { .. }
            | WorkspaceOperation::ResetWorktree { .. }
            | WorkspaceOperation::ArchiveProjectlessWorkspace { .. }
            | WorkspaceOperation::RestoreProjectlessWorkspace { .. } => {
                WORKTREE_REQUEST_TIMEOUT
            }
            _ => REQUEST_TIMEOUT,
        },
        Command::RewindSessionToMessage { .. } => WORKTREE_REQUEST_TIMEOUT,
        _ => REQUEST_TIMEOUT,
    }
}

enum Outgoing {
    Message(ClientMessage),
    Shutdown,
}

struct ClientInner {
    outgoing: Sender<Outgoing>,
    /// The address this connection was opened against — the one a webhook
    /// URL must point at to reach the same daemon.
    address: String,
    daemon_version: String,
    daemon_commit: Option<String>,
    pending: Mutex<HashMap<Uuid, Sender<Result<ResponsePayload, RpcError>>>>,
    sessions: Mutex<HashMap<(Uuid, Uuid), Sender<SequencedEvent>>>,
    /// Friend-session watchers: deliver every event for the session
    /// regardless of the sharer's runtime id — runtimes rotate on the
    /// friend's side and a watcher follows them all.
    session_watchers: Mutex<HashMap<Uuid, Vec<Sender<SequencedEvent>>>>,
    pending_events: Mutex<HashMap<(Uuid, Uuid), VecDeque<SequencedEvent>>>,
    task_state_subscribers: Mutex<Vec<Sender<u64>>>,
    settings_subscribers: Mutex<Vec<Sender<DaemonSettings>>>,
    friends_subscribers: Mutex<Vec<Sender<waku_protocol::friends::FriendsState>>>,
    pairing_subscribers: Mutex<Vec<Sender<waku_protocol::pairing::PairingState>>>,
    automations_subscribers: Mutex<Vec<Sender<waku_protocol::automations::AutomationsState>>>,
    /// `ReviewChanged` broadcasts — the origin URL whose QA state moved.
    review_subscribers: Mutex<Vec<Sender<String>>>,
    /// `(session_id, revoked)` — a watched friend session's stream ended.
    friend_session_closed_subscribers: Mutex<Vec<Sender<(Uuid, bool)>>>,
    last_sequences: Mutex<HashMap<(Uuid, Uuid), LastSequence>>,
    disconnected: AtomicBool,
}

#[derive(Clone, Copy)]
struct LastSequence {
    epoch: Uuid,
    sequence: u64,
}

#[derive(Clone)]
pub struct DaemonClient {
    inner: Arc<ClientInner>,
}

impl DaemonClient {
    pub fn connect(address: &str, token: String) -> anyhow::Result<Self> {
        Self::connect_with_resume(address, token, Vec::new())
    }

    pub fn connect_with_resume(
        address: &str,
        token: String,
        resume_from: Vec<ReplayCursor>,
    ) -> anyhow::Result<Self> {
        let last_sequences = resume_from
            .iter()
            .map(|cursor| {
                (
                    (cursor.session_id, cursor.runtime_id),
                    LastSequence {
                        epoch: cursor.epoch,
                        sequence: cursor.sequence,
                    },
                )
            })
            .collect();
        let url = daemon_url(address)?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_WIRE_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_WIRE_MESSAGE_BYTES));
        let (mut socket, _) =
            tungstenite::client::connect_with_config(url.as_str(), Some(config), 3)
                .context("could not connect to Goddard daemon")?;
        set_client_read_timeout(&mut socket, Some(Duration::from_secs(5)))?;
        write_json(
            &mut socket,
            &ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                token,
                client_id: Uuid::new_v4(),
                resume_from,
            },
        )?;
        let hello = read_server_message(&mut socket)?;
        let (daemon_version, daemon_commit) = match hello {
            ServerMessage::Hello {
                protocol_version,
                daemon_version,
                daemon_commit,
            } if protocol_version == PROTOCOL_VERSION => (daemon_version, daemon_commit),
            ServerMessage::Hello {
                protocol_version, ..
            } => bail!(
                "daemon protocol {protocol_version} does not match desktop protocol {PROTOCOL_VERSION}"
            ),
            ServerMessage::Rejected { message } => bail!("daemon rejected connection: {message}"),
            other => bail!("daemon sent an invalid handshake response: {other:?}"),
        };
        set_client_read_timeout(&mut socket, Some(READ_POLL_INTERVAL))?;

        let (outgoing, outgoing_rx) = unbounded();
        let inner = Arc::new(ClientInner {
            outgoing,
            address: address.to_owned(),
            daemon_version,
            daemon_commit,
            pending: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            session_watchers: Mutex::new(HashMap::new()),
            pending_events: Mutex::new(HashMap::new()),
            task_state_subscribers: Mutex::new(Vec::new()),
            settings_subscribers: Mutex::new(Vec::new()),
            friends_subscribers: Mutex::new(Vec::new()),
            pairing_subscribers: Mutex::new(Vec::new()),
            automations_subscribers: Mutex::new(Vec::new()),
            review_subscribers: Mutex::new(Vec::new()),
            friend_session_closed_subscribers: Mutex::new(Vec::new()),
            last_sequences: Mutex::new(last_sequences),
            disconnected: AtomicBool::new(false),
        });
        let thread_inner = inner.clone();
        std::thread::Builder::new()
            .name("goddard-daemon-client".into())
            .spawn(move || run_client(socket, outgoing_rx, thread_inner))
            .context("could not start Goddard daemon client thread")?;
        Ok(Self { inner })
    }

    /// The address this client connected to — `ws://`/`wss://` URL or a
    /// bare `host:port`, exactly as passed to [`connect`]. Plain-HTTP routes
    /// on the daemon (automation webhooks) share the same port.
    ///
    /// [`connect`]: Self::connect
    pub fn address(&self) -> &str {
        &self.inner.address
    }

    /// The version the connected daemon reported in the hello handshake.
    pub fn daemon_version(&self) -> &str {
        &self.inner.daemon_version
    }

    /// The commit the connected daemon was built from, when it reported one.
    pub fn daemon_commit(&self) -> Option<&str> {
        self.inner.daemon_commit.as_deref()
    }

    pub fn subscribe(&self, session_id: Uuid, runtime_id: Uuid) -> Receiver<SequencedEvent> {
        let (events, receiver) = unbounded();
        let key = (session_id, runtime_id);
        let mut sessions = self.inner.sessions.lock();
        sessions.insert(key, events.clone());
        // Keep the subscription lock while draining the pre-subscription
        // replay queue. The socket thread takes these locks in the same order,
        // so a new live event cannot overtake older replayed events here.
        if let Some(buffered) = self.inner.pending_events.lock().remove(&key) {
            for event in buffered {
                let _ = events.send(event);
            }
        }
        receiver
    }

    /// Every event for `session_id` under any runtime — the watch side of
    /// a friend's shared session, where the sharer's runtime ids are
    /// foreign and rotate on reconnect.
    pub fn subscribe_session_events(&self, session_id: Uuid) -> Receiver<SequencedEvent> {
        let (events, receiver) = unbounded();
        self.inner
            .session_watchers
            .lock()
            .entry(session_id)
            .or_default()
            .push(events);
        receiver
    }

    /// Whether two handles send through the same WebSocket connection.
    ///
    /// The daemon supervisor publishes replacement clients after a managed
    /// restart. Runtime adapters use this identity check to ignore the
    /// subscription's initial snapshot and wait for an actual replacement.
    pub fn same_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn is_disconnected(&self) -> bool {
        self.inner.disconnected.load(Ordering::Acquire)
    }

    /// Cheap liveness check: any daemon reply — even an error — proves the
    /// request pipeline is answering. `false` means the daemon is gone or
    /// unresponsive.
    pub fn probe(&self, timeout: Duration) -> bool {
        if self.inner.disconnected.load(Ordering::Acquire) {
            return false;
        }
        let request_id = Uuid::new_v4();
        let (response, response_rx) = bounded(1);
        self.inner.pending.lock().insert(request_id, response);
        if self
            .inner
            .outgoing
            .send(Outgoing::Message(ClientMessage::Request(Request {
                request_id,
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::GetSettings,
            })))
            .is_err()
        {
            self.inner.pending.lock().remove(&request_id);
            return false;
        }
        match response_rx.recv_timeout(timeout) {
            Ok(_) => true,
            Err(_) => {
                self.inner.pending.lock().remove(&request_id);
                false
            }
        }
    }

    /// Mark this connection dead without waiting for the socket to notice:
    /// pending requests and event subscribers are released exactly as if the
    /// socket had closed, and the socket thread is asked to shut down. Used
    /// when the daemon stops answering while keeping the socket open.
    pub fn force_disconnect(&self) {
        fail_connection(&self.inner);
        let _ = self.inner.outgoing.send(Outgoing::Shutdown);
    }

    pub fn unsubscribe(&self, session_id: Uuid, runtime_id: Uuid) {
        self.inner.sessions.lock().remove(&(session_id, runtime_id));
    }

    pub fn subscribe_task_state(&self) -> Receiver<u64> {
        let (events, receiver) = unbounded();
        self.inner.task_state_subscribers.lock().push(events);
        receiver
    }

    /// Every `settingsChanged` the daemon broadcasts — a client edit or an
    /// agent settings write — lands here as the authoritative document.
    pub fn subscribe_settings(&self) -> Receiver<DaemonSettings> {
        let (events, receiver) = unbounded();
        self.inner.settings_subscribers.lock().push(events);
        receiver
    }

    /// Every `friendsChanged` the daemon broadcasts — requests, offers,
    /// transfer progress — lands here as the authoritative document.
    pub fn subscribe_friends(&self) -> Receiver<waku_protocol::friends::FriendsState> {
        let (events, receiver) = unbounded();
        self.inner.friends_subscribers.lock().push(events);
        receiver
    }

    /// Every `pairingChanged` the daemon broadcasts — pair requests
    /// arriving, resolving, or paired clients being revoked.
    pub fn subscribe_pairing(&self) -> Receiver<waku_protocol::pairing::PairingState> {
        let (events, receiver) = unbounded();
        self.inner.pairing_subscribers.lock().push(events);
        receiver
    }

    /// Every `automationsChanged` the daemon broadcasts — a definition edit
    /// or a run recording progress — lands here as the authoritative
    /// document.
    pub fn subscribe_automations(&self) -> Receiver<waku_protocol::automations::AutomationsState> {
        let (events, receiver) = unbounded();
        self.inner.automations_subscribers.lock().push(events);
        receiver
    }

    /// Every `reviewChanged` the daemon broadcasts — an origin URL whose
    /// QA review state moved, here or on a friend's machine. Review
    /// surfaces re-read their queue on receipt.
    pub fn subscribe_review(&self) -> Receiver<String> {
        let (events, receiver) = unbounded();
        self.inner.review_subscribers.lock().push(events);
        receiver
    }

    /// A watched friend session's stream ended — `(session_id, revoked)`.
    /// `revoked` means the friend turned sharing off or unshared; `false`
    /// is a disconnect or a deleted session.
    pub fn subscribe_friend_session_closed(&self) -> Receiver<(Uuid, bool)> {
        let (events, receiver) = unbounded();
        self.inner
            .friend_session_closed_subscribers
            .lock()
            .push(events);
        receiver
    }

    pub fn request(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        command: Command,
    ) -> anyhow::Result<ResponsePayload> {
        if self.inner.disconnected.load(Ordering::Acquire) {
            bail!("Goddard daemon is disconnected");
        }
        let request_id = Uuid::new_v4();
        let timeout = request_timeout(&command);
        let (response, response_rx) = bounded(1);
        self.inner.pending.lock().insert(request_id, response);
        let message = ClientMessage::Request(Request {
            request_id,
            session_id,
            runtime_id,
            command,
        });
        if self
            .inner
            .outgoing
            .send(Outgoing::Message(message))
            .is_err()
        {
            self.inner.pending.lock().remove(&request_id);
            bail!("Goddard daemon connection is closed");
        }
        match response_rx.recv_timeout(timeout) {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(error)) => Err(anyhow!(error.localized_message())),
            Err(error) => {
                self.inner.pending.lock().remove(&request_id);
                Err(anyhow!("timed out waiting for Goddard daemon: {error}"))
            }
        }
    }

    pub fn notify(
        &self,
        session_id: Uuid,
        runtime_id: Uuid,
        command: Command,
    ) -> anyhow::Result<()> {
        if self.inner.disconnected.load(Ordering::Acquire) {
            bail!("Goddard daemon is disconnected");
        }
        self.inner
            .outgoing
            .send(Outgoing::Message(ClientMessage::Request(Request {
                // The nil request id is reserved for fire-and-forget controls;
                // the daemon executes them in the runtime mailbox but does
                // not allocate or send a response.
                request_id: Uuid::nil(),
                session_id,
                runtime_id,
                command,
            })))
            .map_err(|_| anyhow!("Goddard daemon connection is closed"))
    }

    pub fn last_sequences(&self) -> Vec<ReplayCursor> {
        self.inner
            .last_sequences
            .lock()
            .iter()
            .map(|(&(session_id, runtime_id), cursor)| ReplayCursor {
                session_id,
                runtime_id,
                epoch: cursor.epoch,
                sequence: cursor.sequence,
            })
            .collect()
    }

    pub fn shutdown(&self) {
        let _ = self.inner.outgoing.send(Outgoing::Shutdown);
    }
}

/// The terminal answer to a `PairRequest` — what the daemon's owner
/// decided about this device.
#[derive(Clone, Debug)]
pub enum PairReply {
    /// Approved; `token` authenticates a normal `DaemonClient::connect`,
    /// `daemon_name` labels the remote host.
    Granted {
        token: String,
        daemon_name: String,
    },
    Declined {
        message: String,
    },
}

/// Ask the daemon at `address` for a client token on this device's
/// behalf. Opens a short-lived socket, sends the pre-hello `PairRequest`,
/// and parks until the owner decides — `timeout` bounds the wait locally;
/// the daemon also expires unanswered requests on its own clock.
pub fn pair(address: &str, device_name: &str, timeout: Duration) -> anyhow::Result<PairReply> {
    let url = daemon_url(address)?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_WIRE_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_WIRE_MESSAGE_BYTES));
    let (mut socket, _) = tungstenite::client::connect_with_config(url.as_str(), Some(config), 3)
        .context("could not connect to Goddard daemon")?;
    write_json(
        &mut socket,
        &ClientMessage::PairRequest {
            protocol_version: PROTOCOL_VERSION,
            device_name: device_name.to_string(),
        },
    )?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            bail!("pair request timed out waiting for approval");
        }
        set_client_read_timeout(&mut socket, Some(remaining.min(READ_POLL_INTERVAL * 200)))?;
        match socket.read() {
            Ok(Message::Text(text)) => {
                let message: ServerMessage = serde_json::from_str(text.as_ref())?;
                match message {
                    ServerMessage::PairPending => {}
                    ServerMessage::PairGranted { token, daemon_name } => {
                        return Ok(PairReply::Granted { token, daemon_name });
                    }
                    ServerMessage::PairDeclined { message } => {
                        return Ok(PairReply::Declined { message });
                    }
                    ServerMessage::Rejected { message } => {
                        bail!("daemon rejected pair request: {message}")
                    }
                    other => bail!("daemon sent an invalid pairing response: {other:?}"),
                }
            }
            Ok(Message::Ping(_)) => {
                let _ = socket.flush();
            }
            Ok(Message::Close(_)) => bail!("daemon closed during pairing"),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if retryable_io(&error) => {}
            Err(error) => return Err(error).context("pair request failed"),
        }
    }
}

fn daemon_url(address: &str) -> anyhow::Result<String> {
    let normalized = if address.starts_with("ws://") || address.starts_with("wss://") {
        address.to_owned()
    } else {
        format!("ws://{address}")
    };
    let mut url = url::Url::parse(&normalized).context("Goddard daemon address is invalid")?;
    url.set_path("/v1");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.into())
}

fn run_client(
    mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
    outgoing: Receiver<Outgoing>,
    inner: Arc<ClientInner>,
) {
    'connection: loop {
        while let Ok(message) = outgoing.try_recv() {
            match message {
                Outgoing::Message(message) => {
                    if write_json(&mut socket, &message).is_err() {
                        break 'connection;
                    }
                }
                Outgoing::Shutdown => {
                    let _ = write_json(&mut socket, &ClientMessage::Shutdown);
                    let _ = socket.flush();
                    break 'connection;
                }
            }
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                let Ok(message) = serde_json::from_str::<ServerMessage>(text.as_ref()) else {
                    continue;
                };
                match message {
                    ServerMessage::Response {
                        request_id,
                        outcome,
                    } => {
                        if let Some(pending) = inner.pending.lock().remove(&request_id) {
                            let result = match outcome {
                                ResponseOutcome::Ok { payload } => Ok(payload),
                                ResponseOutcome::Error { error } => Err(error),
                            };
                            let _ = pending.send(result);
                        }
                    }
                    ServerMessage::Event(event) => {
                        let should_deliver = {
                            let mut sequences = inner.last_sequences.lock();
                            let previous = sequences
                                .entry((event.session_id, event.runtime_id))
                                .or_insert(LastSequence {
                                    epoch: event.epoch,
                                    sequence: 0,
                                });
                            if previous.epoch == event.epoch && event.sequence <= previous.sequence
                            {
                                false
                            } else {
                                previous.epoch = event.epoch;
                                previous.sequence = event.sequence;
                                true
                            }
                        };
                        if should_deliver {
                            if let Some(watchers) =
                                inner.session_watchers.lock().get_mut(&event.session_id)
                            {
                                watchers.retain(|watcher| watcher.send(event.clone()).is_ok());
                            }
                            let key = (event.session_id, event.runtime_id);
                            let sessions = inner.sessions.lock();
                            if let Some(events) = sessions.get(&key) {
                                let _ = events.send(event);
                            } else {
                                let mut pending = inner.pending_events.lock();
                                let buffered = pending.entry(key).or_default();
                                buffered.push_back(event);
                                while buffered.len() > MAX_BUFFERED_EVENTS_PER_RUNTIME {
                                    buffered.pop_front();
                                }
                            }
                        }
                    }
                    ServerMessage::TaskStateChanged { revision } => {
                        inner
                            .task_state_subscribers
                            .lock()
                            .retain(|subscriber| subscriber.send(revision).is_ok());
                    }
                    ServerMessage::SettingsChanged { settings } => {
                        inner
                            .settings_subscribers
                            .lock()
                            .retain(|subscriber| subscriber.send(settings.clone()).is_ok());
                    }
                    ServerMessage::FriendsChanged { state } => {
                        inner
                            .friends_subscribers
                            .lock()
                            .retain(|subscriber| subscriber.send(state.clone()).is_ok());
                    }
                    ServerMessage::PairingChanged { state } => {
                        inner
                            .pairing_subscribers
                            .lock()
                            .retain(|subscriber| subscriber.send(state.clone()).is_ok());
                    }
                    ServerMessage::AutomationsChanged { state } => {
                        inner
                            .automations_subscribers
                            .lock()
                            .retain(|subscriber| subscriber.send(state.clone()).is_ok());
                    }
                    ServerMessage::ReviewChanged { origin_url } => {
                        inner
                            .review_subscribers
                            .lock()
                            .retain(|subscriber| subscriber.send(origin_url.clone()).is_ok());
                    }
                    ServerMessage::FriendSessionClosed {
                        session_id,
                        revoked,
                    } => {
                        inner
                            .friend_session_closed_subscribers
                            .lock()
                            .retain(|subscriber| subscriber.send((session_id, revoked)).is_ok());
                    }
                    ServerMessage::ShuttingDown => break,
                    ServerMessage::Hello { .. }
                    | ServerMessage::Rejected { .. }
                    | ServerMessage::PairPending
                    | ServerMessage::PairGranted { .. }
                    | ServerMessage::PairDeclined { .. } => {}
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) => {
                let _ = socket.flush();
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if retryable_io(&error) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => break,
            Err(_) => break,
        }
    }

    fail_connection(&inner);
}

fn fail_connection(inner: &ClientInner) {
    inner.disconnected.store(true, Ordering::Release);
    let pending = std::mem::take(&mut *inner.pending.lock());
    for (_, response) in pending {
        let _ = response.send(Err(RpcError {
            message: "Goddard daemon disconnected".into(),
            i18n: None,
        }));
    }
    // Closing the desktop transport is not evidence that a daemon-owned
    // provider exited. Drop the subscription senders so runtime adapters can
    // hand off to a replacement client and ask the daemon whether the same
    // runtime still exists. Real provider exits arrive through the replayable
    // `processExited` event emitted by the daemon.
    drop(std::mem::take(&mut *inner.sessions.lock()));
    drop(std::mem::take(&mut *inner.session_watchers.lock()));
    inner.task_state_subscribers.lock().clear();
    inner.settings_subscribers.lock().clear();
    inner.friends_subscribers.lock().clear();
    inner.pairing_subscribers.lock().clear();
    inner.automations_subscribers.lock().clear();
    inner.review_subscribers.lock().clear();
    inner.friend_session_closed_subscribers.lock().clear();
}

fn set_client_read_timeout(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Option<Duration>,
) -> io::Result<()> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
        #[allow(unreachable_patterns)]
        _ => Ok(()),
    }
}

fn retryable_io(error: &io::Error) -> bool {
    retryable_error(error)
}

fn retryable_error(error: &(dyn std::error::Error + 'static)) -> bool {
    if let Some(error) = error.downcast_ref::<io::Error>() {
        if matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
        ) {
            return true;
        }
        #[cfg(unix)]
        if error.raw_os_error() == Some(libc::EAGAIN)
            || error.raw_os_error() == Some(libc::EWOULDBLOCK)
        {
            return true;
        }
    }
    error.source().is_some_and(retryable_error)
}

fn write_json<S: io::Read + io::Write, T: serde::Serialize>(
    socket: &mut WebSocket<S>,
    value: &T,
) -> anyhow::Result<()> {
    let payload = serde_json::to_string(value)?;
    socket.send(Message::Text(payload.into()))?;
    Ok(())
}

fn read_server_message(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> anyhow::Result<ServerMessage> {
    loop {
        match socket.read()? {
            Message::Text(text) => return Ok(serde_json::from_str(text.as_ref())?),
            Message::Ping(_) => socket.flush()?,
            Message::Close(_) => bail!("Goddard daemon closed during handshake"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn worktree_bound_commands_get_the_longer_timeout() {
        let cwd = PathBuf::from("/tmp/waku-test");
        let session_id = Uuid::new_v4();

        for operation in [
            WorkspaceOperation::CaptureTurnStart {
                cwd: cwd.clone(),
                session_id,
                turn_count: 1,
            },
            WorkspaceOperation::CaptureTurn {
                cwd: cwd.clone(),
                session_id,
                turn_count: 1,
            },
            WorkspaceOperation::CreateWorktree {
                project_path: cwd.clone(),
                name: None,
                base_ref: None,
                sync_default_branch: false,
                sync_branches: Vec::new(),
            },
            WorkspaceOperation::CreateWorktreeFromCheckout {
                project_path: cwd.clone(),
                name: None,
            },
            WorkspaceOperation::EnsureWorktree {
                project_path: cwd.clone(),
                path: cwd.clone(),
                branch: None,
                base_ref: None,
            },
            WorkspaceOperation::RemoveWorktree {
                path: cwd.clone(),
                force: false,
            },
            WorkspaceOperation::PruneWorktrees { cwd: cwd.clone() },
            WorkspaceOperation::ResetWorktree {
                path: cwd.clone(),
                base_ref: "HEAD".into(),
            },
            WorkspaceOperation::ArchiveProjectlessWorkspace { path: cwd.clone() },
            WorkspaceOperation::RestoreProjectlessWorkspace { path: cwd.clone() },
        ] {
            assert_eq!(
                request_timeout(&Command::Workspace {
                    operation: operation.clone()
                }),
                WORKTREE_REQUEST_TIMEOUT,
                "{operation:?}"
            );
        }
        assert_eq!(
            request_timeout(&Command::RewindSessionToMessage { turn_count: 2 }),
            WORKTREE_REQUEST_TIMEOUT
        );

        // Reads and ordinary requests keep the short bound.
        assert_eq!(
            request_timeout(&Command::Workspace {
                operation: WorkspaceOperation::ListWorktrees { cwd: cwd.clone() }
            }),
            REQUEST_TIMEOUT
        );
        assert_eq!(request_timeout(&Command::AttachSession), REQUEST_TIMEOUT);
        assert_eq!(
            request_timeout(&Command::Prompt {
                prompt: String::new(),
                turn_id: None,
                message_id: None,
                hidden: false,
                attachments: Vec::new(),
            }),
            REQUEST_TIMEOUT
        );
    }

    #[test]
    fn daemon_endpoint_accepts_addresses_and_secure_urls() {
        assert_eq!(
            daemon_url("127.0.0.1:4312").unwrap(),
            "ws://127.0.0.1:4312/v1"
        );
        assert_eq!(
            daemon_url("wss://waku.example.test/old?ignored=1").unwrap(),
            "wss://waku.example.test/v1"
        );
    }
}
