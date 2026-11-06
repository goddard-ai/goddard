//! Normalized transcript index: flattened entries, lazy payloads, turn rows,
//! and a per-session head — the storage half of bounded transcript views.
//!
//! `session_details.data` stays the authoritative record; these tables are a
//! derived, index-addressable projection written inside the same save
//! transaction, so the index can never disagree with the detail it flattens.
//! A missing `transcript_heads` row means "not converted yet" — readers fall
//! back to a one-time legacy parse through [`StateStore::convert_transcript`],
//! which leaves the same tables it would have maintained.

use std::collections::{HashMap, HashSet};
use std::io;

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;
use waku_protocol::model::{ActivityItem, AgentSession, AgentTurn, Message};
use waku_protocol::transcript::{
    FIND_MAX_MATCHES, FIND_SCAN_BYTES, MESSAGE_INLINE_BYTES, PAYLOAD_CHUNK_BYTES,
    PayloadDescriptor, PayloadWindow, TranscriptCursor, TranscriptEntry, TranscriptEntryData,
    TranscriptFindCursor, TranscriptFindMatch, TranscriptFindResult, TranscriptFindScope,
    TranscriptPayload, TranscriptPayloadField, TranscriptWindow, TranscriptWindowTarget,
};

use crate::persistence::{
    MessageColumns, fingerprint, message_fingerprint, message_from_row, to_io_error,
};

const ENTRY_KIND_MESSAGE: &str = "message";
const ENTRY_KIND_ACTIVITY: &str = "activity";

/// `transcript_payloads.field` values for the activity fields that moved out
/// of `compact`. Kept as literals: they are a storage contract, not a wire
/// enum — [`payload_field_name`] maps both ways.
fn payload_field_name(field: TranscriptPayloadField) -> &'static str {
    match field {
        TranscriptPayloadField::Output => "output",
        TranscriptPayloadField::Arguments => "arguments",
        TranscriptPayloadField::Detail => "detail",
        TranscriptPayloadField::Reasoning => "reasoning",
        TranscriptPayloadField::FileChanges => "file_changes",
        TranscriptPayloadField::ImageUrls => "image_urls",
        TranscriptPayloadField::Content => "content",
        TranscriptPayloadField::DisplayContent => "display_content",
        TranscriptPayloadField::Attachments => "attachments",
        TranscriptPayloadField::Atoms => "atoms",
        TranscriptPayloadField::Notice => "notice",
    }
}

/// Activity-only fields that live in `transcript_payloads` — message fields
/// stay in `messages` columns and are range-read from there.
const ACTIVITY_PAYLOAD_FIELDS: [TranscriptPayloadField; 6] = [
    TranscriptPayloadField::Output,
    TranscriptPayloadField::Arguments,
    TranscriptPayloadField::Detail,
    TranscriptPayloadField::Reasoning,
    TranscriptPayloadField::FileChanges,
    TranscriptPayloadField::ImageUrls,
];

/// FNV-style folding in [`message_fingerprint`]'s shape so every write path
/// hashes content the same way.
struct Fold(u64);

impl Fold {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn value(&mut self, value: u64) {
        self.0 ^= value;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3).rotate_left(23);
    }

    fn text(&mut self, value: &str) {
        self.value(fingerprint(value));
    }

    fn opt_uuid(&mut self, value: Option<Uuid>) {
        match value {
            Some(id) => {
                let (high, low) = id.as_u64_pair();
                self.value(high);
                self.value(low);
            }
            None => self.value(u64::MAX),
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EntryKind {
    Message,
    Activity,
}

impl EntryKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Message => ENTRY_KIND_MESSAGE,
            Self::Activity => ENTRY_KIND_ACTIVITY,
        }
    }
}

struct FlatPayload {
    field: TranscriptPayloadField,
    fingerprint: u64,
    bytes: u64,
    truncated: bool,
    content: Vec<u8>,
}

struct FlatEntry {
    entry_id: Uuid,
    kind: EntryKind,
    turn_id: Option<Uuid>,
    after_message: u32,
    block_index: Option<u32>,
    activity_index: Option<u32>,
    fingerprint: u64,
    /// Stripped `ActivityItem` JSON for activities; `None` for messages —
    /// their columns carry everything.
    compact: Option<String>,
    payloads: Vec<FlatPayload>,
}

/// The JSON encoding one activity payload field gets in `transcript_payloads`.
/// Structured fields serialize their model value so a range read can still
/// return lossless data; text fields store the text verbatim.
fn activity_payload(
    activity: &ActivityItem,
    field: TranscriptPayloadField,
) -> Option<(Vec<u8>, bool)> {
    match field {
        TranscriptPayloadField::Output => activity
            .output
            .as_ref()
            .map(|output| (output.clone().into_bytes(), activity.output_truncated)),
        TranscriptPayloadField::Arguments => activity
            .arguments
            .as_ref()
            .map(|arguments| (arguments.clone().into_bytes(), activity.output_truncated)),
        TranscriptPayloadField::Detail => activity
            .detail
            .as_ref()
            .map(|detail| (detail.clone().into_bytes(), activity.output_truncated)),
        TranscriptPayloadField::Reasoning => activity
            .reasoning
            .as_ref()
            .and_then(|reasoning| serde_json::to_vec(reasoning).ok())
            .map(|json| (json, false)),
        TranscriptPayloadField::FileChanges => (!activity.file_changes.is_empty())
            .then(|| serde_json::to_vec(&activity.file_changes).ok())
            .flatten()
            .map(|json| (json, false)),
        TranscriptPayloadField::ImageUrls => (!activity.image_urls.is_empty())
            .then(|| serde_json::to_vec(&activity.image_urls).ok())
            .flatten()
            .map(|json| (json, false)),
        _ => None,
    }
}

/// The `ActivityItem` shape stored in `transcript_entries.compact`: every
/// bulk field stripped, every display field kept, so the row alone renders
/// the activity line.
fn compact_activity(activity: &ActivityItem) -> ActivityItem {
    let mut compact = activity.clone();
    compact.detail = None;
    compact.arguments = None;
    compact.output = None;
    compact.output_truncated = false;
    compact.image_urls.clear();
    compact.file_changes.clear();
    compact.reasoning = None;
    compact
}

/// Ordering helper: flatten `session` into reading-order entries. Blocks
/// render after `after_message` persisted messages, so the interleave is
/// "activities anchored at K, then message K" — one `seq` per row, shared by
/// the writer and nothing else (reads page by `seq` directly).
fn flatten_transcript(session: &AgentSession) -> Vec<FlatEntry> {
    let mut blocks_at: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, block) in session.transcript_blocks.iter().enumerate() {
        blocks_at
            .entry(block.after_message)
            .or_default()
            .push(index);
    }
    let message_count = session.messages.len();
    let mut entries = Vec::with_capacity(message_count + session.transcript_blocks.len());
    for anchor in 0..=message_count {
        if let Some(indices) = blocks_at.get(&anchor) {
            for &block_index in indices {
                let block = &session.transcript_blocks[block_index];
                for (activity_index, activity) in block.activities.iter().enumerate() {
                    let compact = compact_activity(activity);
                    let compact_json =
                        serde_json::to_string(&compact).unwrap_or_else(|_| "{}".into());
                    let payloads: Vec<FlatPayload> = ACTIVITY_PAYLOAD_FIELDS
                        .iter()
                        .filter_map(|field| {
                            activity_payload(activity, *field).map(|(content, truncated)| {
                                FlatPayload {
                                    field: *field,
                                    fingerprint: fingerprint(&String::from_utf8_lossy(&content)),
                                    bytes: content.len() as u64,
                                    truncated,
                                    content,
                                }
                            })
                        })
                        .collect();
                    let mut fold = Fold::new();
                    fold.value(2);
                    fold.opt_uuid(activity.id.into());
                    fold.opt_uuid(block.turn_id);
                    fold.value(anchor as u64);
                    fold.value(block_index as u64);
                    fold.value(activity_index as u64);
                    fold.text(&compact_json);
                    for payload in &payloads {
                        fold.value(payload.fingerprint);
                    }
                    entries.push(FlatEntry {
                        entry_id: activity.id,
                        kind: EntryKind::Activity,
                        turn_id: block.turn_id,
                        after_message: anchor as u32,
                        block_index: Some(block_index as u32),
                        activity_index: Some(activity_index as u32),
                        fingerprint: fold.finish(),
                        compact: Some(compact_json),
                        payloads,
                    });
                }
            }
        }
        if let Some(message) = session.messages.get(anchor) {
            let mut fold = Fold::new();
            fold.value(1);
            fold.value(message_fingerprint(message, anchor));
            entries.push(FlatEntry {
                entry_id: message.id,
                kind: EntryKind::Message,
                turn_id: message.turn_id,
                after_message: anchor as u32,
                block_index: None,
                activity_index: None,
                fingerprint: fold.finish(),
                compact: None,
                payloads: Vec::new(),
            });
        }
    }
    entries
}

