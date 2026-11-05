//! Bounded transcript views: compact entries, paged windows, lazy payloads,
//! and daemon-side find.
//!
//! These types let a client open a task on the daemon's *flattened* transcript
//! index — one ordered row per message or activity — without hydrating the
//! whole session. [`crate::model::AgentSession`] remains the authoritative
//! record; a [`TranscriptWindow`] is a presentation slice keyed by absolute
//! positions (`seq`, `after_message`) so it can never be mistaken for, or
//! saved back as, a complete transcript.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{ActivityItem, AgentTurn, Message, QueuedMessage, RuntimeEventCursor};

fn default_true() -> bool {
    true
}

/// Entries the daemon's default "latest" window read returns. Sized so the
/// most recent user turn plus its work almost always fits.
pub const WINDOW_DEFAULT_ENTRIES: u32 = 200;
/// Encoded-size ceiling for a default/latest window.
pub const WINDOW_DEFAULT_BYTES: u32 = 256 * 1024;
/// History-page bound: the first of entries or bytes stops the page.
pub const PAGE_MAX_ENTRIES: u32 = 500;
pub const PAGE_MAX_BYTES: u32 = 512 * 1024;
/// A single payload read never returns more than this.
pub const PAYLOAD_CHUNK_BYTES: u64 = 64 * 1024;
/// Message bodies inside a window are inline up to this many UTF-8 bytes;
/// beyond it the client range-reads the remainder as a payload.
pub const MESSAGE_INLINE_BYTES: usize = 32 * 1024;
/// Find scans are bounded per call — bytes of stored text examined.
pub const FIND_SCAN_BYTES: u64 = 256 * 1024;
/// Find returns at most this many occurrences per call.
pub const FIND_MAX_MATCHES: u32 = 200;

/// A stable position in the flattened entry stream. Validation compares the
/// stored `entry_id` at `seq`: a structural rewrite (rewind, import, splice)
/// shifts entries so the pair no longer matches and the holder resnapshots —
/// ordinary appends never disturb an existing pair.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCursor {
    pub seq: i64,
    pub entry_id: Uuid,
}

/// Which slice of the flattened transcript to read.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TranscriptWindowTarget {
    /// The latest user turn plus its work, bounded — the task-open shape.
    Latest,
    /// The page ending just before `cursor`, for scroll-up.
    Before { cursor: TranscriptCursor },
    /// The page starting just after `cursor`, for gap/refresh fills.
    After { cursor: TranscriptCursor },
    /// A window centered on one entry — search reveal and deep links.
    Around { entry_id: Uuid },
    /// Entries owned by one turn, paged forward from `after` (exclusive) or
    /// from the turn's first entry — expanding a folded work group stays
    /// inside that turn through the same paging contract.
    Turn {
        turn_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<i64>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptWindowRequest {
    pub session_id: Uuid,
    pub target: TranscriptWindowTarget,
    /// Client-requested ceiling; the daemon applies its own caps on top.
    #[serde(default)]
    pub max_entries: Option<u32>,
    #[serde(default)]
    pub max_bytes: Option<u32>,
    /// The caller's cached head revision. When it still matches, the window
    /// returns `unchanged` with no entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_revision: Option<u64>,
}

/// A transcript field that can hold bulk text — the unit lazy loading and
/// find address. `Content`/`DisplayContent`/`Notice`/`Attachments`/`Atoms`
/// belong to message entries, the rest to activity entries.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptPayloadField {
    Output,
    Arguments,
    Detail,
    Reasoning,
    FileChanges,
    ImageUrls,
    Content,
    DisplayContent,
    Attachments,
    Atoms,
    Notice,
}

/// Shape metadata for one payload field: enough to render a placeholder and
/// to issue a range read. `revision` changes when the stored bytes change.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PayloadDescriptor {
    pub field: TranscriptPayloadField,
    pub revision: u64,
    /// Total stored UTF-8 length in bytes.
    pub bytes: u64,
    /// The stored value was already clipped at a source-side cap
    /// (`ActivityItem::output_truncated`); a range read cannot recover more.
    #[serde(default, skip_serializing_if = "crate::model::is_false")]
    pub truncated: bool,
    /// `false` when the payload is gone (detail pruning, retention) — the
    /// field stays a visible placeholder, and find must not reveal it.
    #[serde(default = "default_true")]
    pub available: bool,
}

