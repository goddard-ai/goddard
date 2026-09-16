//! Transcript annotations: commented highlights over agent messages and
//! right-panel file editors.
//!
//! Selecting text inside a single assistant message — or inside a file
//! editor — offers an "Add to chat" pill; accepting it pins a highlight on
//! the passage and opens a floating comment editor. Confirmed annotations
//! stay highlighted — hover previews the comment, a click reopens the editor
//! — and the composer shows an "N annotations" chip until the next
//! submission, which carries the comments to the provider as a quoted header
//! above the typed prompt and echoes the quoted passages in the sent bubble.
//! A file annotation quotes as `@path` plus a `[Selected lines N-M]` marker
//! and a fenced block instead of a plain passage.
//!
//! Annotations are session-scoped and persist inside the composer draft, so
//! they survive restarts and sync like the draft's text. The transcript's
//! painted set sits on [`TranscriptSelection`] so the renderer can reach it
//! from paint closures; each file editor carries its own list on
//! [`RightPanelFileEditor`], painted inside the field and parked with the
//! session's panel state; a file annotation whose editor does not exist waits
//! in `pending_file_annotations`. Session switches park and restore both (see
//! `reset_visible_state`), and sending drains them into the prompt.
//!
//! A submission's drained set also parks under its user message
//! (`sent_annotations`): the prompt header teaches the agent to cite it as
//! "Annotation N", and a reply doing so gets a dotted underline whose hover
//! tooltip shows the quoted passage and comment — see
//! [`Waku::annotation_ref_set`].

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, HitboxBehavior, HitboxId, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, canvas, deferred, div, px,
};

use crate::input::Clear;
use crate::md::render::{TranscriptSelection, text_range_bounds};
use crate::md::selection::{Annotations, FileAnnotation, Span, TextKey, TranscriptAnnotation};
use crate::ui::ActivationExt;
use crate::ui::menu::{DismissMenu, FloatingSurface, MenuAlign};
use crate::ui::shortcut::ShortcutHint;

use super::*;

/// Key context the comment editor card declares, so Escape reaches it as an
/// action whether the field or the card's own controls hold focus.
const ANNOTATION_CONTEXT: &str = "WakuAnnotation";

/// Bind the editor card's own keys. Without this, Escape under the card —
/// focus on the trash button rather than the field — would fall through to
/// the root `Waku` context's `CancelTurn` and kill a running turn.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        DismissMenu,
        Some(ANNOTATION_CONTEXT),
    )]);
}

/// Hover delay before an annotation's comment tooltip appears, matching the
/// settle a native tooltip gives before it shows.
const ANNOTATION_HOVER_DELAY: Duration = Duration::from_millis(400);

/// Which live set an annotation belongs to — the transcript's painted store
/// or a right-panel file editor's own list, keyed by its workspace-relative
/// path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AnnotationTarget {
    Transcript,
    File(String),
}

/// The open comment editor. `is_new` marks an annotation Enter/Escape has not
/// yet confirmed; discarding it removes the highlight too.
#[derive(Clone, Debug)]
pub(super) struct AnnotationEditor {
    pub annotation_id: u64,
    pub is_new: bool,
    /// Focus to hand back when the editor closes — almost always the composer.
    pub previous_focus: Option<FocusHandle>,
    /// The set the edited annotation lives in.
    pub target: AnnotationTarget,
}

/// Mouse-down on a highlight, held until mouse-up proves it was a click
/// (selection stayed empty) rather than the start of a drag.
#[derive(Clone, Debug)]
pub(super) struct AnnotationPress {
    pub id: u64,
    pub position: Point<Pixels>,
    pub target: AnnotationTarget,
}

/// The annotation under the pointer. `visible` flips on after
/// [`ANNOTATION_HOVER_DELAY`] so a passing cursor does not flash the card.
#[derive(Clone, Debug)]
pub(super) struct AnnotationHover {
    pub id: u64,
    pub target: AnnotationTarget,
    pub visible: bool,
}

/// An `Annotation N` citation hit-tested under the pointer: its element, its
/// byte range within it, and the label's 1-based index into the resolved set.
#[derive(Clone, Debug)]
pub(super) struct AnnotationRefHit {
    pub key: TextKey,
    pub range: Range<usize>,
    pub index: usize,
}

/// The citation hover pending or showing its tooltip — the same two-phase
/// settle as [`AnnotationHover`].
#[derive(Clone, Debug)]
pub(super) struct AnnotationRefHover {
    pub key: TextKey,
    pub range: Range<usize>,
    pub index: usize,
    pub visible: bool,
}

/// The quoted passage as the prompt and the sent bubble carry it: a file
/// annotation leads with `@path` and its line-span marker above a fenced
/// block of the selected code; a transcript annotation is just its text.
fn annotation_prompt_passage(annotation: &TranscriptAnnotation) -> String {
    let Some(file) = &annotation.file else {
        return annotation.quoted_text();
    };
    let marker = if file.start_line == file.end_line {
        format!("[Selected line {}]", file.start_line)
    } else {
        format!("[Selected lines {}-{}]", file.start_line, file.end_line)
    };
    format!(
        "@{}\n{}\n```\n{}\n```",
        file.path,
        marker,
        annotation.quoted_text().trim_end()
    )
}

/// The prompt block prepended to a submission carrying annotations.
///
/// Each passage is quoted and labelled so the agent can cite the comment's
/// target; the trailing instruction is what makes the labels usable.
pub(super) fn annotation_prompt_prefix(annotations: &[TranscriptAnnotation]) -> String {
    if annotations.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (index, annotation) in annotations.iter().enumerate() {
        out.push_str(&format!("Annotation {}:\n", index + 1));
        for line in annotation_prompt_passage(annotation).lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("\nComment: ");
        out.push_str(annotation.comment.trim());
        out.push_str("\n\n");
    }
    out.push_str(
        "When responding, refer to the annotations above by their label (e.g. \"Annotation 1\") when appropriate.\n\n",
    );
    out
}