struct TranscriptHead {
    revision: u64,
    entry_count: i64,
    message_count: u32,
    queued_messages: String,
    details_pruned: bool,
}

/// Cheap converted check — the head row is the index's presence marker.
pub(crate) fn has_head(connection: &Connection, session_id: &str) -> io::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM transcript_heads WHERE session_id = ?1",
            params![session_id],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(to_io_error)
}

fn read_head(connection: &Connection, session_id: &str) -> io::Result<Option<TranscriptHead>> {
    connection
        .query_row(
            "SELECT revision, entry_count, message_count, queued_messages, details_pruned
             FROM transcript_heads WHERE session_id = ?1",
            params![session_id],
            |row| {
                Ok(TranscriptHead {
                    revision: row.get::<_, i64>(0)? as u64,
                    entry_count: row.get(1)?,
                    message_count: row.get::<_, i64>(2)? as u32,
                    queued_messages: row.get(3)?,
                    details_pruned: row.get::<_, i64>(4)? != 0,
                })
            },
        )
        .optional()
        .map_err(to_io_error)
}

/// Rebuild `session`'s normalized index inside `transaction`. Returns the
/// head revision the index now sits at — unchanged saves keep the old one.
///
/// The diff keeps appends cheap: a row whose `(entry_id, fingerprint)` still
/// matches its `seq` is not rewritten, and payload rows likewise. Rewinds and
/// splices land as deletes/upserts at their shifted positions, which is what
/// makes `(seq, entry_id)` cursors self-invalidating.
pub(crate) fn write_transcript_index(
    transaction: &Connection,
    session: &AgentSession,
) -> io::Result<u64> {
    let session_id = session.id.to_string();
    let flat = flatten_transcript(session);

    let mut stored_entries: Vec<(Uuid, u64)> = {
        let mut statement = transaction
            .prepare(
                "SELECT entry_id, fingerprint FROM transcript_entries
                 WHERE session_id = ?1 ORDER BY seq",
            )
            .map_err(to_io_error)?;
        statement
            .query_map(params![session_id], |row| {
                Ok((
                    Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    row.get::<_, i64>(1)? as u64,
                ))
            })
            .map_err(to_io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_io_error)?
    };
    let mut stored_payloads: HashMap<(Uuid, String), u64> = {
        let mut statement = transaction
            .prepare(
                "SELECT entry_id, field, fingerprint FROM transcript_payloads
                 WHERE session_id = ?1",
            )
            .map_err(to_io_error)?;
        statement
            .query_map(params![session_id], |row| {
                Ok((
                    (
                        Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        row.get::<_, String>(1)?,
                    ),
                    row.get::<_, i64>(2)? as u64,
                ))
            })
            .map_err(to_io_error)?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(to_io_error)?
    };
    let stored_turns: HashMap<Uuid, String> = {
        let mut statement = transaction
            .prepare("SELECT turn_id, data FROM transcript_turns WHERE session_id = ?1")
            .map_err(to_io_error)?;
        statement
            .query_map(params![session_id], |row| {
                Ok((
                    Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    row.get::<_, String>(1)?,
                ))
            })
            .map_err(to_io_error)?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(to_io_error)?
    };

    let mut changed = false;

    // Entries: upsert rows whose identity or content moved, then drop the
    // tail beyond the new length — rewinds truncate there.
    for (seq, entry) in flat.iter().enumerate() {
        let unchanged = stored_entries
            .get(seq)
            .is_some_and(|stored| *stored == (entry.entry_id, entry.fingerprint));
        if unchanged {
            continue;
        }
        changed = true;
        transaction
            .execute(
                "INSERT INTO transcript_entries
                   (session_id, seq, entry_id, kind, turn_id, after_message,
                    block_index, activity_index, revision, fingerprint, compact, compact_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(session_id, seq) DO UPDATE SET
                   entry_id = excluded.entry_id,
                   kind = excluded.kind,
                   turn_id = excluded.turn_id,
                   after_message = excluded.after_message,
                   block_index = excluded.block_index,
                   activity_index = excluded.activity_index,
                   revision = excluded.revision,
                   fingerprint = excluded.fingerprint,
                   compact = excluded.compact,
                   compact_bytes = excluded.compact_bytes",
                params![
                    session_id,
                    seq as i64,
                    entry.entry_id.to_string(),
                    entry.kind.tag(),
                    entry.turn_id.map(|id| id.to_string()),
                    entry.after_message as i64,
                    entry.block_index.map(|index| index as i64),
                    entry.activity_index.map(|index| index as i64),
                    // Filled in below after the new revision is known — the
                    // placeholder keeps the row shape consistent meanwhile.
                    0_i64,
                    entry.fingerprint as i64,
                    entry.compact,
                    entry.compact.as_ref().map_or(0, |compact| compact.len()) as i64,
                ],
            )
            .map_err(to_io_error)?;
    }
    if stored_entries.len() > flat.len() {
        changed = true;
        transaction
            .execute(
                "DELETE FROM transcript_entries
                 WHERE session_id = ?1 AND seq >= ?2",
                params![session_id, flat.len() as i64],
            )
            .map_err(to_io_error)?;
    }
    stored_entries.truncate(flat.len());
    for (seq, entry) in flat.iter().enumerate() {
        if seq < stored_entries.len() {
            stored_entries[seq] = (entry.entry_id, entry.fingerprint);
        }
    }

    // Payloads: live entry ids bound which stored rows may survive; within a
    // live entry, fields not in `entry.payloads` are deletions.
    let live_entries: HashSet<Uuid> = flat.iter().map(|entry| entry.entry_id).collect();
    let mut live_payloads: HashSet<(Uuid, String)> = HashSet::new();
    for entry in &flat {
        if entry.payloads.is_empty() {
            continue;
        }
        for payload in &entry.payloads {
            let field = payload_field_name(payload.field).to_owned();
            live_payloads.insert((entry.entry_id, field.clone()));
            match stored_payloads.get(&(entry.entry_id, field.clone())) {
                Some(stored) if *stored == payload.fingerprint => continue,
                _ => {
                    changed = true;
                    transaction
                        .execute(
                            "INSERT INTO transcript_payloads
                               (session_id, entry_id, field, revision, fingerprint, bytes, truncated, content)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                             ON CONFLICT(session_id, entry_id, field) DO UPDATE SET
                               revision = excluded.revision,
                               fingerprint = excluded.fingerprint,
                               bytes = excluded.bytes,
                               truncated = excluded.truncated,
                               content = excluded.content",
                            params![
                                session_id,
                                entry.entry_id.to_string(),
                                field,
                                0_i64,
                                payload.fingerprint as i64,
                                payload.bytes as i64,
                                i64::from(payload.truncated),
                                payload.content,
                            ],
                        )
                        .map_err(to_io_error)?;
                }
            }
        }
    }
    let obsolete: Vec<(Uuid, String)> = stored_payloads
        .keys()
        .filter(|(entry_id, field)| {
            !live_entries.contains(entry_id) || !live_payloads.contains(&(*entry_id, field.clone()))
        })
        .cloned()
        .collect();
    for (entry_id, field) in obsolete {
        changed = true;
        stored_payloads.remove(&(entry_id, field.clone()));
        transaction
            .execute(
                "DELETE FROM transcript_payloads
                 WHERE session_id = ?1 AND entry_id = ?2 AND field = ?3",
                params![session_id, entry_id.to_string(), field],
            )
            .map_err(to_io_error)?;
    }

    // Turns: one row per AgentTurn, keyed by id.
    let mut live_turns: HashSet<Uuid> = HashSet::with_capacity(session.turns.len());
    for (position, turn) in session.turns.iter().enumerate() {
        live_turns.insert(turn.id);
        let data = serde_json::to_string(turn).map_err(to_io_error)?;
        match stored_turns.get(&turn.id) {
            Some(stored) if *stored == data => continue,
            _ => {
                changed = true;
                transaction
                    .execute(
                        "INSERT INTO transcript_turns
                           (session_id, turn_id, turn_count, position, data)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(session_id, turn_id) DO UPDATE SET
                           turn_count = excluded.turn_count,
                           position = excluded.position,
                           data = excluded.data",
                        params![
                            session_id,
                            turn.id.to_string(),
                            turn.turn_count as i64,
                            position as i64,
                            data
                        ],
                    )
                    .map_err(to_io_error)?;
            }
        }
    }
    for turn_id in stored_turns.keys().copied().collect::<Vec<_>>() {
        if live_turns.contains(&turn_id) {
            continue;
        }
        changed = true;
        transaction
            .execute(
                "DELETE FROM transcript_turns WHERE session_id = ?1 AND turn_id = ?2",
                params![session_id, turn_id.to_string()],
            )
            .map_err(to_io_error)?;
    }

    let head = read_head(transaction, &session_id)?;
    let revision = match (&head, changed) {
        (Some(head), false) => head.revision,
        (Some(head), true) => head.revision + 1,
        (None, _) => 1,
    };
    // A head row missing while rows exist can only mean a crashed conversion;
    // write it either way — upsert is idempotent.
    if changed || head.is_none() {
        let queued_messages =
            serde_json::to_string(&session.queued_messages).map_err(to_io_error)?;
        transaction
            .execute(
                "INSERT INTO transcript_heads
                   (session_id, revision, entry_count, message_count, queued_messages, details_pruned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id) DO UPDATE SET
                   revision = excluded.revision,
                   entry_count = excluded.entry_count,
                   message_count = excluded.message_count,
                   queued_messages = excluded.queued_messages,
                   details_pruned = excluded.details_pruned",
                params![
                    session_id,
                    revision as i64,
                    flat.len() as i64,
                    session.messages.len() as i64,
                    queued_messages,
                    i64::from(session.details_pruned),
                ],
            )
            .map_err(to_io_error)?;
        // Backfill the row revisions the placeholder wrote — entry and
        // payload rows changed in this save carry the new head revision.
        transaction
            .execute(
                "UPDATE transcript_entries SET revision = ?2
                 WHERE session_id = ?1 AND revision = 0",
                params![session_id, revision as i64],
            )
            .map_err(to_io_error)?;
        transaction
            .execute(
                "UPDATE transcript_payloads SET revision = ?2
                 WHERE session_id = ?1 AND revision = 0",
                params![session_id, revision as i64],
            )
            .map_err(to_io_error)?;
    }
    Ok(revision)
}

/// Drop every normalized row for a removed session — shares the delete path
/// with `sessions`/`session_details`/`messages`.
pub(crate) fn delete_transcript_index(
    transaction: &Connection,
    session_id: &str,
) -> io::Result<()> {
    for table in [
        "transcript_entries",
        "transcript_payloads",
        "transcript_turns",
        "transcript_heads",
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE session_id = ?1"),
                params![session_id],
            )
            .map_err(to_io_error)?;
    }
    Ok(())
}

