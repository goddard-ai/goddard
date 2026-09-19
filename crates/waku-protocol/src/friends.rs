//! Wire types for friend-to-friend file sharing. Friends live on the
//! daemon (the `waku-share` iroh endpoint), so these are plain data — node
//! ids and tickets cross the wire as strings and are parsed daemon-side.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::git::SyncInProgress;

/// Whole friends document, carried wholesale on every change — it's small
/// and every client applies it, same convention as `SettingsChanged`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FriendsState {
    /// This install's shareable code (`gfr-<endpoint id>`).
    pub friend_code: String,
    /// The name friends see on our requests and offers.
    pub display_name: String,
    pub friends: Vec<FriendInfo>,
    /// Friend requests awaiting a local decision.
    pub incoming_requests: Vec<FriendRequestInfo>,
    /// Requests we've sent that haven't been answered.
    pub outgoing_requests: Vec<FriendRequestInfo>,
    pub transfers: Vec<TransferInfo>,
    /// Repos we share with friends. `[]` matches older daemons.
    #[serde(default)]
    pub shared_projects: Vec<SharedProjectInfo>,
    /// Repos friends share with us, with daemon-side matching against
    /// our projects already resolved.
    #[serde(default)]
    pub incoming_shares: Vec<IncomingShareInfo>,
    /// Sync relationships we opted into — one per (friend, origin).
    #[serde(default)]
    pub sync_links: Vec<SyncLinkInfo>,
    /// Sync decisions waiting on the user (conflicts, refused syncs).
    #[serde(default)]
    pub sync_alerts: Vec<SyncAlertInfo>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FriendInfo {
    /// `gfr-…` / endpoint id string.
    pub node_id: String,
    /// The friend's self-reported display name.
    pub name: String,
    /// Local-only override — render this instead of `name` when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
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
    /// The agent session the daemon materialized for a completed incoming
    /// transfer — clients reopen it from transfer history and badge it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    Outgoing,
    Incoming,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    /// Offer sent / received, fetch not yet flowing.
    Pending,
    Transferring,
    Done,
    Failed,
    Cancelled,
}

/// A repo we share with a friend.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SharedProjectInfo {
    /// Friend's endpoint id string.
    pub peer_id: String,
    pub peer_name: String,
    /// Our project name — display only.
    pub project_name: String,
    /// The repo's `origin` fetch URL as we advertised it.
    pub origin_url: String,
    /// Our local checkout — the sync loop works here.
    #[ts(type = "string")]
    pub repo_path: PathBuf,
    /// The friend enabled sync on this share — the link is mutual.
    pub peer_sync_enabled: bool,
    pub shared_at_ms: u64,
}

/// A repo a friend shares with us.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IncomingShareInfo {
    pub peer_id: String,
    pub peer_name: String,
    /// Their project name — display only.
    pub project_name: String,
    pub origin_url: String,
    /// Our project with the same origin remote — `None` means we don't
    /// have this repo, so there's nothing to sync.
    #[ts(type = "string | null")]
    pub matched_path: Option<PathBuf>,
    pub matched_project_name: Option<String>,
    /// We already opted in — the `sync_links` row is authoritative.
    pub sync_enabled: bool,
    pub received_at_ms: u64,
}

/// A sync relationship we opted into — branches are synced when the
/// friend pushes, and our commits auto-push unless `auto_push` is off.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SyncLinkInfo {
    pub id: String,
    pub peer_id: String,
    pub peer_name: String,
    pub origin_url: String,
    #[ts(type = "string")]
    pub repo_path: PathBuf,
    /// Commits landing on enabled branches push to origin automatically.
    pub auto_push: bool,
    /// Branches we integrate when the peer pushes.
    pub enabled_branches: Vec<String>,
    /// Enabled branches paused by an abort until manually synced.
    pub paused_branches: Vec<String>,
    /// The peer's side of the link is up — they see our push notices.
    pub peer_sync_enabled: bool,
    pub created_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SyncAlertKind {
    /// The integration stopped on conflicts; the user picks
    /// resolve-in-chat / merge-instead / abort.
    Conflict,
    /// Sync refused to touch a dirty checkout — retry once it's clean.
    RefusedDirtyWorktree,
}

/// A sync decision waiting on the user. Conflicts carry the stopped
/// integration and the worktree that owns it — `repo_path` for the
/// checked-out branch, a temp worktree for one checked out nowhere.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SyncAlertInfo {
    pub id: String,
    pub link_id: String,
    pub peer_name: String,
    pub branch: String,
    /// The project the link syncs — display context for the alert.
    #[ts(type = "string")]
    pub repo_path: PathBuf,
    pub kind: SyncAlertKind,
    /// Which integration is stopped — `rebase`/`merge`; `None` for
    /// refusals, which own no git state.
    pub in_progress: Option<SyncInProgress>,
    /// Working-tree paths still carrying conflict markers.
    pub files: Vec<String>,
    /// Where the conflict lives — point resolve-in-chat here.
    #[ts(type = "string")]
    pub worktree_path: PathBuf,
    /// The worktree is a daemon-managed temp — cleaned up on resolution.
    pub temp_worktree: bool,
    pub at_ms: u64,
}

/// What the user chose on a sync alert. Resolve-in-chat is handled
/// client-side (it spawns a task); these go to the daemon.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FriendSyncAlertAction {
    /// Abort the stopped rebase and retry as a merge.
    MergeInstead,
    /// Abort the integration and pause the branch until a manual sync.
    Abort,
    /// Close the alert without touching git state — refused syncs.
    Dismiss,
}
