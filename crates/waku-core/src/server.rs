use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use anyhow::{Context as _, bail};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use subtle::ConstantTimeEq as _;
use tungstenite::handshake::server::{
    ErrorResponse, Request as HandshakeRequest, Response as HandshakeResponse,
};
use tungstenite::http::{StatusCode, header::ORIGIN};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket, accept_hdr_with_config};
use uuid::Uuid;

use waku_protocol::event_to_wire;

use crate::model::{AgentSession, DriverEvent, Project, ProviderKind, SessionStatus};
use crate::protocol::MAX_WIRE_MESSAGE_BYTES;
use crate::protocol::{
    ClientMessage, Command, DaemonExposure, PROTOCOL_VERSION, ReplayCursor, Request,
    ResponseOutcome, ResponsePayload, RpcError, SequencedEvent, ServerMessage, WireDriverEvent,
};

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(25);
/// A socket write that makes no progress for this long means the client is
/// not reading. Keeping the connection would let its event queue grow without
/// bound, so it is dropped; clients reconnect and resume from their cursors.
const SOCKET_WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HANDSHAKE_MESSAGE_BYTES: usize = 64 * 1024;
/// Webhook sniffing needs only the request line — a `POST` plus the trigger
/// path is well under a kilobyte; anything longer falls through to the
/// WebSocket handshake to reject.
const MAX_REQUEST_LINE_BYTES: usize = 1024;
const MAX_CONNECTIONS: usize = 64;
const MAX_REPLAY_EVENTS_PER_SESSION: usize = 2048;
/// Per-subscriber cap on queued daemon messages. A subscriber that falls this
/// far behind live events is dropped instead of buffered for, so a stalled
/// client cannot make the daemon's heap grow at the stream rate forever.
/// (Max 8 replaying runtimes at `MAX_REPLAY_EVENTS_PER_SESSION` fits; an
/// overflow during replay or live emission closes the connection and the
/// client reconnects from its last replayed cursor.)
const MAX_QUEUED_MESSAGES_PER_SUBSCRIBER: usize = 16384;
const MAX_CACHED_RESPONSES: usize = 2048;
/// Responses are cached by request id so a client that missed a reply can
/// fetch it after reconnecting. Caching is bounded by bytes as well as count:
/// outcomes such as a hydrated session can be megabytes, and a count-only cap
/// would let a handful of them pin hundreds of megabytes in the daemon.
const MAX_CACHED_RESPONSE_BYTES: usize = 24 * 1024 * 1024;
const NATIVE_CLIENT_HEADER: &str = "x-waku-client";
const NATIVE_CLIENT_HEADER_VALUE: &str = "native";

#[derive(Clone, Debug, Default)]
pub struct ServerOptions {
    /// Browser WebSocket handshakes carry an Origin header. Most native clients
    /// do not; React Native does and identifies itself with `x-waku-client`.
    /// An empty set therefore still permits native clients only.
    pub allowed_origins: HashSet<String>,
    /// Only a daemon owned by the desktop process should accept the global
    /// shutdown control message. Service-managed daemons keep running when an
    /// authenticated client disconnects.
    pub allow_shutdown: bool,
    /// Commit the daemon binary was built from, reported in the hello so a
    /// connected client can show exactly which source is serving it.
    pub build_commit: Option<String>,
}

struct ConnectionPermit(Arc<AtomicUsize>);

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub trait Backend: Send + Sync + 'static {
    /// Resolve a scoped agent credential to the Waku task that owns it. The
    /// default answers for backends without an agent surface.
    fn authenticate_agent(&self, token: &str) -> Option<Uuid> {
        let _ = token;
        None
    }

    /// Whether a paired client's minted token authenticates as a full
    /// client. The default answers for backends without a pairing store.
    fn authenticate_paired(&self, token: &str) -> bool {
        let _ = token;
        false
    }

    /// Queue a device name for a local pairing decision and wait for it.
    /// Runs on the connection's own thread. The default declines — pairing
    /// exists only where the backend stores minted tokens.
    fn request_pair(&self, device_name: &str, transport: &str) -> crate::pairing::PairReply {
        let _ = (device_name, transport);
        crate::pairing::PairReply::Declined {
            message: "this daemon does not support pairing".into(),
        }
    }

    /// Where the pairing document's `PairingChanged` broadcast is
    /// installed — `serve` sets this before accepting connections.
    fn set_pairing_sink(&self, _sink: crate::pairing::PairingSink) {}

    /// The name granted clients and LAN browsers see for this daemon.
    fn daemon_name(&self) -> String {
        "Goddard".into()
    }

    /// The `(name, instance_id)` a non-loopback listener advertises over
    /// DNS-SD. `None` (the default) suppresses the advertisement entirely.
    fn lan_advertisement(&self) -> Option<(String, String)> {
        None
    }

    /// Record the port a runtime-opened non-loopback listener bound, so LAN
    /// discovery can report it. `None` once that listener closes.
    fn note_exposed_port(&self, _port: Option<u16>) {}

    /// Kick any lazily-started reachability surfaces awake — `serve` calls
    /// this when it binds a non-loopback listener, since being reachable
    /// is exactly when LAN discovery matters.
    fn kickstart_reachability(&self) {}

    /// `agent` is `Some` when the connection authenticated with a scoped
    /// agent credential; the id names the sending Waku task, which the
    /// backend uses for provenance.
    fn handle(
        &self,
        request: Request,
        events: EventSink,
        agent: Option<Uuid>,
    ) -> anyhow::Result<ResponsePayload>;

    fn shutdown(&self) {}

    /// Where async friend/share events are delivered once the hub exists —
    /// `serve` installs this before accepting connections.
    fn set_friends_sink(&self, _sink: crate::share::FriendsSink) {}

    /// Where the share layer reports session-catalog mutations (a transfer
    /// session materialized outside any client request).
    fn set_task_state_sink(&self, _sink: crate::share::TaskNotifier) {}

    /// Where the automation scheduler publishes document changes — `serve`
    /// installs this before accepting connections.
    fn set_automations_sink(&self, _sink: crate::automations::AutomationsSink) {}

    /// Root event sink for work the backend initiates without a client
    /// request — a scheduled automation dispatching a run.
    fn set_event_source(&self, _events: EventSink) {}

    /// Where the share layer reports QA review state moving — here or on
    /// a friend's machine.
    fn set_review_notifier(&self, _notifier: crate::share::ReviewNotifier) {}

    /// Where the hub registers per-session event streams for friends
    /// watching shared sessions — installed by `serve` once the hub exists.
    fn set_session_streamer(&self, _streamer: crate::share::SessionStreamer) {}

    /// Where friend-session updates arriving from peer subscriptions are
    /// published — the hub turns them into `ServerMessage`s for clients.
    fn set_friend_session_sink(&self, _sink: crate::share::FriendSessionSink) {}

    /// A plain-HTTP `POST /automations/{id}/trigger?key=…` on the daemon
    /// listener. Backends without automations report every id as unknown.
    fn trigger_automation_webhook(
        &self,
        automation_id: Uuid,
        key: &str,
    ) -> anyhow::Result<Option<waku_protocol::automations::AutomationRun>> {
        let _ = (automation_id, key);
        Ok(None)
    }
}

#[derive(Clone)]
pub struct EventSink {
    session_id: Uuid,
    runtime_id: Uuid,
    hub: Arc<Hub>,
    /// The subscriber this request arrived on; `settings_changed` skips it —
    /// the initiator already holds the document it sent. `u64::MAX` is the
    /// "no source" sentinel agent connections and tests use.
    source_subscriber_id: u64,
}

impl EventSink {
    pub fn send(&self, event: WireDriverEvent) -> anyhow::Result<()> {
        self.hub.emit(self.session_id, self.runtime_id, event, true);
        Ok(())
    }

    /// Push the daemon's settings document to every subscribed client except
    /// the request's own connection — the initiator already holds the
    /// document it sent.
    pub fn settings_changed(&self, settings: crate::DaemonSettings) {
        self.hub
            .settings_changed(settings, self.source_subscriber_id);
    }

    /// The connection the in-flight request arrived on. The dispatcher sets
    /// this so broadcasts the request triggers can skip their source.
    pub(crate) fn with_source_subscriber(mut self, id: u64) -> Self {
        self.source_subscriber_id = id;
        self
    }

    /// Broadcast a live-only event without retaining it in the replay journal.
    /// High-volume PTY output is meaningful only to a terminal emulator that
    /// is currently attached; replaying raw chunks into a fresh emulator would
    /// also retain an unbounded terminal transcript in daemon memory.
    pub fn send_ephemeral(&self, event: WireDriverEvent) -> anyhow::Result<()> {
        self.hub
            .emit(self.session_id, self.runtime_id, event, false);
        Ok(())
    }

    /// A sink bound to a private hub — events go nowhere. Fallback for code
    /// paths that run before `serve` installs the real event source (tests,
    /// a backend constructed without a server).
    pub(crate) fn detached() -> EventSink {
        EventSink {
            session_id: Uuid::nil(),
            runtime_id: Uuid::nil(),
            hub: Arc::new(Hub::default()),
            source_subscriber_id: u64::MAX,
        }
    }

    /// Retarget this sink at another session/runtime pair. Agent commands
    /// are addressed by the request's own ids, but the events they generate
    /// belong to the target session's stream.
    pub fn for_session(&self, session_id: Uuid, runtime_id: Uuid) -> EventSink {
        EventSink {
            session_id,
            runtime_id,
            hub: self.hub.clone(),
            source_subscriber_id: self.source_subscriber_id,
        }
    }

    /// Register a runtime the request itself did not start — an agent
    /// command cold-starting a stored task — and retarget this sink at it.
    pub fn begin_session_runtime(&self, session_id: Uuid, runtime_id: Uuid) -> EventSink {
        self.hub.begin_runtime(session_id, runtime_id);
        self.for_session(session_id, runtime_id)
    }

    /// Retire the runtime this sink currently targets, e.g. when a runtime
    /// it just started fails before its first event.
    pub fn end_session_runtime(&self) {
        self.hub.end_runtime(self.session_id, Some(self.runtime_id));
    }

    /// Emit the runtime-ended signal clients already understand. Used
    /// before `end_session_runtime` on teardown paths with no provider
    /// process left to report its own exit — idle eviction, forced
    /// shutdown. Without it an attached client keeps a stale driver
    /// handle, and because runtime commands are fire-and-forget the next
    /// prompt vanishes into a runtime the daemon no longer has.
    pub fn notify_runtime_ended(&self) {
        if let Ok(wire) = event_to_wire(DriverEvent::ProcessExited) {
            let _ = self.send(wire);
        }
    }

    /// Replay depth retained for `session_id` — tests assert a dead
    /// runtime's backlog is gone, not just unreachable.
    #[cfg(test)]
    pub(crate) fn journaled_event_count(&self, session_id: Uuid) -> usize {
        self.hub
            .state
            .lock()
            .journal
            .iter()
            .filter(|((session, _), _)| *session == session_id)
            .map(|(_, events)| events.len())
            .sum()
    }
}

/// One connected client's delivery state.
struct Subscriber {
    messages: Sender<ServerMessage>,
    /// Signalled when the subscriber falls too far behind and is dropped from
    /// the hub. The connection polls it and closes, letting the client
    /// reconnect and resume from its last replayed cursor.
    kicked: Sender<()>,
    /// When set, only `Event`s for this session reach the subscriber —
    /// friend session feeds subscribe to one session, not the journal.
    session_filter: Option<Uuid>,
}

impl Subscriber {
    fn new(messages: Sender<ServerMessage>) -> (Self, Receiver<()>) {
        let (kicked, kicked_rx) = unbounded();
        (
            Self {
                messages,
                kicked,
                session_filter: None,
            },
            kicked_rx,
        )
    }

    fn for_session(messages: Sender<ServerMessage>, session_id: Uuid) -> (Self, Receiver<()>) {
        let (mut subscriber, kicked_rx) = Self::new(messages);
        subscriber.session_filter = Some(session_id);
        (subscriber, kicked_rx)
    }

    /// Whether `message` is deliverable to this subscriber.
    fn accepts(&self, message: &ServerMessage) -> bool {
        match self.session_filter {
            Some(session_id) => {
                matches!(message, ServerMessage::Event(event) if event.session_id == session_id)
            }
            None => true,
        }
    }
}

#[derive(Default)]
struct HubState {
    next_subscriber_id: u64,
    task_state_revision: u64,
    subscribers: HashMap<u64, Subscriber>,
    active_runtimes: HashMap<Uuid, Uuid>,
    next_sequences: HashMap<(Uuid, Uuid), u64>,
    journal: HashMap<(Uuid, Uuid), VecDeque<SequencedEvent>>,
    responses: VecDeque<(Uuid, ResponseOutcome, usize)>,
    cached_response_bytes: usize,
    catalog_projects: HashMap<Uuid, ProjectCatalogEntry>,
    catalog_sessions: HashMap<Uuid, SessionCatalogEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectCatalogEntry {
    name: String,
    path: std::path::PathBuf,
    created_at: u64,
}

impl From<&Project> for ProjectCatalogEntry {
    fn from(project: &Project) -> Self {
        Self {
            name: project.name.clone(),
            path: project.path.clone(),
            created_at: project.created_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionCatalogEntry {
    title: String,
    auto_title: Option<String>,
    project_id: Uuid,
    provider: ProviderKind,
    model: Option<String>,
    status: SessionStatus,
    created_at: u64,
    last_reply_at: Option<u64>,
}

impl From<&AgentSession> for SessionCatalogEntry {
    fn from(session: &AgentSession) -> Self {
        Self {
            title: session.title.clone(),
            auto_title: session.auto_title.clone(),
            project_id: session.project_id,
            provider: session.provider,
            model: session.model.clone(),
            status: session.status,
            created_at: session.created_at,
            last_reply_at: session.last_reply_at,
        }
    }
}

struct Hub {
    epoch: Uuid,
    state: Mutex<HubState>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            epoch: Uuid::new_v4(),
            state: Mutex::new(HubState::default()),
        }
    }
}

/// One filtered session subscription owned by the share layer — the
/// receiver yields only `ServerMessage::Event`s for its session, replayed
/// journal first. Drop unsubscribes.
pub struct SessionStream {
    pub events: Receiver<ServerMessage>,
    /// Fires if the hub drops the subscription for lagging — the feed
    /// ends so the viewer resubscribes with a fresh snapshot.
    pub kicked: Receiver<()>,
    hub: Weak<Hub>,
    subscriber_id: u64,
}

impl Drop for SessionStream {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.unsubscribe(self.subscriber_id);
        }
    }
}

struct DispatchedRequest {
    request: Request,
    outgoing: Sender<ServerMessage>,
    source_subscriber_id: u64,
    /// `Some` when the sending connection authenticated with a scoped agent
    /// credential; the id names the Waku task that credential belongs to.
    agent: Option<Uuid>,
}

struct RuntimeMailbox {
    id: Uuid,
    sender: Sender<DispatchedRequest>,
}

struct RequestDispatcher {
    backend: Arc<dyn Backend>,
    hub: Arc<Hub>,
    /// A live provider runtime is an actor owned by the daemon, not by any
    /// particular WebSocket connection. One mailbox per session preserves
    /// lifecycle order across desktop and web clients without serializing
    /// unrelated sessions or read-only requests.
    runtime_mailboxes: Arc<Mutex<HashMap<Uuid, RuntimeMailbox>>>,
    /// The runtime-controlled non-loopback listener, installed by `serve`
    /// once this dispatcher is shared so an exposure request arriving on
    /// any listener can rebind it.
    exposure: OnceLock<Arc<ExposureControl>>,
}

