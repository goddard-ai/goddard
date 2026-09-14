//! Transcript annotations: commented highlights over agent messages.
//!
//! Selecting text inside a single assistant message offers an "Add to chat"
//! pill; accepting it pins a highlight on the passage and opens a floating
//! comment editor. Confirmed annotations stay highlighted — hover previews the
//! comment, a click reopens the editor — and the composer shows an
//! "N annotations" chip until the next submission, which carries the comments
//! to the provider as a quoted header above the typed prompt and echoes the
//! quoted passages in the sent bubble.
//!
//! Annotations live in memory only, one set per session. The painted set sits
//! on [`TranscriptSelection`] so the renderer can reach it from paint
//! closures; session switches park and restore it (see
//! `reset_visible_state`), and sending drains it into the prompt.

use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, canvas, deferred, div, px,
};

use crate::input::Clear;
use crate::md::render::{TranscriptSelection, text_range_bounds};
use crate::md::selection::{Span, TranscriptAnnotation};
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

/// The open comment editor. `is_new` marks an annotation Enter/Escape has not
/// yet confirmed; discarding it removes the highlight too.
#[derive(Clone, Debug)]
pub(super) struct AnnotationEditor {
    pub annotation_id: u64,
    pub is_new: bool,
    /// Focus to hand back when the editor closes — almost always the composer.
    pub previous_focus: Option<FocusHandle>,
}

/// Mouse-down on a highlight, held until mouse-up proves it was a click
/// (selection stayed empty) rather than the start of a drag.
#[derive(Clone, Copy, Debug)]
pub(super) struct AnnotationPress {
    pub id: u64,
    pub position: Point<Pixels>,
}

/// The annotation under the pointer. `visible` flips on after
/// [`ANNOTATION_HOVER_DELAY`] so a passing cursor does not flash the card.
#[derive(Clone, Copy, Debug)]
pub(super) struct AnnotationHover {
    pub id: u64,
    pub visible: bool,
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
        for line in annotation.quoted_text().lines() {
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
        .map(TranscriptAnnotation::quoted_text)
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
        for line in annotation.quoted_text().lines() {
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

    /// ⌘L with the pill's selection on screen is the same as clicking it.
    /// Without an annotatable selection the chord keeps its global meaning,
    /// so the transcript's context binding forwards to FocusComposer.
    pub(super) fn add_to_chat_action(
        &mut self,
        _: &AddToChat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.annotatable_selection().is_some() {
            self.annotate_selection(window, cx);
        } else {
            self.focus_composer_action(&FocusComposer, window, cx);
        }
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
            });
            annotations.hovered = None;
        }
        self.transcript_selection.selection.borrow_mut().clear();
        self.annotation_hover = None;
        self.open_annotation_editor(id, true, window, cx);
        // Focus is on the just-clicked "Add to chat" button, which is gone by
        // the time the editor closes — return to the composer instead.
        let composer_focus = self.composer_focus(cx);
        if let Some(editor) = self.annotation_editor.as_mut() {
            editor.previous_focus = Some(composer_focus);
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let comment = {
            let annotations = self.transcript_selection.annotations.borrow();
            annotations
                .items
                .iter()
                .find(|annotation| annotation.id == id)
                .map(|annotation| annotation.comment.clone())
        };
        let Some(comment) = comment else {
            return;
        };
        self.annotation_editor = Some(AnnotationEditor {
            annotation_id: id,
            is_new,
            previous_focus: window.focused(cx),
        });
        self.transcript_selection.annotations.borrow_mut().editing = Some(id);
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
        {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            if let Some(annotation) = annotations
                .items
                .iter_mut()
                .find(|annotation| annotation.id == editor.annotation_id)
            {
                annotation.comment = comment;
            }
            annotations.editing = None;
        }
        self.restore_annotation_focus(editor.previous_focus, cx);
        cx.notify();
    }

