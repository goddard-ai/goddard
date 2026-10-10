//! Inline feedback on Boss memory records: notes pinned on a whole record
//! or an exact passage, reviewed together, and submitted to the Boss through
//! the ordinary chat composer.
//!
//! The Memory surface — the unified records feed this module serves, and
//! the interim per-bucket detail that hosts it today — owns only browsing:
//! rows, search, filters, and pagination. This module owns everything about
//! a *note*: creation, the one floating editor, the pending batch, review,
//! and folding the batch into a Boss-bound submission.
//!
//! # Feed integration API
//!
//! A record row integrates by passing a [`MemoryRecordRef`] — the record's
//! stable identity, provenance, and current text — to these pieces:
//!
//! - [`memory_row_key`] — the `MarkdownCtx::new` row argument so the row's
//!   painted text registers for selection under this record's stable key.
//! - [`Waku::memory_feedback_selection`] — the `TranscriptSelection` the
//!   ctx takes, the scroll frame's `md::render::frame_reset`, and the input
//!   overlay's `md::render::install_selection_input` plus
//!   [`Waku::install_memory_annotation_input`].
//! - [`Waku::render_memory_record_annotate_button`] — the per-row "Add
//!   note" affordance (whole-memory targeting).
//! - [`Waku::render_memory_record_notes`] — the record's pending notes,
//!   rendered beneath its row.
//! - [`Waku::render_memory_annotation_offer`] — the deferred "Add
//!   annotation" pill over a settled passage selection.
//! - [`Waku::render_memory_annotation_editor`] and
//!   [`Waku::render_memory_annotation_tooltip`] — the deferred floating
//!   editor and highlight tooltip, mounted over the feed area.
//! - [`Waku::render_memory_feedback_supplement`] — batch count, Review,
//!   and the ordinary composer, mounted beneath the feed.
//! - [`Waku::memory_feedback_hidden_count`] — how many pending notes the
//!   feed's current rows cannot show, for the count's filter warning.
//! - [`Waku::consume_memory_feedback_locate`] — a Review "Show memory"
//!   request the feed honors (scrolling, clearing a filter) then clears.
//!
//! Pending notes key on [`MemoryAnnotationKey`] — daemon, bucket, record
//! reference — never a row index, so recycling, scrolling, and filtering
//! cannot misattribute them. Submissions ride the existing Boss delivery
//! semantics: an armed `BossCommandContext::MemoryFeedback` retargets the
//! composer's next message at the Boss chat and folds the batch into the
//! ordinary annotation pipeline (`submission.annotations`), so numbering,
//! queueing, failure restore, and `"Annotation N"` citations in the reply
//! behave exactly like transcript annotations. The send always asks the
//! Boss to *consider* the feedback — it never claims memory changed.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, HitboxId, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, WeakEntity, deferred, div, px,
};

use waku_client::DaemonKey;

use crate::input::Clear;
use crate::md::render::{MarkdownView, TranscriptSelection, text_range_bounds};
use crate::md::selection::{MemoryNoteStatus, MemorySource, Span, TranscriptAnnotation};
use crate::ui::ActivationExt;
use crate::ui::menu::{DismissMenu, FloatingSurface, MenuAlign};

use super::annotations::annotation_quote_preview;
use super::boss::{BossTab, MemoryFeedProject, boss_button, memory_record_age};
use super::*;

/// Key context the note editor card declares, so Escape reaches it as an
/// action whether the field or the card's controls hold focus — the same
/// convention the transcript's annotation card follows.
const MEMORY_NOTE_CONTEXT: &str = "MemoryNote";

/// Bind the editor card's own keys. Without this, Escape under the card —
/// focus on a button rather than the field — would fall through to the
/// root `Workspace` context's `CancelTurn` and kill a running turn.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        DismissMenu,
        Some(MEMORY_NOTE_CONTEXT),
    )]);
}

/// Hover delay before a pinned note's tooltip appears, matching the
/// transcript annotation's settle.
const MEMORY_NOTE_HOVER_DELAY: Duration = Duration::from_millis(400);

/// The selection row prefix every memory record's text registers under —
/// `memory-record-{daemon}:{bucket}:{reference}` keeps selection spans and
/// pinned highlights bound to the record's stable identity, never a row
/// index.
const MEMORY_ROW_PREFIX: &str = "memory-record-";

/// Stable identity of one memory record — what pending feedback keys on.
///
/// `bucket` is the engine's bucket id and `reference` the record's own
/// (`note-<seq>` for an original note, `summary-<start>-<end>` for a stored
/// summary): provenance the Boss can resolve back to the record, and never
/// a visible label or a row position.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct MemoryAnnotationKey {
    /// The daemon whose Boss owns the record.
    pub daemon: DaemonKey,
    pub bucket: String,
    pub reference: String,
}

/// `DaemonKey` inside [`MemorySource`], whose crate cannot name the type:
/// `Some(host id)` for a remote daemon, `None` for local.
fn daemon_uuid(daemon: DaemonKey) -> Option<Uuid> {
    match daemon {
        DaemonKey::Local => None,
        DaemonKey::Remote(host) => Some(host),
    }
}

/// The inverse of [`daemon_uuid`].
fn memory_daemon(uuid: Option<Uuid>) -> DaemonKey {
    uuid.map_or(DaemonKey::Local, DaemonKey::Remote)
}

/// The row key a record's painted text registers under — the `row` argument
/// to `MarkdownCtx::new` for the record's text.
pub(super) fn memory_row_key(key: &MemoryAnnotationKey) -> Rc<str> {
    let daemon = match key.daemon {
        DaemonKey::Local => "local".to_owned(),
        DaemonKey::Remote(host) => format!("remote-{host}"),
    };
    Rc::from(format!(
        "{MEMORY_ROW_PREFIX}{daemon}:{}:{}",
        key.bucket, key.reference
    ))
}

/// The record a selection row belongs to — the inverse of
/// [`memory_row_key`]. `None` for rows outside the Memory surface.
fn memory_key_from_row(row: &str) -> Option<MemoryAnnotationKey> {
    let rest = row.strip_prefix(MEMORY_ROW_PREFIX)?;
    let mut parts = rest.splitn(3, ':');
    let daemon = match parts.next()? {
        "local" => DaemonKey::Local,
        remote => DaemonKey::Remote(Uuid::parse_str(remote.strip_prefix("remote-")?).ok()?),
    };
    Some(MemoryAnnotationKey {
        daemon,
        bucket: parts.next()?.to_owned(),
        reference: parts.next()?.to_owned(),
    })
}