impl Hub {
    fn event_sink(self: &Arc<Self>, session_id: Uuid, runtime_id: Uuid) -> EventSink {
        EventSink {
            session_id,
            runtime_id,
            hub: self.clone(),
            source_subscriber_id: u64::MAX,
        }
    }

    fn begin_runtime(&self, session_id: Uuid, runtime_id: Uuid) {
        let mut state = self.state.lock();
        state.active_runtimes.insert(session_id, runtime_id);
        state
            .next_sequences
            .retain(|(candidate, _), _| *candidate != session_id);
        state
            .journal
            .retain(|(candidate, _), _| *candidate != session_id);
    }

    fn end_runtime(&self, session_id: Uuid, runtime_id: Option<Uuid>) {
        let mut state = self.state.lock();
        let matches_active = runtime_id
            .is_none_or(|runtime_id| state.active_runtimes.get(&session_id) == Some(&runtime_id));
        if !matches_active {
            return;
        }
        state.active_runtimes.remove(&session_id);
        state
            .next_sequences
            .retain(|(candidate, _), _| *candidate != session_id);
        state
            .journal
            .retain(|(candidate, _), _| *candidate != session_id);
    }

    fn emit(&self, session_id: Uuid, runtime_id: Uuid, event: WireDriverEvent, replayable: bool) {
        let mut state = self.state.lock();
        if state.active_runtimes.get(&session_id) != Some(&runtime_id) {
            return;
        }
        let sequence = state
            .next_sequences
            .entry((session_id, runtime_id))
            .or_default();
        *sequence = sequence.saturating_add(1);
        let event = SequencedEvent {
            session_id,
            runtime_id,
            epoch: self.epoch,
            sequence: *sequence,
            event,
        };
        if replayable {
            let journal = state.journal.entry((session_id, runtime_id)).or_default();
            journal.push_back(event.clone());
            while journal.len() > MAX_REPLAY_EVENTS_PER_SESSION {
                journal.pop_front();
            }
        }
        Self::broadcast(&mut state, &ServerMessage::Event(event), None);
    }

    fn subscribe(&self, resume_from: &[ReplayCursor], subscriber: Subscriber) -> u64 {
        let mut state = self.state.lock();
        let id = state.next_subscriber_id;
        state.next_subscriber_id = state.next_subscriber_id.saturating_add(1);
        // The subscriber is registered before replay so an oversized journal
        // can only truncate the replay, never leave a live connection without
        // events. Replay runs under the hub lock so no live event can
        // interleave with the journal catch-up, and pushes into the bounded
        // queue are non-blocking. Dropping the oldest replayed events matches
        // the journal's own rollover semantics; clients reconcile any gap
        // from session state.
        let messages = subscriber.messages.clone();
        let session_filter = subscriber.session_filter;
        state.subscribers.insert(id, subscriber);
        for (&(session_id, runtime_id), events) in &state.journal {
            if session_filter.is_some_and(|filter| filter != session_id) {
                continue;
            }
            let sequence = resume_from
                .iter()
                .find(|cursor| {
                    cursor.session_id == session_id
                        && cursor.runtime_id == runtime_id
                        && cursor.epoch == self.epoch
                })
                .map(|cursor| cursor.sequence)
                .unwrap_or_default();
            for event in events.iter().filter(|event| event.sequence > sequence) {
                if messages.len() >= MAX_QUEUED_MESSAGES_PER_SUBSCRIBER
                    || messages
                        .try_send(ServerMessage::Event(event.clone()))
                        .is_err()
                {
                    return id;
                }
            }
        }
        id
    }

    fn unsubscribe(&self, subscriber_id: u64) {
        self.state.lock().subscribers.remove(&subscriber_id);
    }

    /// Sends `message` to every subscriber except `skip`, dropping any whose
    /// bounded queue is full. Buffering a stalled client at the daemon's
    /// event rate is how resident memory grew without bound; a dropped
    /// subscriber's connection observes the kick, closes, and reconnects with
    /// replay cursors, so no event stream is lost permanently.
    fn broadcast(state: &mut HubState, message: &ServerMessage, skip: Option<u64>) {
        let mut overwhelmed = Vec::new();
        for (&subscriber_id, subscriber) in &state.subscribers {
            if Some(subscriber_id) == skip || !subscriber.accepts(message) {
                continue;
            }
            if subscriber.messages.len() >= MAX_QUEUED_MESSAGES_PER_SUBSCRIBER
                || subscriber.messages.try_send(message.clone()).is_err()
            {
                overwhelmed.push((subscriber_id, subscriber.kicked.clone()));
            }
        }
        for (subscriber_id, kicked) in overwhelmed {
            state.subscribers.remove(&subscriber_id);
            let _ = kicked.send(());
        }
    }

    fn task_state_changed(&self, source_subscriber_id: u64) {
        let mut state = self.state.lock();
        Self::broadcast_task_state_changed(&mut state, source_subscriber_id);
    }

    fn replace_task_catalog(&self, projects: &[Project], sessions: &[AgentSession]) {
        let mut state = self.state.lock();
        state.catalog_projects = projects
            .iter()
            .map(|project| (project.id, ProjectCatalogEntry::from(project)))
            .collect();
        state.catalog_sessions = sessions
            .iter()
            .map(|session| (session.id, SessionCatalogEntry::from(session)))
            .collect();
    }

    fn task_state_saved(
        &self,
        source_subscriber_id: u64,
        projects: &[Project],
        sessions: &[AgentSession],
    ) {
        let mut state = self.state.lock();
        let mut changed = false;
        for project in projects {
            let next = ProjectCatalogEntry::from(project);
            changed |= state
                .catalog_projects
                .insert(project.id, next.clone())
                .is_none_or(|previous| previous != next);
        }
        for session in sessions {
            let next = SessionCatalogEntry::from(session);
            changed |= state
                .catalog_sessions
                .insert(session.id, next.clone())
                .is_none_or(|previous| previous != next);
        }
        if changed {
            Self::broadcast_task_state_changed(&mut state, source_subscriber_id);
        }
    }

    fn task_project_removed(&self, source_subscriber_id: u64, project_id: Uuid) {
        let mut state = self.state.lock();
        let removed_project = state.catalog_projects.remove(&project_id).is_some();
        let session_count = state.catalog_sessions.len();
        state
            .catalog_sessions
            .retain(|_, session| session.project_id != project_id);
        if removed_project || state.catalog_sessions.len() != session_count {
            Self::broadcast_task_state_changed(&mut state, source_subscriber_id);
        }
    }

    fn broadcast_task_state_changed(state: &mut HubState, source_subscriber_id: u64) {
        state.task_state_revision = state.task_state_revision.saturating_add(1);
        let message = ServerMessage::TaskStateChanged {
            revision: state.task_state_revision,
        };
        Self::broadcast(state, &message, Some(source_subscriber_id));
    }

    fn settings_changed(&self, settings: crate::DaemonSettings, source_subscriber_id: u64) {
        let mut state = self.state.lock();
        Self::broadcast(
            &mut state,
            &ServerMessage::SettingsChanged { settings },
            Some(source_subscriber_id),
        );
    }

    /// The friends document changed outside any request — an incoming
    /// friend request, an offer, a finished transfer. Broadcast to every
    /// subscriber; there is no initiator to skip.
    fn friends_changed(&self, state: waku_protocol::friends::FriendsState) {
        let mut hub_state = self.state.lock();
        Self::broadcast(
            &mut hub_state,
            &ServerMessage::FriendsChanged { state },
            None,
        );
    }

    /// The pairing document changed outside any request — a pair request
    /// arrived, resolved, or a paired client was revoked.
    fn pairing_changed(&self, state: waku_protocol::pairing::PairingState) {
        let mut hub_state = self.state.lock();
        Self::broadcast(
            &mut hub_state,
            &ServerMessage::PairingChanged { state },
            None,
        );
    }

    /// The automations document changed outside any request — the scheduler
    /// recorded a run transition, or another client edited a definition.
    /// Broadcast to every subscriber; there is no initiator to skip.
    fn automations_changed(&self, state: waku_protocol::automations::AutomationsState) {
        let mut hub_state = self.state.lock();
        Self::broadcast(
            &mut hub_state,
            &ServerMessage::AutomationsChanged { state },
            None,
        );
    }

    /// QA review state moved for `origin_url` — a local approve/reject/
    /// promote, or a friend's ref notice. Review surfaces re-read their
    /// queue rather than trusting this hint's payload.
    fn review_changed(&self, origin_url: String) {
        let mut hub_state = self.state.lock();
        Self::broadcast(
            &mut hub_state,
            &ServerMessage::ReviewChanged { origin_url },
            None,
        );
    }

    /// A filtered subscription to one session's events, for the share
    /// layer pumping a friend's watch stream. Dropping the returned
    /// stream unsubscribes.
    fn session_stream(
        self: &Arc<Self>,
        session_id: Uuid,
        resume: Option<ReplayCursor>,
    ) -> SessionStream {
        let (messages, events) = bounded(MAX_QUEUED_MESSAGES_PER_SUBSCRIBER);
        let (subscriber, kicked) = Subscriber::for_session(messages, session_id);
        let resume_from: Vec<ReplayCursor> = resume.into_iter().collect();
        let subscriber_id = self.subscribe(&resume_from, subscriber);
        SessionStream {
            events,
            kicked,
            hub: Arc::downgrade(self),
            subscriber_id,
        }
    }

    /// Push an event arriving from a friend's daemon. Ids, epoch, and
    /// sequence are the sharer's and stay verbatim so a viewer resuming
    /// after a reconnect passes a cursor the sharer's journal understands.
    fn emit_external(&self, event: SequencedEvent) {
        let mut state = self.state.lock();
        if state.active_runtimes.get(&event.session_id) != Some(&event.runtime_id) {
            // A runtime boundary on the sharer's side starts a fresh
            // journal here, same as `begin_runtime` for local runtimes.
            state
                .active_runtimes
                .insert(event.session_id, event.runtime_id);
            state
                .next_sequences
                .retain(|(session, _), _| *session != event.session_id);
            state
                .journal
                .retain(|(session, _), _| *session != event.session_id);
        }
        let journal = state
            .journal
            .entry((event.session_id, event.runtime_id))
            .or_default();
        journal.push_back(event.clone());
        while journal.len() > MAX_REPLAY_EVENTS_PER_SESSION {
            journal.pop_front();
        }
        Self::broadcast(&mut state, &ServerMessage::Event(event), None);
    }

    /// A watched friend session's stream ended — clear its runtime and
    /// tell clients whether the friend revoked sharing or just went away.
    fn friend_session_closed(&self, session_id: Uuid, revoked: bool) {
        self.end_runtime(session_id, None);
        let mut state = self.state.lock();
        Self::broadcast(
            &mut state,
            &ServerMessage::FriendSessionClosed {
                session_id,
                revoked,
            },
            None,
        );
    }

    fn cached_response(&self, request_id: Uuid) -> Option<ResponseOutcome> {
        self.state
            .lock()
            .responses
            .iter()
            .rev()
            .find_map(|(cached_id, outcome, _)| (*cached_id == request_id).then(|| outcome.clone()))
    }

    fn cache_response(&self, request_id: Uuid, outcome: ResponseOutcome) {
        // File bytes are bulk data that clients read once; caching a copy per
        // request would duplicate every open file in daemon memory, and the
        // request is cheap to re-run if it is ever retried.
        if matches!(
            &outcome,
            ResponseOutcome::Ok {
                payload: ResponsePayload::BlobData { .. }
            }
        ) {
            return;
        }
        // Outcomes carry serde payloads whose in-memory size tracks their
        // serialized form closely enough for a cache budget.
        let bytes = serde_json::to_string(&outcome)
            .map(|serialized| serialized.len())
            .unwrap_or(0);
        let mut state = self.state.lock();
        state.cached_response_bytes = state.cached_response_bytes.saturating_add(bytes);
        state.responses.push_back((request_id, outcome, bytes));
        while state.responses.len() > MAX_CACHED_RESPONSES
            || state.cached_response_bytes > MAX_CACHED_RESPONSE_BYTES
        {
            if let Some((_, _, evicted_bytes)) = state.responses.pop_front() {
                state.cached_response_bytes =
                    state.cached_response_bytes.saturating_sub(evicted_bytes);
            }
        }
    }
}

impl RequestDispatcher {
    fn new(backend: Arc<dyn Backend>, hub: Arc<Hub>) -> Self {
        Self {
            backend,
            hub,
            runtime_mailboxes: Arc::new(Mutex::new(HashMap::new())),
            exposure: OnceLock::new(),
        }
    }

    /// Apply a requested exposure state. `None` closes the exposed listener;
    /// `Some` opens or rebinds it — a failed bind leaves the running
    /// listener untouched.
    fn set_daemon_exposure(
        &self,
        exposure: Option<DaemonExposure>,
    ) -> anyhow::Result<ResponsePayload> {
        self.exposure
            .get()
            .context("this daemon's listener cannot change at runtime")?
            .set(exposure)
    }

    fn authenticate_agent(&self, token: &str) -> Option<Uuid> {
        self.backend.authenticate_agent(token)
    }

    fn authenticate_paired(&self, token: &str) -> bool {
        self.backend.authenticate_paired(token)
    }

    fn request_pair(&self, device_name: &str) -> crate::pairing::PairReply {
        self.backend.request_pair(device_name, "ws")
    }

    fn trigger_automation_webhook(
        &self,
        automation_id: Uuid,
        key: &str,
    ) -> anyhow::Result<Option<waku_protocol::automations::AutomationRun>> {
        self.backend.trigger_automation_webhook(automation_id, key)
    }

    fn dispatch(
        &self,
        request: Request,
        outgoing: Sender<ServerMessage>,
        source_subscriber_id: u64,
        agent: Option<Uuid>,
    ) {
        if command_targets_runtime(&request.command) {
            self.dispatch_runtime(request, outgoing, source_subscriber_id, agent);
        } else {
            self.dispatch_independent(request, outgoing, source_subscriber_id, agent);
        }
    }

    fn dispatch_independent(
        &self,
        request: Request,
        outgoing: Sender<ServerMessage>,
        source_subscriber_id: u64,
        agent: Option<Uuid>,
    ) {
        let backend = self.backend.clone();
        let hub = self.hub.clone();
        let failed_request_id = request.request_id;
        let failed_session_id = request.session_id;
        let failed_outgoing = outgoing.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("goddard-daemon-request".into())
            .spawn(move || {
                handle_request(request, outgoing, source_subscriber_id, agent, backend, hub);
            })
        {
            send_dispatch_error(
                failed_request_id,
                failed_session_id,
                failed_outgoing,
                &self.hub,
                format!("could not start daemon request worker: {error}"),
            );
        }
    }

    fn dispatch_runtime(
        &self,
        request: Request,
        outgoing: Sender<ServerMessage>,
        source_subscriber_id: u64,
        agent: Option<Uuid>,
    ) {
        let session_id = request.session_id;
        let failed_request_id = request.request_id;
        let failed_outgoing = outgoing.clone();
        let mut dispatched = DispatchedRequest {
            request,
            outgoing,
            source_subscriber_id,
            agent,
        };
        loop {
            let mut mailboxes = self.runtime_mailboxes.lock();
            if let Some(mailbox) = mailboxes.get(&session_id) {
                match mailbox.sender.send(dispatched) {
                    Ok(()) => return,
                    Err(error) => {
                        dispatched = error.0;
                        mailboxes.remove(&session_id);
                        continue;
                    }
                }
            }

            let mailbox_id = Uuid::new_v4();
            let (sender, requests) = unbounded();
            sender
                .send(dispatched)
                .expect("a new runtime mailbox still has its receiver");
            mailboxes.insert(
                session_id,
                RuntimeMailbox {
                    id: mailbox_id,
                    sender,
                },
            );

            let backend = self.backend.clone();
            let hub = self.hub.clone();
            let mailbox_registry = Arc::downgrade(&self.runtime_mailboxes);
            let worker = std::thread::Builder::new()
                .name(format!("goddard-daemon-runtime-{session_id}"))
                .spawn(move || {
                    run_runtime_mailbox(
                        session_id,
                        mailbox_id,
                        requests,
                        mailbox_registry,
                        backend,
                        hub,
                    );
                });
            if let Err(error) = worker {
                if mailboxes
                    .get(&session_id)
                    .is_some_and(|mailbox| mailbox.id == mailbox_id)
                {
                    mailboxes.remove(&session_id);
                }
                drop(mailboxes);
                send_dispatch_error(
                    failed_request_id,
                    session_id,
                    failed_outgoing,
                    &self.hub,
                    format!("could not start runtime worker: {error}"),
                );
            }
            return;
        }
    }
}

