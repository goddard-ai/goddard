//! The client half of bounded transcript windows.
//!
//! A windowed session keeps the ordinary `AgentSession` shape — renderers,
//! folds, and live driver-event merging all keep working on a materialized
//! *slice* of the flattened stream instead of the whole transcript. The
//! daemon's index gives absolute positions (`seq`, `after_message`), so a
//! page of older entries splices ahead of the slice while a page of newer
//! ones appends behind it; block anchors rebase against `message_base`, the
//! absolute index of `messages[0]`.
//!
//! `has_more_before`/`has_more_after` are the only virtuality renderers
//! must know about: a top sentinel fetches `Before`, a bottom one fetches
//! `After`, and every full-history consumer (export, provider switch, agent
//! reads) takes the complete-detail path instead of the window.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;
use waku_protocol::model::{AgentSession, AgentTurn, Message, TranscriptBlock};
use waku_protocol::transcript::{
    PayloadDescriptor, TranscriptEntryData, TranscriptPayloadField, TranscriptWindow,
};

/// How a window response merges into the view: the first snapshot replaces
/// the slice wholesale; pages extend one end of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowMerge {
    Before,
    After,
}

/// One payload field's lazy state — the descriptor the window carried plus
/// any bytes a range read already fetched.
#[derive(Clone, Debug)]
pub(super) struct LazyPayload {
    pub descriptor: PayloadDescriptor,
    /// Bytes fetched so far — may be shorter than `descriptor.bytes` while a
    /// range read is still open. `None` until the first chunk lands; message
    /// `Content`/`DisplayContent` fields seed from the inline head.
    pub content: Option<String>,
    pub loading: bool,
    /// The descriptor's revision changed or the daemon reported the field
    /// unavailable (pruned) — expansion shows the tombstone, never a stale
    /// read.
    pub unavailable: Option<String>,
}

/// Per-session materialization state for one windowed transcript.
///
/// `seq_lo..seq_hi` is the contiguous absolute range currently materialized
/// into `session.messages`/`transcript_blocks`. The session's own vectors
/// hold exactly that slice — live events keep mutating it in place.
pub(super) struct TranscriptViewState {
    pub seq_lo: i64,
    pub seq_hi: i64,
    /// Head revision the materialized slice was built from.
    pub revision: u64,
    /// Total entries the daemon retains — the scroll model's extent.
    pub entry_count: i64,
    /// Absolute index of `messages[0]` — block anchors subtract it.
    pub message_base: u32,
    /// Total messages in the authoritative transcript (not the slice).
    pub message_count: u32,
    pub has_more_before: bool,
    pub has_more_after: bool,
    /// Edge cursors for the next `Before`/`After` page.
    pub previous: Option<waku_protocol::transcript::TranscriptCursor>,
    pub next: Option<waku_protocol::transcript::TranscriptCursor>,
    /// Entry ids by `seq` — live event updates outside the materialized
    /// range are dropped, and spliced pages detect overlap by position.
    pub entry_seq: HashMap<Uuid, i64>,
    /// Lazy activity/message payload state, keyed by entry id + field.
    pub payloads: HashMap<(Uuid, TranscriptPayloadField), LazyPayload>,
    /// In-flight page reads — one per edge at a time.
    pub loading_before: bool,
    pub loading_after: bool,
    /// The last failed edge read, surfaced on its sentinel row.
    pub before_error: Option<String>,
    pub after_error: Option<String>,
}

impl TranscriptViewState {
    /// The materialized slice holds every retained entry — safe to persist
    /// as detail and for full-history consumers to read in place.
    pub fn is_complete(&self) -> bool {
        !self.has_more_before && !self.has_more_after
    }
}

/// Turn window entries into a session-shaped slice: messages and activity
/// blocks exactly as a hydrated session would present them, with
/// `after_message` rebased so `messages[0]` holds absolute index
/// `window.message_base`.
pub(super) fn materialize_session(
    session: &mut AgentSession,
    window: &TranscriptWindow,
) -> TranscriptViewState {
    let mut view = TranscriptViewState {
        seq_lo: i64::MAX,
        seq_hi: 0,
        revision: window.revision,
        entry_count: window.entry_count,
        message_base: window.message_base,
        message_count: window.message_count,
        has_more_before: window.has_more_before,
        has_more_after: window.has_more_after,
        previous: window.previous,
        next: window.next,
        entry_seq: HashMap::new(),
        payloads: HashMap::new(),
        loading_before: false,
        loading_after: false,
        before_error: None,
        after_error: None,
    };
    let (mut messages, mut blocks) = flatten_entries(&mut view, window);
    for block in &mut blocks {
        block.after_message = block
            .after_message
            .saturating_sub(window.message_base as usize);
    }
    session.messages = std::mem::take(&mut messages);
    session.transcript_blocks = blocks;
    merge_turns(&mut session.turns, &window.turns);
    session.queued_messages = window.queued_messages.clone();
    if let Some(cursor) = window.event_cursor {
        session.runtime_event_cursor = Some(cursor);
    }
    session.detail_loaded = true;
    view
}