/// Element-id-safe key path — bucket id and record id, no display names.
fn memory_key_path(key: &MemoryAnnotationKey) -> String {
    format!("{}:{}", key.bucket, key.reference)
}

/// One memory record as the feed presents it: stable identity, provenance,
/// and current text. Feed rows hand this to the feedback API; pin-time
/// snapshots inside pending annotations come from `text` then.
#[derive(Clone)]
pub(super) struct MemoryRecordRef {
    /// `key.bucket` is the feed's `bucketId`; `key.reference` its `id`.
    pub key: MemoryAnnotationKey,
    /// The note's sequence — the `note-<seq>` display reference.
    pub sequence: u64,
    /// The bucket's display name — the `goddard` in `goddard · 5m`.
    pub bucket: String,
    /// The verified project association, when the feed resolved one —
    /// `None` means a confirmed "No project", never an unknown one.
    pub project: Option<MemoryFeedProject>,
    /// The original note's creation time.
    pub created_at: u64,
    /// The record's displayed text.
    pub text: Rc<str>,
}

impl MemoryRecordRef {
    /// The provenance snapshot a new annotation carries.
    fn source(&self) -> MemorySource {
        MemorySource {
            daemon: daemon_uuid(self.key.daemon),
            bucket_id: self.key.bucket.clone(),
            record_id: self.key.reference.clone(),
            bucket: self.bucket.clone(),
            sequence: self.sequence,
            project: self.project.as_ref().map(|project| {
                crate::md::selection::MemoryProjectSource {
                    key: project.key.clone(),
                    name: project.name.clone(),
                }
            }),
            created_at: self.created_at,
            record_text: self.text.to_string(),
            status: None,
        }
    }
}

/// The open note editor. `is_new` marks a note Enter/Escape has not yet
/// saved; discarding it removes the pin too.
#[derive(Clone, Debug)]
pub(super) struct MemoryNoteEditor {
    pub annotation_id: u64,
    pub is_new: bool,
    /// Focus to hand back when the editor closes — the control or note the
    /// user came from.
    pub previous_focus: Option<FocusHandle>,
}

/// Mouse-down on a pinned passage highlight, held until mouse-up proves it
/// was a click (selection stayed empty) rather than the start of a drag.
#[derive(Clone, Debug)]
struct MemoryAnnotationPress {
    id: u64,
    position: Point<Pixels>,
}

/// The pinned note under the pointer; `visible` flips on after
/// [`MEMORY_NOTE_HOVER_DELAY`] so a passing cursor does not flash the card.
#[derive(Clone, Debug)]
struct MemoryAnnotationHover {
    id: u64,
    visible: bool,
}

/// Everything pending feedback owns — kept apart from the transcript's
/// annotation state so the two never cross-talk.
#[derive(Default)]
pub(super) struct MemoryFeedback {
    /// The selection surface memory rows paint into. Its `annotations`
    /// store is the pending batch itself: passage pins paint highlights
    /// straight out of it, and `Rc<RefCell>` is how the renderer reaches
    /// them. Notes key by [`MemoryAnnotationKey`], in creation order.
    pub selection: TranscriptSelection,
    /// The one open note editor, wherever it is anchored.
    pub editor: Option<MemoryNoteEditor>,
    /// Highlight under the pointer; `visible` once the hover delay elapsed.
    hover: Option<MemoryAnnotationHover>,
    /// A mouse-down on a highlight, pending its mouse-up.
    press: Option<MemoryAnnotationPress>,
    /// Review open — every pending note grouped by memory.
    pub review_open: bool,
    /// The composer supplement's visibility latch: revealed by the first
    /// pending note, hidden again only once the batch is empty and the
    /// composer holds no text either.
    composer_revealed: bool,
    /// A Review "Show memory" request the feed consumes while laying out.
    locate: Option<MemoryAnnotationKey>,
    /// Markdown views for the interim detail's record rows — one per
    /// record so the flatten cache survives between frames.
    pub record_views: HashMap<MemoryAnnotationKey, MarkdownView>,
}

impl MemoryFeedback {
    /// The key every pending annotation's `memory` carries — the batch
    /// holds nothing else.
    fn key_of(annotation: &TranscriptAnnotation) -> Option<MemoryAnnotationKey> {
        let memory = annotation.memory.as_ref()?;
        Some(MemoryAnnotationKey {
            daemon: memory_daemon(memory.daemon),
            bucket: memory.bucket_id.clone(),
            reference: memory.record_id.clone(),
        })
    }
}

impl Waku {
    /// The `TranscriptSelection` a memory feed hands to `MarkdownCtx::new`
    /// and its per-frame `frame_reset`/input overlay.
    pub(super) fn memory_feedback_selection(&self) -> TranscriptSelection {
        self.memory_feedback.selection.clone()
    }

    /// Pending notes pinned on `key`, in creation order.
    fn memory_notes_for(&self, key: &MemoryAnnotationKey) -> Vec<TranscriptAnnotation> {
        let mut notes: Vec<_> = self
            .memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .iter()
            .filter(|annotation| MemoryFeedback::key_of(annotation).as_ref() == Some(key))
            .cloned()
            .collect();
        notes.sort_by_key(|annotation| annotation.id);
        notes
    }

    /// Pending notes whose daemon is `daemon`, in creation order.
    fn memory_notes_for_daemon(&self, daemon: DaemonKey) -> Vec<TranscriptAnnotation> {
        let mut notes: Vec<_> = self
            .memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .iter()
            .filter(|annotation| {
                MemoryFeedback::key_of(annotation).is_some_and(|key| key.daemon == daemon)
            })
            .cloned()
            .collect();
        notes.sort_by_key(|annotation| annotation.id);
        notes
    }

    /// How many pending notes the feed's current rows cannot show — the
    /// filter warning beside the batch count. `visible` answers whether a
    /// record's row is rendered right now; the feed supplies it because it
    /// owns search and filters.
    pub(super) fn memory_feedback_hidden_count(
        &self,
        visible: impl Fn(&MemoryAnnotationKey) -> bool,
    ) -> usize {
        self.memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .iter()
            .filter(|annotation| {
                MemoryFeedback::key_of(annotation)
                    .as_ref()
                    .is_some_and(|key| !visible(key))
            })
            .count()
    }

    /// The pending batch's daemon while its feedback composer is live: the
    /// open Memory page's boss, or the boss chat on screen. `None` when no
    /// notes wait or the surface showing has no boss to answer to — the
    /// submission then falls through to the selected session.
    pub(super) fn memory_feedback_armed_daemon(&self) -> Option<DaemonKey> {
        if !self.state.boss_experiment_enabled {
            return None;
        }
        if self
            .memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .is_empty()
        {
            return None;
        }
        let daemon = match self.boss_ui.page {
            Some((key, BossTab::Memory)) => key,
            _ => self.boss_chat_key()?,
        };
        (!self.memory_notes_for_daemon(daemon).is_empty()).then_some(daemon)
    }