pub fn serve(
    listener: TcpListener,
    token: String,
    backend: Arc<dyn Backend>,
    shutdown: Arc<AtomicBool>,
    options: ServerOptions,
) -> anyhow::Result<()> {
    listener
        .set_nonblocking(true)
        .context("could not configure Goddard daemon listener")?;
    let hub = Arc::new(Hub::default());
    {
        let hub = hub.clone();
        backend.set_friends_sink(Arc::new(move |state| hub.friends_changed(state)));
    }
    {
        let hub = hub.clone();
        backend.set_pairing_sink(Arc::new(move |state| hub.pairing_changed(state)));
    }
    {
        let hub = hub.clone();
        backend.set_task_state_sink(Arc::new(move || hub.task_state_changed(u64::MAX)));
    }
    {
        let hub = hub.clone();
        backend.set_automations_sink(Arc::new(move |state| hub.automations_changed(state)));
    }
    backend.set_event_source(hub.event_sink(Uuid::nil(), Uuid::nil()));
    {
        let hub = hub.clone();
        backend.set_review_notifier(Arc::new(move |origin_url| hub.review_changed(origin_url)));
    }
    {
        let hub = hub.clone();
        backend.set_session_streamer(Arc::new(move |session_id, resume| {
            hub.session_stream(session_id, resume)
        }));
    }
    {
        let hub = hub.clone();
        backend.set_friend_session_sink(Arc::new(move |update| match update {
            crate::share::FriendSessionUpdate::Event(event) => hub.emit_external(event),
            crate::share::FriendSessionUpdate::Closed {
                session_id,
                revoked,
            } => hub.friend_session_closed(session_id, revoked),
        }));
    }
    let dispatcher = Arc::new(RequestDispatcher::new(backend.clone(), hub.clone()));
    let options = Arc::new(options);
    // A daemon reachable off-loopback advertises itself on the LAN and
    // wakes its share endpoint so iroh discovery can find it too. Held for
    // the server's lifetime; the registration drops with it.
    let _lan_advert = listener.local_addr().ok().and_then(|address| {
        if address.ip().is_loopback() {
            return None;
        }
        backend.kickstart_reachability();
        backend.lan_advertisement().and_then(|(name, instance_id)| {
            match crate::lan::LanAdvert::start(
                &name,
                &instance_id,
                address.port(),
                PROTOCOL_VERSION,
            ) {
                Ok(advert) => Some(advert),
                Err(error) => {
                    eprintln!("could not advertise the daemon on the LAN: {error:#}");
                    None
                }
            }
        })
    });
    let active_connections = Arc::new(AtomicUsize::new(0));
    // Installed before the first connection can arrive so an exposure
    // request on any listener can rebind the non-loopback socket.
    let _ = dispatcher.exposure.set(Arc::new(ExposureControl {
        dispatcher: Arc::downgrade(&dispatcher),
        hub: hub.clone(),
        backend: backend.clone(),
        server_shutdown: shutdown.clone(),
        base_options: (*options).clone(),
        active_connections: active_connections.clone(),
        active: Mutex::new(None),
    }));
    accept_loop(
        listener,
        token,
        dispatcher,
        hub,
        shutdown.clone(),
        shutdown,
        options,
        active_connections,
    )?;
    backend.shutdown();
    Ok(())
}

/// The runtime-controlled non-loopback listener. `serve` owns exactly one,
/// held by the dispatcher so a `setDaemonExposure` request on any listener
/// reaches it.
struct ExposureControl {
    dispatcher: Weak<RequestDispatcher>,
    hub: Arc<Hub>,
    backend: Arc<dyn Backend>,
    /// The daemon-wide stop: an exposed listener ends when it fires too.
    server_shutdown: Arc<AtomicBool>,
    /// `allow_shutdown` and `build_commit` inherit from the primary
    /// listener; origins and the token always come from the request.
    base_options: ServerOptions,
    /// The connection cap is shared with the primary listener.
    active_connections: Arc<AtomicUsize>,
    active: Mutex<Option<ExposedListener>>,
}

struct ExposedListener {
    config: DaemonExposure,
    port: u16,
    shutdown: Arc<AtomicBool>,
    _advert: Option<crate::lan::LanAdvert>,
    thread: std::thread::JoinHandle<()>,
}

impl ExposedListener {
    /// Flag the accept loop and its connections closed, then wait for the
    /// socket to drop — bounded by the nonblocking accept poll.
    fn stop(self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.thread.join();
    }
}

impl ExposureControl {
    fn set(&self, exposure: Option<DaemonExposure>) -> anyhow::Result<ResponsePayload> {
        let exposure = exposure.map(DaemonExposure::validated).transpose()?;
        let mut active = self.active.lock();
        let Some(config) = exposure else {
            let previous = active.take();
            self.backend.note_exposed_port(None);
            drop(active);
            if let Some(previous) = previous {
                previous.stop();
            }
            return Ok(ResponsePayload::Exposure { port: None });
        };
        if let Some(current) = active.as_ref()
            && current.config == config
        {
            return Ok(ResponsePayload::Exposure {
                port: Some(current.port),
            });
        }
        let Some(dispatcher) = self.dispatcher.upgrade() else {
            bail!("the daemon server is shutting down");
        };
        // Bind before dropping the current listener: a failed rebind leaves
        // the running exposure untouched.
        let listener = TcpListener::bind(("0.0.0.0", config.port))
            .with_context(|| format!("could not expose the daemon on port {}", config.port))?;
        listener
            .set_nonblocking(true)
            .context("could not configure the exposed listener")?;
        let port = listener.local_addr()?.port();
        let shutdown = Arc::new(AtomicBool::new(false));
        // Reachable is exactly when LAN discovery matters: wake the share
        // endpoint and register `_waku._tcp` just like a startup bind.
        self.backend.kickstart_reachability();
        let advert = self
            .backend
            .lan_advertisement()
            .and_then(|(name, instance_id)| {
                match crate::lan::LanAdvert::start(&name, &instance_id, port, PROTOCOL_VERSION) {
                    Ok(advert) => Some(advert),
                    Err(error) => {
                        eprintln!("could not advertise the daemon on the LAN: {error:#}");
                        None
                    }
                }
            });
        let options = Arc::new(ServerOptions {
            allowed_origins: config.allowed_origins.iter().cloned().collect(),
            ..self.base_options.clone()
        });
        let thread = {
            let token = config.token.clone();
            let hub = self.hub.clone();
            let listener_shutdown = shutdown.clone();
            let server_shutdown = self.server_shutdown.clone();
            let active_connections = self.active_connections.clone();
            std::thread::Builder::new()
                .name("goddard-daemon-exposed".into())
                .spawn(move || {
                    if let Err(error) = accept_loop(
                        listener,
                        token,
                        dispatcher,
                        hub,
                        listener_shutdown,
                        server_shutdown,
                        options,
                        active_connections,
                    ) {
                        eprintln!("exposed daemon listener failed: {error:#}");
                    }
                })
                .context("could not start the exposed daemon listener")?
        };
        self.backend.note_exposed_port(Some(port));
        let previous = active.replace(ExposedListener {
            config,
            port,
            shutdown,
            _advert: advert,
            thread,
        });
        drop(active);
        if let Some(previous) = previous {
            previous.stop();
        }
        Ok(ResponsePayload::Exposure { port: Some(port) })
    }
}

/// One listener's accept loop. `shutdown` ends only this listener — the
/// exposed socket gets a flag of its own so unexposing does not bounce the
/// daemon — while `server_shutdown` ends everything.
fn accept_loop(
    listener: TcpListener,
    token: String,
    dispatcher: Arc<RequestDispatcher>,
    hub: Arc<Hub>,
    shutdown: Arc<AtomicBool>,
    server_shutdown: Arc<AtomicBool>,
    options: Arc<ServerOptions>,
    active_connections: Arc<AtomicUsize>,
) -> anyhow::Result<()> {
    while !shutdown.load(Ordering::Acquire) && !server_shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if active_connections
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                        (active < MAX_CONNECTIONS).then_some(active + 1)
                    })
                    .is_err()
                {
                    continue;
                }
                let connection_permit = ConnectionPermit(active_connections.clone());
                let token = token.clone();
                let dispatcher = dispatcher.clone();
                let hub = hub.clone();
                let shutdown = shutdown.clone();
                let server_shutdown = server_shutdown.clone();
                let options = options.clone();
                std::thread::Builder::new()
                    .name("goddard-daemon-connection".into())
                    .spawn(move || {
                        let _connection_permit = connection_permit;
                        if let Err(error) = handle_connection(
                            stream,
                            &token,
                            dispatcher,
                            hub,
                            shutdown,
                            server_shutdown,
                            &options,
                        ) {
                            eprintln!("goddard-daemon connection ended: {error:#}");
                        }
                    })
                    .context("could not start Goddard daemon connection thread")?;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("Goddard daemon listener failed"),
        }
    }
    // This listener's connections watch only its flag — make sure they end
    // when the loop exits for either reason.
    shutdown.store(true, Ordering::Release);
    Ok(())
}

fn handle_connection(
    stream: TcpStream,
    expected_token: &str,
    dispatcher: Arc<RequestDispatcher>,
    hub: Arc<Hub>,
    shutdown: Arc<AtomicBool>,
    server_shutdown: Arc<AtomicBool>,
    options: &ServerOptions,
) -> anyhow::Result<()> {
    // Accepted sockets can inherit the listener's nonblocking flag on some
    // platforms. The handshake is deliberately blocking; steady-state reads
    // get their bounded polling behavior from SO_RCVTIMEO below.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    if answer_automation_webhook(&stream, &dispatcher)? {
        return Ok(());
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_HANDSHAKE_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_HANDSHAKE_MESSAGE_BYTES));
    let allowed_origins = options.allowed_origins.clone();
    let mut socket = accept_hdr_with_config(
        stream,
        move |request: &HandshakeRequest, response: HandshakeResponse| {
            validate_handshake(request, response, &allowed_origins)
        },
        Some(config),
    )
    .context("WebSocket handshake failed")?;
    let hello = read_client_message(&mut socket)?;
    // A pair request is the only legal pre-hello message: park this
    // connection until a connected client answers it, reply, and close —
    // the pairing socket never becomes a session client.
    if let ClientMessage::PairRequest {
        protocol_version,
        device_name,
    } = &hello
    {
        if *protocol_version != PROTOCOL_VERSION {
            write_json(
                &mut socket,
                &ServerMessage::PairDeclined {
                    message: format!(
                        "protocol {protocol_version} is unsupported; expected {PROTOCOL_VERSION}"
                    ),
                },
            )?;
            return Ok(());
        }
        write_json(&mut socket, &ServerMessage::PairPending)?;
        let reply = match dispatcher.request_pair(device_name) {
            crate::pairing::PairReply::Granted { token } => ServerMessage::PairGranted {
                token,
                daemon_name: dispatcher.backend.daemon_name(),
            },
            crate::pairing::PairReply::Declined { message }
            | crate::pairing::PairReply::Busy { message } => {
                ServerMessage::PairDeclined { message }
            }
        };
        write_json(&mut socket, &reply)?;
        return Ok(());
    }
    // `primary` marks the daemon's own bearer token — paired devices and
    // scoped agent credentials are full clients, but they may not mint new
    // listeners (and therefore new credentials) through `setDaemonExposure`.
    let (resume_from, agent, primary) = match hello {
        ClientMessage::Hello {
            protocol_version, ..
        } if protocol_version != PROTOCOL_VERSION => {
            write_json(
                &mut socket,
                &ServerMessage::Rejected {
                    message: format!(
                        "protocol {protocol_version} is unsupported; expected {PROTOCOL_VERSION}"
                    ),
                },
            )?;
            return Ok(());
        }
        ClientMessage::Hello {
            token, resume_from, ..
        } if token_matches(expected_token, &token) => (resume_from, None, true),
        ClientMessage::Hello {
            token, resume_from, ..
        } if dispatcher.authenticate_paired(&token) => (resume_from, None, false),
        ClientMessage::Hello { token, .. } => match dispatcher.authenticate_agent(&token) {
            // A scoped agent credential names the Waku task it belongs to.
            // The connection gets command responses but no event replay or
            // broadcast — its token proves nothing about which sessions it
            // may observe.
            Some(agent_task) => (Vec::new(), Some(agent_task), false),
            None => {
                write_json(
                    &mut socket,
                    &ServerMessage::Rejected {
                        message: "authentication failed".into(),
                    },
                )?;
                return Ok(());
            }
        },
        _ => bail!("first daemon message was not a hello"),
    };
    write_json(
        &mut socket,
        &ServerMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            daemon_commit: options.build_commit.clone(),
            agent_cli_available: crate::agent::agent_cli_path().is_ok(),
        },
    )?;
    socket.set_config(|config| {
        config.max_message_size = Some(MAX_WIRE_MESSAGE_BYTES);
        config.max_frame_size = Some(MAX_WIRE_MESSAGE_BYTES);
    });
    socket
        .get_mut()
        .set_read_timeout(Some(SOCKET_POLL_INTERVAL))?;
    // A client that stops reading must not let its outgoing queue grow at the
    // daemon's event rate; a write stalled past this timeout drops it.
    socket
        .get_mut()
        .set_write_timeout(Some(SOCKET_WRITE_STALL_TIMEOUT))?;

    let (outgoing, outgoing_rx) = bounded(MAX_QUEUED_MESSAGES_PER_SUBSCRIBER);
    let (subscriber, kicked) = Subscriber::new(outgoing.clone());
    // Agent connections receive no event stream, so they never join the hub.
    // The sentinel id can only ever be their own: real ids count up from 0.
    let subscriber_id = if agent.is_some() {
        u64::MAX
    } else {
        hub.subscribe(&resume_from, subscriber)
    };

    'connection: while !shutdown.load(Ordering::Acquire) {
        if kicked.try_recv().is_ok() {
            break;
        }
        while let Ok(message) = outgoing_rx.try_recv() {
            if write_json(&mut socket, &message).is_err() {
                break 'connection;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => match serde_json::from_str(text.as_ref()) {
                Ok(ClientMessage::Request(request)) => {
                    if agent.is_some() && !is_agent_command(&request.command) {
                        let request_id = request.request_id;
                        if !request_id.is_nil() {
                            let _ = outgoing.send(ServerMessage::Response {
                                request_id,
                                outcome: ResponseOutcome::Error {
                                    error: RpcError::from(anyhow::anyhow!(
                                        "an agent credential may only run agent commands"
                                    )),
                                },
                            });
                        }
                    } else if let Command::SetDaemonExposure { exposure } = &request.command {
                        // Listener control lives here rather than in the
                        // dispatch path because only this connection knows
                        // whether the caller presented the primary token.
                        let outcome = if primary {
                            match dispatcher.set_daemon_exposure(exposure.clone()) {
                                Ok(payload) => ResponseOutcome::Ok { payload },
                                Err(error) => ResponseOutcome::Error {
                                    error: RpcError::from(error),
                                },
                            }
                        } else {
                            ResponseOutcome::Error {
                                error: RpcError::from(anyhow::anyhow!(
                                    "daemon exposure requires the primary authentication token"
                                )),
                            }
                        };
                        if !request.request_id.is_nil() {
                            let _ = outgoing.send(ServerMessage::Response {
                                request_id: request.request_id,
                                outcome,
                            });
                        }
                    } else {
                        dispatcher.dispatch(request, outgoing.clone(), subscriber_id, agent);
                    }
                }
                Ok(ClientMessage::Shutdown) => {
                    if agent.is_none() && options.allow_shutdown {
                        write_json(&mut socket, &ServerMessage::ShuttingDown)?;
                        server_shutdown.store(true, Ordering::Release);
                        shutdown.store(true, Ordering::Release);
                        break;
                    }
                    write_json(
                        &mut socket,
                        &ServerMessage::Rejected {
                            message: "daemon shutdown is managed by its service owner".into(),
                        },
                    )?;
                }
                // Late hellos and pair requests are protocol noise on an
                // authenticated connection — pairing only lives pre-hello.
                Ok(ClientMessage::Hello { .. } | ClientMessage::PairRequest { .. }) => {}
                Err(error) => {
                    eprintln!("goddard-daemon ignored invalid message: {error}");
                }
            },
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) => {
                let _ = socket.flush();
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if retryable_io(&error) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => break,
            Err(error) => return Err(error).context("Goddard daemon WebSocket failed"),
        }
    }
    hub.unsubscribe(subscriber_id);
    Ok(())
}