/// Detail pruning keeps activity skeletons but drops their bulk fields —
/// mirror that by clearing payload rows and stamping the head, so a window
/// read reports the fields `available: false` instead of resurrecting them.
pub(crate) fn prune_transcript_index(transaction: &Connection, session_id: &str) -> io::Result<()> {
    let Some(head) = read_head(transaction, session_id)? else {
        return Ok(());
    };
    if head.details_pruned {
        return Ok(());
    }
    transaction
        .execute(
            "DELETE FROM transcript_payloads WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(to_io_error)?;
    transaction
        .execute(
            "UPDATE transcript_heads
             SET details_pruned = 1, revision = ?2 WHERE session_id = ?1",
            params![session_id, (head.revision + 1) as i64],
        )
        .map_err(to_io_error)?;
    Ok(())
}

struct EntryRow {
    seq: i64,
    entry_id: Uuid,
    kind: EntryKind,
    turn_id: Option<Uuid>,
    after_message: u32,
    block_index: Option<u32>,
    activity_index: Option<u32>,
    revision: u64,
    compact: Option<String>,
}

fn read_entry_range(
    connection: &Connection,
    session_id: &str,
    seq_lo: i64,
    seq_hi: i64,
) -> io::Result<Vec<EntryRow>> {
    let mut statement = connection
        .prepare(
            "SELECT seq, entry_id, kind, turn_id, after_message, block_index,
                    activity_index, revision, compact
             FROM transcript_entries
             WHERE session_id = ?1 AND seq >= ?2 AND seq < ?3
             ORDER BY seq",
        )
        .map_err(to_io_error)?;
    let rows = statement
        .query_map(params![session_id, seq_lo, seq_hi], |row| {
            Ok(EntryRow {
                seq: row.get(0)?,
                entry_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                kind: if row.get::<_, String>(2)? == ENTRY_KIND_MESSAGE {
                    EntryKind::Message
                } else {
                    EntryKind::Activity
                },
                turn_id: row
                    .get::<_, Option<String>>(3)?
                    .and_then(|id| Uuid::parse_str(&id).ok()),
                after_message: row.get::<_, i64>(4)? as u32,
                block_index: row.get::<_, Option<i64>>(5)?.map(|index| index as u32),
                activity_index: row.get::<_, Option<i64>>(6)?.map(|index| index as u32),
                revision: row.get::<_, i64>(7)? as u64,
                compact: row.get(8)?,
            })
        })
        .map_err(to_io_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_io_error)?;
    Ok(rows)
}

fn entry_at_seq(
    connection: &Connection,
    session_id: &str,
    seq: i64,
) -> io::Result<Option<(i64, Uuid)>> {
    connection
        .query_row(
            "SELECT seq, entry_id FROM transcript_entries
             WHERE session_id = ?1 AND seq = ?2",
            params![session_id, seq],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                ))
            },
        )
        .optional()
        .map_err(to_io_error)
}

/// The messages-table row shape `message_from_row` parses, with byte-clipped
/// `content`/`display_content` and their true totals fetched alongside.
struct MessageRow {
    message: Message,
    content_bytes: u64,
    content_clipped: bool,
    display_content_bytes: Option<u64>,
    display_content_clipped: bool,
}

fn read_window_messages(
    connection: &Connection,
    session_id: &str,
    positions: &[u32],
) -> io::Result<HashMap<u32, MessageRow>> {
    if positions.is_empty() {
        return Ok(HashMap::new());
    }
    let inline_cap = (MESSAGE_INLINE_BYTES + 8) as i64;
    // Positions are window-local message anchors — bounded by the page size.
    let placeholders = positions.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT position, id, turn_id, role,
                CAST(substr(CAST(content AS BLOB), 1, {inline_cap}) AS BLOB),
                octet_length(content),
                CAST(substr(CAST(COALESCE(display_content, '') AS BLOB), 1, {inline_cap}) AS BLOB),
                CASE WHEN display_content IS NULL THEN NULL ELSE octet_length(display_content) END,
                attachments, atoms, created_at, streaming, sent_by_task, hidden,
                notice, context_mark, report_trigger, reference_context
         FROM messages
         WHERE session_id = ?1 AND position IN ({placeholders})"
    );
    let mut params_vec: Vec<rusqlite::types::Value> = Vec::with_capacity(positions.len() + 1);
    params_vec.push(rusqlite::types::Value::Text(session_id.to_owned()));
    for position in positions {
        params_vec.push(rusqlite::types::Value::Integer(*position as i64));
    }
    let mut statement = connection.prepare(&sql).map_err(to_io_error)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(params_vec), |row| {
            let position = row.get::<_, i64>(0)? as u32;
            let content_blob = row.get::<_, Vec<u8>>(4)?;
            let content_bytes = row.get::<_, i64>(5)? as u64;
            // `substr` of a zero-byte blob is NULL — an absent/empty
            // display_content reads the same way.
            let display_blob = row.get::<_, Option<Vec<u8>>>(6)?.unwrap_or_default();
            let display_bytes = row.get::<_, Option<i64>>(7)?.map(|bytes| bytes as u64);
            let columns: MessageColumns = (
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                String::from_utf8_lossy(&clip_utf8(&content_blob, MESSAGE_INLINE_BYTES))
                    .into_owned(),
                if display_bytes.is_some() {
                    Some(
                        String::from_utf8_lossy(&clip_utf8(&display_blob, MESSAGE_INLINE_BYTES))
                            .into_owned(),
                    )
                } else {
                    None
                },
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
                row.get(14)?,
                row.get(15)?,
                row.get(16)?,
                row.get(17)?,
            );
            Ok((position, columns, content_bytes, display_bytes))
        })
        .map_err(to_io_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_io_error)?;
    let mut messages = HashMap::with_capacity(rows.len());
    for (position, columns, content_bytes, display_bytes) in rows {
        let Some(mut message) = message_from_row(columns) else {
            continue;
        };
        let content_clipped = (message.content.len() as u64) < content_bytes;
        let display_clipped = display_bytes.is_some_and(|bytes| {
            (message.display_content.as_ref().map_or(0, |d| d.len()) as u64) < bytes
        });
        // A row whose content is a mid-clip loses the boundary code point —
        // drop the lossy tail so the visible text never renders a
        // replacement char the range read would contradict.
        if content_clipped && message.content.ends_with('\u{FFFD}') {
            message.content.pop();
        }
        if display_clipped
            && let Some(display) = &mut message.display_content
            && display.ends_with('\u{FFFD}')
        {
            display.pop();
        }
        messages.insert(
            position,
            MessageRow {
                message,
                content_bytes,
                content_clipped,
                display_content_bytes: display_bytes,
                display_content_clipped: display_clipped,
            },
        );
    }
    Ok(messages)
}