    /// The record's current text from the loaded feed — `None` when the
    /// feed has not loaded or no longer lists the record (removed, or no
    /// longer accessible under the caller's grants).
    fn current_memory_text(&self, key: &MemoryAnnotationKey) -> Option<Rc<str>> {
        let feed = self.boss_ui.memory_feed.get(&key.daemon)?;
        feed.records
            .iter()
            .find(|record| record.bucket_id == key.bucket && record.id == key.reference)
            .map(|record| Rc::from(record.text.as_str()))
    }

    /// Refresh every pending note's changed/unavailable status against the
    /// loaded records. An unloaded bucket leaves the status as it was — its
    /// absence proves nothing about the record.
    pub(super) fn refresh_memory_note_status(&mut self) {
        let mut annotations = self.memory_feedback.selection.annotations.borrow_mut();
        for annotation in &mut annotations.items {
            let Some(memory) = annotation.memory.as_mut() else {
                continue;
            };
            let key = MemoryAnnotationKey {
                daemon: memory_daemon(memory.daemon),
                bucket: memory.bucket_id.clone(),
                reference: memory.record_id.clone(),
            };
            if !self
                .boss_ui
                .memory_feed
                .get(&key.daemon)
                .is_some_and(|feed| feed.loaded)
            {
                continue;
            }
            memory.status = match self.current_memory_text(&key) {
                Some(text) if text.as_ref() == memory.record_text => None,
                Some(_) => Some(MemoryNoteStatus::Changed),
                None => Some(MemoryNoteStatus::Unavailable),
            };
        }
    }

    /// The settled selection's spans when they sit entirely inside one
    /// memory record — the only selection "Add annotation" may pin.
    /// Cross-row grabs fail the same-row check rather than silently
    /// targeting the first row, and selections in metadata never reach a
    /// record's registered text.
    fn annotatable_memory_selection(&self) -> Option<(MemoryAnnotationKey, Vec<Span>)> {
        let selection = self.memory_feedback.selection.selection.borrow();
        if selection.is_dragging() || selection.is_empty() {
            return None;
        }
        let spans = selection.spans();
        let first_row = spans.first()?.key.row.clone();
        if !spans.iter().all(|span| span.key.row == first_row) {
            return None;
        }
        let key = memory_key_from_row(&first_row)?;
        Some((key, spans.to_vec()))
    }