/// Merge a `Before`/`After` page into the materialized slice. Pages carry
/// absolute anchors; the merge rebases once and tolerates up to one
/// overlapping cursor entry.
pub(super) fn splice_window(
    session: &mut AgentSession,
    view: &mut TranscriptViewState,
    window: &TranscriptWindow,
    direction: WindowMerge,
) {
    if window.invalid_cursor {
        return;
    }
    // Edge state moves even on an empty page — a terminal `Before` response
    // still drops `has_more_before` so the sentinel retires.
    view.revision = window.revision;
    view.entry_count = window.entry_count;
    view.message_count = window.message_count;
    match direction {
        WindowMerge::Before => {
            view.has_more_before = window.has_more_before;
            view.previous = window.previous;
        }
        WindowMerge::After => {
            view.has_more_after = window.has_more_after;
            view.next = window.next;
        }
    }
    if window.unchanged || window.entries.is_empty() {
        return;
    }
    merge_turns(&mut session.turns, &window.turns);

    // Live events append directly to the session vectors, so an `After`
    // page can resurface entries already materialized; skip them by id —
    // entry ids ARE message/activity ids, shared with the live path.
    let skip: HashSet<Uuid> = match direction {
        WindowMerge::After => session
            .messages
            .iter()
            .map(|message| message.id)
            .chain(
                session
                    .transcript_blocks
                    .iter()
                    .flat_map(|block| block.activities.iter().map(|item| item.id)),
            )
            .chain(view.entry_seq.keys().copied())
            .collect(),
        WindowMerge::Before => HashSet::new(),
    };

    // Entries arrive with absolute anchors; the vectors carry them until the
    // single rebase below. `scratch` collects only this page's id/payload
    // bookkeeping.
    let mut scratch = TranscriptViewState {
        seq_lo: i64::MAX,
        seq_hi: 0,
        revision: view.revision,
        entry_count: view.entry_count,
        message_base: 0,
        message_count: view.message_count,
        has_more_before: false,
        has_more_after: false,
        previous: None,
        next: None,
        entry_seq: HashMap::new(),
        payloads: HashMap::new(),
        loading_before: false,
        loading_after: false,
        before_error: None,
        after_error: None,
    };
    let entries: Vec<_> = window
        .entries
        .iter()
        .filter(|entry| !skip.contains(&entry.entry_id()))
        .cloned()
        .collect();
    if entries.is_empty() {
        return;
    }
    let filtered = TranscriptWindow {
        entries,
        ..window.clone()
    };
    let (messages, blocks) = flatten_entries(&mut scratch, &filtered);
    view.payloads.extend(scratch.payloads);
    view.entry_seq.extend(scratch.entry_seq);

    match direction {
        WindowMerge::Before => {
            let prepended = messages.len();
            let new_base = (view.message_base as usize).saturating_sub(prepended);
            session.messages.splice(0..0, messages);
            for block in &mut session.transcript_blocks {
                block.after_message += prepended;
            }
            for mut block in blocks {
                block.after_message = block.after_message.saturating_sub(new_base);
                session.transcript_blocks.push(block);
            }
            view.message_base = new_base as u32;
        }
        WindowMerge::After => {
            session.messages.extend(messages);
            for mut block in blocks {
                block.after_message = block
                    .after_message
                    .saturating_sub(view.message_base as usize);
                session.transcript_blocks.push(block);
            }
        }
    }
    // Stream order keeps anchors non-decreasing, so a stable sort restores
    // position order while keeping older seq first inside one anchor.
    session
        .transcript_blocks
        .sort_by_key(|block| block.after_message);
    merge_adjacent_blocks(&mut session.transcript_blocks);
    if let Some(first) = window.entries.first() {
        view.seq_lo = view.seq_lo.min(first.seq);
    }
    if let Some(last) = window.entries.last() {
        view.seq_hi = view.seq_hi.max(last.seq + 1);
    }
}

/// Blocks arriving at a spliced edge can share an `(after_message, turn_id)`
/// pair with an existing block — fold adjacent duplicates into one block so
/// activity ordering stays stream order.
fn merge_adjacent_blocks(blocks: &mut Vec<TranscriptBlock>) {
    let mut merged: Vec<TranscriptBlock> = Vec::with_capacity(blocks.len());
    for block in blocks.drain(..) {
        if let Some(last) = merged.last_mut()
            && last.after_message == block.after_message
            && last.turn_id == block.turn_id
        {
            last.activities.extend(block.activities);
            continue;
        }
        merged.push(block);
    }
    *blocks = merged;
}

/// Expand the raw entry list into message and block vectors. Anchors stay
/// ABSOLUTE — callers rebase once they know where the page lands.
fn flatten_entries(
    view: &mut TranscriptViewState,
    window: &TranscriptWindow,
) -> (Vec<Message>, Vec<TranscriptBlock>) {
    let mut messages = Vec::new();
    let mut blocks: Vec<TranscriptBlock> = Vec::new();
    let mut block_slots: HashMap<(u32, Option<Uuid>), usize> = HashMap::new();
    for entry in &window.entries {
        view.seq_lo = view.seq_lo.min(entry.seq);
        view.seq_hi = view.seq_hi.max(entry.seq + 1);
        view.entry_seq.insert(entry.entry_id(), entry.seq);
        match &entry.data {
            TranscriptEntryData::Message {
                message, payloads, ..
            } => {
                register_payloads(view, entry.entry_id(), payloads);
                // Text fields arrive as head clips: the inline bytes ARE the
                // stored head, so the lazy field starts loaded-at-head and a
                // continuation read appends at `content.len()`.
                for (field, inline) in [
                    (
                        TranscriptPayloadField::Content,
                        Some(message.content.clone()),
                    ),
                    (
                        TranscriptPayloadField::DisplayContent,
                        message.display_content.clone(),
                    ),
                ] {
                    if let Some(inline) = inline
                        && let Some(lazy) = view.payloads.get_mut(&(entry.entry_id(), field))
                    {
                        lazy.content = Some(inline);
                    }
                }
                messages.push((**message).clone());
            }
            TranscriptEntryData::Activity { activity, payloads } => {
                register_payloads(view, entry.entry_id(), payloads);
                let slot = block_slots
                    .entry((entry.after_message, entry.turn_id))
                    .or_insert_with(|| {
                        blocks.push(TranscriptBlock {
                            after_message: entry.after_message as usize,
                            turn_id: entry.turn_id,
                            activities: Vec::new(),
                        });
                        blocks.len() - 1
                    });
                blocks[*slot].activities.push((**activity).clone());
            }
        }
    }
    (messages, blocks)
}