    /// Escape: discard the draft comment. A never-confirmed annotation loses
    /// its highlight too; an existing one keeps its stored comment untouched.
    fn discard_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.annotation_editor.take() else {
            return;
        };
        {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            if editor.is_new {
                annotations
                    .items
                    .retain(|annotation| annotation.id != editor.annotation_id);
            }
            annotations.editing = None;
        }
        self.annotation_comment_input
            .update(cx, |input, cx| input.set_content("", cx));
        let focus = editor
            .previous_focus
            .unwrap_or_else(|| self.composer_focus(cx));
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The trash button: delete the annotation and its highlight, closing the
    /// editor when it was the one being edited.
    fn remove_annotation(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.transcript_selection
            .annotations
            .borrow_mut()
            .items
            .retain(|annotation| annotation.id != id);
        if let Some(editor) = self
            .annotation_editor
            .take_if(|editor| editor.annotation_id == id)
        {
            self.transcript_selection.annotations.borrow_mut().editing = None;
            self.annotation_comment_input
                .update(cx, |input, cx| input.set_content("", cx));
            let focus = editor
                .previous_focus
                .unwrap_or_else(|| self.composer_focus(cx));
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// The composer chip's X: drop every annotation and close an open editor.
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
        cx.notify();
    }

    /// Editor commit paths that lack a `Window` (the field's own submit
    /// subscription) focus through the stored window handle instead.
    fn restore_annotation_focus(&mut self, previous: Option<FocusHandle>, cx: &mut Context<Self>) {
        let focus = previous.unwrap_or_else(|| self.composer_focus(cx));
        let window_handle = self.window_handle;
        let _ = window_handle.update(cx, |_, window, cx| window.focus(&focus, cx));
    }

    /// Drain the live set for a submission. Parked sets for other sessions are
    /// untouched — only what was on screen ships.
    pub(super) fn drain_transcript_annotations(&mut self) -> Vec<TranscriptAnnotation> {
        self.annotation_editor = None;
        self.annotation_hover = None;
        self.annotation_press = None;
        let mut annotations = self.transcript_selection.annotations.borrow_mut();
        annotations.editing = None;
        annotations.hovered = None;
        std::mem::take(&mut annotations.items)
    }

    /// Drop annotations whose message no longer renders — a rewind removes it
    /// from the session entirely. Runs once per frame and only when the set is
    /// non-empty, so the common path costs one emptiness check.
    pub(super) fn prune_transcript_annotations(&mut self, cx: &mut Context<Self>) {
        {
            let mut annotations = self.transcript_selection.annotations.borrow_mut();
            if annotations.items.is_empty() {
                return;
            }
            let Some(session) = self.selected_session() else {
                annotations.items.clear();
                return;
            };
            annotations.items.retain(|annotation| {
                session.messages.iter().any(|message| {
                    message.id == annotation.message_id && message.role == MessageRole::Assistant
                })
            });
        }
        let editor_gone = self.annotation_editor.as_ref().is_some_and(|editor| {
            !self
                .transcript_selection
                .annotations
                .borrow()
                .items
                .iter()
                .any(|annotation| annotation.id == editor.annotation_id)
        });
        if editor_gone {
            self.transcript_selection.annotations.borrow_mut().editing = None;
            self.annotation_editor = None;
            self.annotation_comment_input
                .update(cx, |input, cx| input.set_content("", cx));
        }
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
        let theme = Theme::current(cx);
        // The chord sits on the transcript's key context, so resolve it as if
        // the transcript were focused — true whenever the pill can show.
        let shortcut_label =
            ShortcutHint::action_in(&AddToChat, &self.transcript_focus).resolve(window);
        let focus = self.transcript_control_focus("annotation-add-to-chat", cx);
        let button = div()
            .id("annotation-add-to-chat")
            .occlude()
            .track_focus(&focus)
            .tab_index(0)
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(9.0))
            .border_1()
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
                element.child(div().flex_none().text_color(theme.text_tertiary).child(label))
            })
            // A mouse-down here must not reach the transcript's selection
            // listeners, which would clear the very selection being offered.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_activation(cx, |this, window, cx| {
                this.annotate_selection(window, cx);
            });
        Some(
            deferred(FloatingSurface::new(
                button.into_any_element(),
                anchor,
                MenuAlign::AboveLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The floating comment editor, anchored below the annotation's first
    /// visible line.
    pub(super) fn render_annotation_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.annotation_editor.as_ref()?;
        let anchor = self.annotation_anchor(editor.annotation_id)?;
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
                    .id("annotation-editor-card")
                    .w(px(280.0))
                    .p(px(6.0))
                    .rounded(px(11.0))
                    .border_1()
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
                                    this.remove_annotation(editor.annotation_id, window, cx);
                                }
                            }),
                    ),
            );
        Some(
            deferred(FloatingSurface::new(
                card.into_any_element(),
                anchor,
                MenuAlign::BelowLeft,
                px(6.0),
                px(8.0),
            ))
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// The comment tooltip, surfaced after the hover delay over a confirmed
    /// annotation. Pointer-transparent by construction: it anchors above the
    /// highlight with a gap and carries no hit targets.
    pub(super) fn render_annotation_tooltip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let hover = self.annotation_hover.filter(|hover| hover.visible)?;
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
        let theme = Theme::current(cx);
        let card = div()
            .max_w(px(320.0))
            .px(px(7.0))
            .py(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_md()
            .text_size(sp(12.5))
            .line_height(sp(15.0))
            .text_color(theme.text_secondary)
            .child(comment);
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

    /// The composer's "N annotations" chip, with an always-visible clear-all
    /// control: tabbable, focus-ringed, activating on Enter or Space.
    pub(super) fn render_annotation_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.transcript_selection.annotations.borrow().items.len();
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
                        .border_1()
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
                                    element.border_1().border_color(theme.accent)
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
    fn install_annotation_input(
        window: &mut Window,
        _cx: &mut App,
        selection: &TranscriptSelection,
        waku: &WeakEntity<Waku>,
    ) {
        window.on_mouse_event({
            let selection = selection.clone();
            let waku = waku.clone();
            move |event: &MouseDownEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                if let Some(id) = annotation_hit_at(&selection, event.position) {
                    let _ = waku.update(cx, |this, _| {
                        this.annotation_press = Some(AnnotationPress {
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
                if phase != DispatchPhase::Bubble || event.dragging() {
                    return;
                }
                let hit = annotation_hit_at(&selection, event.position);
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
                    let _ = waku.update(cx, |this, cx| this.annotation_hover_changed(hit, cx));
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
                    .update(cx, |this, _| this.annotation_press.take())
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
                    this.open_annotation_editor(press.id, false, window, cx)
                });
            }
        });
    }

    fn annotation_hover_changed(&mut self, hit: Option<u64>, cx: &mut Context<Self>) {
        self.annotation_hover = hit.map(|id| AnnotationHover { id, visible: false });
        if let Some(id) = hit {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ANNOTATION_HOVER_DELAY).await;
                let _ = this.update(cx, |this, cx| {
                    if this
                        .annotation_hover
                        .is_some_and(|hover| hover.id == id && !hover.visible)
                    {
                        this.annotation_hover = Some(AnnotationHover { id, visible: true });
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// A zero-size canvas painting the frame's annotation listeners — see
    /// [`Self::install_annotation_input`].
    pub(super) fn transcript_annotation_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.transcript_selection.clone();
        let waku = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, cx| Self::install_annotation_input(window, cx, &selection, &waku),
        )
        .absolute()
        .w(px(0.0))
        .h(px(0.0))
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
}