    /// Create a pending note on a settled memory-text selection and open
    /// the editor on it.
    fn annotate_memory_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((key, spans)) = self.annotatable_memory_selection() else {
            return;
        };
        // Commit an open editor's nonblank comment before the new pin
        // opens — clicking another memory's action saves first.
        self.commit_memory_note_editor(cx);
        let id = self.annotation_next_id;
        self.annotation_next_id += 1;
        let record = self.memory_record_ref(&key);
        // When the record's row text is all the snapshot there is — the
        // bucket unloaded between paint and pin — the spans' own text is
        // the context the submission carries.
        let record = if record.text.is_empty() {
            let joined = spans
                .iter()
                .map(|span| span.text[span.range.clone()].to_owned())
                .collect::<Vec<_>>()
                .join("\n");
            MemoryRecordRef {
                text: Rc::from(joined.as_str()),
                ..record
            }
        } else {
            record
        };
        {
            let mut annotations = self.memory_feedback.selection.annotations.borrow_mut();
            annotations.items.push(TranscriptAnnotation {
                id,
                message_id: Uuid::nil(),
                spans,
                comment: String::new(),
                file: None,
                history: None,
                memory: Some(record.source()),
            });
            annotations.hovered = None;
        }
        self.memory_feedback
            .selection
            .selection
            .borrow_mut()
            .clear();
        self.memory_feedback.hover = None;
        self.open_memory_note_editor(id, true, window, cx);
        self.memory_feedback.composer_revealed = true;
        cx.notify();
    }

    /// Create a pending whole-memory note on `record` and open the editor.
    /// The row's action handler — and its keyboard equivalent — lands here.
    pub(super) fn annotate_memory_record(
        &mut self,
        record: &MemoryRecordRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Clicking another memory's action saves the open editor's nonblank
        // comment before this one opens — the transcript's local-save
        // convention, kept verbatim.
        self.commit_memory_note_editor(cx);
        let id = self.annotation_next_id;
        self.annotation_next_id += 1;
        self.memory_feedback
            .selection
            .annotations
            .borrow_mut()
            .items
            .push(TranscriptAnnotation {
                id,
                message_id: Uuid::nil(),
                // A whole-memory pin carries no passage spans: nothing
                // highlights, and `quoted_text` answers the snapshot.
                spans: Vec::new(),
                comment: String::new(),
                file: None,
                history: None,
                memory: Some(record.source()),
            });
        self.open_memory_note_editor(id, true, window, cx);
        self.memory_feedback.composer_revealed = true;
        cx.notify();
    }

    /// The record descriptor a key resolves to from the loaded feed —
    /// pin-time snapshots come from its `text`; an empty `text` when the
    /// feed has not loaded lets callers fall back to the spans' quote.
    fn memory_record_ref(&self, key: &MemoryAnnotationKey) -> MemoryRecordRef {
        let record = self.boss_ui.memory_feed.get(&key.daemon).and_then(|feed| {
            feed.records
                .iter()
                .find(|record| record.bucket_id == key.bucket && record.id == key.reference)
        });
        MemoryRecordRef {
            key: key.clone(),
            sequence: record.map_or(0, |record| record.sequence),
            bucket: record.map_or_else(|| key.bucket.clone(), |record| record.bucket.clone()),
            project: record.and_then(|record| record.project.clone()),
            created_at: record.map_or(0, |record| record.created_at),
            text: record.map_or_else(|| Rc::from(""), |record| Rc::from(record.text.as_str())),
        }
    }

    /// Write the field's text onto the note and close the editor — the
    /// local-save convention: a nonblank comment sticks, an empty new
    /// editor closes without adding anything, an emptied existing note
    /// keeps its saved comment.
    pub(super) fn commit_memory_note_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.memory_feedback.editor.take() else {
            return;
        };
        let comment = self.memory_note_input.read(cx).content().to_owned();
        {
            let mut annotations = self.memory_feedback.selection.annotations.borrow_mut();
            if editor.is_new && comment.trim().is_empty() {
                annotations
                    .items
                    .retain(|annotation| annotation.id != editor.annotation_id);
            } else if !comment.trim().is_empty()
                && let Some(annotation) = annotations
                    .items
                    .iter_mut()
                    .find(|annotation| annotation.id == editor.annotation_id)
            {
                annotation.comment = comment;
            }
            annotations.editing = None;
        }
        self.restore_memory_note_focus(editor.previous_focus, cx);
        cx.notify();
    }

    /// Escape or Cancel: discard a new unsaved pin entirely; for an added
    /// note, cancel only the edit — the saved comment stays.
    fn discard_memory_note_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.memory_feedback.editor.take() else {
            return;
        };
        {
            let mut annotations = self.memory_feedback.selection.annotations.borrow_mut();
            if editor.is_new {
                annotations
                    .items
                    .retain(|annotation| annotation.id != editor.annotation_id);
            }
            annotations.editing = None;
        }
        self.memory_note_input
            .update(cx, |input, cx| input.set_content("", cx));
        let focus = editor
            .previous_focus
            .unwrap_or_else(|| self.composer_focus(cx));
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Delete a pending note, closing the editor when it was the one open.
    fn remove_memory_note(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.memory_feedback
            .selection
            .annotations
            .borrow_mut()
            .items
            .retain(|annotation| annotation.id != id);
        if let Some(editor) = self
            .memory_feedback
            .editor
            .take_if(|editor| editor.annotation_id == id)
        {
            self.memory_feedback
                .selection
                .annotations
                .borrow_mut()
                .editing = None;
            self.memory_note_input
                .update(cx, |input, cx| input.set_content("", cx));
            let focus = editor
                .previous_focus
                .unwrap_or_else(|| self.composer_focus(cx));
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Drop one daemon's pending batch — the composer annotation chip's X
    /// clears only what it counted. Other daemons' notes — and an editor
    /// still open on one — stay pending.
    pub(super) fn clear_memory_notes(&mut self, daemon: DaemonKey) {
        let mut annotations = self.memory_feedback.selection.annotations.borrow_mut();
        annotations.items.retain(|annotation| {
            !MemoryFeedback::key_of(annotation).is_some_and(|key| key.daemon == daemon)
        });
        annotations.hovered = None;
        if annotations
            .editing
            .is_some_and(|id| !annotations.items.iter().any(|item| item.id == id))
        {
            annotations.editing = None;
            self.memory_feedback.editor = None;
        }
        self.memory_feedback.hover = None;
        self.memory_feedback.press = None;
    }

    /// Drop pending notes whose records a fresh feed no longer lists — the
    /// plan's rule for a permission or scope change that takes a record out
    /// of reach: the note's captured quote must not outlive the access it
    /// came from.
    pub(super) fn prune_memory_feedback(&mut self, daemon: DaemonKey, cx: &mut Context<Self>) {
        let Some(feed) = self.boss_ui.memory_feed.get(&daemon) else {
            return;
        };
        if !feed.loaded {
            return;
        }
        let mut annotations = self.memory_feedback.selection.annotations.borrow_mut();
        annotations.items.retain(|annotation| {
            let Some(key) = MemoryFeedback::key_of(annotation) else {
                return true;
            };
            key.daemon != daemon
                || feed
                    .records
                    .iter()
                    .any(|record| record.bucket_id == key.bucket && record.id == key.reference)
        });
        // An editor on a pruned note closes; notes still present stay open.
        if let Some(editor_id) = annotations.editing
            && !annotations
                .items
                .iter()
                .any(|annotation| annotation.id == editor_id)
        {
            annotations.editing = None;
            self.memory_feedback.editor = None;
        }
        drop(annotations);
        // Stale record views go with their records.
        self.memory_feedback.record_views.retain(|key, _| {
            key.daemon != daemon
                || feed
                    .records
                    .iter()
                    .any(|record| record.bucket_id == key.bucket && record.id == key.reference)
        });
        cx.notify();
    }

    /// The armed batch's size — the composer chip's memory count and the
    /// "is there anything to send" check.
    pub(super) fn memory_feedback_count(&self) -> usize {
        self.memory_feedback_armed_daemon()
            .map_or(0, |daemon| self.memory_notes_for_daemon(daemon).len())
    }

    /// Open the floating editor on a pending note — a fresh pin opens with
    /// an empty field, an added note with its saved comment.
    fn open_memory_note_editor(
        &mut self,
        id: u64,
        is_new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let comment = self
            .memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .iter()
            .find(|annotation| annotation.id == id)
            .map(|annotation| annotation.comment.clone());
        let Some(comment) = comment else {
            return;
        };
        // One editor at a time: an already-open one's editing emphasis in
        // the store would otherwise stay stuck on.
        self.memory_feedback.editor = Some(MemoryNoteEditor {
            annotation_id: id,
            is_new,
            previous_focus: window.focused(cx),
        });
        self.memory_feedback
            .selection
            .annotations
            .borrow_mut()
            .editing = Some(id);
        self.memory_note_input
            .update(cx, |input, cx| input.set_content(comment, cx));
        // The card is a deferred element; its focus handle joins the
        // dispatch tree only after the deferred draw, so focus lands two
        // frames out — the transcript editor's own convention.
        let focus = self.memory_note_input.read(cx).focus();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
        cx.notify();
    }

    /// Editor commit paths that lack a `Window` (the field's own submit
    /// subscription) focus through the stored window handle instead.
    fn restore_memory_note_focus(&mut self, previous: Option<FocusHandle>, cx: &mut Context<Self>) {
        let focus = previous.unwrap_or_else(|| self.composer_focus(cx));
        let window_handle = self.window_handle;
        let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
    }

    /// Drain this daemon's pending notes for a Boss-bound submission — in
    /// creation order, after locally saving any open nonblank editor.
    /// Other daemons' notes stay pending.
    pub(super) fn drain_memory_feedback(
        &mut self,
        daemon: DaemonKey,
        cx: &mut Context<Self>,
    ) -> Vec<TranscriptAnnotation> {
        self.commit_memory_note_editor(cx);
        self.memory_feedback.hover = None;
        self.memory_feedback.press = None;
        self.memory_feedback.review_open = false;
        self.refresh_memory_note_status();
        let mut annotations = self.memory_feedback.selection.annotations.borrow_mut();
        annotations.editing = None;
        annotations.hovered = None;
        let (drained, kept): (Vec<_>, Vec<_>) =
            annotations.items.drain(..).partition(|annotation| {
                MemoryFeedback::key_of(annotation).is_some_and(|key| key.daemon == daemon)
            });
        annotations.items = kept;
        drop(annotations);
        let mut drained = drained;
        drained.sort_by_key(|annotation| annotation.id);
        drained
    }

    /// Hand a failed submission's memory notes back to the pending batch —
    /// the restore counterpart of [`Self::drain_memory_feedback`].
    pub(super) fn restore_memory_feedback(&mut self, annotations: Vec<TranscriptAnnotation>) {
        self.memory_feedback
            .selection
            .annotations
            .borrow_mut()
            .items
            .extend(annotations);
    }

    /// A Review "Show memory" request the feed consumes while laying out —
    /// scrolling to the record, clearing the filter that hides it — then
    /// clears. `None` while nothing was asked.
    pub(super) fn consume_memory_feedback_locate(
        &mut self,
        daemon: DaemonKey,
    ) -> Option<MemoryAnnotationKey> {
        self.memory_feedback
            .locate
            .as_ref()
            .is_some_and(|key| key.daemon == daemon)
            .then(|| self.memory_feedback.locate.take())
            .flatten()
    }

    // ── Anchors ───────────────────────────────────────────────────────

    /// First on-screen glyph rect of a span's range — the offer pill's
    /// anchor. `None` when the element is not registered this frame.
    fn memory_spans_anchor(&self, spans: &[Span]) -> Option<Bounds<Pixels>> {
        let registry = self.memory_feedback.selection.registry.borrow();
        let mut union: Option<Bounds<Pixels>> = None;
        for span in spans {
            let Some(entry) = registry
                .entries()
                .iter()
                .find(|entry| entry.key == span.key)
            else {
                continue;
            };
            if entry.geometry.is_missing() {
                continue;
            }
            let end = span.range.end.min(entry.text.len());
            for rect in text_range_bounds(&entry.geometry, &(span.range.start..end)) {
                union = Some(match union {
                    Some(u) => u.union(&rect),
                    None => rect,
                });
            }
        }
        union
    }

    /// The target record's painted extent — the note editor anchors beneath
    /// the whole memory, not beneath a selected passage. `None` while no
    /// element of the row registered this frame: scrolled or filtered out.
    fn memory_row_anchor(&self, key: &MemoryAnnotationKey) -> Option<Bounds<Pixels>> {
        let row = memory_row_key(key);
        let registry = self.memory_feedback.selection.registry.borrow();
        let mut union: Option<Bounds<Pixels>> = None;
        for entry in registry.entries() {
            if entry.key.row != row || entry.geometry.is_missing() {
                continue;
            }
            for rect in text_range_bounds(&entry.geometry, &(0..entry.text.len())) {
                union = Some(match union {
                    Some(u) => u.union(&rect),
                    None => rect,
                });
            }
        }
        union
    }

    /// The hit-tested annotation id under `position`, by its painted
    /// highlight — the same registry read the transcript's own hit test
    /// makes.
    fn memory_annotation_hit_at(&self, position: Point<Pixels>) -> Option<u64> {
        let annotations = self.memory_feedback.selection.annotations.borrow();
        let registry = self.memory_feedback.selection.registry.borrow();
        annotations
            .items
            .iter()
            .flat_map(|annotation| {
                annotation
                    .spans
                    .iter()
                    .map(move |span| (annotation.id, span))
            })
            .find(|(_, span)| {
                let Some(entry) = registry
                    .entries()
                    .iter()
                    .find(|entry| entry.key == span.key)
                else {
                    return false;
                };
                if entry.geometry.is_missing() {
                    return false;
                }
                text_range_bounds(&entry.geometry, &span.range)
                    .iter()
                    .any(|bounds| bounds.contains(&position))
            })
            .map(|(id, _)| id)
    }

    /// The memory-surface annotation mouse listeners — the same
    /// press/hover/click pattern as the transcript's
    /// `install_annotation_input`, hit-testing the memory selection's
    /// painted highlights against its own press/hover slots. The caller
    /// prepaints a region hitbox whose id gates them so a floating surface
    /// covering the region doesn't trigger highlights beneath it.
    pub(super) fn install_memory_annotation_input(
        region: HitboxId,
        window: &mut Window,
        _cx: &mut App,
        selection: &TranscriptSelection,
        waku: &WeakEntity<Waku>,
    ) {
        window.on_mouse_event({
            let waku = waku.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !region.is_hovered(window)
                {
                    return;
                }
                let hit = waku
                    .read_with(cx, |this, _| this.memory_annotation_hit_at(event.position))
                    .ok()
                    .flatten();
                if let Some(id) = hit {
                    let _ = waku.update(cx, |this, _| {
                        this.memory_feedback.press = Some(MemoryAnnotationPress {
                            id,
                            position: event.position,
                        });
                    });
                }
            }
        });

        window.on_mouse_event({
            let selection = selection.clone();
            let waku = waku.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.dragging() || !region.is_hovered(window)
                {
                    return;
                }
                let hit = waku
                    .read_with(cx, |this, _| this.memory_annotation_hit_at(event.position))
                    .ok()
                    .flatten();
                let changed = {
                    let mut annotations = selection.annotations.borrow_mut();
                    if annotations.hovered == hit {
                        false
                    } else {
                        annotations.hovered = hit;
                        true
                    }
                };
                if changed {
                    let _ = waku.update(cx, |this, cx| {
                        this.memory_annotation_hover_changed(hit, cx);
                    });
                    window.refresh();
                }
            }
        });

        window.on_mouse_event({
            let selection = selection.clone();
            let waku = waku.clone();
            move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                let press = waku
                    .update(cx, |this, _| this.memory_feedback.press.take())
                    .ok()
                    .flatten();
                let Some(press) = press else {
                    return;
                };
                // A drag that started on a highlight ends with a selection,
                // not a press — only a clean click reopens the note.
                if !selection.selection.borrow().is_empty() {
                    return;
                }
                let moved = event.position - press.position;
                if moved.x.abs() > px(4.0) || moved.y.abs() > px(4.0) {
                    return;
                }
                let still_hit = waku
                    .read_with(cx, |this, _| this.memory_annotation_hit_at(event.position))
                    .ok()
                    .flatten()
                    == Some(press.id);
                if !still_hit {
                    return;
                }
                let mut settled = selection.selection.borrow_mut();
                if settled.release() {
                    settled.clear();
                }
                drop(settled);
                let _ = waku.update(cx, |this, cx| {
                    this.open_memory_note_editor(press.id, false, window, cx)
                });
            }
        });
    }

    /// Two-phase hover like the transcript's: the highlight emphasises at
    /// once but the tooltip waits out its settle before showing.
    fn memory_annotation_hover_changed(&mut self, hit: Option<u64>, cx: &mut Context<Self>) {
        self.memory_feedback.hover = hit.map(|id| MemoryAnnotationHover { id, visible: false });
        if let Some(id) = hit {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(MEMORY_NOTE_HOVER_DELAY)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this
                        .memory_feedback
                        .hover
                        .as_ref()
                        .is_some_and(|hover| hover.id == id && !hover.visible)
                    {
                        this.memory_feedback.hover =
                            Some(MemoryAnnotationHover { id, visible: true });
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    // ── Render ─────────────────────────────────────────────────────────

    /// The "Add note" affordance a memory row carries — quiet until the row
    /// is hovered or it is focused, matching the row's other controls.
    /// Keyboard focus reaches it on its own: discovery never depends on the
    /// pointer alone.
    pub(super) fn render_memory_record_annotate_button(
        &self,
        record: &MemoryRecordRef,
        group: &SharedString,
        aria_name: String,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let record = record.clone();
        div()
            .id(SharedString::from(format!(
                "boss-memory-annotate-{}",
                memory_key_path(&record.key)
            )))
            .tab_index(0)
            .flex_none()
            .p(px(4.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .opacity(0.45)
            .group_hover(group.clone(), |style| style.opacity(1.0))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()).opacity(1.0))
            .tooltip(Tooltip::text(tr!("memory.add_note_to_memory")))
            .aria_label(aria_name)
            .child(icon("icons/message-square.svg", 13.0, theme.text_secondary))
            .on_activation(cx, move |this, window, cx| {
                this.annotate_memory_record(&record, window, cx)
            })
    }

    /// A pending note beneath its memory: the quote for a passage pin or
    /// "Whole memory" otherwise, the saved comment, and edit/remove icon
    /// buttons — the row's unambiguous way to reach either, so overlapping
    /// highlights never make a note unreachable.
    pub(super) fn render_memory_record_notes(
        &mut self,
        record: &MemoryRecordRef,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = Theme::current(cx);
        self.memory_notes_for(&record.key)
            .into_iter()
            .map(|annotation| {
                let quote = if annotation.spans.is_empty() {
                    tr!("memory.whole_memory")
                } else {
                    tr!(
                        "memory.selected_quote",
                        quote = annotation_quote_preview(&annotation)
                    )
                };
                let edit_id = annotation.id;
                let remove_id = annotation.id;
                let edit_focus =
                    self.transcript_control_focus(format!("memory-note-edit-{edit_id}"), cx);
                let remove_focus =
                    self.transcript_control_focus(format!("memory-note-remove-{remove_id}"), cx);
                div()
                    .pl(px(10.0))
                    .border_l_2()
                    .border_color(theme.accent.opacity(0.4))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_tertiary)
                                    .child(quote),
                            )
                            .child(
                                icon_button(
                                    SharedString::from(format!("memory-note-edit-{edit_id}")),
                                    "icons/pencil.svg",
                                    theme,
                                )
                                .track_focus(&edit_focus)
                                .tab_index(0)
                                .tooltip(Tooltip::text(tr!("memory.edit_note")))
                                .on_activation(
                                    cx,
                                    move |this, window, cx| {
                                        this.open_memory_note_editor(edit_id, false, window, cx)
                                    },
                                ),
                            )
                            .child(
                                icon_button(
                                    SharedString::from(format!("memory-note-remove-{remove_id}")),
                                    "icons/trash.svg",
                                    theme,
                                )
                                .track_focus(&remove_focus)
                                .tab_index(0)
                                .tooltip(Tooltip::text(tr!("memory.remove_note")))
                                .on_activation(
                                    cx,
                                    move |this, window, cx| {
                                        this.remove_memory_note(remove_id, window, cx)
                                    },
                                ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(sp(12.5))
                            .text_color(theme.text)
                            .child(annotation.comment.clone()),
                    )
                    .into_any_element()
            })
            .collect()
    }

    /// The floating "Add annotation" pill over a settled memory-text
    /// selection — the transcript selection interaction verbatim.
    pub(super) fn render_memory_annotation_offer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (_key, spans) = self.annotatable_memory_selection()?;
        let anchor = self.memory_spans_anchor(&spans)?;
        let theme = Theme::current(cx);
        let focus = self.transcript_control_focus("memory-add-annotation", cx);
        let button = div()
            .id("memory-add-annotation")
            .track_focus(&focus)
            .tab_index(0)
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_subtle)
            .bg(theme.raised)
            .shadow_lg()
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(sp(12.0))
            .text_color(theme.text)
            .cursor_pointer()
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            // The pill swallows its own mouse-down so clicking it cannot
            // clear the selection it offers to pin.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(icon("icons/chat.svg", 12.0, theme.text_secondary))
            .child(tr!("memory.add_annotation"))
            .on_activation(cx, |this, window, cx| {
                this.annotate_memory_selection(window, cx)
            });
        Some(
            deferred(FloatingSurface::new(
                motion::surface_enter("memory-annotate-selection-enter", button).into_any_element(),
                anchor,
                MenuAlign::AboveLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The floating note editor, anchored beneath the whole target memory.
    /// When the target leaves the visible area — scrolled or filtered
    /// out — the local-save convention applies here: a nonblank comment
    /// saves into the batch and stays editable in Review, an empty new
    /// editor closes without adding anything.
    pub(super) fn render_memory_annotation_editor(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.memory_feedback.editor.clone()?;
        let annotation = self
            .memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .iter()
            .find(|annotation| annotation.id == editor.annotation_id)
            .cloned()?;
        let key = MemoryFeedback::key_of(&annotation)?;
        let Some(anchor) = self.memory_row_anchor(&key) else {
            // The target scrolled or filtered out — never strand the card
            // over unrelated rows.
            self.commit_memory_note_editor(cx);
            return None;
        };
        let theme = Theme::current(cx);
        let record = self.memory_record_ref(&key);
        let (scope, quote) = if annotation.spans.is_empty() {
            (
                tr!("memory.whole_memory"),
                memory_excerpt(&record.text, 200),
            )
        } else {
            (
                tr!("memory.selected_text"),
                annotation_quote_preview(&annotation),
            )
        };
        let provenance = memory_provenance_label(&record, annotation.memory.as_ref());
        let save_label = if editor.is_new {
            tr!("memory.add_note")
        } else {
            tr!("memory.save_note")
        };
        let remove_focus = self.transcript_control_focus("memory-note-remove", cx);
        let save_focus = self.transcript_control_focus("memory-note-save", cx);
        let cancel_focus = self.transcript_control_focus("memory-note-cancel", cx);
        let card = div()
            .occlude()
            .key_context(MEMORY_NOTE_CONTEXT)
            .on_action(cx.listener(|this, _: &DismissMenu, window, cx| {
                this.discard_memory_note_editor(window, cx);
            }))
            // The field's escape arrives as `Clear` — it is not opted into
            // `clear_on_escape`, so the action propagates up to the card.
            .on_action(cx.listener(|this, _: &Clear, window, cx| {
                this.discard_memory_note_editor(window, cx);
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.commit_memory_note_editor(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("memory-note-editor-card")
                    .w(px(340.0))
                    .p(px(10.0))
                    .font_family(crate::fonts::current(cx).ui)
                    .rounded(px(11.0))
                    .border(hairline())
                    .border_color(theme.border_subtle)
                    .bg(theme.raised)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_secondary)
                                    .child(scope),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_tertiary)
                                    .child(provenance),
                            ),
                    )
                    .child(
                        div()
                            .id("memory-note-quote")
                            .max_h(px(120.0))
                            .overflow_y_scroll()
                            .rounded(px(6.0))
                            .bg(theme.inset)
                            .px(px(8.0))
                            .py(px(6.0))
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(quote),
                    )
                    .child(self.memory_note_input.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                icon_button("memory-note-remove", "icons/trash.svg", theme)
                                    .track_focus(&remove_focus)
                                    .tab_index(0)
                                    .tooltip(Tooltip::text(tr!("memory.remove_note")))
                                    .on_activation(cx, |this, window, cx| {
                                        if let Some(editor) = this.memory_feedback.editor.clone() {
                                            this.remove_memory_note(
                                                editor.annotation_id,
                                                window,
                                                cx,
                                            );
                                        }
                                    }),
                            )
                            .child(div().flex_1())
                            .child(
                                boss_button("memory-note-cancel", tr!("common.cancel"), &theme)
                                    .on_activation(cx, |this, window, cx| {
                                        this.discard_memory_note_editor(window, cx)
                                    })
                                    .track_focus(&cancel_focus)
                                    .child(tr!("common.cancel")),
                            )
                            .child(
                                boss_button("memory-note-save", save_label.clone(), &theme)
                                    .on_activation(cx, |this, _, cx| {
                                        this.commit_memory_note_editor(cx)
                                    })
                                    .track_focus(&save_focus)
                                    .child(save_label),
                            ),
                    ),
            );
        Some(
            deferred(FloatingSurface::new(
                motion::surface_enter("memory-note-editor-enter", card).into_any_element(),
                anchor,
                MenuAlign::BelowLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// The hover card over a pinned passage: its saved comment, after the
    /// same settle the transcript's tooltips observe.
    pub(super) fn render_memory_annotation_tooltip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let hover = self
            .memory_feedback
            .hover
            .as_ref()
            .filter(|hover| hover.visible)?;
        let annotation = self
            .memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .iter()
            .find(|annotation| annotation.id == hover.id)
            .cloned()?;
        let key = MemoryFeedback::key_of(&annotation)?;
        let record = self.memory_record_ref(&key);
        let anchor = self.memory_spans_anchor(&annotation.spans)?;
        let theme = Theme::current(cx);
        let comment = annotation.comment.trim().to_owned();
        let card =
            div()
                .id("memory-note-tooltip")
                .w(px(260.0))
                .p(px(8.0))
                .rounded(px(8.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.raised)
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(memory_provenance_label(&record, annotation.memory.as_ref())),
                )
                .child(div().text_size(sp(12.5)).text_color(theme.text).child(
                    if comment.is_empty() {
                        tr!("memory.empty_note")
                    } else {
                        comment
                    },
                ));
        Some(
            deferred(FloatingSurface::new(
                card.into_any_element(),
                anchor,
                MenuAlign::AboveLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// The batch's presence under the feed — the count with its hidden-by-
    /// filter warning, the Review entry, then the exact normal Boss chat
    /// composer, unchanged. `visible` answers whether a record's row is
    /// rendered right now; the feed supplies it because it owns search and
    /// filters. `None` until the first pending note reveals the composer,
    /// and again once the batch is empty and the composer holds no text.
    pub(super) fn render_memory_feedback_supplement(
        &mut self,
        window: &mut Window,
        visible: impl Fn(&MemoryAnnotationKey) -> bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        let count = self
            .memory_feedback
            .selection
            .annotations
            .borrow()
            .items
            .len();
        let composer_empty = self.composer.read(cx).content(cx).trim().is_empty();
        // The visibility latch: the first note reveals the composer; an
        // emptied batch hides it again only while nothing was typed.
        if count > 0 {
            self.memory_feedback.composer_revealed = true;
        } else if composer_empty {
            self.memory_feedback.composer_revealed = false;
        }
        if !self.memory_feedback.composer_revealed {
            return None;
        }
        let hidden = self.memory_feedback_hidden_count(&visible);
        let bar = (count > 0).then(|| {
            let label = if count == 1 {
                tr!("memory.pending_one")
            } else {
                tr!("memory.pending_many", count = count)
            };
            let hidden_label = match hidden {
                0 => None,
                1 => Some(tr!("memory.hidden_one")),
                n => Some(tr!("memory.hidden_many", count = n)),
            };
            div()
                .px(px(20.0))
                .pb(px(6.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(sp(12.0))
                                .text_color(theme.text_secondary)
                                .child(label),
                        )
                        .when_some(hidden_label, |element, label| {
                            element.child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_tertiary)
                                    .child(label),
                            )
                        }),
                )
                .child(
                    boss_button("memory-review", tr!("memory.review"), &theme)
                        .on_activation(cx, |this, _, cx| {
                            this.memory_feedback.review_open = !this.memory_feedback.review_open;
                            cx.notify();
                        })
                        .child(tr!("memory.review")),
                )
        });
        let review = self
            .memory_feedback
            .review_open
            .then(|| self.render_memory_feedback_review(cx))
            .flatten();
        Some(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(theme.separator)
                .when_some(review, |element, review| element.child(review))
                .when_some(bar, |element, bar| element.child(bar))
                .child(self.render_composer(window, cx))
                .into_any_element(),
        )
    }

    /// The Review surface: every pending note grouped by its memory —
    /// including records the current rows hide — each group naming the
    /// bucket, time, project, and excerpt the batch counts on, plus a
    /// "Show memory" locate for the feed to honor.
    fn render_memory_feedback_review(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        self.refresh_memory_note_status();
        let items: Vec<TranscriptAnnotation> = {
            let mut items = self
                .memory_feedback
                .selection
                .annotations
                .borrow()
                .items
                .clone();
            items.sort_by_key(|annotation| annotation.id);
            items
        };
        if items.is_empty() {
            return None;
        }
        // Group by memory in first-seen order; numbering keeps the batch's
        // global creation order across groups.
        let mut groups: Vec<(MemoryAnnotationKey, Vec<(usize, TranscriptAnnotation)>)> = Vec::new();
        for (index, annotation) in items.iter().enumerate() {
            let Some(key) = MemoryFeedback::key_of(annotation) else {
                continue;
            };
            match groups.iter_mut().find(|(group, _)| *group == key) {
                Some((_, notes)) => notes.push((index, annotation.clone())),
                None => groups.push((key, vec![(index, annotation.clone())])),
            }
        }
        let groups = groups
            .into_iter()
            .map(|(key, notes)| {
                let record = self.memory_record_ref(&key);
                let group_key = key.clone();
                let locate_focus = self.transcript_control_focus(
                    format!("memory-locate-{}", memory_key_path(&key)),
                    cx,
                );
                let group_label = notes
                    .first()
                    .and_then(|(_, annotation)| {
                        annotation
                            .memory
                            .as_ref()
                            .map(|memory| memory_provenance_label(&record, Some(memory)))
                    })
                    .unwrap_or_else(|| memory_provenance_label(&record, None));
                let note_rows: Vec<AnyElement> =
                    notes
                        .into_iter()
                        .map(|(index, annotation)| {
                            let quote = if annotation.spans.is_empty() {
                                tr!("memory.whole_memory")
                            } else {
                                tr!(
                                    "memory.selected_quote",
                                    quote = annotation_quote_preview(&annotation)
                                )
                            };
                            let status = annotation
                                .memory
                                .as_ref()
                                .and_then(|memory| memory.status)
                                .map(|status| match status {
                                    MemoryNoteStatus::Changed => tr!("memory.changed"),
                                    MemoryNoteStatus::Unavailable => tr!("memory.unavailable"),
                                });
                            let edit_id = annotation.id;
                            let remove_id = annotation.id;
                            let edit_focus = self.transcript_control_focus(
                                format!("memory-review-edit-{edit_id}"),
                                cx,
                            );
                            let remove_focus = self.transcript_control_focus(
                                format!("memory-review-remove-{remove_id}"),
                                cx,
                            );
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .py(px(4.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_size(sp(11.0))
                                                .text_color(theme.text_tertiary)
                                                .child(tr!(
                                                    "memory.annotation_label",
                                                    index = index + 1
                                                )),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(sp(11.0))
                                                .text_color(theme.text_tertiary)
                                                .child(quote),
                                        )
                                        .when_some(status, |element, status| {
                                            element.child(
                                                div()
                                                    .flex_none()
                                                    .text_size(sp(11.0))
                                                    .text_color(theme.text_tertiary)
                                                    .child(status),
                                            )
                                        })
                                        .child(
                                            icon_button(
                                                SharedString::from(format!(
                                                    "memory-review-edit-{edit_id}"
                                                )),
                                                "icons/pencil.svg",
                                                theme,
                                            )
                                            .track_focus(&edit_focus)
                                            .tab_index(0)
                                            .tooltip(Tooltip::text(tr!("memory.edit_note")))
                                            .on_activation(cx, move |this, window, cx| {
                                                this.open_memory_note_editor(
                                                    edit_id, false, window, cx,
                                                )
                                            }),
                                        )
                                        .child(
                                            icon_button(
                                                SharedString::from(format!(
                                                    "memory-review-remove-{remove_id}"
                                                )),
                                                "icons/trash.svg",
                                                theme,
                                            )
                                            .track_focus(&remove_focus)
                                            .tab_index(0)
                                            .tooltip(Tooltip::text(tr!("memory.remove_note")))
                                            .on_activation(cx, move |this, window, cx| {
                                                this.remove_memory_note(remove_id, window, cx)
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(sp(12.5))
                                        .text_color(theme.text)
                                        .child(annotation.comment.clone()),
                                )
                                .into_any_element()
                        })
                        .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.separator)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_secondary)
                                    .child(group_label),
                            )
                            .child(
                                icon_button(
                                    SharedString::from(format!(
                                        "memory-locate-{}",
                                        memory_key_path(&group_key)
                                    )),
                                    "icons/eye.svg",
                                    theme,
                                )
                                .track_focus(&locate_focus)
                                .tab_index(0)
                                .tooltip(Tooltip::text(tr!("memory.locate")))
                                .on_activation(
                                    cx,
                                    move |this, window, cx| {
                                        this.locate_memory_note_target(&group_key, window, cx)
                                    },
                                ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(memory_excerpt(&record.text, 160)),
                    )
                    .children(note_rows)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        Some(
            div()
                .id("memory-review")
                .max_h(px(280.0))
                .overflow_y_scroll()
                .px(px(20.0))
                .py(px(4.0))
                .flex()
                .flex_col()
                .children(groups)
                .into_any_element(),
        )
    }

    /// Honor a Review "Show memory": park the request and open the
    /// record's Memory page. The section consumes the request — clearing
    /// its search and project filter, revealing the row — because browse
    /// state is the feed's to own.
    fn locate_memory_note_target(
        &mut self,
        key: &MemoryAnnotationKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.memory_feedback.locate = Some(key.clone());
        self.memory_feedback.review_open = false;
        if self.boss_ui.page != Some((key.daemon, BossTab::Memory)) {
            self.open_boss_page(key.daemon, BossTab::Memory, window, cx);
        }
        cx.notify();
    }
}

/// A short single-line excerpt of a record — the editor header's and
/// Review's "the target remains clear" context.
fn memory_excerpt(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (index, ch) in flat.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

/// The `bucket · 5m · project` provenance line editors and review groups
/// repeat — the record's label and muted relative creation time on one
/// line, plus the project association when the feed resolved one. "No
/// project" shows only where the group needs it; the row's own line omits
/// it like the feed does.
fn memory_provenance_label(record: &MemoryRecordRef, memory: Option<&MemorySource>) -> String {
    let (bucket, project, created_at) = match memory {
        Some(memory) => (
            memory.bucket.clone(),
            memory
                .project
                .as_ref()
                .and_then(|project| project.name.clone()),
            Some(memory.created_at),
        ),
        None => (
            record.bucket.clone(),
            record
                .project
                .as_ref()
                .and_then(|project| project.name.clone()),
            (record.created_at > 0).then_some(record.created_at),
        ),
    };
    let mut label = bucket;
    if let Some(created_at) = created_at {
        label.push_str(&format!(" · {}", relative_time(created_at)));
    }
    if let Some(project) = project {
        label.push_str(&format!(" · {project}"));
    }
    label
}

/// The compact relative age the provenance line carries — `now`, `5m`,
/// `2h`, `3d` — the feed's own helper so the two never drift apart.
fn relative_time(created_at: u64) -> String {
    memory_record_age(unix_time().saturating_sub(created_at))
}
