//! Stable daemon-host Git ref names; constructing a ref performs no I/O.

use uuid::Uuid;

pub fn checkpoint_ref(session_id: Uuid, turn_count: usize) -> String {
    format!("refs/waku/session-{session_id}-turn-{turn_count}")
}

pub fn turn_start_ref(session_id: Uuid, turn_count: usize) -> String {
    format!("refs/waku/session-{session_id}-turn-start-{turn_count}")
}

pub fn turn_diff_base_ref(session_id: Uuid, turn_count: usize) -> String {
    format!("refs/waku/session-{session_id}-turn-diff-{turn_count}")
}

/// Snapshot of the whole worktree taken when a session is archived, so the
/// worktree can be removed without losing work. Living under the shared
/// `refs/waku/session-<id>-` prefix means session ref deletion — including
/// the archived-session retention purge — collects it with the rest.
pub fn archive_ref(session_id: Uuid) -> String {
    format!("refs/waku/session-{session_id}-archive")
}