/// Truncate a fetched BLOB prefix at the last full code point — the byte
/// budget is on UTF-8 bytes, never mid-sequence.
fn clip_utf8<'a>(bytes: &'a [u8], cap: usize) -> &'a [u8] {
    if bytes.len() <= cap {
        return bytes;
    }
    let mut end = cap;
    while end > 0 && (bytes[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    &bytes[..end]
}

fn read_entry_payloads(
    connection: &Connection,
    session_id: &str,
    entry_ids: &[Uuid],
) -> io::Result<HashMap<Uuid, Vec<PayloadDescriptor>>> {
    if entry_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = entry_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT entry_id, field, revision, bytes, truncated
         FROM transcript_payloads
         WHERE session_id = ?1 AND entry_id IN ({placeholders})"
    );
    let mut params_vec: Vec<rusqlite::types::Value> = Vec::with_capacity(entry_ids.len() + 1);
    params_vec.push(rusqlite::types::Value::Text(session_id.to_owned()));
    for entry_id in entry_ids {
        params_vec.push(rusqlite::types::Value::Text(entry_id.to_string()));
    }
    let mut statement = connection.prepare(&sql).map_err(to_io_error)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(params_vec), |row| {
            Ok((
                Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
                row.get::<_, i64>(4)? != 0,
            ))
        })
        .map_err(to_io_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_io_error)?;
    let mut payloads: HashMap<Uuid, Vec<PayloadDescriptor>> = HashMap::new();
    for (entry_id, field, revision, bytes, truncated) in rows {
        let Some(field) = ACTIVITY_PAYLOAD_FIELDS
            .iter()
            .find(|known| payload_field_name(**known) == field)
            .copied()
        else {
            continue;
        };
        payloads
            .entry(entry_id)
            .or_default()
            .push(PayloadDescriptor {
                field,
                revision,
                bytes,
                truncated,
                available: true,
            });
    }
    // Fields present in the activity but already pruned have no row — the
    // descriptor still lists them as unavailable placeholders.
    Ok(payloads)
}

/// Estimated wire size of one assembled entry, for byte-budget trimming.
/// `compact` JSON + inline content + a fixed per-descriptor overhead.
fn entry_wire_bytes(entry: &TranscriptEntry) -> usize {
    let base = match &entry.data {
        TranscriptEntryData::Message {
            message, payloads, ..
        } => {
            message.content.len()
                + message.display_content.as_ref().map_or(0, |d| d.len())
                + 512
                + payloads.len() * 96
        }
        TranscriptEntryData::Activity { activity, payloads } => {
            serde_json::to_string(activity).map_or(1024, |json| json.len()) + payloads.len() * 96
        }
    };
    base + 96
}

