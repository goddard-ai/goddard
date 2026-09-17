//! Wire types for friend-to-friend file sharing. Friends live on the
//! daemon (the `waku-share` iroh endpoint), so these are plain data — node
//! ids and tickets cross the wire as strings and are parsed daemon-side.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Whole friends document, carried wholesale on every change — it's small
/// and every client applies it, same convention as `SettingsChanged`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FriendsState {
    /// This install's shareable code (`gfr-<endpoint id>`).
    pub friend_code: String,
    pub friends: Vec<FriendInfo>,
    /// Friend requests awaiting a local decision.
    pub incoming_requests: Vec<FriendRequestInfo>,
    /// Requests we've sent that haven't been answered.
    pub outgoing_requests: Vec<FriendRequestInfo>,
    pub transfers: Vec<TransferInfo>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FriendInfo {
    /// `gfr-…` / endpoint id string.
    pub node_id: String,
    pub name: String,
    /// Result of the most recent on-demand probe — presence is lazy, so this
    /// is only as fresh as the last dial; `None` means never seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<u64>,
    /// The daemon's latest probe verdict; may be stale — Send still dials.
    pub online: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FriendRequestInfo {
    pub node_id: String,
    /// Self-reported display name from the requester.
    pub name: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TransferInfo {
    pub id: Uuid,
    pub direction: TransferDirection,
    /// Friend's endpoint id string.
    pub peer_id: String,
    pub peer_name: String,
    /// Display title — file or folder name.
    pub title: String,
    /// Optional note the sender attached to the offer.
    pub note: Option<String>,
    pub status: TransferStatus,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Where received files landed (incoming transfers only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    Outgoing,
    Incoming,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    /// Offer sent / received, fetch not yet flowing.
    Pending,
    Transferring,
    Done,
    Failed,
    Cancelled,
}