/// The daemon's one plain-HTTP route: `POST /automations/{id}/trigger?key=…`
/// fires a webhook-armed automation. Webhook callers do not speak the
/// WebSocket protocol, so the request line is sniffed before the handshake;
/// anything else falls through with the peeked bytes untouched. `Ok(true)`
/// means a response was written and the connection is done.
fn answer_automation_webhook(
    stream: &TcpStream,
    dispatcher: &RequestDispatcher,
) -> anyhow::Result<bool> {
    let mut buffer = [0u8; MAX_REQUEST_LINE_BYTES];
    let mut filled = 0usize;
    let line = loop {
        match stream.peek(&mut buffer[filled..]) {
            Ok(0) => return Ok(false),
            Ok(read) => {
                filled += read;
                if let Some(end) = buffer[..filled].windows(2).position(|pair| pair == b"\r\n") {
                    break String::from_utf8_lossy(&buffer[..end]).into_owned();
                }
                if filled == buffer.len() {
                    return Ok(false);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("could not peek at the daemon request"),
        }
    };
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if !target.starts_with("/automations/") {
        return Ok(false);
    }
    let respond = |status: StatusCode, body: String| -> anyhow::Result<bool> {
        let mut writer = io::BufWriter::new(stream);
        write!(
            writer,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )?;
        writer.flush()?;
        // FIN follows the response, then drain whatever the caller still
        // had in flight — closing with unread request data would RST the
        // connection and could cost the caller the response.
        stream.shutdown(std::net::Shutdown::Write).ok();
        let mut inbound = stream;
        let _ = io::copy(&mut inbound, &mut io::sink());
        Ok(true)
    };
    if method != "POST" {
        return respond(
            StatusCode::METHOD_NOT_ALLOWED,
            "{\"error\":\"automation triggers require POST\"}".to_owned(),
        );
    }
    let path = target.split('?').next().unwrap_or_default();
    let id = path
        .strip_prefix("/automations/")
        .and_then(|route| route.strip_suffix("/trigger"))
        .and_then(|id| Uuid::parse_str(id).ok());
    let Some(id) = id else {
        return respond(
            StatusCode::NOT_FOUND,
            "{\"error\":\"unknown webhook endpoint\"}".to_owned(),
        );
    };
    let key = target
        .split_once('?')
        .map(|(_, query)| query)
        .and_then(|query| query.split('&').find_map(|pair| pair.strip_prefix("key=")))
        .unwrap_or_default();
    match dispatcher.trigger_automation_webhook(id, key) {
        Ok(Some(run)) => respond(
            StatusCode::OK,
            serde_json::json!({ "runId": run.id, "status": run.status }).to_string(),
        ),
        Ok(None) => respond(
            StatusCode::NOT_FOUND,
            "{\"error\":\"unknown automation\"}".to_owned(),
        ),
        Err(error) => respond(
            StatusCode::CONFLICT,
            serde_json::json!({ "error": format!("{error:#}") }).to_string(),
        ),
    }
}

fn validate_handshake(
    request: &HandshakeRequest,
    response: HandshakeResponse,
    allowed_origins: &HashSet<String>,
) -> Result<HandshakeResponse, ErrorResponse> {
    if request.uri().path() != "/v1" {
        return Err(handshake_error(
            StatusCode::NOT_FOUND,
            "unknown daemon endpoint",
        ));
    }
    let is_native_client = request
        .headers()
        .get(NATIVE_CLIENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == NATIVE_CLIENT_HEADER_VALUE);
    if let Some(origin) = request.headers().get(ORIGIN) {
        let allowed = origin
            .to_str()
            .ok()
            .is_some_and(|origin| allowed_origins.contains(origin));
        if !allowed && !is_native_client {
            return Err(handshake_error(
                StatusCode::FORBIDDEN,
                "WebSocket origin is not allowed",
            ));
        }
    }
    Ok(response)
}

fn handshake_error(status: StatusCode, message: &str) -> ErrorResponse {
    tungstenite::http::Response::builder()
        .status(status)
        .body(Some(message.to_owned()))
        .expect("static WebSocket rejection is valid")
}

fn token_matches(expected: &str, candidate: &str) -> bool {
    expected.as_bytes().ct_eq(candidate.as_bytes()).into()
}

/// The whole command surface a scoped agent credential can reach. These are
/// deliberately absent from `command_targets_runtime` so they dispatch on
/// their own workers — the backend serializes target-session delivery itself.
/// Cross-task commands sit behind `agent_tools_enabled`; self-rename has a
/// per-task grant. Custom commands use the default-on agent settings surface,
/// gated inside the backend by `agent_settings_enabled`.
fn is_agent_command(command: &Command) -> bool {
    matches!(
        command,
        Command::AgentCreateSession { .. }
            | Command::AgentPrompt { .. }
            | Command::AgentRenameSelf { .. }
            | Command::AgentReadSession { .. }
            | Command::AgentSearchSessions { .. }
            | Command::UpsertCustomCommand { .. }
            | Command::RemoveCustomCommand { .. }
            | Command::ListCustomCommands
    )
}

fn command_targets_runtime(command: &Command) -> bool {
    matches!(
        command,
        Command::AttachSession
            | Command::Start { .. }
            | Command::Prompt { .. }
            | Command::Steer { .. }
            | Command::Cancel
            | Command::CancelComputerUse
            | Command::RefreshBackgroundWork
            | Command::StopBackgroundWork { .. }
            | Command::Respond { .. }
            | Command::RespondUserInput { .. }
            | Command::ClarifyUserInput { .. }
            | Command::CancelUserInput { .. }
            | Command::RunComputerTool { .. }
            | Command::RejectComputerTool { .. }
            | Command::ApplyOptions { .. }
            | Command::Rollback { .. }
            | Command::Fork { .. }
            | Command::ForkSessionFromResponse { .. }
            | Command::RewindSessionToMessage { .. }
            | Command::OpenTerminal { .. }
            | Command::WriteTerminal { .. }
            | Command::ResizeTerminal { .. }
            | Command::CloseTerminal
            | Command::CloseSession
            | Command::RemoveSession
    )
}

fn run_runtime_mailbox(
    session_id: Uuid,
    mailbox_id: Uuid,
    requests: Receiver<DispatchedRequest>,
    mailbox_registry: Weak<Mutex<HashMap<Uuid, RuntimeMailbox>>>,
    backend: Arc<dyn Backend>,
    hub: Arc<Hub>,
) {
    let mut active_runtime_id = None;
    let mut pending = None;
    loop {
        let dispatched = match pending.take() {
            Some(request) => request,
            None => match requests.recv() {
                Ok(request) => request,
                Err(_) => return,
            },
        };
        let runtime_id = dispatched.request.runtime_id;
        let starts_runtime = matches!(
            &dispatched.request.command,
            Command::Start { .. } | Command::OpenTerminal { .. }
        );
        let closes_runtime = matches!(
            &dispatched.request.command,
            Command::CloseSession | Command::CloseTerminal | Command::RemoveSession
        );
        let removes_session = matches!(&dispatched.request.command, Command::RemoveSession);
        let handled = handle_request(
            dispatched.request,
            dispatched.outgoing,
            dispatched.source_subscriber_id,
            dispatched.agent,
            backend.clone(),
            hub.clone(),
        );

        if handled.executed {
            if starts_runtime {
                active_runtime_id =
                    matches!(&handled.outcome, ResponseOutcome::Ok { .. }).then_some(runtime_id);
            } else if let ResponseOutcome::Ok {
                payload:
                    ResponsePayload::SessionRuntime {
                        runtime_id: Some(attached_runtime_id),
                        ..
                    },
            } = &handled.outcome
            {
                // A replacement mailbox can rediscover a provider runtime
                // that survived its previous actor worker.
                active_runtime_id = Some(*attached_runtime_id);
            } else if closes_runtime {
                if (removes_session || active_runtime_id == Some(runtime_id))
                    && matches!(&handled.outcome, ResponseOutcome::Ok { .. })
                {
                    hub.end_runtime(session_id, (!removes_session).then_some(runtime_id));
                    active_runtime_id = None;
                }
            } else if active_runtime_id.is_none()
                && !matches!(
                    &handled.outcome,
                    ResponseOutcome::Ok {
                        payload: ResponsePayload::SessionRuntime { .. }
                    }
                )
                && matches!(&handled.outcome, ResponseOutcome::Ok { .. })
            {
                // Recover the supervisor state if a previous mailbox worker
                // exited unexpectedly while the backend runtime stayed alive.
                active_runtime_id = Some(runtime_id);
            }
        }

        if active_runtime_id.is_none() {
            pending =
                take_queued_request_or_retire(session_id, mailbox_id, &requests, &mailbox_registry);
            if pending.is_none() {
                return;
            }
        }
    }
}

fn take_queued_request_or_retire(
    session_id: Uuid,
    mailbox_id: Uuid,
    requests: &Receiver<DispatchedRequest>,
    mailbox_registry: &Weak<Mutex<HashMap<Uuid, RuntimeMailbox>>>,
) -> Option<DispatchedRequest> {
    let Some(mailbox_registry) = mailbox_registry.upgrade() else {
        return requests.try_recv().ok();
    };
    // Dispatchers send while holding this same lock. Therefore an empty
    // receiver followed by removal is atomic with respect to a new command:
    // it either joins this actor before retirement or creates its successor.
    let mut mailboxes = mailbox_registry.lock();
    match requests.try_recv() {
        Ok(request) => Some(request),
        Err(crossbeam_channel::TryRecvError::Empty) => {
            if mailboxes
                .get(&session_id)
                .is_some_and(|mailbox| mailbox.id == mailbox_id)
            {
                mailboxes.remove(&session_id);
            }
            None
        }
        Err(crossbeam_channel::TryRecvError::Disconnected) => None,
    }
}

struct HandledRequest {
    outcome: ResponseOutcome,
    executed: bool,
}

enum TaskCatalogAction {
    None,
    Load,
    Save { projects: Vec<Project> },
    RemoveProject { project_id: Uuid },
    Changed,
}

fn handle_request(
    request: Request,
    outgoing: Sender<ServerMessage>,
    source_subscriber_id: u64,
    agent: Option<Uuid>,
    backend: Arc<dyn Backend>,
    hub: Arc<Hub>,
) -> HandledRequest {
    let request_id = request.request_id;
    let notification = request_id.is_nil();
    let session_id = request.session_id;
    let runtime_id = request.runtime_id;
    let task_catalog_action = task_catalog_action(&request.command);
    // A fire-and-forget command gets no response, so a rejection must be
    // named before the request moves into the backend for the log line
    // below to know what failed.
    let command_kind = notification.then(|| command_kind(&request.command));
    let starts_runtime = matches!(
        &request.command,
        Command::Start { .. } | Command::OpenTerminal { .. }
    );
    let (outcome, executed) = if agent.is_some() && !is_agent_command(&request.command) {
        // A scoped credential is confined to the agent command surface no
        // matter which dispatch path a request arrived on.
        (
            ResponseOutcome::Error {
                error: RpcError::from(anyhow::anyhow!(
                    "an agent credential may only run agent commands"
                )),
            },
            false,
        )
    } else if !notification && let Some(cached) = hub.cached_response(request_id) {
        (cached, false)
    } else {
        if starts_runtime {
            hub.begin_runtime(session_id, runtime_id);
        }
        let outcome = match backend.handle(
            request,
            hub.event_sink(session_id, runtime_id)
                .with_source_subscriber(source_subscriber_id),
            agent,
        ) {
            Ok(payload) => ResponseOutcome::Ok { payload },
            Err(error) => ResponseOutcome::Error {
                error: RpcError::from(error),
            },
        };
        if !notification {
            hub.cache_response(request_id, outcome.clone());
        }
        (outcome, true)
    };
    if executed && starts_runtime && matches!(&outcome, ResponseOutcome::Error { .. }) {
        hub.end_runtime(session_id, Some(runtime_id));
    }
    if executed {
        match (&task_catalog_action, &outcome) {
            (
                TaskCatalogAction::Load,
                ResponseOutcome::Ok {
                    payload:
                        ResponsePayload::TaskState {
                            projects, sessions, ..
                        },
                },
            ) => hub.replace_task_catalog(projects, sessions),
            (
                TaskCatalogAction::Save { projects },
                ResponseOutcome::Ok {
                    payload: ResponsePayload::TaskStateSaved { sessions },
                },
            ) => hub.task_state_saved(source_subscriber_id, projects, sessions),
            (TaskCatalogAction::RemoveProject { project_id }, ResponseOutcome::Ok { .. }) => {
                hub.task_project_removed(source_subscriber_id, *project_id)
            }
            (TaskCatalogAction::Changed, ResponseOutcome::Ok { .. }) => {
                hub.task_state_changed(source_subscriber_id);
            }
            _ => {}
        }
    }
    if !notification {
        let _ = outgoing.send(ServerMessage::Response {
            request_id,
            outcome: outcome.clone(),
        });
    } else if let ResponseOutcome::Error { error } = &outcome {
        // No response channel exists — without this line a rejected
        // prompt, e.g. one sent to a runtime the daemon already retired,
        // leaves no trace anywhere.
        eprintln!(
            "goddard-daemon: fire-and-forget {} for session {session_id} failed: {}",
            command_kind.as_deref().unwrap_or("command"),
            error.message
        );
    }
    HandledRequest { outcome, executed }
}

/// A command's wire `type` tag, for naming a fire-and-forget failure
/// without dumping its payload (prompt text, attachments) into stderr.
fn command_kind(command: &Command) -> String {
    serde_json::to_value(command)
        .ok()
        .and_then(|value| value.get("type")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn task_catalog_action(command: &Command) -> TaskCatalogAction {
    match command {
        Command::LoadTaskState => TaskCatalogAction::Load,
        Command::SaveTaskState { projects, .. } => TaskCatalogAction::Save {
            projects: projects.clone(),
        },
        Command::RemoveProject { project_id } => {
            TaskCatalogAction::RemoveProject {
                project_id: *project_id,
            }
        }
        Command::RemoveSession
        | Command::ForkSessionFromResponse { .. }
        | Command::RewindSessionToMessage { .. }
        // Agent commands mutate daemon-owned task state directly; attached
        // clients learn about the new task or prompt from the revision bump.
        | Command::AgentCreateSession { .. }
        | Command::AgentPrompt { .. }
        | Command::AgentRenameSelf { .. }
        | Command::CancelQueuedPrompt { .. } => TaskCatalogAction::Changed,
        _ => TaskCatalogAction::None,
    }
}

fn send_dispatch_error(
    request_id: Uuid,
    session_id: Uuid,
    outgoing: Sender<ServerMessage>,
    hub: &Arc<Hub>,
    message: String,
) {
    if request_id.is_nil() {
        // Fire-and-forget commands have no response channel — the log is
        // the only place this failure can surface.
        eprintln!(
            "goddard-daemon: fire-and-forget command for session {session_id} failed to dispatch: {message}"
        );
        return;
    }
    let outcome = hub
        .cached_response(request_id)
        .unwrap_or_else(|| ResponseOutcome::Error {
            error: RpcError {
                message,
                i18n: None,
            },
        });
    hub.cache_response(request_id, outcome.clone());
    let _ = outgoing.send(ServerMessage::Response {
        request_id,
        outcome,
    });
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

fn read_client_message(socket: &mut WebSocket<TcpStream>) -> anyhow::Result<ClientMessage> {
    loop {
        match socket.read()? {
            Message::Text(text) => return Ok(serde_json::from_str(text.as_ref())?),
            Message::Ping(_) => socket.flush()?,
            Message::Close(_) => bail!("client closed during daemon handshake"),
            _ => {}
        }
    }
}

fn write_json<S: io::Read + io::Write, T: serde::Serialize>(
    socket: &mut WebSocket<S>,
    value: &T,
) -> anyhow::Result<()> {
    socket.send(Message::Text(serde_json::to_string(value)?.into()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::daemon::WakuBackend;
    #[cfg(unix)]
    use crate::model::Project;
    use crate::model::{AgentSession, ProviderKind};
    #[cfg(unix)]
    use crate::persistence::StateStore;
    #[cfg(unix)]
    use crate::settings::DaemonSettingsStore;
    use crate::{DaemonSettings, WireDriverStartOptions};
    #[cfg(unix)]
    use base64::Engine as _;
    use crossbeam_channel::{RecvTimeoutError, bounded};
    use serde_json::json;
    use std::path::PathBuf;
    use waku_client::{DaemonClient, DaemonSupervisor};

    #[derive(Default)]
    struct TestBackend {
        runtimes: Mutex<HashMap<Uuid, Uuid>>,
    }

    impl Backend for TestBackend {
        fn handle(
            &self,
            request: Request,
            events: EventSink,
            _agent: Option<Uuid>,
        ) -> anyhow::Result<ResponsePayload> {
            let session_id = request.session_id;
            let runtime_id = request.runtime_id;
            match request.command {
                Command::Start { .. } => {
                    self.runtimes.lock().insert(session_id, runtime_id);
                    events.send(WireDriverEvent::new("connected", json!({})))?;
                    Ok(ResponsePayload::Started {
                        supports_steer: true,
                        supports_user_input_actions: false,
                    })
                }
                Command::AttachSession => Ok(ResponsePayload::SessionRuntime {
                    runtime_id: self.runtimes.lock().get(&session_id).copied(),
                    supports_steer: true,
                    supports_user_input_actions: false,
                }),
                Command::GetSettings => Ok(ResponsePayload::Settings {
                    settings: DaemonSettings::default(),
                }),
                Command::Prompt { prompt, .. } => {
                    events.send(WireDriverEvent::new("textDelta", json!(prompt)))?;
                    Ok(ResponsePayload::Ack)
                }
                Command::CloseSession => {
                    self.runtimes.lock().remove(&session_id);
                    Ok(ResponsePayload::Ack)
                }
                _ => Ok(ResponsePayload::Ack),
            }
        }
    }

    #[derive(Default)]
    struct TaskStateBackend {
        sessions: Mutex<Vec<AgentSession>>,
    }

    impl Backend for TaskStateBackend {
        fn handle(
            &self,
            request: Request,
            _events: EventSink,
            _agent: Option<Uuid>,
        ) -> anyhow::Result<ResponsePayload> {
            match request.command {
                Command::SaveTaskState { sessions, .. } => {
                    let mut stored = self.sessions.lock();
                    for session in sessions {
                        if let Some(existing) =
                            stored.iter_mut().find(|existing| existing.id == session.id)
                        {
                            *existing = session;
                        } else {
                            stored.push(session);
                        }
                    }
                    Ok(ResponsePayload::TaskStateSaved {
                        sessions: stored.clone(),
                    })
                }
                Command::LoadTaskState => Ok(ResponsePayload::TaskState {
                    projects: Vec::new(),
                    sessions: self.sessions.lock().clone(),
                    default_cwd: PathBuf::from("/tmp"),
                    projectless_root: Some(PathBuf::from("/tmp/.goddard/projects")),
                }),
                _ => Ok(ResponsePayload::Ack),
            }
        }
    }

    /// A pairing backend backed by the real service — requests flow
    /// through `request_blocking` and the test drives decisions through
    /// the same commands a connected client would.
    struct PairingBackend {
        pairing: Arc<crate::pairing::PairingService>,
    }

    impl Backend for PairingBackend {
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
            "testbox".into()
        }

        fn handle(
            &self,
            request: Request,
            _events: EventSink,
            _agent: Option<Uuid>,
        ) -> anyhow::Result<ResponsePayload> {
            match request.command {
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
                _ => Ok(ResponsePayload::Ack),
            }
        }
    }

    struct PairingDir(PathBuf);
    impl Drop for PairingDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn pairing_backend() -> (PairingDir, Arc<PairingBackend>) {
        let dir = std::env::temp_dir().join(format!("waku-pairing-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (
            PairingDir(dir.clone()),
            Arc::new(PairingBackend {
                pairing: Arc::new(crate::pairing::PairingService::new(&dir, "testbox".into())),
            }),
        )
    }

    fn pairing_server(backend: Arc<dyn Backend>) -> (std::net::SocketAddr, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                backend,
                server_shutdown,
                ServerOptions::default(),
            )
            .unwrap()
        });
        (address, shutdown)
    }

    /// Runs `pair` on a worker thread — the call blocks until a decision.
    fn pair_device(
        address: String,
        device: &'static str,
    ) -> std::thread::JoinHandle<anyhow::Result<waku_client::PairReply>> {
        std::thread::spawn(move || waku_client::pair(&address, device, Duration::from_secs(30)))
    }

    #[test]
    fn pair_request_waits_for_approval_then_connects() {
        let (_dir, backend) = pairing_backend();
        let (address, shutdown) = pairing_server(backend);
        let owner = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let pairing_updates = owner.subscribe_pairing();

        let request = pair_device(address.to_string(), "phone");
        // The owner's pairing document shows the pending device; approving
        // it by command is what releases the token.
        let pending = loop {
            match pairing_updates
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                state if !state.pending.is_empty() => break state.pending[0].clone(),
                _ => continue,
            }
        };
        assert_eq!(pending.device_name, "phone");
        assert_eq!(pending.transport, "ws");
        owner
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::RespondPairRequest {
                    request_id: pending.request_id,
                    accept: true,
                },
            )
            .unwrap();

        let waku_client::PairReply::Granted { token, daemon_name } =
            request.join().unwrap().unwrap()
        else {
            panic!("expected the pair request to be granted");
        };
        assert_eq!(daemon_name, "testbox");

        // The minted token authenticates like the bearer, as a full client.
        let paired = DaemonClient::connect(&address.to_string(), token.clone()).unwrap();
        assert!(matches!(
            paired
                .request(Uuid::nil(), Uuid::nil(), Command::GetPairing)
                .unwrap(),
            ResponsePayload::Pairing { .. }
        ));

        // Revoking it through the owner rejects the next connection.
        let client_id = paired
            .request(Uuid::nil(), Uuid::nil(), Command::GetPairing)
            .map(|payload| match payload {
                ResponsePayload::Pairing { state } => state.clients[0].client_id,
                _ => panic!("expected pairing state"),
            })
            .unwrap();
        owner
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::RevokePairedClient { client_id },
            )
            .unwrap();
        assert!(DaemonClient::connect(&address.to_string(), token).is_err());

        paired.shutdown();
        owner.shutdown();
        shutdown.store(true, Ordering::Release);
    }

    #[test]
    fn declined_pair_request_gets_no_token() {
        let (_dir, backend) = pairing_backend();
        let (address, shutdown) = pairing_server(backend.clone());
        let owner = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let pairing_updates = owner.subscribe_pairing();

        let request = pair_device(address.to_string(), "phone");
        let pending = loop {
            match pairing_updates
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            {
                state if !state.pending.is_empty() => break state.pending[0].clone(),
                _ => continue,
            }
        };
        owner
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::RespondPairRequest {
                    request_id: pending.request_id,
                    accept: false,
                },
            )
            .unwrap();

        assert!(matches!(
            request.join().unwrap().unwrap(),
            waku_client::PairReply::Declined { .. }
        ));
        // A decline is not a permission: the store stays empty.
        assert!(matches!(
            owner
                .request(Uuid::nil(), Uuid::nil(), Command::GetPairing)
                .unwrap(),
            ResponsePayload::Pairing { state } if state.clients.is_empty()
        ));

        owner.shutdown();
        shutdown.store(true, Ordering::Release);
    }

    #[test]
    fn task_state_revisions_notify_other_clients_only() {
        let hub = Hub::default();
        let (source_tx, source_rx) = unbounded();
        let source_id = hub.subscribe(&[], Subscriber::new(source_tx).0);
        let (observer_tx, observer_rx) = unbounded();
        hub.subscribe(&[], Subscriber::new(observer_tx).0);

        hub.task_state_changed(source_id);

        assert!(source_rx.try_recv().is_err());
        assert!(matches!(
            observer_rx.recv_timeout(Duration::from_secs(1)),
            Ok(ServerMessage::TaskStateChanged { revision: 1 })
        ));
    }

    /// A friend session feed subscribes through `session_stream`: it sees
    /// only its session's events, replays journal entries, and its drop
    /// unsubscribes without touching other subscribers.
    #[test]
    fn session_stream_filters_and_replays() {
        let hub = Arc::new(Hub::default());
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();
        let runtime = Uuid::new_v4();
        let delta = |sequence| WireDriverEvent::new("textDelta", json!(sequence));

        hub.begin_runtime(session_a, runtime);
        hub.begin_runtime(session_b, runtime);
        hub.emit(session_a, runtime, delta(1), true);
        hub.emit(session_b, runtime, delta(1), true);

        let stream = hub.session_stream(session_a, None);
        let replayed = stream.events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            replayed,
            ServerMessage::Event(SequencedEvent {
                session_id: got,
                sequence: 1,
                ..
            }) if got == session_a
        ));
        // session_b's event is not replayed to a filtered stream.
        assert!(stream.events.try_recv().is_err());

        hub.emit(session_b, runtime, delta(2), true);
        hub.emit(session_a, runtime, delta(2), true);
        let live = stream.events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            live,
            ServerMessage::Event(SequencedEvent { sequence: 2, .. })
        ));
        assert!(stream.events.try_recv().is_err());
    }

    /// `emit_external` injects a friend's events under their own ids and
    /// sequences; `friend_session_closed` clears the runtime and
    /// broadcasts the close notice.
    #[test]
    fn friend_session_events_reach_subscribers() {
        let hub = Hub::default();
        let (tx, rx) = unbounded();
        hub.subscribe(&[], Subscriber::new(tx).0);

        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        hub.emit_external(SequencedEvent {
            session_id,
            runtime_id,
            epoch,
            sequence: 7,
            event: WireDriverEvent::new("textDelta", json!("hi")),
        });
        match rx.recv_timeout(Duration::from_secs(1)).unwrap() {
            ServerMessage::Event(event) => {
                assert_eq!(event.session_id, session_id);
                assert_eq!(event.sequence, 7);
            }
            other => panic!("expected Event, got {other:?}"),
        }

        hub.friend_session_closed(session_id, true);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ServerMessage::FriendSessionClosed {
                session_id: got,
                revoked: true,
            } if got == session_id
        ));
        // The runtime was retired — another emit under it restarts the
        // journal rather than appending to a dead runtime's stream.
        hub.emit_external(SequencedEvent {
            session_id,
            runtime_id,
            epoch,
            sequence: 8,
            event: WireDriverEvent::new("textDelta", json!("late")),
        });
        match rx.recv_timeout(Duration::from_secs(1)).unwrap() {
            ServerMessage::Event(event) => assert_eq!(event.sequence, 8),
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[test]
    fn websocket_task_state_changes_reach_another_client() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(TaskStateBackend::default()),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let source = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let observer = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let source_revisions = source.subscribe_task_state();
        let observer_revisions = observer.subscribe_task_state();
        // The handshake can finish before the server registers its subscriber.
        // Complete the observer's initial load before another client publishes.
        assert!(matches!(
            observer
                .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
                .unwrap(),
            ResponsePayload::TaskState { .. }
        ));
        let session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let session_id = session.id;

        assert!(matches!(
            source
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    Command::SaveTaskState {
                        projects: Vec::new(),
                        live_session_ids: vec![session_id],
                        sessions: vec![session],
                        session_tails: Vec::new(),
                    },
                )
                .unwrap(),
            ResponsePayload::TaskStateSaved { .. }
        ));
        assert_eq!(
            observer_revisions.recv_timeout(Duration::from_secs(1)),
            Ok(1)
        );
        assert!(source_revisions.try_recv().is_err());
        let ResponsePayload::TaskState { sessions, .. } = observer
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected daemon task state");
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, session_id);

        // Streaming checkpoints update transcript detail and `updated_at`,
        // but do not change anything rendered in another client's task
        // catalog. They must not trigger a list reload for every stream save.
        let mut checkpoint = sessions[0].clone();
        checkpoint.updated_at = checkpoint.updated_at.saturating_add(1);
        source
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: Vec::new(),
                    live_session_ids: vec![session_id],
                    sessions: vec![checkpoint],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();
        assert!(
            observer_revisions
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );

        // Desktop persistence uses fire-and-forget notifications, while Web
        // uses requests. Both directions must wake the other application's
        // catalog without echoing back to the source connection.
        let second = AgentSession::new(Uuid::new_v4(), ProviderKind::Claude);
        let second_id = second.id;
        observer
            .notify(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: Vec::new(),
                    live_session_ids: vec![session_id, second_id],
                    sessions: vec![second],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();
        assert_eq!(source_revisions.recv_timeout(Duration::from_secs(1)), Ok(2));
        assert!(observer_revisions.try_recv().is_err());
        let ResponsePayload::TaskState { sessions, .. } = source
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected daemon task state");
        };
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|session| session.id == second_id));

        source.shutdown();
        server.join().unwrap();
    }

    #[test]
    fn hello_reports_the_daemon_build_commit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(TaskStateBackend::default()),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    build_commit: Some("abc1234".into()),
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        assert_eq!(client.daemon_commit(), Some("abc1234"));
        assert_eq!(client.daemon_version(), env!("CARGO_PKG_VERSION"));

        client.shutdown();
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stale_projection_cannot_resurrect_a_removed_session() {
        let root = std::env::temp_dir().join(format!("waku-remove-race-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let stale_client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let remover = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let project = Project::from_path(root.join("repo"));
        let mut session = AgentSession::new(project.id, ProviderKind::Codex);
        session.begin_turn("persist me");
        stale_client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![project.clone()],
                    live_session_ids: vec![session.id],
                    sessions: vec![session.clone()],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();
        remover
            .request(session.id, Uuid::nil(), Command::RemoveSession)
            .unwrap();
        let ResponsePayload::TaskStateSaved { sessions } = stale_client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![project],
                    live_session_ids: vec![session.id],
                    sessions: vec![session],
                    session_tails: Vec::new(),
                },
            )
            .unwrap()
        else {
            panic!("expected task-state save response");
        };
        assert!(sessions.is_empty());
        let ResponsePayload::TaskState { sessions, .. } = stale_client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected task state");
        };
        assert!(sessions.is_empty());

        stale_client.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_unstarted_draft_is_never_catalogued() {
        let root = std::env::temp_dir().join(format!("waku-draft-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let project = Project::from_path(root.join("repo"));
        // A detail-loaded draft: real to the client, but it owns no row.
        let draft = AgentSession::new(project.id, ProviderKind::Codex);
        let ResponsePayload::TaskStateSaved { sessions } = client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![project.clone()],
                    live_session_ids: vec![draft.id],
                    sessions: vec![draft.clone()],
                    session_tails: Vec::new(),
                },
            )
            .unwrap()
        else {
            panic!("expected task-state save response");
        };
        assert!(sessions.is_empty());
        let ResponsePayload::TaskState { sessions, .. } = client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected task state");
        };
        assert!(sessions.is_empty());

        // Once the draft starts it is catalogued like any other session.
        let mut started = draft;
        started.begin_turn("run it");
        let ResponsePayload::TaskStateSaved { sessions } = client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![project],
                    live_session_ids: vec![started.id],
                    sessions: vec![started.clone()],
                    session_tails: Vec::new(),
                },
            )
            .unwrap()
        else {
            panic!("expected task-state save response");
        };
        assert_eq!(sessions.len(), 1);
        let ResponsePayload::TaskState { sessions, .. } = client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected task state");
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, started.id);

        client.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The projectless workspace root is a process-global slot, so tests that
    /// need it point it at one shared throwaway directory — every test writes
    /// the same value, making the set idempotent regardless of ordering, and
    /// per-test paths stay unique through their uuid names.
    #[cfg(unix)]
    fn projectless_test_root() -> PathBuf {
        std::env::temp_dir()
            .join("waku-projectless-test-root")
            .join("projects")
    }

    /// Clients that provision a projectless workspace persist its project
    /// row before the first prompt's session exists; the save's orphan sweep
    /// must leave a freshly created row alone or the submit fails on a
    /// project the daemon forgot it just catalogued. Classification is
    /// path-based, so the workspace directory never needs to exist.
    #[cfg(unix)]
    #[test]
    fn a_fresh_projectless_project_survives_until_its_first_task() {
        crate::projectless::set_workspace_root(Some(projectless_test_root()));
        let root = std::env::temp_dir().join(format!("waku-projectless-gc-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let workspace_root = projectless_test_root();
        let project = Project::from_path(
            workspace_root
                .join("2026-01-01")
                .join(format!("grace-{}", Uuid::new_v4())),
        );
        assert!(project.is_projectless());
        client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![project.clone()],
                    live_session_ids: vec![],
                    sessions: vec![],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();
        let ResponsePayload::TaskState { projects, .. } = client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected task state");
        };
        assert!(
            projects.iter().any(|item| item.id == project.id),
            "a pending projectless project must outlive the save"
        );

        // A row past the grace window with no task is still swept — the
        // window only covers provisioning, not abandoned orphans.
        let mut stale = Project::from_path(
            workspace_root
                .join("2026-01-01")
                .join(format!("orphan-{}", Uuid::new_v4())),
        );
        stale.created_at = crate::model::unix_time() - 2 * 24 * 60 * 60;
        client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![stale.clone()],
                    live_session_ids: vec![],
                    sessions: vec![],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();
        let ResponsePayload::TaskState { projects, .. } = client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected task state");
        };
        assert!(
            !projects.iter().any(|item| item.id == stale.id),
            "an orphaned projectless project must still be swept"
        );

        // A task-claimed row survives regardless of age.
        let mut claimed = Project::from_path(
            workspace_root
                .join("2026-01-01")
                .join(format!("claimed-{}", Uuid::new_v4())),
        );
        claimed.created_at = stale.created_at;
        let mut session = AgentSession::new(claimed.id, ProviderKind::Codex);
        session.begin_turn("run it");
        client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![claimed.clone()],
                    live_session_ids: vec![session.id],
                    sessions: vec![session],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();
        let ResponsePayload::TaskState { projects, .. } = client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected task state");
        };
        assert!(
            projects.iter().any(|item| item.id == claimed.id),
            "a projectless project with a task is never swept"
        );

        client.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    /// A projectless workspace directory can vanish between draft creation
    /// and the first prompt — trash emptied, archive cleanup, a stale
    /// listing. The daemon owns the scratch space, so Start recreates it
    /// instead of dying inside the provider spawn with an opaque ENOENT.
    #[cfg(unix)]
    #[test]
    fn start_recreates_a_missing_projectless_workspace() {
        let root = std::env::temp_dir().join(format!("waku-start-cwd-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        // The daemon's restore writes land under the shared test root, not
        // the real home.
        crate::projectless::set_workspace_root(Some(projectless_test_root()));
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let missing_workspace = projectless_test_root()
            .join("2026-01-01")
            .join(format!("gone-{}", Uuid::new_v4()));
        let mut options = WireDriverStartOptions {
            binary: PathBuf::from("/nonexistent/waku-test-provider"),
            cwd: missing_workspace.clone(),
            ..test_start_options()
        };
        // The launch still fails — no provider binary — but the missing
        // projectless cwd is recreated first.
        let _ = client.request(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Command::Start {
                options: options.clone(),
            },
        );
        assert!(
            missing_workspace.is_dir(),
            "start must recreate a missing projectless workspace"
        );

        // An ordinary missing cwd names the path instead of spawning blind.
        options.cwd = root.join("repo-that-was-deleted");
        let error = client
            .request(Uuid::new_v4(), Uuid::new_v4(), Command::Start { options })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("the task's working directory does not exist"),
            "{error}"
        );
        assert!(!root.join("repo-that-was-deleted").exists());

        client.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(
            projectless_test_root()
                .parent()
                .expect("the test root has a parent"),
        )
        .ok();
    }

    #[cfg(unix)]
    fn serve_task_state(
        root: &std::path::Path,
        state: crate::persistence::PersistedState,
    ) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
        let store = StateStore::daemon(root.join("app.db"));
        let mut state = state;
        store.save(&mut state).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            store,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });
        (address, server)
    }

    #[cfg(unix)]
    fn hydrate(client: &DaemonClient, session_id: Uuid) -> AgentSession {
        let ResponsePayload::Session {
            session: Some(session),
        } = client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::HydrateSession { session_id },
            )
            .unwrap()
        else {
            panic!("expected the stored session to hydrate");
        };
        session
    }

    /// The post-restart orphaned-runtime save: a session that was busy when
    /// the daemon stopped is interrupted and saved while still a skeleton on
    /// the client. Its column update must land without the projection's
    /// placeholder workspace or empty transcript erasing stored detail.
    #[cfg(unix)]
    #[test]
    fn a_skeleton_save_updates_columns_without_erasing_detail() {
        let root = std::env::temp_dir().join(format!("waku-skeleton-save-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut state = crate::persistence::PersistedState::fresh(root.join("repo"));
        let session_id = state.sessions[0].id;
        let worktree = crate::model::SessionWorkspace::Worktree {
            path: root.join("repo-worktrees/task"),
            name: "task".into(),
            branch: Some("waku/task".into()),
            base_branch: None,
        };
        {
            let session = &mut state.sessions[0];
            session.workspace = worktree.clone();
            session.begin_turn("ship it");
            session.finish_active_turn(crate::model::TurnStatus::Completed);
            session.status = SessionStatus::Working;
            session.runtime_event_cursor = Some(crate::model::RuntimeEventCursor {
                runtime_id: Uuid::new_v4(),
                epoch: Uuid::new_v4(),
                sequence: 3,
            });
        }
        let (address, server) = serve_task_state(&root, state);
        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

        // The reattach path hydrates the daemon's copy before the attach
        // fails, so the merge below runs against a hydrated, busy session.
        let hydrated = hydrate(&client, session_id);
        assert_eq!(hydrated.workspace, worktree);

        let mut skeleton = hydrated.list_projection();
        skeleton.status = SessionStatus::Idle;
        client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: Vec::new(),
                    live_session_ids: vec![session_id],
                    sessions: vec![skeleton],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();

        let after = hydrate(&client, session_id);
        assert_eq!(after.status, SessionStatus::Idle);
        assert_eq!(after.workspace, worktree);
        assert_eq!(after.turns.len(), 1);

        // The stored detail survived too — this is not just in memory.
        let store = StateStore::daemon(root.join("app.db"));
        let mut stored = store.load().unwrap().sessions;
        let stored = stored
            .iter_mut()
            .find(|session| session.id == session_id)
            .unwrap();
        store.hydrate(stored).unwrap();
        assert_eq!(stored.workspace, worktree);
        assert_eq!(stored.turns.len(), 1);

        client.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    /// Pinning or archiving a task that was never opened since launch sends
    /// its skeleton: the columns must merge while the stored transcript and
    /// workspace stay untouched, and an unknown skeleton creates nothing.
    #[cfg(unix)]
    #[test]
    fn a_skeleton_save_merges_columns_and_never_creates_a_row() {
        let root = std::env::temp_dir().join(format!("waku-skeleton-pin-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut state = crate::persistence::PersistedState::fresh(root.join("repo"));
        let session_id = state.sessions[0].id;
        let worktree = crate::model::SessionWorkspace::Worktree {
            path: root.join("repo-worktrees/task"),
            name: "task".into(),
            branch: None,
            base_branch: None,
        };
        {
            let session = &mut state.sessions[0];
            session.workspace = worktree.clone();
            session.begin_turn("ship it");
            session.finish_active_turn(crate::model::TurnStatus::Completed);
        }
        let (address, server) = serve_task_state(&root, state);
        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

        // The daemon's copy stays a skeleton here — nothing hydrated it.
        let ResponsePayload::TaskState { sessions, .. } = client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected daemon task state");
        };
        let mut skeleton = sessions
            .into_iter()
            .find(|session| session.id == session_id)
            .unwrap();
        assert!(!skeleton.detail_loaded);
        skeleton.pinned_at = Some(1);
        skeleton.updated_at += 1;
        let ghost = AgentSession::new(
            crate::model::Project::from_path(root.join("repo")).id,
            ProviderKind::Codex,
        )
        .list_projection();
        let ghost_id = ghost.id;
        client
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: Vec::new(),
                    live_session_ids: vec![session_id],
                    sessions: vec![skeleton, ghost],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();

        let after = hydrate(&client, session_id);
        assert_eq!(after.pinned_at, Some(1));
        assert_eq!(after.workspace, worktree);
        assert_eq!(after.turns.len(), 1);

        // The projection of a task the daemon never stored creates no row,
        // and the stored task's skeleton still reports its worktree — the
        // sidebar's badge must not depend on a hydrate.
        let ResponsePayload::TaskState { sessions, .. } = client
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap()
        else {
            panic!("expected daemon task state");
        };
        assert!(!sessions.iter().any(|session| session.id == ghost_id));
        assert_eq!(
            sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| &session.workspace),
            Some(&worktree)
        );

        client.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_scoped_agent_token_reaches_only_agent_commands() {
        let root = std::env::temp_dir().join(format!("goddard-agent-auth-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let settings = DaemonSettingsStore::open(root.join("settings.json")).unwrap();
        settings
            .replace(DaemonSettings {
                agent_tools_enabled: true,
                // A binary that cannot exist keeps the cold-start path from
                // ever spawning a real provider in a test.
                provider_binary_overrides: HashMap::from([(
                    ProviderKind::Codex,
                    "/nonexistent/waku-test-provider".into(),
                )]),
                ..DaemonSettings::default()
            })
            .unwrap();
        let backend = WakuBackend::new(settings, StateStore::daemon(root.join("app.db"))).unwrap();
        let sender_id = Uuid::new_v4();
        let agent_token = backend.agent.mint(sender_id);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        // A token the daemon never minted is no credential.
        let error = match DaemonClient::connect(&address.to_string(), "forged".into()) {
            Ok(_) => panic!("a forged token must not authenticate"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("authentication failed"),
            "{error}"
        );

        let agent = DaemonClient::connect(&address.to_string(), agent_token).unwrap();
        let human = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

        // A target task the daemon knows but has never run.
        let project = Project::from_path(root.join("repo"));
        let mut target = AgentSession::new(project.id, ProviderKind::Codex);
        target.provider_cursor = Some(crate::model::ProviderResumeCursor::from_session_id(
            ProviderKind::Codex,
            "thread-42".into(),
        ));
        let target_id = target.id;
        human
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::SaveTaskState {
                    projects: vec![project],
                    live_session_ids: vec![],
                    sessions: vec![target],
                    session_tails: Vec::new(),
                },
            )
            .unwrap();

        // The credential is confined to the agent command surface.
        let error = agent
            .request(Uuid::nil(), Uuid::nil(), Command::LoadTaskState)
            .unwrap_err();
        assert!(
            error.to_string().contains("may only run agent commands"),
            "{error}"
        );

        // Unknown targets are refused rather than queued.
        let error = agent
            .request(
                sender_id,
                Uuid::nil(),
                Command::AgentPrompt {
                    task_id: Some(Uuid::new_v4()),
                    thread_id: None,
                    provider: None,
                    prompt: "hi".into(),
                    delivery: crate::AgentPromptDelivery::Queue,
                },
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("unknown to the daemon"),
            "{error}"
        );

        // A provider-native thread id resolves to the same task; steer mode
        // on a task with no running turn is a clean error, not a cold start.
        let error = agent
            .request(
                sender_id,
                Uuid::nil(),
                Command::AgentPrompt {
                    task_id: None,
                    thread_id: Some("thread-42".into()),
                    provider: None,
                    prompt: "hi".into(),
                    delivery: crate::AgentPromptDelivery::Steer,
                },
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("no running session to steer"),
            "{error}"
        );

        // Queue mode on a known task attempts the cold start; the fake
        // binary makes the launch itself the deterministic failure.
        let error = agent
            .request(
                sender_id,
                Uuid::nil(),
                Command::AgentPrompt {
                    task_id: Some(target_id),
                    thread_id: None,
                    provider: None,
                    prompt: "hi".into(),
                    delivery: crate::AgentPromptDelivery::Queue,
                },
            )
            .unwrap_err();
        assert!(
            !error.to_string().contains("unknown to the daemon"),
            "{error}"
        );

        agent.shutdown();
        human.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn agent_commands_require_the_daemon_setting() {
        let root = std::env::temp_dir().join(format!("goddard-agent-gate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let sender_id = Uuid::new_v4();
        let agent_token = backend.agent.mint(sender_id);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let agent = DaemonClient::connect(&address.to_string(), agent_token).unwrap();
        let human = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let error = agent
            .request(
                sender_id,
                Uuid::nil(),
                Command::AgentPrompt {
                    task_id: Some(Uuid::new_v4()),
                    thread_id: None,
                    provider: None,
                    prompt: "hi".into(),
                    delivery: crate::AgentPromptDelivery::Queue,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("disabled"), "{error}");

        human.shutdown();
        agent.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn websocket_round_trip_sequences_provider_events() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(TestBackend::default()),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        assert!(
            DaemonClient::connect(&address.to_string(), "wrong-secret".into()).is_err(),
            "the server must reject a client before it can issue requests"
        );
        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let response = client
            .request(
                session_id,
                runtime_id,
                Command::Start {
                    options: WireDriverStartOptions {
                        provider: "codex".into(),
                        binary: PathBuf::from("codex"),
                        cwd: PathBuf::from("."),
                        mode: "fullAccess".into(),
                        model: None,
                        reasoning_effort: None,
                        service_tier: None,
                        context_window: None,
                        agent_preset: None,
                        computer_use_enabled: false,
                        read_own_transcript: false,
                        provider_cursor: None,
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            response,
            ResponsePayload::Started {
                supports_steer: true,
                ..
            }
        ));
        // Start can emit before a refreshed app discovers and subscribes to
        // the daemon-owned runtime. The client must retain that replay.
        let events = client.subscribe(session_id, runtime_id);
        let event = events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(event.runtime_id, runtime_id);
        assert_eq!(event.sequence, 1);
        assert_eq!(event.event.kind, "connected");

        client.shutdown();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(1)),
            Err(RecvTimeoutError::Disconnected)
        ));
        server.join().unwrap();
    }

    #[test]
    fn late_client_attaches_to_replay_and_live_runtime_events() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(TestBackend::default()),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let source = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let source_events = source.subscribe(session_id, runtime_id);
        source
            .request(
                session_id,
                runtime_id,
                Command::Start {
                    options: WireDriverStartOptions {
                        provider: "codex".into(),
                        binary: PathBuf::from("codex"),
                        cwd: PathBuf::from("."),
                        mode: "fullAccess".into(),
                        model: None,
                        reasoning_effort: None,
                        service_tier: None,
                        context_window: None,
                        agent_preset: None,
                        computer_use_enabled: false,
                        read_own_transcript: false,
                        provider_cursor: None,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            source_events
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .event
                .kind,
            "connected"
        );

        let late = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        assert!(matches!(
            late.request(session_id, Uuid::nil(), Command::AttachSession)
                .unwrap(),
            ResponsePayload::SessionRuntime {
                runtime_id: Some(attached),
                supports_steer: true,
                ..
            } if attached == runtime_id
        ));
        let late_events = late.subscribe(session_id, runtime_id);
        let replayed = late_events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(replayed.sequence, 1);
        assert_eq!(replayed.event.kind, "connected");

        source
            .request(
                session_id,
                runtime_id,
                Command::Prompt {
                    prompt: "streamed from the first client".into(),
                    turn_id: None,
                    message_id: None,
                    hidden: false,
                    attachments: Vec::new(),
                },
            )
            .unwrap();
        for events in [&source_events, &late_events] {
            let live = events.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(live.sequence, 2);
            assert_eq!(live.event.kind, "textDelta");
            assert_eq!(live.event.payload, json!("streamed from the first client"));
        }

        source
            .request(session_id, runtime_id, Command::CloseSession)
            .unwrap();
        let after_close = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        assert!(matches!(
            after_close
                .request(session_id, Uuid::nil(), Command::AttachSession)
                .unwrap(),
            ResponsePayload::SessionRuntime {
                runtime_id: None,
                supports_steer: true,
                ..
            }
        ));
        let stale_events = after_close.subscribe(session_id, runtime_id);
        assert!(
            stale_events
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "an explicitly closed runtime must not replay into future clients"
        );

        source.shutdown();
        server.join().unwrap();
    }

    #[test]
    fn remote_supervisor_reconnects_without_losing_the_daemon_runtime() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        // Keep the address reserved between servers. If the first listener is
        // dropped before the replacement binds, a parallel test can claim the
        // newly freed ephemeral port and make this test fail with AddrInUse.
        let replacement_listener = listener.try_clone().unwrap();
        let address = listener.local_addr().unwrap();
        let backend = Arc::new(TestBackend::default());
        let first_shutdown = Arc::new(AtomicBool::new(false));
        let first_server = {
            let backend = backend.clone();
            let shutdown = first_shutdown.clone();
            std::thread::spawn(move || {
                serve(
                    listener,
                    "secret".into(),
                    backend,
                    shutdown,
                    ServerOptions::default(),
                )
                .unwrap()
            })
        };

        let supervisor = DaemonSupervisor::connect(&address.to_string(), "secret".into()).unwrap();
        let clients = supervisor.subscribe_clients();
        let initial = clients.recv_timeout(Duration::from_secs(1)).unwrap();
        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        initial
            .request(
                session_id,
                runtime_id,
                Command::Start {
                    options: WireDriverStartOptions {
                        provider: "codex".into(),
                        binary: PathBuf::from("codex"),
                        cwd: PathBuf::from("."),
                        mode: "fullAccess".into(),
                        model: None,
                        reasoning_effort: None,
                        service_tier: None,
                        context_window: None,
                        agent_preset: None,
                        computer_use_enabled: false,
                        read_own_transcript: false,
                        provider_cursor: None,
                    },
                },
            )
            .unwrap();

        first_shutdown.store(true, Ordering::Release);
        first_server.join().unwrap();

        let second_shutdown = Arc::new(AtomicBool::new(false));
        let second_server = {
            let backend = backend.clone();
            let shutdown = second_shutdown.clone();
            std::thread::spawn(move || {
                serve(
                    replacement_listener,
                    "secret".into(),
                    backend,
                    shutdown,
                    ServerOptions::default(),
                )
                .unwrap()
            })
        };

        let replacement = clients.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!initial.same_connection(&replacement));
        assert!(matches!(
            replacement
                .request(session_id, Uuid::nil(), Command::AttachSession)
                .unwrap(),
            ResponsePayload::SessionRuntime {
                runtime_id: Some(attached),
                supports_steer: true,
                ..
            } if attached == runtime_id
        ));

        second_shutdown.store(true, Ordering::Release);
        second_server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dropping_an_idle_terminal_does_not_wait_for_output() {
        let root = std::env::temp_dir().join(format!("waku-terminal-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let hub = Arc::new(Hub::default());
        let terminal = crate::terminal::DaemonTerminal::open_with_shell(
            &root,
            80,
            24,
            hub.event_sink(Uuid::new_v4(), Uuid::new_v4()),
            terminal_test_shell("while IFS= read -r line; do :; done"),
        )
        .unwrap();
        let (dropped, finished) = bounded(1);
        std::thread::spawn(move || {
            drop(terminal);
            let _ = dropped.send(());
        });

        assert!(
            finished.recv_timeout(Duration::from_secs(3)).is_ok(),
            "dropping an idle daemon terminal blocked on its output reader"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn websocket_terminal_round_trip_streams_input_and_output() {
        websocket_terminal_round_trip(false);
    }

    #[cfg(unix)]
    #[test]
    fn websocket_terminal_close_does_not_wait_for_a_shell_ignoring_hangup() {
        websocket_terminal_round_trip(true);
    }

    #[cfg(unix)]
    fn terminal_test_shell(script: &str) -> alacritty_terminal::tty::Shell {
        // Do not load the developer's or CI runner's login files, prompt
        // plugins, or terminal capability queries in a transport test.
        alacritty_terminal::tty::Shell::new("/bin/sh".into(), vec!["-c".into(), script.into()])
    }

    #[cfg(unix)]
    fn websocket_terminal_round_trip(ignore_hangup: bool) {
        let root = std::env::temp_dir().join(format!("waku-terminal-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap()
        .with_terminal_shell(terminal_test_shell(&format!(
            "{}\nprintf 'ready:%s\\n' \"$$\"\nwhile IFS= read -r line; do printf 'received:%s\\n' \"$line\"; done",
            if ignore_hangup { "trap '' HUP" } else { ":" },
        )));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });

        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let terminal_id = Uuid::new_v4();
        let events = client.subscribe(terminal_id, terminal_id);
        assert!(matches!(
            client
                .request(
                    terminal_id,
                    terminal_id,
                    Command::OpenTerminal {
                        cwd: root.clone(),
                        cols: 80,
                        rows: 24,
                        owner: None,
                    },
                )
                .unwrap(),
            ResponsePayload::Ack
        ));
        // Wait until the shell has installed its signal handler. Receiving
        // local echo alone does not prove that shell startup has completed.
        let ready = terminal_output_until(&events, b"\n");
        let child_pid: libc::pid_t = String::from_utf8_lossy(&ready)
            .trim()
            .strip_prefix("ready:")
            .unwrap()
            .parse()
            .unwrap();
        client
            .request(
                terminal_id,
                terminal_id,
                Command::WriteTerminal {
                    data: b"waku-terminal-round-trip\r".to_vec(),
                },
            )
            .unwrap();

        // The response prefix is absent from the input, so a PTY echo cannot
        // satisfy this assertion before the child has actually read it.
        terminal_output_until(&events, b"received:waku-terminal-round-trip");

        let (closed, finished) = bounded(1);
        let closing_client = client.clone();
        let close = std::thread::spawn(move || {
            let _ = closed.send(closing_client.request(
                terminal_id,
                terminal_id,
                Command::CloseTerminal,
            ));
        });
        let result = finished.recv_timeout(Duration::from_secs(3));
        if result.is_err() {
            // Clean up the fixture even when shutdown regresses, and fail
            // here instead of waiting for the client's 120-second timeout.
            unsafe {
                libc::kill(child_pid, libc::SIGKILL);
            }
        }
        client.shutdown();
        server.join().unwrap();
        close.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            matches!(result, Ok(Ok(ResponsePayload::Ack))),
            "closing daemon terminal did not complete: {result:?}"
        );
    }

    #[cfg(unix)]
    fn terminal_output_until(events: &Receiver<SequencedEvent>, marker: &[u8]) -> Vec<u8> {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        let mut seen_events = Vec::new();
        while std::time::Instant::now() < deadline
            && !output.windows(marker.len()).any(|window| window == marker)
        {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let Ok(event) = events.recv_timeout(remaining) else {
                break;
            };
            seen_events.push(event.event.kind.clone());
            if event.event.kind != "terminalOutput" {
                continue;
            }
            let data = event.event.payload["data"].as_str().unwrap();
            output.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .unwrap(),
            );
        }
        assert!(
            output.windows(marker.len()).any(|window| window == marker),
            "daemon terminal did not return the shell marker; events={seen_events:?}, output={}",
            String::from_utf8_lossy(&output)
        );
        output
    }

    #[test]
    fn nil_request_ids_execute_without_responses_or_cache_entries() {
        let (outgoing, responses) = unbounded();
        let hub = Arc::new(Hub::default());
        let handled = handle_request(
            Request {
                request_id: Uuid::nil(),
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::GetSettings,
            },
            outgoing,
            0,
            None,
            Arc::new(TestBackend::default()),
            hub.clone(),
        );

        assert!(handled.executed);
        assert!(matches!(handled.outcome, ResponseOutcome::Ok { .. }));
        assert!(responses.try_recv().is_err());
        assert!(hub.cached_response(Uuid::nil()).is_none());
    }

    #[test]
    fn browser_origins_are_denied_unless_explicitly_allowed() {
        let request = HandshakeRequest::builder()
            .uri("/v1")
            .header(ORIGIN, "https://app.waku.test")
            .body(())
            .unwrap();
        let response = HandshakeResponse::new(());
        assert_eq!(
            validate_handshake(&request, response, &HashSet::new())
                .unwrap_err()
                .status(),
            StatusCode::FORBIDDEN
        );

        let allowed = HashSet::from(["https://app.waku.test".to_owned()]);
        assert!(validate_handshake(&request, HandshakeResponse::new(()), &allowed).is_ok());
        let native = HandshakeRequest::builder().uri("/v1").body(()).unwrap();
        assert!(validate_handshake(&native, HandshakeResponse::new(()), &HashSet::new()).is_ok());

        let react_native = HandshakeRequest::builder()
            .uri("/v1")
            .header(ORIGIN, "http://192.168.0.114:34125")
            .header(NATIVE_CLIENT_HEADER, NATIVE_CLIENT_HEADER_VALUE)
            .body(())
            .unwrap();
        assert!(
            validate_handshake(&react_native, HandshakeResponse::new(()), &HashSet::new()).is_ok()
        );

        let forged_native = HandshakeRequest::builder()
            .uri("/v1")
            .header(ORIGIN, "https://attacker.example")
            .header(NATIVE_CLIENT_HEADER, "browser")
            .body(())
            .unwrap();
        assert_eq!(
            validate_handshake(&forged_native, HandshakeResponse::new(()), &HashSet::new())
                .unwrap_err()
                .status(),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn daemon_tokens_require_an_exact_match() {
        assert!(token_matches("secret", "secret"));
        assert!(!token_matches("secret", "Secret"));
        assert!(!token_matches("secret", "secret-extra"));
    }

    #[test]
    fn replaced_runtime_ignores_late_events_from_the_old_generation() {
        let hub = Arc::new(Hub::default());
        let session_id = Uuid::new_v4();
        let old_runtime_id = Uuid::new_v4();
        let new_runtime_id = Uuid::new_v4();
        let (outgoing, events) = unbounded();
        hub.subscribe(&[], Subscriber::new(outgoing).0);

        hub.begin_runtime(session_id, old_runtime_id);
        let old_sink = hub.event_sink(session_id, old_runtime_id);
        old_sink
            .send(WireDriverEvent::new("old", serde_json::Value::Null))
            .unwrap();
        assert!(matches!(
            events.recv().unwrap(),
            ServerMessage::Event(event) if event.runtime_id == old_runtime_id
        ));

        hub.begin_runtime(session_id, new_runtime_id);
        old_sink
            .send(WireDriverEvent::new("stale", serde_json::Value::Null))
            .unwrap();
        hub.event_sink(session_id, new_runtime_id)
            .send(WireDriverEvent::new("new", serde_json::Value::Null))
            .unwrap();

        let ServerMessage::Event(event) = events.recv().unwrap() else {
            panic!("expected a daemon event");
        };
        assert_eq!(event.runtime_id, new_runtime_id);
        assert_eq!(event.sequence, 1);
        assert_eq!(event.event.kind, "new");
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn replay_cursor_from_an_old_daemon_epoch_does_not_hide_new_events() {
        let hub = Arc::new(Hub::default());
        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        hub.begin_runtime(session_id, runtime_id);
        hub.event_sink(session_id, runtime_id)
            .send(WireDriverEvent::new("new-daemon", serde_json::Value::Null))
            .unwrap();

        let (outgoing, events) = unbounded();
        hub.subscribe(
            &[ReplayCursor {
                session_id,
                runtime_id,
                epoch: Uuid::nil(),
                sequence: u64::MAX,
            }],
            Subscriber::new(outgoing).0,
        );

        let ServerMessage::Event(event) = events.recv().unwrap() else {
            panic!("expected the new daemon event to replay");
        };
        assert_eq!(event.epoch, hub.epoch);
        assert_eq!(event.sequence, 1);
    }

    #[test]
    fn subscriber_that_cannot_keep_up_is_kicked_and_removed() {
        let hub = Arc::new(Hub::default());
        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        // Capacity one and no drainer: the second queued message overflows.
        let (outgoing, events) = bounded(1);
        let (subscriber, kicked) = Subscriber::new(outgoing);
        hub.subscribe(&[], subscriber);
        hub.begin_runtime(session_id, runtime_id);
        let sink = hub.event_sink(session_id, runtime_id);

        sink.send(WireDriverEvent::new("one", serde_json::Value::Null))
            .unwrap();
        // Nobody drains, so a second queued message overflows: the subscriber
        // is dropped from the hub and its connection is told to close.
        sink.send(WireDriverEvent::new("two", serde_json::Value::Null))
            .unwrap();
        assert!(kicked.try_recv().is_ok());

        assert!(matches!(
            events.try_recv(),
            Ok(ServerMessage::Event(event)) if event.event.kind == "one"
        ));
        assert!(events.try_recv().is_err());
        sink.send(WireDriverEvent::new("three", serde_json::Value::Null))
            .unwrap();
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn a_retired_runtime_tells_attached_clients_it_ended() {
        let hub = Arc::new(Hub::default());
        let session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let (outgoing, events) = bounded(4);
        hub.subscribe(&[], Subscriber::new(outgoing).0);
        hub.begin_runtime(session_id, runtime_id);
        let sink = hub.event_sink(session_id, runtime_id);

        // Idle eviction's order: the ended signal must go out before the
        // hub retires routing — afterwards emit drops it as stale.
        sink.notify_runtime_ended();
        sink.end_session_runtime();

        let ServerMessage::Event(event) = events.recv().unwrap() else {
            panic!("expected the runtime-ended event to reach the client");
        };
        assert_eq!(event.session_id, session_id);
        assert_eq!(event.runtime_id, runtime_id);
        assert_eq!(event.event.kind, "processExited");
        // Nothing lingers to replay: a reconnecting client learns the
        // runtime is gone from its attach response, not a stale entry.
        assert_eq!(sink.journaled_event_count(session_id), 0);
    }

    #[test]
    fn response_cache_evicts_oversized_and_byte_budget_exceeding_outcomes() {
        let hub = Hub::default();
        let oversized_id = Uuid::new_v4();
        let oversized = ResponseOutcome::Ok {
            payload: ResponsePayload::Cursor {
                cursor: Some(serde_json::Value::String(
                    "x".repeat(MAX_CACHED_RESPONSE_BYTES + 1),
                )),
            },
        };
        hub.cache_response(oversized_id, oversized);
        // A single outcome larger than the whole byte budget is never
        // retained; retrying the request runs it again instead of pinning a
        // >64 MB payload in daemon memory.
        assert!(hub.cached_response(oversized_id).is_none());

        let kept_id = Uuid::new_v4();
        hub.cache_response(
            kept_id,
            ResponseOutcome::Ok {
                payload: ResponsePayload::Ack,
            },
        );
        assert!(hub.cached_response(kept_id).is_some());
        assert!(hub.cached_response(oversized_id).is_none());
    }

    struct BlockingProbeBackend {
        probe_started: Sender<()>,
        release_probe: Receiver<()>,
    }

    impl Backend for BlockingProbeBackend {
        fn handle(
            &self,
            request: Request,
            _: EventSink,
            _agent: Option<Uuid>,
        ) -> anyhow::Result<ResponsePayload> {
            if matches!(request.command, Command::ProbeProvider { .. }) {
                self.probe_started.send(()).unwrap();
                self.release_probe.recv().unwrap();
            }
            Ok(ResponsePayload::Ack)
        }
    }

    #[test]
    fn slow_background_command_does_not_block_session_hydration() {
        let (outgoing, response_rx) = unbounded();
        let (probe_started, probe_started_rx) = bounded(1);
        let (release_probe, release_probe_rx) = bounded(1);
        let backend: Arc<dyn Backend> = Arc::new(BlockingProbeBackend {
            probe_started,
            release_probe: release_probe_rx,
        });
        let hub = Arc::new(Hub::default());
        let dispatcher = RequestDispatcher::new(backend, hub);

        let probe_id = Uuid::new_v4();
        dispatcher.dispatch(
            Request {
                request_id: probe_id,
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::ProbeProvider {
                    provider: crate::model::ProviderKind::Codex,
                    binary_override: None,
                    discover_models: false,
                    probe_version: false,
                },
            },
            outgoing.clone(),
            0,
            None,
        );
        probe_started_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        let hydration_id = Uuid::new_v4();
        dispatcher.dispatch(
            Request {
                request_id: hydration_id,
                session_id: Uuid::nil(),
                runtime_id: Uuid::nil(),
                command: Command::HydrateSession {
                    session_id: Uuid::new_v4(),
                },
            },
            outgoing,
            0,
            None,
        );
        assert!(matches!(
            response_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ServerMessage::Response { request_id, .. } if request_id == hydration_id
        ));

        release_probe.send(()).unwrap();
        assert!(matches!(
            response_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ServerMessage::Response { request_id, .. } if request_id == probe_id
        ));
    }

    struct RuntimeOrderingBackend {
        blocked_session_id: Uuid,
        handled: Sender<(Uuid, &'static str)>,
        release_start: Receiver<()>,
    }

    impl Backend for RuntimeOrderingBackend {
        fn handle(
            &self,
            request: Request,
            _: EventSink,
            _agent: Option<Uuid>,
        ) -> anyhow::Result<ResponsePayload> {
            let command = match request.command {
                Command::Start { .. } => {
                    self.handled.send((request.session_id, "start")).unwrap();
                    if request.session_id == self.blocked_session_id {
                        self.release_start.recv().unwrap();
                    }
                    return Ok(ResponsePayload::Started {
                        supports_steer: true,
                        supports_user_input_actions: false,
                    });
                }
                Command::Prompt { .. } => "prompt",
                Command::CloseSession => "close",
                _ => "other",
            };
            self.handled.send((request.session_id, command)).unwrap();
            Ok(ResponsePayload::Ack)
        }
    }

    #[test]
    fn runtime_commands_are_ordered_per_session_without_blocking_other_sessions() {
        let blocked_session_id = Uuid::new_v4();
        let blocked_runtime_id = Uuid::new_v4();
        let other_session_id = Uuid::new_v4();
        let other_runtime_id = Uuid::new_v4();
        let (handled, handled_rx) = unbounded();
        let (release_start, release_start_rx) = bounded(1);
        let dispatcher = RequestDispatcher::new(
            Arc::new(RuntimeOrderingBackend {
                blocked_session_id,
                handled,
                release_start: release_start_rx,
            }),
            Arc::new(Hub::default()),
        );
        let (start_outgoing, start_responses) = unbounded();
        let (second_client_outgoing, second_client_responses) = unbounded();
        let (other_outgoing, other_responses) = unbounded();

        let blocked_start_id = Uuid::new_v4();
        dispatcher.dispatch(
            Request {
                request_id: blocked_start_id,
                session_id: blocked_session_id,
                runtime_id: blocked_runtime_id,
                command: Command::Start {
                    options: test_start_options(),
                },
            },
            start_outgoing,
            0,
            None,
        );
        assert_eq!(
            handled_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (blocked_session_id, "start")
        );

        let prompt_id = Uuid::new_v4();
        dispatcher.dispatch(
            Request {
                request_id: prompt_id,
                session_id: blocked_session_id,
                runtime_id: blocked_runtime_id,
                command: Command::Prompt {
                    prompt: "after start".into(),
                    turn_id: None,
                    message_id: None,
                    hidden: false,
                    attachments: Vec::new(),
                },
            },
            second_client_outgoing,
            0,
            None,
        );

        let other_start_id = Uuid::new_v4();
        dispatcher.dispatch(
            Request {
                request_id: other_start_id,
                session_id: other_session_id,
                runtime_id: other_runtime_id,
                command: Command::Start {
                    options: test_start_options(),
                },
            },
            other_outgoing.clone(),
            0,
            None,
        );
        assert_eq!(
            handled_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (other_session_id, "start")
        );
        assert!(matches!(
            other_responses
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            ServerMessage::Response { request_id, .. } if request_id == other_start_id
        ));
        assert!(handled_rx.recv_timeout(Duration::from_millis(50)).is_err());

        release_start.send(()).unwrap();
        assert!(matches!(
            start_responses
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            ServerMessage::Response { request_id, .. } if request_id == blocked_start_id
        ));
        assert_eq!(
            handled_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            (blocked_session_id, "prompt")
        );
        assert!(matches!(
            second_client_responses
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            ServerMessage::Response { request_id, .. } if request_id == prompt_id
        ));

        let blocked_close_id = Uuid::new_v4();
        dispatcher.dispatch(
            Request {
                request_id: blocked_close_id,
                session_id: blocked_session_id,
                runtime_id: blocked_runtime_id,
                command: Command::CloseSession,
            },
            other_outgoing.clone(),
            0,
            None,
        );
        let other_close_id = Uuid::new_v4();
        dispatcher.dispatch(
            Request {
                request_id: other_close_id,
                session_id: other_session_id,
                runtime_id: other_runtime_id,
                command: Command::CloseSession,
            },
            other_outgoing,
            0,
            None,
        );
        let mut close_responses = [false; 2];
        for _ in 0..2 {
            let ServerMessage::Response { request_id, .. } = other_responses
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
            else {
                panic!("expected a close response");
            };
            if request_id == blocked_close_id {
                close_responses[0] = true;
            } else if request_id == other_close_id {
                close_responses[1] = true;
            }
        }
        assert_eq!(close_responses, [true, true]);
    }

    fn test_start_options() -> WireDriverStartOptions {
        WireDriverStartOptions {
            provider: "codex".into(),
            binary: PathBuf::from("codex"),
            cwd: PathBuf::from("."),
            mode: "fullAccess".into(),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_window: None,
            agent_preset: None,
            computer_use_enabled: false,
            read_own_transcript: false,
            provider_cursor: None,
        }
    }

    #[derive(Default)]
    struct WebhookBackend {
        calls: Mutex<Vec<(Uuid, String)>>,
    }

    impl Backend for WebhookBackend {
        fn handle(
            &self,
            _request: Request,
            _events: EventSink,
            _agent: Option<Uuid>,
        ) -> anyhow::Result<ResponsePayload> {
            Ok(ResponsePayload::Ack)
        }

        fn trigger_automation_webhook(
            &self,
            automation_id: Uuid,
            key: &str,
        ) -> anyhow::Result<Option<waku_protocol::automations::AutomationRun>> {
            self.calls.lock().push((automation_id, key.to_owned()));
            if key != "secret-key" {
                return Ok(None);
            }
            let now = waku_protocol::model::unix_time();
            Ok(Some(waku_protocol::automations::AutomationRun {
                id: Uuid::new_v4(),
                automation_id,
                trigger: waku_protocol::automations::AutomationTrigger::Webhook,
                status: waku_protocol::automations::AutomationRunStatus::Pending,
                scheduled_for: now,
                started_at: None,
                finished_at: None,
                session_id: None,
                error: None,
                precheck: None,
                refusal_key: None,
                refusal_count: 0,
                created_at: now,
                updated_at: now,
            }))
        }
    }

    #[test]
    fn automation_webhook_answers_plain_http_posts() {
        use std::io::Read as _;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let backend = Arc::new(WebhookBackend::default());
        let server_backend = backend.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                server_backend,
                server_shutdown,
                ServerOptions::default(),
            )
            .unwrap()
        });

        let automation_id = Uuid::new_v4();
        let post = |target: &str| -> String {
            let mut stream = TcpStream::connect(address).unwrap();
            write!(
                stream,
                "POST {target} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n"
            )
            .unwrap();
            stream.flush().unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };

        let response = post(&format!(
            "/automations/{automation_id}/trigger?key=secret-key"
        ));
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(
            backend.calls.lock().as_slice(),
            &[(automation_id, "secret-key".to_owned())]
        );

        let response = post(&format!("/automations/{automation_id}/trigger?key=wrong"));
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
        let response = post("/automations/not-a-uuid/trigger?key=secret-key");
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");

        // A non-POST request on the route is a 405; anything else — like the
        // WebSocket client below — still falls through to the handshake.
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "GET /automations/{automation_id}/trigger?key=secret-key HTTP/1.1\r\nHost: localhost\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 405"), "{response}");

        DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        shutdown.store(true, Ordering::Release);
        server.join().unwrap();
    }

    #[test]
    fn runtime_exposure_opens_and_closes_a_second_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(TestBackend::default()),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });
        let client = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();
        let expose = |exposure: Option<DaemonExposure>| {
            client
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    Command::SetDaemonExposure { exposure },
                )
                .unwrap()
        };

        let port = match expose(Some(DaemonExposure {
            port: 0,
            allowed_origins: Vec::new(),
            token: "remote".into(),
        })) {
            ResponsePayload::Exposure { port: Some(port) } => port,
            other => panic!("unexpected exposure response: {other:?}"),
        };

        // The exposed listener authenticates with its own token…
        let remote = DaemonClient::connect(&format!("127.0.0.1:{port}"), "remote".into()).unwrap();
        assert!(matches!(
            remote
                .request(Uuid::nil(), Uuid::nil(), Command::GetSettings)
                .unwrap(),
            ResponsePayload::Settings { .. }
        ));
        // …and rejects the loopback token.
        assert!(DaemonClient::connect(&format!("127.0.0.1:{port}"), "secret".into()).is_err());

        // Reapplying the same config is a no-op — the listener survives.
        assert!(matches!(
            expose(Some(DaemonExposure {
                port: 0,
                allowed_origins: Vec::new(),
                token: "remote".into(),
            })),
            ResponsePayload::Exposure {
                port: Some(same)
            } if same == port
        ));
        remote
            .request(Uuid::nil(), Uuid::nil(), Command::GetSettings)
            .unwrap();

        // Unexposing closes the listener; the loopback side never noticed.
        assert!(matches!(
            expose(None),
            ResponsePayload::Exposure { port: None }
        ));
        assert!(DaemonClient::connect(&format!("127.0.0.1:{port}"), "remote".into()).is_err());
        client
            .request(Uuid::nil(), Uuid::nil(), Command::GetSettings)
            .unwrap();

        drop(remote);
        client.shutdown();
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn paired_and_agent_tokens_cannot_change_exposure() {
        let root = std::env::temp_dir().join(format!("waku-exposure-gate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let backend = WakuBackend::new(
            DaemonSettingsStore::open(root.join("settings.json")).unwrap(),
            StateStore::daemon(root.join("app.db")),
        )
        .unwrap();
        let agent_token = backend.agent.mint(Uuid::new_v4());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = shutdown.clone();
        let server = std::thread::spawn(move || {
            serve(
                listener,
                "secret".into(),
                Arc::new(backend),
                server_shutdown,
                ServerOptions {
                    allow_shutdown: true,
                    ..ServerOptions::default()
                },
            )
            .unwrap()
        });
        let owner = DaemonClient::connect(&address.to_string(), "secret".into()).unwrap();

        // A device pairs through the owner-approved flow, then connects
        // with its minted token.
        let pairing = std::thread::spawn({
            let address = address.to_string();
            move || waku_client::pair(&address, "test-device", Duration::from_secs(10))
        });
        let request_id = {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                let found = match owner
                    .request(Uuid::nil(), Uuid::nil(), Command::GetPairing)
                    .unwrap()
                {
                    ResponsePayload::Pairing { state } => {
                        state.pending.first().map(|pending| pending.request_id)
                    }
                    _ => None,
                };
                if let Some(request_id) = found {
                    break request_id;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "pair request never arrived"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        owner
            .request(
                Uuid::nil(),
                Uuid::nil(),
                Command::RespondPairRequest {
                    request_id,
                    accept: true,
                },
            )
            .unwrap();
        let waku_client::PairReply::Granted { token, .. } = pairing.join().unwrap().unwrap() else {
            panic!("pair request was not granted");
        };

        let paired = DaemonClient::connect(&address.to_string(), token).unwrap();
        let agent = DaemonClient::connect(&address.to_string(), agent_token).unwrap();
        for (name, client) in [("paired", &paired), ("agent", &agent)] {
            let error = client
                .request(
                    Uuid::nil(),
                    Uuid::nil(),
                    Command::SetDaemonExposure { exposure: None },
                )
                .unwrap_err();
            assert!(
                error.to_string().contains("primary authentication token")
                    || error.to_string().contains("agent credential"),
                "{name}: {error}"
            );
        }

        owner.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