/// The byte range a window actually carried for a field that exceeded its
/// inline cap — the explicit continuation the plan requires instead of
/// silent clipping.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PayloadWindow {
    pub offset: u64,
    pub len: u64,
    pub total: u64,
}

/// One flattened transcript row.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    /// Absolute position in the flattened stream.
    pub seq: i64,
    /// Revision this row last changed at — equals the head revision of the
    /// write that produced it.
    pub revision: u64,
    pub turn_id: Option<Uuid>,
    /// Absolute persisted-message anchor. For a message this is its index;
    /// for an activity it is the owning block's anchor. Never a
    /// window-local position — renderers subtract `message_base`.
    pub after_message: u32,
    /// The owning `transcript_blocks` index for activity entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_index: Option<u32>,
    /// The activity's index inside its block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_index: Option<u32>,
    pub data: TranscriptEntryData,
}

impl TranscriptEntry {
    /// The stable row identity — the message or activity id.
    pub fn entry_id(&self) -> Uuid {
        match &self.data {
            TranscriptEntryData::Message { message, .. } => message.id,
            TranscriptEntryData::Activity { activity, .. } => activity.id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TranscriptEntryData {
    /// `message.content`/`display_content` hold the bytes `window` describes
    /// — the head range when inline-capped, the complete text otherwise.
    Message {
        message: Box<Message>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_window: Option<PayloadWindow>,
        /// Ranged fields: content, display_content, attachments, atoms,
        /// notice — present even when a window already carried them so the
        /// client can range-read the rest.
        payloads: Vec<PayloadDescriptor>,
    },
    /// `activity` carries only compact metadata; every bulk field was
    /// stripped and described in `payloads`.
    Activity {
        activity: Box<ActivityItem>,
        payloads: Vec<PayloadDescriptor>,
    },
}

/// A bounded slice of the flattened transcript plus the anchors a view needs
/// to merge it into what it already holds.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptWindow {
    pub session_id: Uuid,
    /// Head content revision at read time — cache validation and the
    /// `known_revision` fast path compare against it.
    pub revision: u64,
    /// No changes since `known_revision`; `entries` is empty.
    #[serde(default, skip_serializing_if = "crate::model::is_false")]
    pub unchanged: bool,
    /// The cursor in `target` no longer validates — a structural rewrite
    /// moved its position. `entries` is empty; the caller resnapshots with
    /// `latest` (or its last known-good cursor) rather than splicing this
    /// response into the view.
    #[serde(default, skip_serializing_if = "crate::model::is_false")]
    pub invalid_cursor: bool,
    pub entries: Vec<TranscriptEntry>,
    /// Cursor for the entry just outside each edge — `previous` continues
    /// upward (`Before`), `next` continues downward (`After`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<TranscriptCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<TranscriptCursor>,
    #[serde(default)]
    pub has_more_before: bool,
    #[serde(default)]
    pub has_more_after: bool,
    /// Absolute index of the first message the window materializes —
    /// everything in `entries` anchors relative to this.
    #[serde(default)]
    pub message_base: u32,
    /// Total messages in the authoritative transcript.
    pub message_count: u32,
    /// Total entries in the authoritative transcript.
    pub entry_count: i64,
    /// Normalized rows exist; `false` means every read re-parses the legacy
    /// detail blob (`legacy_read` reports when this one did).
    #[serde(default)]
    pub converted: bool,
    #[serde(default)]
    pub legacy_read: bool,
    /// Turns owning entries in the window, plus the latest turn — row
    /// builders read status/timing from these.
    #[serde(default)]
    pub turns: Vec<AgentTurn>,
    /// Live control surface: queued submissions the view renders above the
    /// composer without hydrating the session.
    #[serde(default)]
    pub queued_messages: Vec<QueuedMessage>,
    /// Runtime replay position at snapshot time; the client applies live
    /// events strictly after it so snapshot and stream meet at one boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_cursor: Option<RuntimeEventCursor>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPayloadRequest {
    pub session_id: Uuid,
    pub entry_id: Uuid,
    pub field: TranscriptPayloadField,
    /// Optimistic check: `stale` when the stored field moved past this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    /// Byte offset into the stored UTF-8 text.
    #[serde(default)]
    pub offset: u64,
    /// Requested byte count; the daemon caps it at `PAYLOAD_CHUNK_BYTES`.
    #[serde(default)]
    pub limit: u64,
}

/// One byte-range read of a payload field.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPayload {
    pub entry_id: Uuid,
    pub field: TranscriptPayloadField,
    /// Live revision of the stored bytes — compare against
    /// `expected_revision`; on mismatch `stale` is set and the returned
    /// range is still valid for the newer bytes.
    pub revision: u64,
    #[serde(default, skip_serializing_if = "crate::model::is_false")]
    pub stale: bool,
    pub total_bytes: u64,
    /// The returned slice: `content` covers `[offset, offset+len)`, and is
    /// always complete UTF-8 — the server widens to code-point boundaries.
    pub offset: u64,
    pub content: String,
    /// `offset + content.len() == total_bytes`.
    #[serde(default)]
    pub complete: bool,
    /// Why nothing could be returned: pruning, retention, or a field that
    /// never existed. `None` on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

/// Which stored surfaces `findTranscript` scans. Absent means the default
/// visible corpus: conversation text plus tool output.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptFindScope {
    /// Message bodies (`content`).
    Conversation,
    /// Activity `output` — the long tool-result text.
    Outputs,
    /// Activity `arguments`.
    Arguments,
    /// Activity reasoning content.
    Reasoning,
    /// Activity `detail`, title, and display target/description.
    Details,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptFindRequest {
    pub session_id: Uuid,
    /// Literal substring — the same case-insensitive ASCII folding the
    /// existing message search applies. Punctuation, short needles, and
    /// multiline text all match as written.
    pub query: String,
    /// `None`/empty → `Conversation` + `Outputs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<TranscriptFindScope>,
    /// Resume a progressive scan — pass the previous response's cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<TranscriptFindCursor>,
    /// Only report occurrences at or after this entry position — a "find
    /// next" anchor; earlier occurrences are counted but not returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<TranscriptCursor>,
    /// Revision the caller's match list was computed against — results
    /// report `revision` so a stale list can be re-resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_revision: Option<u64>,
}

/// Where a bounded scan stopped so the next call continues exactly there —
/// the *next* (entry, field, byte) position to examine, not the last done.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptFindCursor {
    /// Entry position the scan resumes at — `u64::MAX` marks "start".
    pub seq: i64,
    /// Field inside that entry to resume at; `None` resumes at the first
    /// in-scope field of the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<TranscriptPayloadField>,
    /// Byte offset into that field — matches may continue inside one large
    /// payload.
    #[serde(default)]
    pub field_offset: u64,
}

/// One occurrence, byte-exact so a range read can fetch just the match's
/// neighborhood.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptFindMatch {
    pub entry_id: Uuid,
    pub seq: i64,
    pub field: TranscriptPayloadField,
    /// Byte offset of the match inside the stored field text.
    pub offset: u64,
    /// Match length in bytes — equals the needle length for literal search.
    pub len: u64,
    /// Context around the match, trimmed to code-point boundaries.
    pub snippet: String,
    /// Byte offset of the match inside `snippet`.
    pub match_offset_in_snippet: u64,
    /// Head revision the match was found under.
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptFindResult {
    pub session_id: Uuid,
    /// Head revision scanned — matches found under an older revision than
    /// the caller's view must be re-resolved before reveal.
    pub revision: u64,
    pub matches: Vec<TranscriptFindMatch>,
    /// Occurrences in this result batch — the caller accumulates; a full
    /// total exists only once `complete` is true.
    pub count: u64,
    /// `true` once the whole retained transcript was scanned — until then
    /// `count` is a lower bound and `cursor` resumes the scan.
    #[serde(default)]
    pub complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<TranscriptFindCursor>,
    /// `false` while background conversion is still populating searchable
    /// fields — a `complete` scan reported `0` cannot mean "absent" yet.
    #[serde(default = "default_true")]
    pub index_complete: bool,
}

/// Per-connection interest in lazy payloads, sent with
/// [`crate::protocol::Command::SetTaskView`]. Default (`None` on the wire)
/// preserves legacy delivery: every detail event ships complete.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TaskViewPayloadInterest {
    /// Ship activity payloads inline as before.
    #[default]
    All,
    /// Strip bulk fields from detail events; the client range-reads the
    /// fields it renders expanded.
    Compact,
}