/// One bounded read of the flattened index. `None` means the session has no
/// head row — the caller converts it (one-time legacy parse) and retries.
pub(crate) fn read_transcript_window(
    connection: &Connection,
    session_id: Uuid,
    target: &TranscriptWindowTarget,
    max_entries: u32,
    max_bytes: u32,
    known_revision: Option<u64>,
) -> io::Result<Option<TranscriptWindow>> {
    let sid = session_id.to_string();
    let Some(head) = read_head(connection, &sid)? else {
        return Ok(None);
    };
    let queued_messages: Vec<waku_protocol::model::QueuedMessage> =
        serde_json::from_str(&head.queued_messages).unwrap_or_default();
    let mut window = TranscriptWindow {
        session_id,
        revision: head.revision,
        unchanged: false,
        invalid_cursor: false,
        entries: Vec::new(),
        previous: None,
        next: None,
        has_more_before: false,
        has_more_after: false,
        message_base: 0,
        message_count: head.message_count,
        entry_count: head.entry_count,
        converted: true,
        legacy_read: false,
        turns: Vec::new(),
        navigation: None,
        queued_messages,
        event_cursor: None,
    };
    if known_revision == Some(head.revision) {
        window.unchanged = true;
        return Ok(Some(window));
    }

    let limit = max_entries.max(1).min(2000) as i64;
    let (mut seq_lo, mut seq_hi, invalid): (i64, i64, bool) = match target {
        TranscriptWindowTarget::Latest => {
            // The latest user turn anchors the window's top; a turn bigger
            // than the budget yields its tail rather than its head — the
            // last entries are the live/most recent content.
            let start: Option<i64> = connection
                .query_row(
                    "SELECT min(e.seq) FROM transcript_entries e
                     INNER JOIN (
                       SELECT turn_id FROM transcript_turns
                       WHERE session_id = ?1
                       ORDER BY position DESC LIMIT 1
                     ) latest ON latest.turn_id = e.turn_id
                     WHERE e.session_id = ?1",
                    params![sid],
                    |row| row.get(0),
                )
                .optional()
                .map_err(to_io_error)?
                .flatten();
            let lo = start.unwrap_or(0).max(head.entry_count - limit);
            (lo, head.entry_count, false)
        }
        TranscriptWindowTarget::Before { cursor } => {
            match entry_at_seq(connection, &sid, cursor.seq)? {
                Some((_, id)) if id == cursor.entry_id => {
                    ((cursor.seq - limit).max(0), cursor.seq, false)
                }
                _ => (0, 0, true),
            }
        }
        TranscriptWindowTarget::After { cursor } => {
            match entry_at_seq(connection, &sid, cursor.seq)? {
                Some((_, id)) if id == cursor.entry_id => (
                    cursor.seq + 1,
                    (cursor.seq + 1 + limit).min(head.entry_count),
                    false,
                ),
                _ => (0, 0, true),
            }
        }
        TranscriptWindowTarget::Around { entry_id } => {
            let seq: Option<i64> = connection
                .query_row(
                    "SELECT seq FROM transcript_entries
                     WHERE session_id = ?1 AND entry_id = ?2",
                    params![sid, entry_id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(to_io_error)?;
            match seq {
                Some(seq) => {
                    let half = limit / 2;
                    (
                        (seq - half).max(0),
                        (seq + half).min(head.entry_count),
                        false,
                    )
                }
                None => (0, 0, true),
            }
        }
        TranscriptWindowTarget::Turn { turn_id, after } => {
            let lo: Option<i64> = if let Some(after) = after {
                Some(*after + 1)
            } else {
                connection
                    .query_row(
                        "SELECT min(seq) FROM transcript_entries
                         WHERE session_id = ?1 AND turn_id = ?2",
                        params![sid, turn_id.to_string()],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(to_io_error)?
                    .flatten()
            };
            match lo {
                Some(lo) => (lo, (lo + limit).min(head.entry_count), false),
                // An unknown turn is a client-side structural miss, like an
                // invalid cursor.
                None => (0, 0, true),
            }
        }
    };
    if invalid {
        window.invalid_cursor = true;
        return Ok(Some(window));
    }

    // A turn-scoped read filters by turn_id; everything else is a
    // contiguous seq range.
    let rows = if let TranscriptWindowTarget::Turn { turn_id, .. } = target {
        let mut statement = connection
            .prepare(
                "SELECT seq, entry_id, kind, turn_id, after_message, block_index,
                        activity_index, revision, compact
                 FROM transcript_entries
                 WHERE session_id = ?1 AND turn_id = ?2 AND seq >= ?3
                 ORDER BY seq LIMIT ?4",
            )
            .map_err(to_io_error)?;
        statement
            .query_map(
                params![sid, turn_id.to_string(), seq_lo, seq_hi - seq_lo],
                |row| {
                    Ok(EntryRow {
                        seq: row.get(0)?,
                        entry_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                        kind: if row.get::<_, String>(2)? == ENTRY_KIND_MESSAGE {
                            EntryKind::Message
                        } else {
                            EntryKind::Activity
                        },
                        turn_id: row
                            .get::<_, Option<String>>(3)?
                            .and_then(|id| Uuid::parse_str(&id).ok()),
                        after_message: row.get::<_, i64>(4)? as u32,
                        block_index: row.get::<_, Option<i64>>(5)?.map(|index| index as u32),
                        activity_index: row.get::<_, Option<i64>>(6)?.map(|index| index as u32),
                        revision: row.get::<_, i64>(7)? as u64,
                        compact: row.get(8)?,
                    })
                },
            )
            .map_err(to_io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_io_error)?
    } else {
        read_entry_range(connection, &sid, seq_lo, seq_hi)?
    };

    // The byte budget trims from the far edge of the requested direction:
    // `before`/`latest`/`around` keep the tail, `after`/`turn` keep the head.
    let byte_limit = max_bytes.max(4096) as usize;
    let trim_front = matches!(
        target,
        TranscriptWindowTarget::Latest
            | TranscriptWindowTarget::Before { .. }
            | TranscriptWindowTarget::Around { .. }
    );
    let assembled = assemble_entries(connection, &sid, &rows)?;
    let mut sized: Vec<(usize, TranscriptEntry)> = assembled
        .into_iter()
        .map(|entry| (entry_wire_bytes(&entry), entry))
        .collect();
    let mut total: usize = sized.iter().map(|(bytes, _)| *bytes).sum();
    while total > byte_limit && !sized.is_empty() {
        let (bytes, _) = if trim_front {
            sized.remove(0)
        } else {
            sized.pop().expect("sized nonempty")
        };
        total -= bytes;
    }
    if let TranscriptWindowTarget::Around { entry_id } = target {
        // The anchor must survive trimming: restore a front-trimmed window
        // ending at it before losing the reveal target entirely.
        if !sized.is_empty() && sized.iter().all(|(_, entry)| entry.entry_id() != *entry_id) {
            let anchor = rows.iter().position(|row| row.entry_id == *entry_id);
            if let Some(index) = anchor {
                let reassembled = assemble_entries(connection, &sid, &rows[index..=index])?;
                sized = reassembled
                    .into_iter()
                    .map(|entry| (entry_wire_bytes(&entry), entry))
                    .collect();
            }
        }
    }
    let entries: Vec<TranscriptEntry> = sized.into_iter().map(|(_, entry)| entry).collect();
    if !entries.is_empty() {
        seq_lo = entries.first().expect("entries nonempty").seq;
        seq_hi = entries.last().expect("entries nonempty").seq + 1;
        window.has_more_before = seq_lo > 0;
        window.has_more_after = seq_hi < head.entry_count;
        // Before/After exclude their cursor entry. Anchor continuations to
        // the actual loaded edges, after byte trimming, so no entry is skipped.
        window.previous = window.has_more_before.then_some(TranscriptCursor {
            seq: seq_lo,
            entry_id: entries.first().expect("entries nonempty").entry_id(),
        });
        window.next = window.has_more_after.then_some(TranscriptCursor {
            seq: seq_hi - 1,
            entry_id: entries.last().expect("entries nonempty").entry_id(),
        });
        window.message_base = entries
            .iter()
            .find(|entry| matches!(entry.data, TranscriptEntryData::Message { .. }))
            .map(|entry| entry.after_message)
            .unwrap_or_else(|| entries.first().expect("entries nonempty").after_message);
    }
    window.entries = entries;

    // Turn metadata for the covered ids plus the latest turn — row builders
    // read status/timing without the detail blob.
    let mut turn_ids: HashSet<String> = window
        .entries
        .iter()
        .filter_map(|entry| entry.turn_id.map(|id| id.to_string()))
        .collect();
    let latest_turn: Option<String> = connection
        .query_row(
            "SELECT turn_id FROM transcript_turns
             WHERE session_id = ?1 ORDER BY position DESC LIMIT 1",
            params![sid],
            |row| row.get(0),
        )
        .optional()
        .map_err(to_io_error)?;
    turn_ids.extend(latest_turn);
    if !turn_ids.is_empty() {
        let placeholders = turn_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT data FROM transcript_turns
             WHERE session_id = ?1 AND turn_id IN ({placeholders})"
        );
        let mut params_vec: Vec<rusqlite::types::Value> = Vec::with_capacity(turn_ids.len() + 1);
        params_vec.push(rusqlite::types::Value::Text(sid.clone()));
        for turn_id in &turn_ids {
            params_vec.push(rusqlite::types::Value::Text(turn_id.clone()));
        }
        let mut statement = connection.prepare(&sql).map_err(to_io_error)?;
        window.turns = statement
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                row.get::<_, String>(0)
            })
            .map_err(to_io_error)?
            .filter_map(|row| row.ok())
            .filter_map(|data| serde_json::from_str::<AgentTurn>(&data).ok())
            .collect();
    }
    if matches!(
        target,
        TranscriptWindowTarget::Latest | TranscriptWindowTarget::Around { .. }
    ) {
        // Read only a compact prompt index, never historical message bodies or
        // tool payloads. Ordinary history pages reuse this snapshot client-side.
        let mut statement = connection
            .prepare(
                "SELECT id, position, substr(COALESCE(display_content, content), 1, 400)
             FROM messages WHERE session_id = ?1 AND role = 'user' AND hidden = 0
             ORDER BY position",
            )
            .map_err(to_io_error)?;
        let rows = statement
            .query_map(params![sid], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(to_io_error)?;
        let mut navigation = Vec::new();
        for row in rows {
            let (id, message_index, prompt) = row.map_err(to_io_error)?;
            let message_id = Uuid::parse_str(&id)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            navigation.push(waku_protocol::transcript::TranscriptNavigationEntry {
                message_id,
                message_index,
                prompt,
            });
        }
        window.navigation = Some(navigation);
    }
    Ok(Some(window))
}

/// Hydrate `rows` into wire entries: message rows join `messages` (bodies
/// byte-clipped with continuation descriptors), activity rows parse their
/// `compact` JSON and describe their payload rows.
fn assemble_entries(
    connection: &Connection,
    session_id: &str,
    rows: &[EntryRow],
) -> io::Result<Vec<TranscriptEntry>> {
    let positions: Vec<u32> = rows
        .iter()
        .filter(|row| row.kind == EntryKind::Message)
        .map(|row| row.after_message)
        .collect();
    let messages = read_window_messages(connection, session_id, &positions)?;
    let activity_ids: Vec<Uuid> = rows
        .iter()
        .filter(|row| row.kind == EntryKind::Activity)
        .map(|row| row.entry_id)
        .collect();
    let payloads = read_entry_payloads(connection, session_id, &activity_ids)?;

    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let data = match row.kind {
            EntryKind::Message => {
                let Some(stored) = messages.get(&row.after_message) else {
                    // A message row gone mid-conversion: skip the entry —
                    // the next revision converges it.
                    continue;
                };
                let mut descriptors = vec![PayloadDescriptor {
                    field: TranscriptPayloadField::Content,
                    revision: row.revision,
                    bytes: stored.content_bytes,
                    truncated: false,
                    available: true,
                }];
                if let Some(bytes) = stored.display_content_bytes {
                    descriptors.push(PayloadDescriptor {
                        field: TranscriptPayloadField::DisplayContent,
                        revision: row.revision,
                        bytes,
                        truncated: false,
                        available: true,
                    });
                }
                if stored.display_content_clipped || stored.content_clipped {
                    // Continuation descriptors already cover the ranges.
                }
                let content_window = stored.content_clipped.then(|| PayloadWindow {
                    offset: 0,
                    len: stored.message.content.len() as u64,
                    total: stored.content_bytes,
                });
                TranscriptEntryData::Message {
                    message: Box::new(stored.message.clone()),
                    content_window,
                    payloads: descriptors,
                }
            }
            EntryKind::Activity => {
                let activity: ActivityItem =
                    serde_json::from_str(row.compact.as_deref().unwrap_or("{}"))
                        .map_err(to_io_error)?;
                TranscriptEntryData::Activity {
                    activity: Box::new(activity),
                    payloads: payloads.get(&row.entry_id).cloned().unwrap_or_default(),
                }
            }
        };
        entries.push(TranscriptEntry {
            seq: row.seq,
            revision: row.revision,
            turn_id: row.turn_id,
            after_message: row.after_message,
            block_index: row.block_index,
            activity_index: row.activity_index,
            data,
        });
    }
    Ok(entries)
}