/// The user's own words in a drained set: the comments when any were written,
/// else the quoted passages. Titles, generated names and a restored composer
/// draft read this — never the transport wrapper itself.
pub(super) fn annotation_display_content(annotations: &[TranscriptAnnotation]) -> String {
    let comments = annotations
        .iter()
        .map(|annotation| annotation.comment.trim())
        .filter(|comment| !comment.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !comments.is_empty() {
        return comments;
    }
    annotations
        .iter()
        .map(|annotation| {
            // The selected code makes a poor title; its file is the summary.
            annotation
                .file
                .as_ref()
                .map(|file| format!("@{}", file.path))
                .unwrap_or_else(|| annotation.quoted_text())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the sent user bubble shows: each annotated passage as a quote block
/// with its comment, then the typed text — the transport's annotation
/// context without the "Annotation N" labels.
pub(super) fn annotation_bubble_content(
    annotations: &[TranscriptAnnotation],
    typed: &str,
) -> String {
    let mut out = String::new();
    for annotation in annotations {
        for line in annotation_prompt_passage(annotation).lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
        let comment = annotation.comment.trim();
        if !comment.is_empty() {
            out.push('\n');
            out.push_str(comment);
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(typed);
    out
}

/// The session annotation store is parked in `transcript_annotations` keyed by
/// session id; the live set is the `annotations` handle on the transcript's
/// selection state, repopulated on every session switch.
impl Waku {
    /// The settled selection's spans when they sit entirely inside one
    /// assistant message — the only selection "Add to chat" may annotate.
    /// Reasoning, tool output, user messages and cross-row selections all fail
    /// the row-key or role check.
    fn annotatable_selection(&self) -> Option<(Uuid, Vec<Span>)> {
        let selection = self.transcript_selection.selection.borrow();
        if selection.is_dragging() || selection.is_empty() {
            return None;
        }
        let spans = selection.spans();
        let first_row = spans.first()?.key.row.clone();
        let message_id = Uuid::parse_str(first_row.strip_prefix("message-")?).ok()?;
        if !spans.iter().all(|span| span.key.row == first_row) {
            return None;
        }
        let session = self.selected_session()?;
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)?;
        (message.role == MessageRole::Assistant).then(|| (message_id, spans.to_vec()))
    }

    /// The file editor's settled selection when it can be annotated:
    /// non-empty and not mid-drag. File selections are always one contiguous
    /// range, so those are the only conditions.
    fn annotatable_file_selection(&self, relative_path: &str, cx: &App) -> Option<Range<usize>> {
        let editor = self.right_panel_file_editors.get(relative_path)?;
        let field = editor.state.read(cx);
        let range = field.selected_range();
        if field.is_selecting() || range.is_empty() {
            return None;
        }
        Some(range)
    }

    /// The live annotation set for `target` — the transcript's painted store
    /// or one file editor's own list. `None` once the file's editor is gone.
    fn annotation_store(&self, target: &AnnotationTarget) -> Option<Rc<RefCell<Annotations>>> {
        match target {
            AnnotationTarget::Transcript => Some(self.transcript_selection.annotations.clone()),
            AnnotationTarget::File(path) => self
                .right_panel_file_editors
                .get(path)
                .map(|editor| editor.annotations.clone()),
        }
    }

    /// First on-screen glyph rect for `spans`, in window coordinates, for a
    /// floating surface to anchor to. `None` when the annotated row is
    /// virtualized away or the text no longer matches its snapshot.
    fn spans_anchor(&self, spans: &[Span]) -> Option<Bounds<Pixels>> {
        let registry = self.transcript_selection.registry.borrow();
        let viewport = self.active_transcript_rows().viewport_bounds();
        for span in spans {
            let Some(entry) = registry
                .entries()
                .iter()
                .find(|entry| entry.key == span.key)
            else {
                continue;
            };
            if entry.geometry.is_missing() || !annotation_span_live(span, &entry.text) {
                continue;
            }
            let end = span.range.end.min(entry.text.len());
            for rect in text_range_bounds(&entry.geometry, &(span.range.start..end)) {
                if rect.bottom() > viewport.top() && rect.top() < viewport.bottom() {
                    return Some(rect);
                }
            }
        }
        None
    }

    fn annotation_anchor(&self, annotation_id: u64) -> Option<Bounds<Pixels>> {
        let annotations = self.transcript_selection.annotations.borrow();
        let annotation = annotations
            .items
            .iter()
            .find(|annotation| annotation.id == annotation_id)?;
        self.spans_anchor(&annotation.spans)
    }

    /// First on-screen glyph rect of a file annotation's range — the anchor
    /// for its comment editor and hover tooltip. `None` when the range
    /// scrolled out of the editor viewport or the file's text moved under it.
    fn file_annotation_anchor(
        &self,
        relative_path: &str,
        annotation_id: u64,
        cx: &App,
    ) -> Option<Bounds<Pixels>> {
        let editor = self.right_panel_file_editors.get(relative_path)?;
        let annotations = editor.annotations.borrow();
        let annotation = annotations
            .items
            .iter()
            .find(|annotation| annotation.id == annotation_id)?;
        let file = annotation.file.as_ref()?;
        let input = editor.state.read(cx);
        if !file_annotation_live(annotation, input.content()) {
            return None;
        }
        let viewport = self.right_panel_editor_scroll_handle.bounds();
        input
            .range_bounds(&file.range)
            .into_iter()
            .find(|rect| rect.bottom() > viewport.top() && rect.top() < viewport.bottom())
    }

    /// The file annotation whose highlight contains `position`, hit-tested by
    /// the glyph under the pointer — `TextInput::offset_for_position` already
    /// rejects clicks past a line's end or in the margins.
    fn file_annotation_hit_at(
        &self,
        relative_path: &str,
        position: Point<Pixels>,
        cx: &App,
    ) -> Option<u64> {
        let editor = self.right_panel_file_editors.get(relative_path)?;
        let annotations = editor.annotations.borrow();
        if annotations.items.is_empty() {
            return None;
        }
        let input = editor.state.read(cx);
        let offset = input.offset_for_position(position)?;
        annotations.items.iter().find_map(|annotation| {
            let file = annotation.file.as_ref()?;
            (file.range.contains(&offset) && file_annotation_live(annotation, input.content()))
                .then_some(annotation.id)
        })
    }

    /// ⌘L with the pill's selection on screen is the same as clicking it.
    /// Without an annotatable selection the chord keeps its global meaning,
    /// so the context bindings forward to FocusComposer. A focused file
    /// editor's selection wins — its FileEditorPane binding only fires there.
    pub(super) fn add_to_chat_action(
        &mut self,
        _: &AddToChat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self
            .visible_right_panel_file_path()
            .filter(|path| self.file_editor_selection_focused(path, window, cx))
        {
            self.annotate_file_selection(&path, window, cx);
        } else if self.annotatable_selection().is_some() {
            self.annotate_selection(window, cx);
        } else {
            self.focus_composer_action(&FocusComposer, window, cx);
        }
    }

    /// The visible file editor is focused and holding a selection — the case
    /// the FileEditorPane `secondary-l` binding dispatches for.
    fn file_editor_selection_focused(
        &self,
        relative_path: &str,
        window: &Window,
        cx: &App,
    ) -> bool {
        let Some(editor) = self.right_panel_file_editors.get(relative_path) else {
            return false;
        };
        let field = editor.state.read(cx);
        field.focus().is_focused(window) && !field.selected_range().is_empty()
    }

    /// Turn the settled selection into a new annotation and open its comment
    /// editor.
    fn annotate_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((message_id, spans)) = self.annotatable_selection() else {
            return;
        };
        let id = self.annotation_next_id;
        self.annotation_next_id = self.annotation_next_id.wrapping_add(1);
        {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            annotations.items.push(TranscriptAnnotation {
                id,
                message_id,
                spans,
                comment: String::new(),
                file: None,
            });
            annotations.hovered = None;
        }
        self.transcript_selection.selection.borrow_mut().clear();
        self.annotation_hover = None;
        self.schedule_composer_draft_save(cx);
        self.open_annotation_editor(id, true, AnnotationTarget::Transcript, window, cx);
        // Focus is on the just-clicked "Add to chat" button, which is gone by
        // the time the editor closes — return to the composer instead.
        let composer_focus = self.composer_focus(cx);
        if let Some(editor) = self.annotation_editor.as_mut() {
            editor.previous_focus = Some(composer_focus);
        }
    }

    /// The file-editor counterpart of [`Self::annotate_selection`]: the
    /// selection becomes a pinned highlight on the field — keyed by byte
    /// range, kept while its snapshot still matches there — the live
    /// selection collapses, and the comment editor opens over it.
    fn annotate_file_selection(
        &mut self,
        relative_path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((range, text, start_line, end_line, state, field_focus)) = self
            .right_panel_file_editors
            .get(relative_path)
            .and_then(|editor| {
                let field = editor.state.read(cx);
                let range = field.selected_range();
                if field.is_selecting() || range.is_empty() {
                    return None;
                }
                let text = field.content().get(range.clone())?.to_owned();
                let start_line = 1 + field.content().get(..range.start)?.matches('\n').count();
                let end_line = start_line + text.matches('\n').count();
                Some((
                    range,
                    text,
                    start_line,
                    end_line,
                    editor.state.clone(),
                    field.focus(),
                ))
            })
        else {
            return;
        };
        let id = self.annotation_next_id;
        self.annotation_next_id = self.annotation_next_id.wrapping_add(1);
        if let Some(editor) = self.right_panel_file_editors.get(relative_path) {
            let mut annotations = editor.annotations.borrow_mut();
            annotations.items.push(TranscriptAnnotation {
                id,
                message_id: Uuid::nil(),
                // One span over the selected text alone — enough for
                // `quoted_text`, which is all a file annotation reads spans
                // for. The `file` range is what paints and hit-tests.
                spans: vec![Span {
                    key: TextKey::new(format!("file:{relative_path}"), 0),
                    range: 0..text.len(),
                    text: Rc::from(text.as_str()),
                    block_break: false,
                }],
                comment: String::new(),
                file: Some(FileAnnotation {
                    path: relative_path.to_owned(),
                    range,
                    start_line,
                    end_line,
                }),
            });
            annotations.hovered = None;
        }
        state.update(cx, |input, cx| input.clear_selection(cx));
        self.annotation_hover = None;
        self.schedule_composer_draft_save(cx);
        self.open_annotation_editor(
            id,
            true,
            AnnotationTarget::File(relative_path.to_owned()),
            window,
            cx,
        );
        // As on the transcript, the just-clicked pill is gone when the editor
        // closes — hand focus back to the field the selection came from.
        if let Some(editor) = self.annotation_editor.as_mut() {
            editor.previous_focus = Some(field_focus);
        }
    }

    /// Open the comment editor over `id`'s highlight. `is_new` controls what
    /// Escape does: remove an unconfirmed annotation, or close leaving the
    /// stored comment — the editor only writes on commit, so reverting is
    /// simply not writing.
    fn open_annotation_editor(
        &mut self,
        id: u64,
        is_new: bool,
        target: AnnotationTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let comment = self.annotation_store(&target).and_then(|store| {
            store
                .borrow()
                .items
                .iter()
                .find(|annotation| annotation.id == id)
                .map(|annotation| annotation.comment.clone())
        });
        let Some(comment) = comment else {
            return;
        };
        // One editor at a time: an already-open one's editing emphasis in
        // its own store would otherwise stay stuck on.
        if let Some(previous) = self.annotation_editor.as_ref()
            && let Some(store) = self.annotation_store(&previous.target)
        {
            store.borrow_mut().editing = None;
        }
        self.annotation_editor = Some(AnnotationEditor {
            annotation_id: id,
            is_new,
            previous_focus: window.focused(cx),
            target: target.clone(),
        });
        if let Some(store) = self.annotation_store(&target) {
            store.borrow_mut().editing = Some(id);
        }
        self.annotation_comment_input
            .update(cx, |input, cx| input.set_content(comment, cx));
        // The card is a deferred element; its focus handle joins the dispatch
        // tree only after the deferred draw, so focus lands two frames out.
        let focus = self.annotation_comment_input.read(cx).focus();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
        cx.notify();
    }

    /// Write the field's text onto the annotation and close the editor.
    pub(super) fn commit_annotation_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.annotation_editor.take() else {
            return;
        };
        let comment = self.annotation_comment_input.read(cx).content().to_owned();
        if let Some(store) = self.annotation_store(&editor.target) {
            let mut annotations = store.borrow_mut();
            if let Some(annotation) = annotations
                .items
                .iter_mut()
                .find(|annotation| annotation.id == editor.annotation_id)
            {
                annotation.comment = comment;
            }
            annotations.editing = None;
        }
        self.schedule_composer_draft_save(cx);
        self.restore_annotation_focus(editor.previous_focus, cx);
        cx.notify();
    }

    /// Escape: discard the draft comment. A never-confirmed annotation loses
    /// its highlight too; an existing one keeps its stored comment untouched.
    fn discard_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.annotation_editor.take() else {
            return;
        };
        if let Some(store) = self.annotation_store(&editor.target) {
            let mut annotations = store.borrow_mut();
            if editor.is_new {
                annotations
                    .items
                    .retain(|annotation| annotation.id != editor.annotation_id);
            }
            annotations.editing = None;
        }
        self.annotation_comment_input
            .update(cx, |input, cx| input.set_content("", cx));
        self.schedule_composer_draft_save(cx);
        let focus = editor
            .previous_focus
            .unwrap_or_else(|| self.composer_focus(cx));
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The trash button: delete the annotation and its highlight, closing the
    /// editor when it was the one being edited.
    fn remove_annotation(
        &mut self,
        id: u64,
        target: AnnotationTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(store) = self.annotation_store(&target) {
            store
                .borrow_mut()
                .items
                .retain(|annotation| annotation.id != id);
        }
        if let Some(editor) = self
            .annotation_editor
            .take_if(|editor| editor.annotation_id == id)
        {
            if let Some(store) = self.annotation_store(&editor.target) {
                store.borrow_mut().editing = None;
            }
            self.annotation_comment_input
                .update(cx, |input, cx| input.set_content("", cx));
            let focus = editor
                .previous_focus
                .unwrap_or_else(|| self.composer_focus(cx));
            window.focus(&focus, cx);
        }
        self.schedule_composer_draft_save(cx);
        cx.notify();
    }

    /// The composer chip's X: drop every annotation — transcript and file —
    /// and close an open editor.
    fn clear_annotations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.annotation_editor.take() {
            let focus = editor
                .previous_focus
                .unwrap_or_else(|| self.composer_focus(cx));
            window.focus(&focus, cx);
        }
        self.annotation_hover = None;
        self.annotation_press = None;
        {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            annotations.items.clear();
            annotations.hovered = None;
            annotations.editing = None;
        }
        for editor in self.right_panel_file_editors.values() {
            let mut annotations = editor.annotations.borrow_mut();
            annotations.items.clear();
            annotations.hovered = None;
            annotations.editing = None;
        }
        self.pending_file_annotations.clear();
        self.schedule_composer_draft_save(cx);
        cx.notify();
    }

    /// Editor commit paths that lack a `Window` (the field's own submit
    /// subscription) focus through the stored window handle instead.
    fn restore_annotation_focus(&mut self, previous: Option<FocusHandle>, cx: &mut Context<Self>) {
        let focus = previous.unwrap_or_else(|| self.composer_focus(cx));
        let window_handle = self.window_handle;
        let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
    }

    /// The composer chip count and the "is there anything to send" check:
    /// transcript annotations, every file editor's, and restored file
    /// annotations still waiting on their editor.
    pub(super) fn annotation_count(&self) -> usize {
        self.transcript_selection.annotations.borrow().items.len()
            + self
                .right_panel_file_editors
                .values()
                .map(|editor| editor.annotations.borrow().items.len())
                .sum::<usize>()
            + self
                .pending_file_annotations
                .values()
                .map(Vec::len)
                .sum::<usize>()
    }

    pub(super) fn has_annotations(&self) -> bool {
        !self
            .transcript_selection
            .annotations
            .borrow()
            .items
            .is_empty()
            || self
                .right_panel_file_editors
                .values()
                .any(|editor| !editor.annotations.borrow().items.is_empty())
            || self
                .pending_file_annotations
                .values()
                .any(|items| !items.is_empty())
    }

    /// Drain the live sets for a submission — transcript annotations plus
    /// every file editor's and any still pending, merged in creation order so
    /// the "Annotation N" labels match the order the user made them. Parked
    /// sets for other sessions are untouched — only what was on screen ships.
    pub(super) fn drain_annotations(&mut self) -> Vec<TranscriptAnnotation> {
        self.annotation_editor = None;
        self.annotation_hover = None;
        self.annotation_press = None;
        let mut items = {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            annotations.editing = None;
            annotations.hovered = None;
            std::mem::take(&mut annotations.items)
        };
        for editor in self.right_panel_file_editors.values() {
            let mut annotations = editor.annotations.borrow_mut();
            annotations.editing = None;
            annotations.hovered = None;
            items.extend(annotations.items.drain(..));
        }
        items.extend(
            std::mem::take(&mut self.pending_file_annotations)
                .into_values()
                .flatten(),
        );
        items.sort_by_key(|annotation| annotation.id);
        items
    }

    /// Park a submission's drained annotations under the user message that
    /// carries them, so an "Annotation N" citation in the reply keeps a
    /// referent for its underline and tooltip. Called once the message exists
    /// in the session; a submission that is later unwound leaves a set keyed
    /// to a message id nothing can resolve, which is dead weight only.
    pub(super) fn record_sent_annotations(
        &mut self,
        session_id: Uuid,
        user_message_id: Uuid,
        annotations: &[TranscriptAnnotation],
    ) {
        if annotations.is_empty() {
            return;
        }
        self.sent_annotations
            .entry(session_id)
            .or_default()
            .push((user_message_id, Rc::new(annotations.to_vec())));
    }

    /// The annotation set an `Annotation N` citation inside `message`'s reply
    /// resolves against: the set carried by the most recent annotated user
    /// message before it. Cached under the row-kinds fingerprint like the
    /// response footers — sends and rewinds both move it.
    pub(super) fn annotation_ref_set(
        &self,
        message_id: Uuid,
    ) -> Option<Rc<Vec<TranscriptAnnotation>>> {
        self.refresh_transcript_row_kinds();
        let fingerprint = self.transcript_row_kinds_fingerprint.get();
        if self.annotation_ref_sets_fingerprint.get() != fingerprint {
            let mut resolved = HashMap::new();
            if let Some(session) = self.selected_session()
                && let Some(sets) = self.sent_annotations.get(&session.id)
            {
                let mut current = None;
                for message in &session.messages {
                    if message.role == MessageRole::User {
                        if let Some((_, set)) = sets.iter().find(|(id, _)| *id == message.id) {
                            current = Some(set.clone());
                        }
                    } else if message.role == MessageRole::Assistant
                        && let Some(set) = &current
                    {
                        resolved.insert(message.id, set.clone());
                    }
                }
            }
            *self.annotation_ref_sets.borrow_mut() = resolved;
            self.annotation_ref_sets_fingerprint.set(fingerprint);
        }
        self.annotation_ref_sets.borrow().get(&message_id).cloned()
    }

    /// First on-screen glyph rect of a citation's range, for the tooltip to
    /// anchor to. `None` when the row is virtualized away.
    fn annotation_ref_anchor(&self, key: &TextKey, range: &Range<usize>) -> Option<Bounds<Pixels>> {
        let registry = self.transcript_selection.registry.borrow();
        let entry = registry.entries().iter().find(|entry| entry.key == *key)?;
        if entry.geometry.is_missing() {
            return None;
        }
        let viewport = self.active_transcript_rows().viewport_bounds();
        text_range_bounds(&entry.geometry, range)
            .into_iter()
            .find(|rect| rect.bottom() > viewport.top() && rect.top() < viewport.bottom())
    }

    /// Drop annotations whose message no longer renders — a rewind removes it
    /// from the session entirely. Runs once per frame and only when the set is
    /// non-empty, so the common path costs one emptiness check.
    pub(super) fn prune_transcript_annotations(&mut self, cx: &mut Context<Self>) {
        let pruned = {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            if annotations.items.is_empty() {
                return;
            }
            let before = annotations.items.len();
            match self.selected_session() {
                Some(session) => {
                    annotations.items.retain(|annotation| {
                        session.messages.iter().any(|message| {
                            message.id == annotation.message_id
                                && message.role == MessageRole::Assistant
                        })
                    });
                }
                None => annotations.items.clear(),
            }
            annotations.items.len() != before
        };
        if pruned {
            self.schedule_composer_draft_save(cx);
        }
        let editor_gone = self.annotation_editor.as_ref().is_some_and(|editor| {
            self.annotation_store(&editor.target).is_none_or(|store| {
                !store
                    .borrow()
                    .items
                    .iter()
                    .any(|annotation| annotation.id == editor.annotation_id)
            })
        });
        if editor_gone {
            if let Some(store) = self
                .annotation_editor
                .as_ref()
                .and_then(|editor| self.annotation_store(&editor.target))
            {
                store.borrow_mut().editing = None;
            }
            self.annotation_editor = None;
            self.annotation_comment_input
                .update(cx, |input, cx| input.set_content("", cx));
        }
    }

    /// The "Add to chat" pill shared by the transcript's and the file
    /// editor's offers: focusable, focus-ringed, activating on Enter or
    /// Space, and swallowing its mouse-down so it can't clear the selection
    /// being offered.
    fn add_to_chat_button(
        &self,
        element_id: &'static str,
        focus_key: &'static str,
        shortcut_label: Option<String>,
        cx: &mut Context<Self>,
        on_accept: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let focus = self.transcript_control_focus(focus_key, cx);
        div()
            .id(element_id)
            .occlude()
            .track_focus(&focus)
            .tab_index(0)
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(9.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_md()
            .flex()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(sp(12.5))
            .line_height(sp(14.0))
            .text_color(theme.text)
            .focus_visible(|element| element.border_color(theme.accent))
            .child(icon("icons/compose.svg", 12.0, theme.text_secondary))
            .child(tr!("annotations.add_to_chat"))
            .when_some(shortcut_label, |element, label| {
                element.child(
                    div()
                        .flex_none()
                        .text_color(theme.text_tertiary)
                        .child(label),
                )
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_activation(cx, move |this, window, cx| on_accept(this, window, cx))
    }

    /// The floating "Add to chat" pill over a settled assistant-message
    /// selection.
    pub(super) fn render_annotation_offer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (_, spans) = self.annotatable_selection()?;
        let anchor = self.spans_anchor(&spans)?;
        // The chord sits on the transcript's key context, so resolve it as if
        // the transcript were focused — true whenever the pill can show.
        let shortcut_label =
            ShortcutHint::action_in(&AddToChat, &self.transcript_focus).resolve(window, cx);
        let button = self.add_to_chat_button(
            "annotation-add-to-chat",
            "annotation-add-to-chat",
            shortcut_label,
            cx,
            |this, window, cx| this.annotate_selection(window, cx),
        );
        Some(
            deferred(FloatingSurface::new(
                motion::surface_enter(
                    "annotate-selection-enter",
                    button,
                    MenuAlign::AboveLeft.anchor_point(anchor, px(6.0)),
                )
                .into_any_element(),
                anchor,
                MenuAlign::AboveLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The floating "Add to chat" pill over a settled file-editor selection —
    /// the same control, anchored to the field's own painted selection.
    pub(super) fn render_file_annotation_offer(
        &self,
        relative_path: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let range = self.annotatable_file_selection(relative_path, cx)?;
        let editor = self.right_panel_file_editors.get(relative_path)?;
        let field = editor.state.read(cx);
        let viewport = self.right_panel_editor_scroll_handle.bounds();
        let anchor = field
            .range_bounds(&range)
            .into_iter()
            .find(|rect| rect.bottom() > viewport.top() && rect.top() < viewport.bottom())?;
        // Resolve the chord as if the field were focused — the FileEditorPane
        // binding, not the transcript's.
        let shortcut_label =
            ShortcutHint::action_in(&AddToChat, &field.focus()).resolve(window, cx);
        let path = relative_path.to_owned();
        let button = self.add_to_chat_button(
            "file-annotation-add-to-chat",
            "file-annotation-add-to-chat",
            shortcut_label,
            cx,
            move |this, window, cx| this.annotate_file_selection(&path, window, cx),
        );
        Some(
            deferred(FloatingSurface::new(
                motion::surface_enter(
                    "annotate-file-selection-enter",
                    button,
                    MenuAlign::AboveLeft.anchor_point(anchor, px(6.0)),
                )
                .into_any_element(),
                anchor,
                MenuAlign::AboveLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The shared comment-editor card. The caller anchors it below the
    /// annotation's first visible line — transcript span or file range.
    fn annotation_editor_card(
        &self,
        element_id: &'static str,
        anchor: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let trash_focus = self.transcript_control_focus("annotation-remove", cx);
        let card = div()
            .occlude()
            .key_context(ANNOTATION_CONTEXT)
            .on_action(cx.listener(|this, _: &DismissMenu, window, cx| {
                this.discard_annotation_editor(window, cx);
            }))
            // The field's escape arrives as `Clear` — it is not opted into
            // `clear_on_escape`, so the action propagates up to the card.
            .on_action(cx.listener(|this, _: &Clear, window, cx| {
                this.discard_annotation_editor(window, cx);
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.commit_annotation_editor(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id(element_id)
                    .w(px(280.0))
                    .p(px(6.0))
                    .rounded(px(11.0))
                    .border(hairline())
                    .border_color(theme.border_strong)
                    .bg(theme.raised)
                    .shadow_lg()
                    .flex()
                    .items_start()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.annotation_comment_input.clone()),
                    )
                    .child(
                        icon_button("annotation-remove", "icons/trash.svg", theme)
                            .track_focus(&trash_focus)
                            .tab_index(0)
                            .tooltip(Tooltip::text(tr!("annotations.remove")))
                            .on_activation(cx, |this, window, cx| {
                                if let Some(editor) = this.annotation_editor.clone() {
                                    this.remove_annotation(
                                        editor.annotation_id,
                                        editor.target.clone(),
                                        window,
                                        cx,
                                    );
                                }
                            }),
                    ),
            );
        deferred(FloatingSurface::new(
            motion::surface_enter(
                "annotation-editor-enter",
                card,
                MenuAlign::BelowLeft.anchor_point(anchor, px(6.0)),
            )
            .into_any_element(),
            anchor,
            MenuAlign::BelowLeft,
            px(6.0),
            px(8.0),
        ))
        .with_priority(3)
        .into_any_element()
    }

    /// The floating comment editor, anchored below the annotation's first
    /// visible line.
    pub(super) fn render_annotation_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.annotation_editor.as_ref()?;
        if editor.target != AnnotationTarget::Transcript {
            return None;
        }
        let anchor = self.annotation_anchor(editor.annotation_id)?;
        Some(self.annotation_editor_card("annotation-editor-card", anchor, cx))
    }

    /// The same floating comment editor over a file annotation, anchored
    /// below its first visible line in the editor.
    pub(super) fn render_file_annotation_editor(
        &self,
        relative_path: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.annotation_editor.as_ref()?;
        if editor.target != AnnotationTarget::File(relative_path.to_owned()) {
            return None;
        }
        let anchor = self.file_annotation_anchor(relative_path, editor.annotation_id, cx)?;
        Some(self.annotation_editor_card("annotation-editor-card", anchor, cx))
    }

    /// The shared comment tooltip card: the annotation's comment, anchored
    /// above the highlight. Pointer-transparent by construction — no hit
    /// targets.
    fn annotation_tooltip_card(
        &self,
        anchor: Bounds<Pixels>,
        comment: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let card = div()
            .max_w(px(320.0))
            .px(px(7.0))
            .py(px(4.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_md()
            .text_size(sp(12.5))
            .line_height(sp(15.0))
            .text_color(theme.text_secondary)
            .child(comment);
        deferred(FloatingSurface::new(
            card.into_any_element(),
            anchor,
            MenuAlign::AboveLeft,
            px(6.0),
            px(8.0),
        ))
        .with_priority(2)
        .into_any_element()
    }

    /// The comment tooltip, surfaced after the hover delay over a confirmed
    /// annotation. Pointer-transparent by construction: it anchors above the
    /// highlight with a gap and carries no hit targets.
    pub(super) fn render_annotation_tooltip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let hover = self
            .annotation_hover
            .as_ref()
            .filter(|hover| hover.visible && hover.target == AnnotationTarget::Transcript)?;
        // The editor for this annotation already shows the comment.
        if self
            .annotation_editor
            .as_ref()
            .is_some_and(|editor| editor.annotation_id == hover.id)
        {
            return None;
        }
        let comment = {
            let annotations = self.transcript_selection.annotations.borrow();
            annotations
                .items
                .iter()
                .find(|annotation| annotation.id == hover.id)
                .map(|annotation| annotation.comment.clone())
        }?;
        if comment.trim().is_empty() {
            return None;
        }
        let anchor = self.annotation_anchor(hover.id)?;
        Some(self.annotation_tooltip_card(anchor, comment, cx))
    }

    /// The same comment tooltip over a file annotation's highlight.
    pub(super) fn render_file_annotation_tooltip(
        &self,
        relative_path: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let hover = self.annotation_hover.as_ref().filter(|hover| {
            hover.visible && hover.target == AnnotationTarget::File(relative_path.to_owned())
        })?;
        if self
            .annotation_editor
            .as_ref()
            .is_some_and(|editor| editor.annotation_id == hover.id)
        {
            return None;
        }
        let comment = self
            .right_panel_file_editors
            .get(relative_path)
            .and_then(|editor| {
                editor
                    .annotations
                    .borrow()
                    .items
                    .iter()
                    .find(|annotation| annotation.id == hover.id)
                    .map(|annotation| annotation.comment.clone())
            })?;
        if comment.trim().is_empty() {
            return None;
        }
        let anchor = self.file_annotation_anchor(relative_path, hover.id, cx)?;
        Some(self.annotation_tooltip_card(anchor, comment, cx))
    }

    /// The citation tooltip, surfaced after the hover delay over an
    /// `Annotation N` mention: the annotated passage dimmed above the user's
    /// comment, so the label resolves to what was actually said. Like the
    /// highlight tooltip it is pointer-transparent — anchored above the
    /// citation with a gap and no hit targets.
    pub(super) fn render_annotation_ref_tooltip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let hover = self
            .annotation_ref_hover
            .as_ref()
            .filter(|hover| hover.visible)?;
        let message_id = Uuid::parse_str(hover.key.row.strip_prefix("message-")?).ok()?;
        let set = self.annotation_ref_set(message_id)?;
        let annotation = set.get(hover.index - 1)?;
        let anchor = self.annotation_ref_anchor(&hover.key, &hover.range)?;
        let theme = Theme::current(cx);
        let quote = annotation_quote_preview(annotation);
        let comment = annotation.comment.trim().to_owned();
        let card = div()
            .max_w(px(320.0))
            .px(px(7.0))
            .py(px(4.0))
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_md()
            .text_size(sp(12.5))
            .line_height(sp(15.0))
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(div().text_color(theme.text_tertiary).child(quote))
            .when(!comment.is_empty(), |card| {
                card.child(div().text_color(theme.text_secondary).child(comment))
            });
        Some(
            deferred(FloatingSurface::new(
                card.into_any_element(),
                anchor,
                MenuAlign::AboveLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The composer's "N annotations" chip — transcript and file annotations
    /// counted together — with an always-visible clear-all control: tabbable,
    /// focus-ringed, activating on Enter or Space.
    pub(super) fn render_annotation_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.annotation_count();
        if count == 0 {
            return None;
        }
        let theme = Theme::current(cx);
        let label = if count == 1 {
            tr!("annotations.count_one")
        } else {
            tr!("annotations.count_many", count = count)
        };
        let clear_focus = self.transcript_control_focus("annotation-clear-all", cx);
        Some(
            div()
                .px(px(14.0))
                .pb(px(6.0))
                .flex()
                .child(
                    div()
                        .flex_none()
                        .rounded_full()
                        .border(hairline())
                        .border_color(theme.border_strong)
                        .pl(px(8.0))
                        .pr(px(4.0))
                        .py(px(2.0))
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .text_size(sp(12.0))
                        .line_height(sp(14.0))
                        .text_color(theme.text_secondary)
                        .child(label)
                        .child(
                            div()
                                .id("annotation-clear-all")
                                .track_focus(&clear_focus)
                                .tab_index(0)
                                .size(px(18.0))
                                .rounded(px(5.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_default()
                                .focus_visible(|element| {
                                    element.border(hairline()).border_color(theme.accent)
                                })
                                .hover(|element| element.bg(theme.overlay_strong))
                                .child(icon("icons/x.svg", 10.0, theme.text_tertiary))
                                .tooltip(Tooltip::text(tr!("annotations.remove_all")))
                                .on_activation(cx, |this, window, cx| {
                                    this.clear_annotations(window, cx);
                                }),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The frame's annotation mouse listeners, layered over the selection's.
    /// Bubble-phase listeners run in reverse paint order, so these fire ahead
    /// of the selection's — the click-vs-drag check below reads `spans`, which
    /// a real drag has already populated by mouse-up.
    ///
    /// Like the selection listeners these bypass hitbox dispatch; the caller
    /// prepaints a region hitbox whose id gates them so a floating surface
    /// covering the region doesn't trigger hovers or presses on the
    /// highlights beneath it.
    fn install_annotation_input(
        region: HitboxId,
        window: &mut Window,
        _cx: &mut App,
        selection: &TranscriptSelection,
        waku: &WeakEntity<Waku>,
    ) {
        window.on_mouse_event({
            let selection = selection.clone();
            let waku = waku.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !region.is_hovered(window)
                {
                    return;
                }
                if let Some(id) = annotation_hit_at(&selection, event.position) {
                    let _ = waku.update(cx, |this, _| {
                        this.annotation_press = Some(AnnotationPress {
                            id,
                            position: event.position,
                            target: AnnotationTarget::Transcript,
                        });
                    });
                } else if let Some(hit) =
                    git_panel::transcript_commit_hit_at(&selection, event.position)
                {
                    let _ = waku.update(cx, |this, _| {
                        this.transcript_commit_press = Some(git_panel::TranscriptCommitPress {
                            key: hit.key,
                            range: hit.range,
                            sha: hit.sha,
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
                let hit = annotation_hit_at(&selection, event.position);
                // A citation inside an annotated passage loses to the
                // highlight — its tooltip already describes the comment.
                let ref_hit = if hit.is_none() {
                    annotation_ref_hit_at(&selection, event.position)
                } else {
                    None
                };
                let commit_hit = if hit.is_none() && ref_hit.is_none() {
                    git_panel::transcript_commit_hit_at(&selection, event.position)
                } else {
                    None
                };
                let hovered_commit = commit_hit
                    .as_ref()
                    .map(|hit| (hit.key.clone(), hit.range.clone()));
                let changed = {
                    let mut annotations = selection.annotations.borrow_mut();
                    let mut hovered_commit_state = selection.hovered_commit.borrow_mut();
                    let hovered_ref = ref_hit
                        .as_ref()
                        .map(|hit| (hit.key.clone(), hit.range.clone()));
                    if annotations.hovered == hit
                        && annotations.hovered_ref == hovered_ref
                        && *hovered_commit_state == hovered_commit
                    {
                        false
                    } else {
                        annotations.hovered = hit;
                        annotations.hovered_ref = hovered_ref;
                        *hovered_commit_state = hovered_commit;
                        true
                    }
                };
                if changed {
                    let _ = waku.update(cx, |this, cx| {
                        this.annotation_hover_changed(
                            hit.map(|id| (id, AnnotationTarget::Transcript)),
                            cx,
                        );
                        this.annotation_ref_hover_changed(ref_hit, cx);
                        this.transcript_commit_hover_changed(commit_hit, cx);
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
                let commit_press = waku
                    .update(cx, |this, _| this.transcript_commit_press.take())
                    .ok()
                    .flatten();
                if let Some(press) = commit_press {
                    let still_hit = git_panel::transcript_commit_hit_at(&selection, event.position)
                        .is_some_and(|hit| {
                            hit.key == press.key && hit.range == press.range && hit.sha == press.sha
                        });
                    let moved = event.position - press.position;
                    if selection.selection.borrow().is_empty()
                        && still_hit
                        && moved.x.abs() <= px(4.0)
                        && moved.y.abs() <= px(4.0)
                    {
                        let _ = waku.update(cx, |this, cx| {
                            this.open_transcript_commit_diff(
                                git_panel::TranscriptCommitHit {
                                    key: press.key,
                                    range: press.range,
                                    sha: press.sha,
                                },
                                cx,
                            );
                        });
                    }
                    return;
                }
                let press = waku
                    .update(cx, |this, _| {
                        this.annotation_press
                            .take_if(|press| press.target == AnnotationTarget::Transcript)
                    })
                    .ok()
                    .flatten();
                let Some(press) = press else {
                    return;
                };
                // A drag that started on a highlight ends with a selection, not
                // a press — only a clean click reopens the editor.
                if !selection.selection.borrow().is_empty() {
                    return;
                }
                let moved = event.position - press.position;
                if moved.x.abs() > px(4.0) || moved.y.abs() > px(4.0) {
                    return;
                }
                if annotation_hit_at(&selection, event.position) != Some(press.id) {
                    return;
                }
                let _ = waku.update(cx, |this, cx| {
                    this.open_annotation_editor(
                        press.id,
                        false,
                        AnnotationTarget::Transcript,
                        window,
                        cx,
                    )
                });
            }
        });
    }

    /// The file editor's annotation mouse listeners — the same
    /// press/hover/click pattern as [`Self::install_annotation_input`],
    /// hit-testing the field's painted highlight ranges. Presses are armed
    /// and consumed under a `File` target so the two listener sets, both
    /// window-level, never steal each other's.
    fn install_file_annotation_input(
        region: HitboxId,
        window: &mut Window,
        _cx: &mut App,
        relative_path: &str,
        waku: &WeakEntity<Waku>,
    ) {
        window.on_mouse_event({
            let path = relative_path.to_owned();
            let waku = waku.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !region.is_hovered(window)
                {
                    return;
                }
                let hit = waku
                    .read_with(cx, |this, cx| {
                        this.file_annotation_hit_at(&path, event.position, cx)
                    })
                    .ok()
                    .flatten();
                if let Some(id) = hit {
                    let _ = waku.update(cx, |this, _| {
                        this.annotation_press = Some(AnnotationPress {
                            id,
                            position: event.position,
                            target: AnnotationTarget::File(path.clone()),
                        });
                    });
                }
            }
        });

        window.on_mouse_event({
            let path = relative_path.to_owned();
            let waku = waku.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.dragging() || !region.is_hovered(window)
                {
                    return;
                }
                let changed = waku
                    .update(cx, |this, cx| {
                        let hit = this.file_annotation_hit_at(&path, event.position, cx);
                        let Some(editor) = this.right_panel_file_editors.get(&path) else {
                            return false;
                        };
                        if editor.annotations.borrow().hovered == hit {
                            return false;
                        }
                        editor.annotations.borrow_mut().hovered = hit;
                        this.annotation_hover_changed(
                            hit.map(|id| (id, AnnotationTarget::File(path.clone()))),
                            cx,
                        );
                        true
                    })
                    .unwrap_or(false);
                if changed {
                    window.refresh();
                }
            }
        });

        window.on_mouse_event({
            let path = relative_path.to_owned();
            let waku = waku.clone();
            move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                let press = waku
                    .update(cx, |this, _| {
                        this.annotation_press
                            .take_if(|press| press.target == AnnotationTarget::File(path.clone()))
                    })
                    .ok()
                    .flatten();
                let Some(press) = press else {
                    return;
                };
                // A drag that started on a highlight ends with a selection,
                // not a press — only a clean click reopens the editor.
                let selected = waku
                    .read_with(cx, |this, cx| {
                        this.right_panel_file_editors
                            .get(&path)
                            .is_some_and(|editor| {
                                !editor.state.read(cx).selected_range().is_empty()
                            })
                    })
                    .unwrap_or(true);
                if selected {
                    return;
                }
                let moved = event.position - press.position;
                if moved.x.abs() > px(4.0) || moved.y.abs() > px(4.0) {
                    return;
                }
                let still_hit = waku
                    .read_with(cx, |this, cx| {
                        this.file_annotation_hit_at(&path, event.position, cx) == Some(press.id)
                    })
                    .unwrap_or(false);
                if !still_hit {
                    return;
                }
                let _ = waku.update(cx, |this, cx| {
                    this.open_annotation_editor(
                        press.id,
                        false,
                        AnnotationTarget::File(path.clone()),
                        window,
                        cx,
                    )
                });
            }
        });
    }

    fn annotation_hover_changed(
        &mut self,
        hit: Option<(u64, AnnotationTarget)>,
        cx: &mut Context<Self>,
    ) {
        self.annotation_hover = hit.clone().map(|(id, target)| AnnotationHover {
            id,
            target,
            visible: false,
        });
        if let Some((id, target)) = hit {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ANNOTATION_HOVER_DELAY).await;
                let _ = this.update(cx, |this, cx| {
                    if this
                        .annotation_hover
                        .as_ref()
                        .is_some_and(|hover| hover.id == id && !hover.visible)
                    {
                        this.annotation_hover = Some(AnnotationHover {
                            id,
                            target,
                            visible: true,
                        });
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn annotation_ref_hover_changed(
        &mut self,
        hit: Option<AnnotationRefHit>,
        cx: &mut Context<Self>,
    ) {
        self.annotation_ref_hover = hit.map(|hit| AnnotationRefHover {
            key: hit.key,
            range: hit.range,
            index: hit.index,
            visible: false,
        });
        if let Some(hover) = &self.annotation_ref_hover {
            let key = hover.key.clone();
            let range = hover.range.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ANNOTATION_HOVER_DELAY).await;
                let _ = this.update(cx, |this, cx| {
                    if this.annotation_ref_hover.as_ref().is_some_and(|hover| {
                        !hover.visible && hover.key == key && hover.range == range
                    }) && let Some(hover) = this.annotation_ref_hover.as_mut()
                    {
                        hover.visible = true;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// A full-size canvas painting the frame's annotation listeners — see
    /// [`Self::install_annotation_input`]. Its bounds mark the region the
    /// window-level listeners may act on.
    pub(super) fn transcript_annotation_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.transcript_selection.clone();
        let waku = cx.entity().downgrade();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
            move |_, region, window, cx| {
                Self::install_annotation_input(region, window, cx, &selection, &waku)
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// The file editor's listener canvas — see
    /// [`Self::install_file_annotation_input`]. Rendered inside the editor's
    /// scroll container so the region only ever covers its text area.
    pub(super) fn file_annotation_input(
        &self,
        relative_path: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = relative_path.to_owned();
        let waku = cx.entity().downgrade();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
            move |_, region, window, cx| {
                Self::install_file_annotation_input(region, window, cx, &path, &waku)
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Push the file editor's live annotation ranges into the field's paint
    /// set. Runs from render — the field's `last_layout` and the annotation
    /// ranges are both in-memory — and costs a few short string compares per
    /// frame, all short-circuited by the field's own unchanged-check.
    pub(super) fn sync_file_annotation_washes(&self, relative_path: &str, cx: &mut Context<Self>) {
        let Some(editor) = self.right_panel_file_editors.get(relative_path) else {
            return;
        };
        let ranges = {
            let annotations = editor.annotations.borrow();
            if annotations.items.is_empty() {
                Vec::new()
            } else {
                let content = editor.state.read(cx).content();
                annotations
                    .items
                    .iter()
                    .filter_map(|annotation| {
                        let file = annotation.file.as_ref()?;
                        file_annotation_live(annotation, content).then(|| {
                            (
                                file.range.clone(),
                                annotations.hovered == Some(annotation.id)
                                    || annotations.editing == Some(annotation.id),
                            )
                        })
                    })
                    .collect()
            }
        };
        editor
            .state
            .update(cx, |input, cx| input.set_annotation_ranges(ranges, cx));
    }
}

/// The annotation span still describes the element's painted text: the
/// snapshot and the live text agree up to the span's end. A message edited or
/// extended ahead of the range shifts the bytes and the highlight drops rather
/// than marking different words.
fn annotation_span_live(span: &Span, text: &str) -> bool {
    let end = span.range.end.min(text.len());
    span.range.start < end
        && end <= span.text.len()
        && span.text.as_bytes()[..end] == text.as_bytes()[..end]
}

/// The file annotation's range still covers its snapshot in the editor's
/// current content — the same drop-rather-than-mismatch rule as
/// [`annotation_span_live`], checked where the passage actually lives.
/// `get` bounds- and boundary-checks the range, so an edited file can only
/// drop the highlight, never panic or mark different words.
fn file_annotation_live(annotation: &TranscriptAnnotation, content: &str) -> bool {
    let Some(file) = &annotation.file else {
        return false;
    };
    let Some(span) = annotation.spans.first() else {
        return false;
    };
    content
        .get(file.range.clone())
        .is_some_and(|slice| slice == &*span.text)
}

/// The annotation whose highlight contains `position`, consulting the frame's
/// painted geometry.
fn annotation_hit_at(selection: &TranscriptSelection, position: Point<Pixels>) -> Option<u64> {
    let annotations = selection.annotations.borrow();
    if annotations.items.is_empty() {
        return None;
    }
    let registry = selection.registry.borrow();
    for annotation in &annotations.items {
        for span in &annotation.spans {
            let Some(entry) = registry
                .entries()
                .iter()
                .find(|entry| entry.key == span.key)
            else {
                continue;
            };
            if entry.geometry.is_missing() || !annotation_span_live(span, &entry.text) {
                continue;
            }
            let end = span.range.end.min(entry.text.len());
            let hit = text_range_bounds(&entry.geometry, &(span.range.start..end))
                .iter()
                .any(|rect| rect.contains(&position));
            if hit {
                return Some(annotation.id);
            }
        }
    }
    None
}

/// The `Annotation N` citation containing `position`, consulting the frame's
/// painted geometry — the underline ranges each element registered as it
/// painted.
fn annotation_ref_hit_at(
    selection: &TranscriptSelection,
    position: Point<Pixels>,
) -> Option<AnnotationRefHit> {
    let registry = selection.registry.borrow();
    for entry in registry.entries() {
        if entry.annotation_refs.is_empty() || entry.geometry.is_missing() {
            continue;
        }
        for (range, index) in &entry.annotation_refs {
            let hit = text_range_bounds(&entry.geometry, range)
                .iter()
                .any(|rect| rect.contains(&position));
            if hit {
                return Some(AnnotationRefHit {
                    key: entry.key.clone(),
                    range: range.clone(),
                    index: *index,
                });
            }
        }
    }
    None
}

/// The quoted passage as the citation tooltip shows it — trimmed and capped
/// so a long selection cannot blow the card up. A file annotation's quote
/// leads with its `@path` and line marker, so the citation reads "Annotation
/// N → that spot in that file".
fn annotation_quote_preview(annotation: &TranscriptAnnotation) -> String {
    const MAX_CHARS: usize = 240;
    let quote = annotation.quoted_text();
    let quote = quote.trim();
    let body = if quote.chars().count() <= MAX_CHARS {
        quote.to_owned()
    } else {
        let preview: String = quote.chars().take(MAX_CHARS).collect();
        format!("{}…", preview.trim_end())
    };
    match &annotation.file {
        Some(file) if file.start_line == file.end_line => {
            format!("@{} · line {}\n{}", file.path, file.start_line, body)
        }
        Some(file) => format!(
            "@{} · lines {}-{}\n{}",
            file.path, file.start_line, file.end_line, body
        ),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::md::selection::TextKey;

    fn annotation(id: u64, text: &str, comment: &str) -> TranscriptAnnotation {
        TranscriptAnnotation {
            id,
            message_id: Uuid::nil(),
            spans: vec![Span {
                key: TextKey::new("message-0000", 0),
                range: 0..text.len(),
                text: Rc::from(text),
                block_break: false,
            }],
            comment: comment.to_owned(),
            file: None,
        }
    }

    /// A file annotation as `annotate_file_selection` builds it: one span
    /// over the selected text and the file provenance alongside.
    fn file_annotation(
        id: u64,
        path: &str,
        text: &str,
        range: Range<usize>,
        start_line: usize,
        end_line: usize,
        comment: &str,
    ) -> TranscriptAnnotation {
        TranscriptAnnotation {
            id,
            message_id: Uuid::nil(),
            spans: vec![Span {
                key: TextKey::new(format!("file:{path}"), 0),
                range: 0..text.len(),
                text: Rc::from(text),
                block_break: false,
            }],
            comment: comment.to_owned(),
            file: Some(FileAnnotation {
                path: path.to_owned(),
                range,
                start_line,
                end_line,
            }),
        }
    }

    #[test]
    fn prompt_prefix_is_empty_without_annotations() {
        assert_eq!(annotation_prompt_prefix(&[]), "");
    }

    #[test]
    fn prompt_prefix_matches_the_submission_format() {
        let annotations = [
            annotation(1, "quoted agent text", "your comment"),
            annotation(2, "second passage", "another comment"),
        ];
        assert_eq!(
            annotation_prompt_prefix(&annotations),
            concat!(
                "Annotation 1:\n> quoted agent text\n\nComment: your comment\n\n",
                "Annotation 2:\n> second passage\n\nComment: another comment\n\n",
                "When responding, refer to the annotations above by their label (e.g. \"Annotation 1\") when appropriate.\n\n",
            )
        );
    }

    #[test]
    fn prompt_prefix_quotes_each_line_of_a_multiline_passage() {
        let annotations = [annotation(1, "first line\nsecond line", "note")];
        assert_eq!(
            annotation_prompt_prefix(&annotations),
            concat!(
                "Annotation 1:\n> first line\n> second line\n\nComment: note\n\n",
                "When responding, refer to the annotations above by their label (e.g. \"Annotation 1\") when appropriate.\n\n",
            )
        );
    }

    #[test]
    fn display_content_falls_back_to_the_quoted_passage() {
        assert_eq!(
            annotation_display_content(&[annotation(1, "the passage", "")]),
            "the passage"
        );
        assert_eq!(
            annotation_display_content(&[annotation(1, "the passage", "a comment")]),
            "a comment"
        );
    }

    #[test]
    fn bubble_content_quotes_each_passage_above_its_comment() {
        assert_eq!(
            annotation_bubble_content(&[annotation(1, "the passage", "a comment")], ""),
            "> the passage\n\na comment\n\n"
        );
        // No comment still marks the attached passage.
        assert_eq!(
            annotation_bubble_content(&[annotation(1, "the passage", "")], ""),
            "> the passage\n\n"
        );
    }

    #[test]
    fn bubble_content_appends_the_typed_text() {
        assert_eq!(
            annotation_bubble_content(
                &[
                    annotation(1, "first passage", "note"),
                    annotation(2, "second\npassage", ""),
                ],
                "fix this",
            ),
            "> first passage\n\nnote\n\n> second\n> passage\n\nfix this"
        );
    }

    #[test]
    fn prompt_prefix_quotes_a_file_annotation_with_path_marker_and_fence() {
        let annotations = [file_annotation(
            1,
            "src/main.rs",
            "fn main() {\n    run();\n}",
            40..59,
            3,
            5,
            "why twice?",
        )];
        assert_eq!(
            annotation_prompt_prefix(&annotations),
            concat!(
                "Annotation 1:\n",
                "> @src/main.rs\n",
                "> [Selected lines 3-5]\n",
                "> ```\n",
                "> fn main() {\n",
                ">     run();\n",
                "> }\n",
                "> ```\n",
                "\nComment: why twice?\n\n",
                "When responding, refer to the annotations above by their label (e.g. \"Annotation 1\") when appropriate.\n\n",
            )
        );
    }

    #[test]
    fn prompt_prefix_marks_a_single_line_file_selection() {
        let annotations = [file_annotation(
            1,
            "src/lib.rs",
            "let x = 1;",
            10..20,
            7,
            7,
            "",
        )];
        assert_eq!(
            annotation_prompt_prefix(&annotations),
            concat!(
                "Annotation 1:\n",
                "> @src/lib.rs\n",
                "> [Selected line 7]\n",
                "> ```\n",
                "> let x = 1;\n",
                "> ```\n",
                "\nComment: \n\n",
                "When responding, refer to the annotations above by their label (e.g. \"Annotation 1\") when appropriate.\n\n",
            )
        );
    }

    #[test]
    fn mixed_annotations_number_in_the_drained_order() {
        let annotations = [
            annotation(1, "quoted agent text", "transcript note"),
            file_annotation(2, "src/main.rs", "run();", 5..11, 9, 9, "file note"),
        ];
        assert_eq!(
            annotation_prompt_prefix(&annotations),
            concat!(
                "Annotation 1:\n> quoted agent text\n\nComment: transcript note\n\n",
                "Annotation 2:\n> @src/main.rs\n> [Selected line 9]\n> ```\n> run();\n> ```\n\nComment: file note\n\n",
                "When responding, refer to the annotations above by their label (e.g. \"Annotation 1\") when appropriate.\n\n",
            )
        );
    }

    #[test]
    fn display_content_summarises_a_file_annotation_by_its_path() {
        let annotations = [file_annotation(1, "src/main.rs", "code", 0..4, 1, 1, "")];
        assert_eq!(annotation_display_content(&annotations), "@src/main.rs");
    }

    #[test]
    fn bubble_content_quotes_a_file_annotation_the_same_way() {
        let annotations = [file_annotation(
            1,
            "src/main.rs",
            "run();",
            5..11,
            9,
            9,
            "note",
        )];
        assert_eq!(
            annotation_bubble_content(&annotations, "fix this"),
            "> @src/main.rs\n> [Selected line 9]\n> ```\n> run();\n> ```\n\nnote\n\nfix this"
        );
    }

    #[test]
    fn file_annotation_stays_live_while_its_range_matches_the_snapshot() {
        let content = "let a = 1;\nlet b = 2;\nlet c = 3;\n";
        let pinned = file_annotation(1, "src/x.rs", "let b = 2;", 11..21, 2, 2, "");
        assert!(file_annotation_live(&pinned, content));
        // An edit ahead of the range shifts the bytes; the highlight drops.
        assert!(!file_annotation_live(
            &pinned,
            "x\nlet a = 1;\nlet b = 2;\nlet c = 3;\n"
        ));
        // A shorter file just stops matching — no panic.
        assert!(!file_annotation_live(&pinned, "let a = 1;\n"));
        // A transcript annotation is never file-live.
        let transcript = annotation(2, "text", "");
        assert!(!file_annotation_live(&transcript, content));
    }
}
