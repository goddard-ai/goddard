//! Callbacks installed by the transport for daemon services.
use std::sync::Arc;
use uuid::Uuid;
use waku_protocol::automations::AutomationsState;
use waku_protocol::friends::FriendsState;
use waku_protocol::pairing::PairingState;
use waku_protocol::{ReplayCursor, SequencedEvent};

/// Installed by the server so async share events (incoming request, offer,
/// progress) reach every subscribed client.
pub type FriendsSink = Arc<dyn Fn(FriendsState) + Send + Sync>;

/// Fired when the share layer changed session/project state — the hub
/// translates it into a `TaskStateChanged` bump for every client.
pub type TaskNotifier = Arc<dyn Fn() + Send + Sync>;

/// Fired when `refs/notes/qa` (or a promoted base branch) moved for an
/// origin — locally or on a friend's machine. The hub broadcasts a
/// `ReviewChanged` with the origin URL so review surfaces re-read.
pub type ReviewNotifier = Arc<dyn Fn(String) + Send + Sync>;

/// A hub-provided live event stream for one session — what the share
/// layer pumps onto a friend's `SessionSubscribe` stream.
pub type SessionStreamer =
    Arc<dyn Fn(Uuid, Option<ReplayCursor>) -> crate::SessionStream + Send + Sync>;

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

/// Broadcast channel the server installs: the whole automations document on
/// every change.
pub type AutomationsSink = Arc<dyn Fn(AutomationsState) + Send + Sync>;

/// Installed by the server so `PairingChanged` reaches every subscriber.
pub type PairingSink = Arc<dyn Fn(PairingState) + Send + Sync>;