/// Range-read one stored field. `None` when the session is unconverted — the
/// caller converts and retries; `unavailable` reports pruned fields.
pub(crate) fn read_transcript_payload(
    connection: &Connection,
    session_id: Uuid,
    entry_id: Uuid,
    field: TranscriptPayloadField,
    expected_revision: Option<u64>,
    offset: u64,
    limit: u64,
) -> io::Result<Option<TranscriptPayload>> {
    let sid = session_id.to_string();
    if read_head(connection, &sid)?.is_none() {
        return Ok(None);
    }
    let entry = connection
        .query_row(
            "SELECT kind, revision FROM transcript_entries
             WHERE session_id = ?1 AND entry_id = ?2",
            params![sid, entry_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64)),
        )
        .optional()
        .map_err(to_io_error)?;
    let Some((kind, entry_revision)) = entry else {
        return Ok(Some(TranscriptPayload {
            entry_id,
            field,
            revision: 0,
            stale: false,
            total_bytes: 0,
            offset,
            content: String::new(),
            complete: true,
            unavailable: Some("entry no longer exists".into()),
        }));
    };
    let limit = limit.clamp(1, PAYLOAD_CHUNK_BYTES) as i64;
    // Message fields read their `messages` columns; activity fields read
    // `transcript_payloads` rows.
    let (content, total_bytes, _truncated, revision, content_offset) = if kind == ENTRY_KIND_MESSAGE
    {
        let column = match field {
            TranscriptPayloadField::Content => "content",
            TranscriptPayloadField::DisplayContent => "display_content",
            TranscriptPayloadField::Attachments => "attachments",
            TranscriptPayloadField::Atoms => "atoms",
            TranscriptPayloadField::Notice => "notice",
            _ => {
                return Ok(Some(TranscriptPayload {
                    entry_id,
                    field,
                    revision: entry_revision,
                    stale: false,
                    total_bytes: 0,
                    offset,
                    content: String::new(),
                    complete: true,
                    unavailable: Some("field does not exist on messages".into()),
                }));
            }
        };
        // Fetch `limit` plus a code-point pad on each side so the returned
        // string is whole UTF-8 regardless of where `offset` landed.
        let row: Option<(Vec<u8>, Option<i64>)> = connection
            .query_row(
                &format!(
                    "SELECT CAST(substr(CAST({column} AS BLOB), ?2, ?3) AS BLOB),
                            octet_length({column})
                     FROM messages WHERE session_id = ?1 AND id = ?4"
                ),
                params![
                    sid,
                    offset.saturating_sub(4).max(0) as i64 + 1,
                    limit + 8,
                    entry_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(to_io_error)?;
        match row {
            Some((blob, Some(total))) => {
                let (text, true_offset) = utf8_range(&blob, offset, limit as u64);
                (text, total as u64, false, entry_revision, true_offset)
            }
            Some((blob, None)) => (
                String::from_utf8_lossy(&blob).into_owned(),
                0,
                false,
                entry_revision,
                offset,
            ),
            _ => {
                return Ok(Some(TranscriptPayload {
                    entry_id,
                    field,
                    revision: entry_revision,
                    stale: false,
                    total_bytes: 0,
                    offset,
                    content: String::new(),
                    complete: true,
                    unavailable: Some("field is absent".into()),
                }));
            }
        }
    } else {
        let row: Option<(Vec<u8>, i64, i64, i64)> = connection
            .query_row(
                "SELECT CAST(substr(content, ?2, ?3) AS BLOB), bytes, truncated, revision
                 FROM transcript_payloads
                 WHERE session_id = ?1 AND entry_id = ?4 AND field = ?5",
                params![
                    sid,
                    offset.saturating_sub(4).max(0) as i64 + 1,
                    limit + 8,
                    entry_id.to_string(),
                    payload_field_name(field),
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(to_io_error)?;
        match row {
            Some((blob, total, truncated, revision)) => {
                let (text, true_offset) = utf8_range(&blob, offset, limit as u64);
                (
                    text,
                    total as u64,
                    truncated != 0,
                    revision as u64,
                    true_offset,
                )
            }
            None => {
                return Ok(Some(TranscriptPayload {
                    entry_id,
                    field,
                    revision: entry_revision,
                    stale: false,
                    total_bytes: 0,
                    offset,
                    content: String::new(),
                    complete: true,
                    unavailable: Some("payload was pruned".into()),
                }));
            }
        }
    };
    let stale = expected_revision.is_some_and(|expected| expected != revision);
    let byte_len = content.len() as u64;
    Ok(Some(TranscriptPayload {
        entry_id,
        field,
        revision,
        stale,
        total_bytes,
        offset: content_offset,
        content,
        complete: content_offset + byte_len >= total_bytes,
        unavailable: None,
    }))
}

/// Extract the whole-UTF-8 string for `[offset, offset+limit)` out of a BLOB
/// fetched with a code-point pad on both ends. Returns the text plus the
/// byte offset it actually starts at — the pad shrinks to the boundary,
/// never shifts it, so reassembly by `offset` stays exact.
fn utf8_range(blob: &[u8], offset: u64, limit: u64) -> (String, u64) {
    // The fetched window starts `pad` bytes before `offset` so a mid-code-
    // point `offset` can still be honored exactly.
    let pad = offset.saturating_sub(4).max(0);
    let want_lo = ((offset - pad) as usize).min(blob.len());
    let want_hi = ((offset + limit - pad) as usize).min(blob.len());
    let mut lo = want_lo;
    while lo < want_hi && (blob[lo] & 0b1100_0000) == 0b1000_0000 {
        lo += 1;
    }
    let mut hi = want_hi;
    // `hi` is an exclusive bound — when it already sits at the buffer end
    // it is on a boundary by construction.
    while hi > lo && hi < blob.len() && (blob[hi] & 0b1100_0000) == 0b1000_0000 {
        hi -= 1;
    }
    (
        String::from_utf8_lossy(&blob[lo.min(hi)..hi]).into_owned(),
        pad + lo as u64,
    )
}

/// One bounded, resumable literal scan over the whole retained transcript —
/// conversation bodies and lazy tool fields alike.
pub(crate) fn find_transcript(
    connection: &Connection,
    session_id: Uuid,
    request: &waku_protocol::transcript::TranscriptFindRequest,
) -> io::Result<TranscriptFindResult> {
    let sid = session_id.to_string();
    let Some(head) = read_head(connection, &sid)? else {
        // Unconverted: report incomplete coverage rather than zero matches.
        return Ok(TranscriptFindResult {
            session_id,
            revision: 0,
            matches: Vec::new(),
            count: 0,
            complete: false,
            cursor: None,
            index_complete: false,
        });
    };
    let scopes: HashSet<TranscriptFindScope> = if request.scopes.is_empty() {
        [
            TranscriptFindScope::Conversation,
            TranscriptFindScope::Outputs,
        ]
        .into_iter()
        .collect()
    } else {
        request.scopes.iter().copied().collect()
    };
    let needle = request.query.to_ascii_lowercase();
    let after_seq = request.after.map(|after| after.seq).unwrap_or(-1);
    let mut matches = Vec::new();
    let mut examined: u64 = 0;
    let resume = request.cursor;
    let mut end: Option<TranscriptFindCursor> = None;
    let mut complete = false;
    // The resume cursor's `seq` is the next entry to examine — a mid-entry
    // stop points at its own entry, a finished one at its successor.
    let mut page_lo = resume.map(|cursor| cursor.seq).unwrap_or(0).max(0);
    const FIND_PAGE: i64 = 512;
    'scan: loop {
        let rows = read_entry_range(
            connection,
            &sid,
            page_lo,
            (page_lo + FIND_PAGE).min(head.entry_count),
        )?;
        let Some(last_seq) = rows.last().map(|last| last.seq) else {
            complete = true;
            break;
        };
        for row in rows {
            // Within one entry the in-scope fields scan in a fixed order so
            // a mid-entry cursor resumes deterministically.
            let fields: Vec<TranscriptPayloadField> = if row.kind == EntryKind::Message {
                if scopes.contains(&TranscriptFindScope::Conversation) {
                    vec![TranscriptPayloadField::Content]
                } else {
                    Vec::new()
                }
            } else {
                let mut fields = Vec::new();
                if scopes.contains(&TranscriptFindScope::Outputs) {
                    fields.push(TranscriptPayloadField::Output);
                }
                if scopes.contains(&TranscriptFindScope::Arguments) {
                    fields.push(TranscriptPayloadField::Arguments);
                }
                if scopes.contains(&TranscriptFindScope::Reasoning) {
                    fields.push(TranscriptPayloadField::Reasoning);
                }
                if scopes.contains(&TranscriptFindScope::Details) {
                    fields.push(TranscriptPayloadField::Detail);
                }
                fields
            };
            for field in fields {
                // A mid-entry resume skips fields already scanned — field
                // order is the enum's declaration order.
                let resuming = resume.is_some_and(|resume| resume.seq == row.seq);
                if resuming
                    && resume.is_some_and(|resume| resume.field.is_some_and(|at| field < at))
                {
                    continue;
                }
                // Cheap existence check — never transfers a non-matching field.
                let hit = field_contains(connection, &sid, &row, field, &needle)?;
                if !hit {
                    continue;
                }
                let mut field_offset =
                    if resuming && resume.is_some_and(|resume| resume.field == Some(field)) {
                        resume.expect("checked above").field_offset
                    } else {
                        0
                    };
                // Chunked fetch bounds a megabyte tool output per call.
                loop {
                    let chunk = read_field_chunk(
                        connection,
                        &sid,
                        &row,
                        field,
                        field_offset,
                        FIND_SCAN_BYTES.min(PAYLOAD_CHUNK_BYTES),
                    )?;
                    let Some((text, total)) = chunk else {
                        break;
                    };
                    examined = examined.saturating_add(text.len() as u64);
                    let folded = text.to_ascii_lowercase();
                    for (index, _) in folded.match_indices(&needle) {
                        if row.seq >= after_seq {
                            let snippet = snippet_at(&text, index, needle.len());
                            matches.push(TranscriptFindMatch {
                                entry_id: row.entry_id,
                                seq: row.seq,
                                field,
                                offset: field_offset + index as u64,
                                len: needle.len() as u64,
                                snippet: snippet.0,
                                match_offset_in_snippet: snippet.1,
                                revision: head.revision,
                            });
                        }
                    }
                    field_offset += text.len() as u64;
                    if matches.len() >= FIND_MAX_MATCHES as usize || examined >= FIND_SCAN_BYTES {
                        end = Some(TranscriptFindCursor {
                            seq: row.seq,
                            field: Some(field),
                            field_offset,
                        });
                        break 'scan;
                    }
                    if field_offset >= total {
                        break;
                    }
                }
            }
            end = Some(TranscriptFindCursor {
                seq: row.seq + 1,
                field: None,
                field_offset: 0,
            });
        }
        if last_seq + 1 >= head.entry_count {
            complete = true;
            break;
        }
        page_lo = last_seq + 1;
    }
    let result_cursor = if complete { None } else { end };
    Ok(TranscriptFindResult {
        session_id,
        revision: head.revision,
        count: matches.len() as u64,
        matches,
        complete,
        cursor: result_cursor,
        index_complete: true,
    })
}

/// `instr` existence check over the stored field — folded ASCII like the
/// existing message search, so punctuation and multiline needles behave the
/// same way on both surfaces.
fn field_contains(
    connection: &Connection,
    session_id: &str,
    row: &EntryRow,
    field: TranscriptPayloadField,
    needle: &str,
) -> io::Result<bool> {
    if row.kind == EntryKind::Message {
        connection
            .query_row(
                "SELECT instr(lower(content), lower(?3)) > 0
                 FROM messages WHERE session_id = ?1 AND id = ?2",
                params![session_id, row.entry_id.to_string(), needle],
                |row| row.get::<_, i64>(0),
            )
            .map(|hit| hit != 0)
            .map_err(to_io_error)
    } else {
        connection
            .query_row(
                "SELECT instr(lower(CAST(content AS TEXT)), lower(?3)) > 0
                 FROM transcript_payloads
                 WHERE session_id = ?1 AND entry_id = ?2 AND field = ?4",
                params![
                    session_id,
                    row.entry_id.to_string(),
                    needle,
                    payload_field_name(field)
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map(|row| row.map_or(false, |hit: i64| hit != 0))
            .map_err(to_io_error)
    }
}

/// Fetch `[offset, offset+len)` of one searchable field — message columns or
/// payload rows, same shape as the range read.
fn read_field_chunk(
    connection: &Connection,
    session_id: &str,
    row: &EntryRow,
    field: TranscriptPayloadField,
    offset: u64,
    limit: u64,
) -> io::Result<Option<(String, u64)>> {
    let limit = limit.clamp(1, PAYLOAD_CHUNK_BYTES) as i64;
    if row.kind == EntryKind::Message {
        let row: Option<(Vec<u8>, i64)> = connection
            .query_row(
                "SELECT CAST(substr(CAST(content AS BLOB), ?2, ?3) AS BLOB),
                        octet_length(content)
                 FROM messages WHERE session_id = ?1 AND id = ?4",
                params![
                    session_id,
                    offset as i64 + 1,
                    limit,
                    row.entry_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(to_io_error)?;
        Ok(row.map(|(blob, total)| {
            (
                String::from_utf8_lossy(&clip_utf8(&blob, limit as usize)).into_owned(),
                total as u64,
            )
        }))
    } else {
        let row: Option<(Vec<u8>, i64)> = connection
            .query_row(
                "SELECT CAST(substr(content, ?2, ?3) AS BLOB), bytes
                 FROM transcript_payloads
                 WHERE session_id = ?1 AND entry_id = ?4 AND field = ?5",
                params![
                    session_id,
                    offset as i64 + 1,
                    limit,
                    row.entry_id.to_string(),
                    payload_field_name(field)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(to_io_error)?;
        Ok(row.map(|(blob, total)| {
            (
                String::from_utf8_lossy(&clip_utf8(&blob, limit as usize)).into_owned(),
                total as u64,
            )
        }))
    }
}

/// Context around a match: up to ~120 bytes centered on it, clipped to
/// code-point boundaries.
fn snippet_at(text: &str, match_offset: usize, match_len: usize) -> (String, u64) {
    const CONTEXT: usize = 60;
    let mut start = match_offset.saturating_sub(CONTEXT);
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (match_offset + match_len + CONTEXT).min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    (text[start..end].to_owned(), (match_offset - start) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActivityKind, MessageRole, ProviderKind, TranscriptBlock, TurnStatus};
    use crate::persistence::{PersistedState, StateStore};
    use std::path::PathBuf;
    use waku_protocol::transcript::TranscriptFindRequest;

    fn temporary_directory() -> PathBuf {
        std::env::temp_dir().join(format!("waku-transcript-index-{}", Uuid::new_v4()))
    }

    fn store_in(directory: &std::path::Path) -> StateStore {
        StateStore::daemon(directory.join("app.db"))
    }

    fn activity(kind: ActivityKind, title: &str, output: Option<&str>) -> ActivityItem {
        let mut item = ActivityItem::new(
            Some(format!("tool-{title}")),
            kind,
            title.to_owned(),
            Some("{\"args\":1}".into()),
            true,
        );
        item.output = output.map(|output| output.to_owned());
        item
    }

    /// Three turns: each one user message, one assistant reply, and one
    /// tool-activity block anchored after the user message.
    fn session_with_transcript() -> AgentSession {
        let mut session = AgentSession::new(Uuid::nil(), ProviderKind::Codex);
        for index in 0..3 {
            let turn_id = session.begin_turn(format!("question {index}"));
            session.transcript_blocks.push(TranscriptBlock {
                after_message: index * 2 + 1,
                turn_id: Some(turn_id),
                activities: vec![activity(
                    ActivityKind::Command,
                    &format!("run-{index}"),
                    Some(&format!("output text number {index} with needle {index}")),
                )],
            });
            session.messages.push(crate::model::Message::new_for_turn(
                MessageRole::Assistant,
                format!("answer {index}"),
                turn_id,
            ));
            session.finish_active_turn(TurnStatus::Completed);
        }
        session
    }

    #[test]
    fn latest_window_pages_back_through_older_entries() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session = session_with_transcript();
        let session_id = session.id;
        state.push_session(session);
        store.save(&mut state).unwrap();

        let revision = store.transcript_revision(session_id).unwrap();
        assert!(revision.is_some(), "save must write the index head");

        // 3 turns x (user + 1 activity + assistant) = 9 flattened entries.
        let latest = store
            .read_transcript_window(
                session_id,
                &TranscriptWindowTarget::Latest,
                4,
                u32::MAX,
                None,
            )
            .unwrap()
            .expect("indexed session");
        assert!(!latest.invalid_cursor);
        assert!(latest.has_more_before);
        assert!(!latest.has_more_after);
        assert_eq!(latest.entry_count, 9);
        assert!(latest.entries.len() <= 4);
        assert!(
            latest.entries.iter().all(|entry| entry.seq >= 5),
            "the tail window holds the last entries"
        );

        // Page back before the window's first entry.
        let before = store
            .read_transcript_window(
                session_id,
                &TranscriptWindowTarget::Before {
                    cursor: latest.previous.expect("older entries remain"),
                },
                4,
                u32::MAX,
                None,
            )
            .unwrap()
            .expect("indexed session");
        assert!(!before.invalid_cursor);
        assert!(
            before
                .entries
                .iter()
                .all(|entry| entry.seq < latest.entries[0].seq)
        );

        // An entry id that never sat at that seq must not silently splice.
        let stale = store
            .read_transcript_window(
                session_id,
                &TranscriptWindowTarget::Before {
                    cursor: TranscriptCursor {
                        seq: 0,
                        entry_id: latest.entries[0].entry_id(),
                    },
                },
                4,
                u32::MAX,
                None,
            )
            .unwrap()
            .expect("indexed session");
        assert!(stale.invalid_cursor);

        // Unchanged short-circuit.
        let unchanged = store
            .read_transcript_window(
                session_id,
                &TranscriptWindowTarget::Latest,
                4,
                u32::MAX,
                revision,
            )
            .unwrap()
            .expect("indexed session");
        assert!(unchanged.unchanged);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn continuation_cursors_visit_every_entry_once_in_both_directions() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let mut session = session_with_transcript();
        // Force byte-limited pages to split earlier than the entry ceiling,
        // while every individual message still fits in the smallest budget.
        for message in &mut session.messages {
            message.content = "response text ".repeat(150);
        }
        let id = session.id;
        let expected: Vec<_> = (0..3)
            .flat_map(|turn| {
                [
                    session.messages[turn * 2].id,
                    session.transcript_blocks[turn].activities[0].id,
                    session.messages[turn * 2 + 1].id,
                ]
            })
            .collect();
        state.push_session(session);
        store.save(&mut state).unwrap();
        for max_entries in [2, 4] {
            for max_bytes in [4096, u32::MAX] {
                let mut page = store
                    .read_transcript_window(
                        id,
                        &TranscriptWindowTarget::Latest,
                        max_entries,
                        max_bytes,
                        None,
                    )
                    .unwrap()
                    .unwrap();
                if max_entries == 4 && max_bytes == 4096 {
                    assert!(
                        page.entries.len() < 3,
                        "fixture must exercise byte trimming"
                    );
                }
                let mut reverse: Vec<_> = page
                    .entries
                    .iter()
                    .rev()
                    .map(TranscriptEntry::entry_id)
                    .collect();
                let mut reads = 0;
                while page.has_more_before {
                    reads += 1;
                    assert!(reads <= expected.len(), "backward paging must terminate");
                    page = store
                        .read_transcript_window(
                            id,
                            &TranscriptWindowTarget::Before {
                                cursor: page.previous.expect("older entries need a continuation"),
                            },
                            max_entries,
                            max_bytes,
                            None,
                        )
                        .unwrap()
                        .unwrap();
                    assert!(
                        !page.entries.is_empty(),
                        "a continuation must make progress"
                    );
                    reverse.extend(page.entries.iter().rev().map(TranscriptEntry::entry_id));
                }
                assert!(page.previous.is_none());
                reverse.reverse();
                assert_eq!(
                    reverse, expected,
                    "backward paging skipped or repeated an entry"
                );
                let mut forward: Vec<_> =
                    page.entries.iter().map(TranscriptEntry::entry_id).collect();
                reads = 0;
                while page.has_more_after {
                    reads += 1;
                    assert!(reads <= expected.len(), "forward paging must terminate");
                    page = store
                        .read_transcript_window(
                            id,
                            &TranscriptWindowTarget::After {
                                cursor: page.next.expect("newer entries need a continuation"),
                            },
                            max_entries,
                            max_bytes,
                            None,
                        )
                        .unwrap()
                        .unwrap();
                    assert!(
                        !page.entries.is_empty(),
                        "a continuation must make progress"
                    );
                    forward.extend(page.entries.iter().map(TranscriptEntry::entry_id));
                }
                assert!(page.next.is_none());
                assert_eq!(
                    forward, expected,
                    "forward paging skipped or repeated an entry"
                );
            }
        }
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn navigation_indexes_all_visible_prompts_and_reveals_an_unloaded_turn() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let mut session = session_with_transcript();
        session.messages[0].content = "large prompt ".repeat(10_000);
        session.begin_hidden_turn("Internal continuation");
        session.finish_active_turn(TurnStatus::Completed);
        let id = session.id;
        let expected: Vec<_> = session
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::User && !message.hidden)
            .map(|message| message.id)
            .collect();
        state.push_session(session);
        store.save(&mut state).unwrap();
        let latest = store
            .read_transcript_window(id, &TranscriptWindowTarget::Latest, 1, 4096, None)
            .unwrap()
            .unwrap();
        let navigation = latest.navigation.as_ref().unwrap();
        assert_eq!(
            navigation
                .iter()
                .map(|turn| turn.message_id)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            navigation
                .iter()
                .all(|turn| turn.prompt.chars().count() <= 400)
        );
        assert!(
            latest.entries.len() <= 1,
            "the index must not hydrate transcript bodies"
        );
        let target = navigation[1].message_id;
        assert!(
            latest
                .entries
                .iter()
                .all(|entry| entry.entry_id() != target)
        );
        let around = store
            .read_transcript_window(
                id,
                &TranscriptWindowTarget::Around { entry_id: target },
                4,
                4096,
                None,
            )
            .unwrap()
            .unwrap();
        assert!(
            around
                .entries
                .iter()
                .any(|entry| entry.entry_id() == target)
        );
        assert_eq!(
            around
                .navigation
                .unwrap()
                .iter()
                .map(|turn| turn.message_id)
                .collect::<Vec<_>>(),
            expected
        );
        let before = store
            .read_transcript_window(
                id,
                &TranscriptWindowTarget::Before {
                    cursor: TranscriptCursor {
                        seq: latest.entries[0].seq,
                        entry_id: latest.entries[0].entry_id(),
                    },
                },
                4,
                4096,
                None,
            )
            .unwrap()
            .unwrap();
        assert!(
            before.navigation.is_none(),
            "ordinary pages reuse the existing index"
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn payload_ranges_and_pruning() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = state.sessions[0].id;
        let output = "0123456789abcdef".repeat(64);
        state.sessions[0].begin_turn("q");
        state.sessions[0].transcript_blocks.push(TranscriptBlock {
            after_message: 1,
            turn_id: None,
            activities: vec![activity(ActivityKind::Command, "run", Some(&output))],
        });
        state.sessions[0].finish_active_turn(TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let window = store
            .read_transcript_window(
                session_id,
                &TranscriptWindowTarget::Latest,
                64,
                u32::MAX,
                None,
            )
            .unwrap()
            .expect("indexed session");
        let activity_entry = window
            .entries
            .iter()
            .find(|entry| matches!(entry.data, TranscriptEntryData::Activity { .. }))
            .expect("activity entry");
        let TranscriptEntryData::Activity { activity, payloads } = &activity_entry.data else {
            unreachable!()
        };
        assert!(activity.output.is_none(), "window keeps payloads lazy");
        let descriptor = payloads
            .iter()
            .find(|descriptor| descriptor.field == TranscriptPayloadField::Output)
            .expect("output descriptor");
        assert_eq!(descriptor.bytes, output.len() as u64);

        let page = store
            .read_transcript_payload(
                session_id,
                activity_entry.entry_id(),
                TranscriptPayloadField::Output,
                None,
                10,
                20,
            )
            .unwrap()
            .expect("payload");
        assert_eq!(page.content, &output[10..30]);
        assert_eq!(page.total_bytes, output.len() as u64);
        assert!(!page.complete);

        // Prune the archived detail: descriptors survive, payloads go away.
        let connection = store.open().unwrap();
        connection
            .execute(
                "UPDATE sessions SET archived_at = 1 WHERE id = ?1",
                params![session_id.to_string()],
            )
            .unwrap();
        store
            .prune_archived_session_details(10, 8, Uuid::nil(), &HashSet::new())
            .unwrap();
        let pruned = store
            .read_transcript_payload(
                session_id,
                activity_entry.entry_id(),
                TranscriptPayloadField::Output,
                None,
                0,
                10,
            )
            .unwrap()
            .expect("payload");
        assert!(pruned.unavailable.is_some());
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn find_scans_messages_and_tool_outputs() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = state.sessions[0].id;
        let turn_id = state.sessions[0].begin_turn("findable question");
        state.sessions[0].transcript_blocks.push(TranscriptBlock {
            after_message: 1,
            turn_id: Some(turn_id),
            activities: vec![activity(
                ActivityKind::Command,
                "run",
                Some("tool says needle-in-output"),
            )],
        });
        state.sessions[0]
            .messages
            .push(crate::model::Message::new_for_turn(
                MessageRole::Assistant,
                "assistant says needle-in-message",
                turn_id,
            ));
        state.sessions[0].finish_active_turn(TurnStatus::Completed);
        store.save(&mut state).unwrap();

        let result = store
            .find_transcript(
                session_id,
                &TranscriptFindRequest {
                    session_id,
                    query: "needle".into(),
                    scopes: Vec::new(),
                    after: None,
                    cursor: None,
                    known_revision: None,
                },
            )
            .unwrap();
        assert!(result.complete);
        assert!(result.index_complete);
        assert_eq!(result.count, 2);
        let fields: Vec<TranscriptPayloadField> = result.matches.iter().map(|m| m.field).collect();
        assert!(fields.contains(&TranscriptPayloadField::Output));
        assert!(fields.contains(&TranscriptPayloadField::Content));

        // Scoped to conversation only: the tool output disappears.
        let scoped = store
            .find_transcript(
                session_id,
                &TranscriptFindRequest {
                    session_id,
                    query: "needle".into(),
                    scopes: vec![TranscriptFindScope::Conversation],
                    after: None,
                    cursor: None,
                    known_revision: None,
                },
            )
            .unwrap();
        assert_eq!(scoped.count, 1);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn legacy_rows_convert_lazily() {
        let directory = temporary_directory();
        let store = store_in(&directory);
        let mut state = PersistedState::fresh(PathBuf::from("/tmp/project"));
        let session_id = state.sessions[0].id;
        state.sessions[0].begin_turn("legacy");
        state.sessions[0].finish_active_turn(TurnStatus::Completed);
        store.save(&mut state).unwrap();

        // Simulate a pre-index save: drop the normalized rows by hand.
        let connection = store.open().unwrap();
        for table in [
            "transcript_entries",
            "transcript_payloads",
            "transcript_turns",
            "transcript_heads",
        ] {
            connection
                .execute(
                    &format!("DELETE FROM {table} WHERE session_id = ?1"),
                    params![session_id.to_string()],
                )
                .unwrap();
        }
        drop(connection);

        assert!(
            store
                .read_transcript_window(
                    session_id,
                    &TranscriptWindowTarget::Latest,
                    4,
                    u32::MAX,
                    None,
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(store.unconverted_transcripts(10).unwrap(), vec![session_id]);
        assert!(store.convert_transcript(session_id).unwrap());
        assert!(store.unconverted_transcripts(10).unwrap().is_empty());

        let window = store
            .read_transcript_window(
                session_id,
                &TranscriptWindowTarget::Latest,
                4,
                u32::MAX,
                None,
            )
            .unwrap()
            .expect("converted session");
        assert_eq!(window.entries.len(), 1);
        std::fs::remove_dir_all(directory).ok();
    }
}