fn register_payloads(
    view: &mut TranscriptViewState,
    entry_id: Uuid,
    descriptors: &[PayloadDescriptor],
) {
    for descriptor in descriptors {
        view.payloads
            .entry((entry_id, descriptor.field))
            .and_modify(|payload| {
                if payload.descriptor.revision != descriptor.revision {
                    // A newer field supersedes fetched bytes.
                    *payload = LazyPayload {
                        descriptor: descriptor.clone(),
                        content: None,
                        loading: false,
                        unavailable: (!descriptor.available)
                            .then(|| "payload was pruned".to_owned()),
                    };
                }
            })
            .or_insert_with(|| LazyPayload {
                descriptor: descriptor.clone(),
                content: None,
                loading: false,
                unavailable: (!descriptor.available).then(|| "payload was pruned".to_owned()),
            });
    }
}

/// Apply one range read to the materialized slice — an activity's bulk
/// field lands on its `ActivityItem`, a message continuation appends to the
/// clipped body. Returns `payload.complete` so callers know whether to
/// keep fetching.
pub(super) fn apply_transcript_payload(
    session: &mut AgentSession,
    view: &mut TranscriptViewState,
    payload: &waku_protocol::transcript::TranscriptPayload,
) -> bool {
    let entry_id = payload.entry_id;
    let field = payload.field;
    let Some(lazy) = view.payloads.get_mut(&(entry_id, field)) else {
        return false;
    };
    lazy.loading = false;
    if let Some(reason) = &payload.unavailable {
        lazy.unavailable = Some(reason.clone());
        return true;
    }
    if payload.stale {
        lazy.content = None;
        lazy.descriptor.revision = payload.revision;
        return true;
    }
    lazy.descriptor.bytes = payload.total_bytes;
    lazy.descriptor.revision = payload.revision;

    // Assemble by absolute offset. Text continuation fields seed from the
    // inline head so a `Next` chunk never overwrites it with padding.
    let seed = match field {
        TranscriptPayloadField::Content => session
            .messages
            .iter()
            .find(|message| message.id == entry_id)
            .map(|message| message.content.clone()),
        TranscriptPayloadField::DisplayContent => session
            .messages
            .iter()
            .find(|message| message.id == entry_id)
            .and_then(|message| message.display_content.clone()),
        _ => None,
    };
    let mut bytes = lazy
        .content
        .clone()
        .or(seed)
        .unwrap_or_default()
        .into_bytes();
    let offset = payload.offset as usize;
    let need = offset + payload.content.len();
    if offset > bytes.len() {
        bytes.resize(offset, b' ');
    }
    if need > bytes.len() {
        bytes.resize(need, 0);
    }
    bytes[offset..need].copy_from_slice(payload.content.as_bytes());
    let assembled = String::from_utf8_lossy(&bytes).into_owned();
    lazy.content = Some(assembled.clone());

    match field {
        TranscriptPayloadField::Output
        | TranscriptPayloadField::Arguments
        | TranscriptPayloadField::Detail => {
            if let Some(activity) = session
                .transcript_blocks
                .iter_mut()
                .flat_map(|block| block.activities.iter_mut())
                .find(|activity| activity.id == entry_id)
            {
                match field {
                    TranscriptPayloadField::Output => activity.output = Some(assembled),
                    TranscriptPayloadField::Arguments => activity.arguments = Some(assembled),
                    TranscriptPayloadField::Detail => activity.detail = Some(assembled),
                    _ => unreachable!(),
                }
            }
        }
        TranscriptPayloadField::Reasoning
        | TranscriptPayloadField::FileChanges
        | TranscriptPayloadField::ImageUrls => {
            if let Some(activity) = session
                .transcript_blocks
                .iter_mut()
                .flat_map(|block| block.activities.iter_mut())
                .find(|activity| activity.id == entry_id)
            {
                match field {
                    TranscriptPayloadField::Reasoning => {
                        activity.reasoning = serde_json::from_str(&assembled).ok();
                    }
                    TranscriptPayloadField::FileChanges => {
                        activity.file_changes =
                            serde_json::from_str(&assembled).unwrap_or_default();
                    }
                    TranscriptPayloadField::ImageUrls => {
                        activity.image_urls = serde_json::from_str(&assembled).unwrap_or_default();
                    }
                    _ => unreachable!(),
                }
            }
        }
        TranscriptPayloadField::Content | TranscriptPayloadField::DisplayContent => {
            if let Some(message) = session
                .messages
                .iter_mut()
                .find(|message| message.id == entry_id)
            {
                match field {
                    TranscriptPayloadField::Content => message.content = assembled,
                    TranscriptPayloadField::DisplayContent => {
                        message.display_content = Some(assembled)
                    }
                    _ => unreachable!(),
                }
            }
        }
        TranscriptPayloadField::Attachments
        | TranscriptPayloadField::Atoms
        | TranscriptPayloadField::Notice => {
            if let Some(message) = session
                .messages
                .iter_mut()
                .find(|message| message.id == entry_id)
            {
                match field {
                    TranscriptPayloadField::Attachments => {
                        message.attachments = serde_json::from_str(&assembled).unwrap_or_default();
                    }
                    TranscriptPayloadField::Atoms => {
                        message.atoms = serde_json::from_str(&assembled).unwrap_or_default();
                    }
                    TranscriptPayloadField::Notice => {
                        message.notice = serde_json::from_str(&assembled).ok();
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    payload.complete
}

/// Newer turn data replaces the stored copy; window pages add turns the
/// slice hasn't seen.
fn merge_turns(turns: &mut Vec<AgentTurn>, arriving: &[AgentTurn]) {
    let mut known: HashSet<Uuid> = turns.iter().map(|turn| turn.id).collect();
    for turn in arriving {
        if let Some(existing) = turns.iter_mut().find(|existing| existing.id == turn.id) {
            *existing = turn.clone();
        } else if known.insert(turn.id) {
            turns.push(turn.clone());
        }
    }
    // Order follows position, not insertion.
    turns.sort_by_key(|turn| turn.turn_count);
}

// ── Waku integration: paging, lazy payloads, sentinel row ────────────────────

use super::*;
use crate::ui::ActivationExt;

/// Keep one viewport ahead of the reader. At the live tail this also fills
/// short initial windows to two screens, independent of collapsed row count.
fn transcript_history_edge_is_near(rows: &ListState, future: bool) -> bool {
    let height = rows.viewport_bounds().size.height;
    if height <= Pixels::ZERO {
        return false;
    }
    // Unmeasured rows contribute no height. Require the content next to the
    // sentinel to be measured before trusting that distance, or a viewport
    // taller than the list's overdraw could drain the entire transcript.
    let edge = if future {
        rows.item_count().saturating_sub(2)
    } else {
        1.min(rows.item_count().saturating_sub(1))
    };
    if !rows
        .measured_heights(edge..edge + 1)
        .first()
        .is_some_and(Option::is_some)
    {
        return false;
    }
    let before = -rows.scroll_px_offset_for_scrollbar().y;
    let distance = if future {
        rows.max_offset_for_scrollbar().y - before
    } else {
        before
    };
    distance <= height
}

/// Vector indices change when a page is prepended. Diff pages by the content
/// they identify so the retained suffix keeps its measurements and position.
#[derive(Clone, Copy, Debug, PartialEq)]
enum HistoryPageRow {
    Message(Uuid),
    TurnBlock(Option<Uuid>, usize),
    ResponseFooter(Uuid, Uuid),
    Other(TranscriptRowKind),
}

fn history_page_rows(
    session: &AgentSession,
    message_base: u32,
    rows: &[TranscriptRowKind],
) -> Vec<HistoryPageRow> {
    rows.iter()
        .map(|row| {
            match *row {
                TranscriptRowKind::Message(index) => session
                    .messages
                    .get(index)
                    .map(|message| HistoryPageRow::Message(message.id)),
                TranscriptRowKind::TurnBlock(index) => {
                    session.transcript_blocks.get(index).map(|block| {
                        HistoryPageRow::TurnBlock(
                            block.turn_id,
                            message_base as usize + block.after_message,
                        )
                    })
                }
                TranscriptRowKind::ResponseFooter(turn, index) => session
                    .messages
                    .get(index)
                    .map(|message| HistoryPageRow::ResponseFooter(turn, message.id)),
                _ => None,
            }
            .unwrap_or(HistoryPageRow::Other(*row))
        })
        .collect()
}

fn splice_history_page_rows(
    rows: &ListState,
    previous: &[HistoryPageRow],
    next: &[HistoryPageRow],
    preserve_scroll: bool,
) {
    let mut scroll = rows.logical_scroll_top();
    // The loading row stays at the top across pages. Anchor its next content
    // row instead, retaining that row's screen position even at the hard edge.
    if matches!(
        previous.get(scroll.item_ix),
        Some(HistoryPageRow::Other(TranscriptRowKind::HistoryBoundary))
    ) {
        scroll.offset_in_item -= rows
            .measured_heights(scroll.item_ix..scroll.item_ix + 1)
            .first()
            .copied()
            .flatten()
            .unwrap_or_default();
        scroll.item_ix += 1;
    }
    let anchor = preserve_scroll
        .then(|| previous.get(scroll.item_ix))
        .flatten()
        .and_then(|key| next.iter().position(|candidate| candidate == key));
    if let Some((range, count)) = super::transcript::transcript_row_splice(previous, next) {
        rows.splice(range, count);
    }
    if let Some(item_ix) = anchor {
        rows.scroll_to(ListOffset {
            item_ix,
            offset_in_item: scroll.offset_in_item,
        });
    }
}

impl Waku {
    fn snapshot_history_page_rows(&self, session_id: Uuid) -> Option<Vec<HistoryPageRow>> {
        if self.state.selected_session != Some(session_id) {
            return None;
        }
        self.sync_transcript_rows();
        if self.transcript_is_scrolled.get() {
            self.pin_transcript_for_disclosure();
        }
        let session = self.selected_session()?;
        let view = self.transcript_views.get(&session_id)?;
        Some(history_page_rows(
            session,
            view.message_base,
            &self.transcript_row_kinds.borrow(),
        ))
    }

    fn splice_history_page(&self, previous: &[HistoryPageRow]) {
        self.refresh_transcript_row_kinds();
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(view) = self.transcript_views.get(&session.id) else {
            return;
        };
        let next = history_page_rows(
            session,
            view.message_base,
            &self.transcript_row_kinds.borrow(),
        );
        let preserve_scroll = self.transcript_is_scrolled.get();
        splice_history_page_rows(&self.transcript_rows, previous, &next, preserve_scroll);
        if self.transcript_anchor.get().is_some() {
            splice_history_page_rows(
                &self.anchored_transcript_rows,
                previous,
                &next,
                preserve_scroll,
            );
        }
    }

    /// Prefetch from measured layout, including on a tail-pinned fresh open.
    /// Failed reads wait for an explicit retry instead of looping every frame.
    pub(super) fn load_transcript_history_at_top(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        self.load_transcript_history_at_bottom(session_id, cx);
        let Some(view) = self.transcript_views.get(&session_id) else {
            return;
        };
        if !view.has_more_before || view.loading_before || view.before_error.is_some() {
            return;
        }
        if transcript_history_edge_is_near(self.active_transcript_rows(), false) {
            self.load_transcript_history_before(session_id, cx);
        }
    }

    /// A mid-transcript window (find reveal) also prefetches toward the tail.
    fn load_transcript_history_at_bottom(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(view) = self.transcript_views.get(&session_id) else {
            return;
        };
        if !view.has_more_after || view.loading_after || view.after_error.is_some() {
            return;
        }
        if transcript_history_edge_is_near(self.active_transcript_rows(), true) {
            self.load_transcript_history_after(session_id, cx);
        }
    }

    /// Fetch the page ending at the window's `previous` cursor and splice it
    /// ahead of the materialized slice, keeping the reader on the same row.
    pub(super) fn load_transcript_history_before(
        &mut self,
        session_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.transcript_views.get(&session_id) else {
            return;
        };
        if view.loading_before || !view.has_more_before {
            return;
        }
        let Some(cursor) = view.previous else {
            return;
        };
        let Some(daemon) = self.daemon_for_session(session_id) else {
            return;
        };
        if !daemon.client().transcript_windows_available() {
            return;
        }
        if let Some(view) = self.transcript_views.get_mut(&session_id) {
            view.loading_before = true;
            view.before_error = None;
        }
        cx.notify();
        let client = daemon.client();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.read_transcript_window(
                        waku_protocol::transcript::TranscriptWindowRequest {
                            session_id,
                            target: waku_protocol::transcript::TranscriptWindowTarget::Before {
                                cursor,
                            },
                            max_entries: None,
                            max_bytes: None,
                            known_revision: None,
                        },
                    )
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let previous = waku.snapshot_history_page_rows(session_id);
                if let Some(view) = waku.transcript_views.get_mut(&session_id) {
                    view.loading_before = false;
                }
                match result {
                    Ok(window) => {
                        if let Some(session) = waku.state.session_mut(session_id)
                            && let Some(view) = waku.transcript_views.get_mut(&session_id)
                        {
                            splice_window(session, view, &window, WindowMerge::Before);
                        }
                        if let Some(previous) = previous {
                            waku.splice_history_page(&previous);
                        }
                    }
                    Err(error) => {
                        if let Some(view) = waku.transcript_views.get_mut(&session_id) {
                            view.before_error = Some(format!("{error:#}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch the page starting at the window's `next` cursor and splice it
    /// behind the materialized slice — the direction a mid-transcript view
    /// (find reveal) fills toward the live tail.
    fn load_transcript_history_after(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(view) = self.transcript_views.get(&session_id) else {
            return;
        };
        if view.loading_after || !view.has_more_after {
            return;
        }
        let Some(cursor) = view.next else {
            return;
        };
        let Some(daemon) = self.daemon_for_session(session_id) else {
            return;
        };
        let client = daemon.client();
        if !client.transcript_windows_available() {
            return;
        }
        if let Some(view) = self.transcript_views.get_mut(&session_id) {
            view.loading_after = true;
            view.after_error = None;
        }
        cx.notify();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.read_transcript_window(
                        waku_protocol::transcript::TranscriptWindowRequest {
                            session_id,
                            target: waku_protocol::transcript::TranscriptWindowTarget::After {
                                cursor,
                            },
                            max_entries: None,
                            max_bytes: None,
                            known_revision: None,
                        },
                    )
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let previous = waku.snapshot_history_page_rows(session_id);
                if let Some(view) = waku.transcript_views.get_mut(&session_id) {
                    view.loading_after = false;
                }
                match result {
                    Ok(window) => {
                        if let Some(session) = waku.state.session_mut(session_id)
                            && let Some(view) = waku.transcript_views.get_mut(&session_id)
                        {
                            splice_window(session, view, &window, WindowMerge::After);
                        }
                        if let Some(previous) = previous {
                            waku.splice_history_page(&previous);
                        }
                    }
                    Err(error) => {
                        if let Some(view) = waku.transcript_views.get_mut(&session_id) {
                            view.after_error = Some(format!("{error:#}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Window edges load automatically; only a failed read needs a control.
    pub(super) fn render_transcript_history_boundary(
        &self,
        session_id: Uuid,
        future: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (loading, error) = self
            .transcript_views
            .get(&session_id)
            .map(|view| {
                if future {
                    (view.loading_after, view.after_error.clone())
                } else {
                    (view.loading_before, view.before_error.clone())
                }
            })
            .unwrap_or_default();
        let label = if error.is_some() {
            tr!("transcript.history_retry")
        } else if future {
            tr!("transcript.future_loading")
        } else {
            tr!("transcript.history_loading")
        };
        let theme = Theme::current(cx);
        let element_id = if future {
            "transcript-future-history"
        } else {
            "transcript-history"
        };
        let focus = self.transcript_control_focus(element_id.to_owned(), cx);
        div()
            .w_full()
            .py(px(12.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .when(error.is_none(), |view| {
                view.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .when(loading, |row| {
                            row.child(motion::spin_slow(icon(
                                "icons/loader-circle.svg",
                                12.0,
                                theme.text_tertiary,
                            )))
                        })
                        .child(label.clone()),
                )
            })
            .when(error.is_some(), |view| {
                view.child(
                    div()
                        .id(element_id)
                        .track_focus(&focus)
                        .tab_index(0)
                        .rounded(px(6.0))
                        .px(px(12.0))
                        .py(px(6.0))
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .focus_visible(|style| style.bg(theme.focus_highlight()))
                        .hover(|style| style.bg(theme.overlay))
                        .child(label)
                        .on_activation(cx, move |this, _, cx| {
                            if loading {
                                return;
                            }
                            if future {
                                this.load_transcript_history_after(session_id, cx);
                            } else {
                                this.load_transcript_history_before(session_id, cx);
                            }
                        }),
                )
            })
            .children(error.map(|error| {
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(error)
            }))
            .into_any_element()
    }

    /// A windowed session whose slice does not cover the retained transcript.
    /// Full-history consumers (export, provider switch, agent reads) must
    /// operate on a detached full read, never this slice.
    pub(super) fn session_transcript_is_partial(&self, session_id: Uuid) -> bool {
        self.transcript_views
            .get(&session_id)
            .is_some_and(|view| !view.is_complete())
    }

    /// Fetch one lazy payload field's next range and apply it to the
    /// materialized slice. Called from expansion affordances; synthesized
    /// descriptors cover compact live events the index never described.
    pub(super) fn ensure_transcript_payload(
        &mut self,
        session_id: Uuid,
        entry_id: Uuid,
        field: TranscriptPayloadField,
        cx: &mut Context<Self>,
    ) {
        let Some(daemon) = self.daemon_for_session(session_id) else {
            return;
        };
        let client = daemon.client();
        if !client.transcript_windows_available() {
            return;
        }
        let Some(view) = self.transcript_views.get_mut(&session_id) else {
            return;
        };
        let key = (entry_id, field);
        let lazy = view.payloads.entry(key).or_insert_with(|| LazyPayload {
            descriptor: PayloadDescriptor {
                field,
                revision: 0,
                bytes: u64::MAX,
                truncated: false,
                available: true,
            },
            content: None,
            loading: false,
            unavailable: None,
        });
        if lazy.loading
            || lazy.unavailable.is_some()
            || lazy
                .content
                .as_ref()
                .is_some_and(|content| content.len() as u64 >= lazy.descriptor.bytes)
        {
            return;
        }
        lazy.loading = true;
        let offset = lazy.content.as_ref().map_or(0, |c| c.len() as u64);
        let expected_revision = (lazy.descriptor.revision != 0).then_some(lazy.descriptor.revision);
        let remaining = lazy.descriptor.bytes.saturating_sub(offset);
        let limit = remaining.min(1 << 20).max(1);
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Chained range reads assemble the whole field: the
                    // daemon may cap below `limit`, so keep reading while
                    // chunks land and `complete` stays false.
                    let mut request = waku_protocol::transcript::TranscriptPayloadRequest {
                        session_id,
                        entry_id,
                        field,
                        expected_revision,
                        offset,
                        limit,
                    };
                    let mut assembled: Option<waku_protocol::transcript::TranscriptPayload> = None;
                    loop {
                        let chunk = client.read_transcript_payload(request.clone())?;
                        let done = chunk.complete
                            || chunk.unavailable.is_some()
                            || chunk.stale
                            || chunk.content.is_empty();
                        request.offset = chunk.offset + chunk.content.len() as u64;
                        match &mut assembled {
                            None => assembled = Some(chunk),
                            Some(acc) => {
                                acc.complete = chunk.complete;
                                acc.total_bytes = chunk.total_bytes;
                                acc.content.push_str(&chunk.content);
                            }
                        }
                        if done {
                            break;
                        }
                    }
                    Ok::<_, anyhow::Error>(assembled.unwrap())
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                match result {
                    Ok(payload) => {
                        if let Some(session) = waku.state.session_mut(session_id)
                            && let Some(view) = waku.transcript_views.get_mut(&session_id)
                        {
                            apply_transcript_payload(session, view, &payload);
                        }
                    }
                    Err(_) => {
                        if let Some(view) = waku.transcript_views.get_mut(&session_id)
                            && let Some(lazy) = view.payloads.get_mut(&key)
                        {
                            lazy.loading = false;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waku_protocol::model::{ActivityKind, MessageRole, ProviderKind};
    use waku_protocol::transcript::{TranscriptCursor, TranscriptEntry};

    struct HistoryViewportHarness {
        rows: ListState,
        height: Pixels,
    }

    impl Render for HistoryViewportHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.0)).h(self.height).child(
                list(self.rows.clone(), |_, _, _| {
                    div().w_full().h(px(100.0)).into_any_element()
                })
                .size_full(),
            )
        }
    }

    #[gpui::test]
    fn history_prefetch_keeps_two_screens_and_tracks_the_viewport(cx: &mut gpui::TestAppContext) {
        let rows = ListState::new(5, ListAlignment::Bottom, px(2048.0))
            .with_uniform_item_height(px(100.0));
        assert!(!transcript_history_edge_is_near(&rows, false));
        let (view, cx) = cx.add_window_view(move |_, _| HistoryViewportHarness {
            rows,
            height: px(300.0),
        });
        cx.run_until_parked();
        cx.read(|cx| {
            // Five rows are less than two 300px screens, even while tail-pinned.
            assert!(transcript_history_edge_is_near(&view.read(cx).rows, false));
        });
        view.update(cx, |view, cx| {
            view.rows.reset(20);
            view.rows.scroll_to_end();
            cx.notify();
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let rows = &view.read(cx).rows;
            assert!(!transcript_history_edge_is_near(rows, false));
            assert!(transcript_history_edge_is_near(rows, true));
        });
        view.update(cx, |view, cx| {
            view.rows.scroll_to(ListOffset {
                item_ix: 4,
                offset_in_item: px(0.0),
            });
            cx.notify();
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let rows = &view.read(cx).rows;
            assert!(!transcript_history_edge_is_near(rows, false));
            assert!(!transcript_history_edge_is_near(rows, true));
        });
        view.update(cx, |view, cx| {
            view.height = px(500.0);
            cx.notify();
        });
        cx.run_until_parked();
        cx.read(|cx| {
            // Resizing makes the same 400px distance fall inside one screen.
            assert!(transcript_history_edge_is_near(&view.read(cx).rows, false));
        });
        view.update(cx, |view, cx| {
            view.height = px(3000.0);
            view.rows.reset(100);
            view.rows.scroll_to_end();
            cx.notify();
        });
        cx.run_until_parked();
        cx.read(|cx| {
            // A viewport taller than overdraw must not mistake unmeasured
            // history for an empty initial window and fetch indefinitely.
            assert!(!transcript_history_edge_is_near(&view.read(cx).rows, false));
        });
    }

    #[gpui::test]
    fn prepending_history_preserves_visible_content_and_stops_prefetch(
        cx: &mut gpui::TestAppContext,
    ) {
        for (alignment, scroll_row, more_before) in [
            (ListAlignment::Top, 3, true),
            (ListAlignment::Top, 0, true),
            (ListAlignment::Bottom, 3, false),
            (ListAlignment::Bottom, 0, false),
        ] {
            let mut session = session();
            let entries = |start: i64| {
                (start..start + 20)
                    .flat_map(|index| {
                        [
                            message_entry(index * 2, &format!("Message {index}"), index as u32),
                            activity_entry(
                                index * 2 + 1,
                                index as u32 + 1,
                                index as u32,
                                0,
                                "Command",
                            ),
                        ]
                    })
                    .collect()
            };
            let latest = window(session.id, entries(40), 40, true, false);
            let mut state = materialize_session(&mut session, &latest);
            let kinds = |session: &AgentSession, state: &TranscriptViewState| {
                let mut kinds = super::super::transcript::folded_transcript_row_kinds(
                    session,
                    &HashSet::new(),
                    None,
                );
                if state.has_more_before {
                    kinds.insert(0, TranscriptRowKind::HistoryBoundary);
                }
                kinds
            };
            let previous =
                history_page_rows(&session, state.message_base, &kinds(&session, &state));
            let (view, cx) = cx.add_window_view(|_, _| HistoryViewportHarness {
                rows: ListState::new(previous.len(), alignment, px(2048.0)),
                height: px(400.0),
            });
            view.update(cx, |view, cx| {
                view.rows.scroll_to(ListOffset {
                    item_ix: scroll_row,
                    offset_in_item: px(17.0),
                });
                cx.notify();
            });
            cx.run_until_parked();
            let anchor_index = scroll_row.max(1);
            let anchor = previous[anchor_index];
            let before_y = cx.read(|cx| {
                let rows = &view.read(cx).rows;
                assert!(transcript_history_edge_is_near(rows, false));
                rows.bounds_for_item(anchor_index).unwrap().top()
            });
            let page = window(session.id, entries(20), 20, more_before, false);
            splice_window(&mut session, &mut state, &page, WindowMerge::Before);
            let next = history_page_rows(&session, state.message_base, &kinds(&session, &state));
            let new_anchor_index = next.iter().position(|key| *key == anchor).unwrap();
            view.update(cx, |view, cx| {
                splice_history_page_rows(&view.rows, &previous, &next, true);
                cx.notify();
            });
            cx.run_until_parked();
            cx.read(|cx| {
                let rows = &view.read(cx).rows;
                assert_eq!(
                    rows.bounds_for_item(new_anchor_index).unwrap().top(),
                    before_y,
                    "the same content must stay at the same screen position"
                );
                assert!(
                    !transcript_history_edge_is_near(rows, false),
                    "a prepended page must move the loading edge away from the reader"
                );
            });
        }
    }

    fn message_entry(seq: i64, content: &str, after_message: u32) -> TranscriptEntry {
        TranscriptEntry {
            seq,
            revision: 1,
            turn_id: None,
            after_message,
            block_index: None,
            activity_index: None,
            data: TranscriptEntryData::Message {
                message: Box::new(Message::new(MessageRole::Assistant, content.to_owned())),
                content_window: None,
                payloads: Vec::new(),
            },
        }
    }

    fn activity_entry(
        seq: i64,
        after_message: u32,
        block_index: u32,
        activity_index: u32,
        title: &str,
    ) -> TranscriptEntry {
        TranscriptEntry {
            seq,
            revision: 1,
            turn_id: None,
            after_message,
            block_index: Some(block_index),
            activity_index: Some(activity_index),
            data: TranscriptEntryData::Activity {
                activity: Box::new(waku_protocol::model::ActivityItem::new(
                    None,
                    ActivityKind::Command,
                    title,
                    None,
                    true,
                )),
                payloads: Vec::new(),
            },
        }
    }

    fn window(
        session_id: Uuid,
        entries: Vec<TranscriptEntry>,
        message_base: u32,
        has_more_before: bool,
        has_more_after: bool,
    ) -> TranscriptWindow {
        TranscriptWindow {
            session_id,
            revision: 1,
            unchanged: false,
            invalid_cursor: false,
            entries,
            previous: None,
            next: None,
            has_more_before,
            has_more_after,
            message_base,
            message_count: 0,
            entry_count: 0,
            converted: true,
            legacy_read: false,
            turns: Vec::new(),
            queued_messages: Vec::new(),
            event_cursor: None,
        }
    }

    fn session() -> AgentSession {
        AgentSession::new(Uuid::new_v4(), ProviderKind::Codex)
    }

    #[test]
    fn materialize_rebases_anchors_and_registers_entries() {
        let session_id = Uuid::new_v4();
        let entries = vec![
            message_entry(10, "latest prompt", 0),
            activity_entry(11, 0, 0, 0, "ls"),
            message_entry(12, "latest answer", 1),
        ];
        // The daemon reports absolute anchors: the prompt is absolute
        // message index 5, so `after_message` values carry that base.
        let mut entries = entries;
        entries[0].after_message = 5;
        entries[1].after_message = 5;
        entries[2].after_message = 6;
        let mut window = window(session_id, entries, 5, true, false);
        window.previous = Some(TranscriptCursor {
            seq: 9,
            entry_id: Uuid::new_v4(),
        });
        window.message_count = 7;
        window.entry_count = 20;

        let mut session = session();
        let view = materialize_session(&mut session, &window);

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].content, "latest prompt");
        assert_eq!(session.transcript_blocks.len(), 1);
        // Absolute anchor 5 minus base 5 — the block anchors at slice index 0.
        assert_eq!(session.transcript_blocks[0].after_message, 0);
        assert_eq!(view.message_base, 5);
        assert!(view.has_more_before);
        assert!(!view.has_more_after);
        assert_eq!(view.entry_seq.len(), 3);
        assert_eq!(view.seq_lo, 10);
        assert_eq!(view.seq_hi, 13);
    }

    #[test]
    fn splice_before_prepends_and_rebases_blocks() {
        let session_id = Uuid::new_v4();
        let mut session = session();
        let mut latest = window(
            session_id,
            vec![
                message_entry(10, "new", 5),
                activity_entry(11, 5, 0, 0, "ls"),
            ],
            5,
            true,
            false,
        );
        latest.previous = Some(TranscriptCursor {
            seq: 9,
            entry_id: Uuid::new_v4(),
        });
        let mut view = materialize_session(&mut session, &latest);

        let older = window(
            session_id,
            vec![
                message_entry(4, "older a", 3),
                message_entry(5, "older b", 4),
                activity_entry(6, 4, 1, 0, "older work"),
            ],
            3,
            false,
            true,
        );
        splice_window(&mut session, &mut view, &older, WindowMerge::Before);

        assert_eq!(
            session
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["older a", "older b", "new"]
        );
        // The existing block shifted by the two prepended messages; the new
        // block rebased from absolute 4 to slice index 1.
        let mut anchors: Vec<usize> = session
            .transcript_blocks
            .iter()
            .map(|block| block.after_message)
            .collect();
        anchors.sort_unstable();
        assert_eq!(anchors, vec![1, 2]);
        assert_eq!(view.message_base, 3);
        assert!(!view.has_more_before);
        // A Before page's `has_more_after` describes the page, not the view —
        // the view's tail state is untouched by prepends.
        assert!(!view.has_more_after);
        assert_eq!(view.seq_lo, 4);
    }

    #[test]
    fn splice_after_skips_already_materialized_ids() {
        let session_id = Uuid::new_v4();
        let mut session = session();
        let latest = window(
            session_id,
            vec![message_entry(10, "only", 5)],
            5,
            false,
            true,
        );
        let mut view = materialize_session(&mut session, &latest);
        let existing = session.messages[0].id;

        // A live append lands outside the index bookkeeping — the After
        // page re-serves it and must not duplicate it.
        let live = Message::new(MessageRole::Assistant, "live".to_owned());
        let live_id = live.id;
        session.messages.push(live.clone());

        let page = window(
            session_id,
            vec![
                message_entry(11, "duplicate", 6),
                // This entry carries the live id — dedupe drops it.
                {
                    let mut entry = message_entry(12, "live", 7);
                    if let TranscriptEntryData::Message { message, .. } = &mut entry.data {
                        message.id = live_id;
                    }
                    entry
                },
            ],
            6,
            false,
            false,
        );
        splice_window(&mut session, &mut view, &page, WindowMerge::After);

        assert_eq!(session.messages.len(), 3);
        assert_eq!(
            session
                .messages
                .iter()
                .filter(|message| message.id == live_id)
                .count(),
            1
        );
        let _ = existing;
        assert!(!view.has_more_after);
    }

    #[test]
    fn empty_before_page_still_retires_the_sentinel() {
        let session_id = Uuid::new_v4();
        let mut session = session();
        let latest = window(
            session_id,
            vec![message_entry(10, "only", 5)],
            5,
            true,
            false,
        );
        let mut view = materialize_session(&mut session, &latest);
        let terminal = window(session_id, Vec::new(), 5, false, false);
        splice_window(&mut session, &mut view, &terminal, WindowMerge::Before);
        assert!(!view.has_more_before);
        assert_eq!(session.messages.len(), 1);
    }
}
