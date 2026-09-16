//! Text selection spanning many painted text elements.
//!
//! GPUI has no built-in selection for text. Zed's markdown selects
//! continuously because its whole document is one element over one text model;
//! the transcript instead renders a *tree* of text elements inside a
//! virtualized list, so this module rebuilds that continuity.
//!
//! Every frame the renderer registers each painted text element in paint order
//! — which is document order — into a [`SelectionRegistry`]. A drag anchored in
//! one element resolves against that registry into per-element [`Span`]s:
//! partial in the anchor and head elements, whole for everything between. The
//! wash paints per element from its span, and copy joins the spans in order.
//!
//! This half is pure and gpui-free so it can be unit-tested; the geometry and
//! mouse listeners live in [`super::render`].

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// Stable identity for one painted text element. `row` scopes it to a
/// transcript row so ids survive virtualized remounts; `index` orders elements
/// within that row.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextKey {
    pub row: Rc<str>,
    pub index: usize,
}

impl TextKey {
    pub fn new(row: impl Into<Rc<str>>, index: usize) -> Self {
        Self {
            row: row.into(),
            index,
        }
    }
}

/// One element's slice of the selection, in document order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub key: TextKey,
    /// Selected byte range of the element's flat text.
    pub range: Range<usize>,
    /// The element's full flat text. Snapshotted when the drag resolves, so
    /// copy still works after the element scrolls out of the registry.
    pub text: Rc<str>,
    /// True when this element starts a new block, so joined copy inserts a
    /// paragraph break rather than a single newline.
    pub block_break: bool,
}

/// The selection state for one transcript.
#[derive(Debug, Default)]
pub struct Selection {
    /// Element that owns the drag: where the mouse went down.
    anchor: Option<TextKey>,
    /// Byte offset of the anchor within its element.
    anchor_offset: usize,
    dragging: bool,
    /// Resolved spans in document order. Empty until a drag moves.
    spans: Vec<Span>,
    /// Span a press falls back to when its release resolves empty — ⌥-click
    /// defers its line selection to mouse-up so a drag can win.
    release_fallback: Option<Span>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|span| span.range.is_empty())
    }

    /// Begin a drag anchored at `offset` in `key`.
    pub fn begin(&mut self, key: TextKey, offset: usize) {
        self.anchor = Some(key);
        self.anchor_offset = offset;
        self.dragging = true;
        self.spans.clear();
        self.release_fallback = None;
    }

    /// Begin with an immediate span: double- or triple-click in one element.
    pub fn begin_with_span(&mut self, key: TextKey, text: Rc<str>, range: Range<usize>) {
        self.anchor = Some(key.clone());
        self.anchor_offset = range.start;
        self.dragging = true;
        self.spans = vec![Span {
            key,
            range,
            text,
            block_break: false,
        }];
        self.release_fallback = None;
    }

    /// Begin a drag that falls back to `fallback` when its release resolves
    /// empty — ⌥-click waits for mouse-up so a drag can win the selection.
    pub fn begin_with_fallback(&mut self, key: TextKey, offset: usize, fallback: Span) {
        self.begin(key, offset);
        self.release_fallback = Some(fallback);
    }

    /// Mouse-up: end the live drag, then apply a fallback armed by
    /// [`Self::begin_with_fallback`] when the drag left nothing selected.
    /// True when one was armed, so the caller can fire its deferred action.
    pub fn release(&mut self) -> bool {
        let fallback = self.release_fallback.take();
        if let Some(key) = self.anchor.clone() {
            self.end_drag(&key);
        }
        let Some(fallback) = fallback else {
            return false;
        };
        if self.is_empty() {
            self.anchor = Some(fallback.key.clone());
            self.anchor_offset = fallback.range.start;
            self.spans = vec![fallback];
        }
        true
    }

    /// The live drag's anchor offset, if `key` owns the drag.
    pub fn drag_anchor(&self, key: &TextKey) -> Option<usize> {
        (self.dragging && self.anchor.as_ref() == Some(key)).then_some(self.anchor_offset)
    }

    pub fn anchor(&self) -> Option<&TextKey> {
        self.anchor.as_ref()
    }

    /// Replace the resolved spans. True when they changed and a repaint is due.
    pub fn set_spans(&mut self, spans: Vec<Span>) -> bool {
        if self.spans == spans {
            return false;
        }
        self.spans = spans;
        true
    }

    /// Shift-click extension: move the head to `head`, a registry
    /// `(element index, byte offset)` point, and return true. Returns false
    /// when nothing is settled to extend, so the caller can start a fresh
    /// drag instead.
    ///
    /// The fixed end is the drag's anchor — unless the click lands on the
    /// anchor's outer side, in which case the far edge stays fixed so an
    /// outside click always grows the selection to contain it (the macOS
    /// convention). The fixed end becomes the new anchor so a press that
    /// turns into a drag keeps extending from it.
    pub fn extend_to<G>(&mut self, registry: &SelectionRegistry<G>, head: (usize, usize)) -> bool {
        if self.is_empty() {
            return false;
        }
        let anchor = self.anchor.as_ref().and_then(|key| {
            registry
                .position(key)
                .map(|index| (index, self.anchor_offset))
        });
        let edges = self
            .spans
            .first()
            .zip(self.spans.last())
            .and_then(|(first, last)| {
                registry
                    .position(&first.key)
                    .zip(registry.position(&last.key))
                    .map(|(s, e)| ((s, first.range.start), (e, last.range.end)))
            });
        let fixed = match (anchor, edges) {
            (Some(anchor), Some((start, end))) if head < start && anchor <= start => end,
            (Some(anchor), Some((start, end))) if head > end && anchor >= end => start,
            (Some(anchor), _) => anchor,
            (None, Some((start, end))) if head < start => end,
            (None, Some((start, _))) => start,
            (None, None) => return false,
        };
        self.spans = registry.resolve(fixed, head);
        self.anchor = Some(registry.entries()[fixed.0].key.clone());
        self.anchor_offset = fixed.1;
        self.dragging = true;
        self.release_fallback = None;
        true
    }

    /// The resolved spans in document order. Empty until a drag moves.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Whether a drag is currently in progress.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Finish `key`'s drag. Returns the selected text when non-empty.
    pub fn end_drag(&mut self, key: &TextKey) -> Option<String> {
        if self.anchor.as_ref() != Some(key) || !self.dragging {
            return None;
        }
        self.dragging = false;
        if self.is_empty() {
            self.clear();
            return None;
        }
        Some(self.text())
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.anchor_offset = 0;
        self.dragging = false;
        self.spans.clear();
        self.release_fallback = None;
    }

    /// The wash range for `key` this frame. `None` means nothing to paint.
    pub fn wash_range(&self, key: &TextKey) -> Option<Range<usize>> {
        self.spans
            .iter()
            .find(|span| span.key == *key && !span.range.is_empty())
            .map(|span| span.range.clone())
    }

    /// The full selected text, or `None` when nothing is selected.
    pub fn selected_text(&self) -> Option<String> {
        (!self.is_empty()).then(|| self.text())
    }

    /// The full selected text, spans joined in document order.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut has_span = false;
        for span in &self.spans {
            if has_span {
                out.push('\n');
                if span.block_break {
                    out.push('\n');
                }
            }
            out.push_str(&span.text[span.range.clone()]);
            has_span = true;
        }
        out
    }
}

/// One painted text element as seen by the current frame. `G` carries whatever
/// geometry the renderer needs to hit-test it — a `TextLayout` in practice, and
/// `()` in tests, which keeps this module free of any UI dependency.
#[derive(Clone, Debug)]
pub struct RegisteredText<G = ()> {
    pub key: TextKey,
    pub text: Rc<str>,
    /// True when this element begins a markdown block, for copy spacing.
    pub block_break: bool,
    /// `Annotation N` citations painted in this element: byte ranges paired
    /// with the label's 1-based index. Only transcript rows that resolve
    /// against a submitted annotation set carry any.
    pub annotation_refs: Vec<(Range<usize>, usize)>,
    /// Commit references painted in this element: byte ranges paired with the
    /// SHA text as it appears in the message.
    pub commit_refs: Vec<(Range<usize>, String)>,
    pub geometry: G,
}

/// The frame's document-ordered text elements. Paint order is document order,
/// so the renderer simply pushes as it paints and clears at the frame's start.
///
/// Holding the geometry here — rather than in each element's own closures — is
/// what lets the transcript install exactly one set of mouse listeners per
/// frame instead of three per painted text element.
#[derive(Debug)]
pub struct SelectionRegistry<G = ()> {
    entries: Vec<RegisteredText<G>>,
}

impl<G> Default for SelectionRegistry<G> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<G> SelectionRegistry<G> {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn push(&mut self, entry: RegisteredText<G>) {
        self.entries.push(entry);
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[RegisteredText<G>] {
        &self.entries
    }

    pub fn position(&self, key: &TextKey) -> Option<usize> {
        self.entries.iter().position(|entry| entry.key == *key)
    }

    /// Resolve a selection between two `(element index, byte offset)` points
    /// into per-element spans. Either direction works; empty slices are
    /// skipped, and the first span never carries a block break.
    pub fn resolve(&self, a: (usize, usize), b: (usize, usize)) -> Vec<Span> {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let mut spans = Vec::new();
        let last = self.entries.len().saturating_sub(1);
        for index in start.0..=end.0.min(last) {
            let entry = &self.entries[index];
            let from = if index == start.0 { start.1 } else { 0 };
            let to = if index == end.0 {
                end.1
            } else {
                entry.text.len()
            };
            let from = clamp_boundary(&entry.text, from);
            let to = clamp_boundary(&entry.text, to);
            // A fully crossed empty code line carries no glyph range to wash,
            // but keeping its span preserves the blank line when copying.
            let crossed_empty = entry.text.is_empty() && index > start.0 && index < end.0;
            if from < to || crossed_empty {
                spans.push(Span {
                    key: entry.key.clone(),
                    range: from..to,
                    text: entry.text.clone(),
                    block_break: entry.block_break && !spans.is_empty(),
                });
            }
        }
        spans
    }
}

/// Clamp a byte offset into `text` and snap it down to a char boundary. Mouse
/// hit-testing lands on boundaries already; this guards the arithmetic paths.
fn clamp_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Where a file annotation's passage was selected: the right-panel editor
/// for `path` (workspace-relative). `range` is the selected byte range inside
/// the file's current text — where the pinned highlight paints — and
/// `start_line`/`end_line` are the 1-based lines covering it at selection
/// time, carried for the `[Selected lines N-M]` prompt marker.
#[derive(Clone, Debug)]
pub struct FileAnnotation {
    pub path: String,
    pub range: Range<usize>,
    pub start_line: usize,
    pub end_line: usize,
}

/// A highlighted passage of transcript text carrying a user comment.
///
/// Annotations are created from a finished selection confined to one agent
/// message. `spans` snapshots the selected text the way copy does, so the
/// quoted passage stays intact even if the message is later edited.
///
/// A file annotation — `file` set — is the same object pinned on a
/// right-panel file editor's selection instead: `spans` holds one span over
/// the selected text alone, `message_id` is nil, and the passage formats as
/// `@path` plus a fenced block rather than a plain quote.
#[derive(Clone, Debug)]
pub struct TranscriptAnnotation {
    pub id: u64,
    /// The message the spans were taken from; they all share one
    /// `message-{id}` row key. Nil when `file` is set.
    pub message_id: uuid::Uuid,
    pub spans: Vec<Span>,
    pub comment: String,
    /// Right-panel file provenance; `None` for transcript annotations.
    pub file: Option<FileAnnotation>,
}

impl TranscriptAnnotation {
    /// The annotated text, spans joined in document order the way copy joins
    /// them.
    pub fn quoted_text(&self) -> String {
        let mut out = String::new();
        let mut has_span = false;
        for span in &self.spans {
            if has_span {
                out.push('\n');
                if span.block_break {
                    out.push('\n');
                }
            }
            out.push_str(&span.text[span.range.clone()]);
            has_span = true;
        }
        out
    }
}

/// The live set of commented highlights over one transcript.
///
/// Painted by the renderer alongside the selection wash: every annotation span
/// keeps a soft fill so it reads as annotated rather than selected.
/// `hovered`/`editing` emphasise one highlight — the one under the pointer or
/// the one whose comment editor is open.
#[derive(Debug, Default)]
pub struct Annotations {
    pub items: Vec<TranscriptAnnotation>,
    pub hovered: Option<u64>,
    /// The `Annotation N` citation under the pointer, identified by its
    /// element and byte range, so its dotted underline can emphasise.
    pub hovered_ref: Option<(TextKey, Range<usize>)>,
    pub editing: Option<u64>,
}

impl Annotations {
    /// Each annotation span painted over `key` this frame, paired with whether
    /// it is emphasised (hovered or being edited).
    pub fn wash_spans<'a>(
        &'a self,
        key: &'a TextKey,
    ) -> impl Iterator<Item = (&'a Span, bool)> + 'a {
        self.items
            .iter()
            .flat_map(|annotation| annotation.spans.iter().map(move |span| (annotation, span)))
            .filter(|(_, span)| span.key == *key)
            .map(|(annotation, span)| {
                (
                    span,
                    self.hovered == Some(annotation.id) || self.editing == Some(annotation.id),
                )
            })
    }
}

/// Shared handles the renderer clones into paint closures.
pub struct SelectionState<G = ()> {
    pub selection: Rc<RefCell<Selection>>,
    pub registry: Rc<RefCell<SelectionRegistry<G>>>,
    /// Commented highlights painted beneath the selection wash. Only the
    /// transcript's selection state populates this; the toast, diff and
    /// skills registries never carry annotations.
    pub annotations: Rc<RefCell<Annotations>>,
    /// The commit reference under the pointer, identified by its element and
    /// byte range so the renderer can emphasise its dotted underline.
    pub hovered_commit: Rc<RefCell<Option<(TextKey, Range<usize>)>>>,
}

impl<G> Clone for SelectionState<G> {
    fn clone(&self) -> Self {
        Self {
            selection: self.selection.clone(),
            registry: self.registry.clone(),
            annotations: self.annotations.clone(),
            hovered_commit: self.hovered_commit.clone(),
        }
    }
}

impl<G> Default for SelectionState<G> {
    fn default() -> Self {
        Self {
            selection: Rc::default(),
            registry: Rc::default(),
            annotations: Rc::default(),
            hovered_commit: Rc::default(),
        }
    }
}

impl<G> SelectionState<G> {
    /// Drop both the persisted spans and this frame's hit-test geometry.
    pub fn clear(&self) {
        self.selection.borrow_mut().clear();
        self.registry.borrow_mut().clear();
    }
}

/// Byte range around `offset` for a double click: a word-ish run, or the
/// single non-space character under the cursor.
pub fn word_range(text: &str, offset: usize) -> Range<usize> {
    let offset = clamp_boundary(text, offset);
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let before = text[..offset].chars().next_back();
    let at = text[offset..].chars().next();

    if !at.is_some_and(is_word) && !before.is_some_and(is_word) {
        return match at {
            Some(ch) if !ch.is_whitespace() => offset..offset + ch.len_utf8(),
            _ => offset..offset,
        };
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_word(*ch))
        .last()
        .map_or(offset, |(index, _)| index);
    let end = text[offset..]
        .char_indices()
        .take_while(|(_, ch)| is_word(*ch))
        .last()
        .map_or(offset, |(index, ch)| offset + index + ch.len_utf8());
    start..end
}

/// Byte range of the logical line containing `offset`, for a triple click.
pub fn line_range(text: &str, offset: usize) -> Range<usize> {
    let offset = clamp_boundary(text, offset);
    let start = text[..offset].rfind('\n').map_or(0, |newline| newline + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |newline| offset + newline);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(entries: &[(&str, &str)]) -> SelectionRegistry {
        // Geometry defaults to `()` here: resolve() is pure index arithmetic.
        let mut registry = SelectionRegistry::default();
        for (index, (row, text)) in entries.iter().enumerate() {
            registry.push(RegisteredText {
                key: TextKey::new(*row, index),
                text: Rc::from(*text),
                block_break: index > 0,
                annotation_refs: Vec::new(),
                commit_refs: Vec::new(),
                geometry: (),
            });
        }
        registry
    }

    fn sample() -> SelectionRegistry {
        registry(&[
            ("r1", "first paragraph"),
            ("r1", "second"),
            ("r2", "third one"),
        ])
    }

    fn selected(registry: &SelectionRegistry, spans: &[Span]) -> Vec<String> {
        let _ = registry;
        spans
            .iter()
            .map(|span| span.text[span.range.clone()].to_owned())
            .collect()
    }

    #[test]
    fn resolves_within_one_element() {
        let registry = sample();
        let spans = registry.resolve((0, 6), (0, 15));
        assert_eq!(selected(&registry, &spans), vec!["paragraph"]);
        // Direction does not matter.
        assert_eq!(registry.resolve((0, 15), (0, 6)), spans);
    }

    #[test]
    fn resolves_across_elements_covering_middles_whole() {
        let registry = sample();
        let spans = registry.resolve((0, 6), (2, 5));
        assert_eq!(
            selected(&registry, &spans),
            vec!["paragraph", "second", "third"]
        );
        // A bottom-up drag resolves identically.
        assert_eq!(registry.resolve((2, 5), (0, 6)), spans);
    }

    #[test]
    fn resolve_clamps_offsets_and_indexes() {
        let registry = sample();
        let spans = registry.resolve((0, 0), (99, 999));
        assert_eq!(
            selected(&registry, &spans),
            vec!["first paragraph", "second", "third one"]
        );
    }

    #[test]
    fn resolve_snaps_offsets_to_char_boundaries() {
        let registry = registry(&[("r1", "héllo")]);
        // Byte 2 is inside the 'é'; snapping down keeps the slice valid.
        let spans = registry.resolve((0, 2), (0, 5));
        assert_eq!(spans[0].range.start, 1);
        assert!(!spans.is_empty());
    }

    #[test]
    fn drag_lifecycle_and_copy_joining() {
        let registry = sample();
        let mut selection = Selection::default();
        let anchor = TextKey::new("r1", 0);

        selection.begin(anchor.clone(), 6);
        assert_eq!(selection.drag_anchor(&anchor), Some(6));
        assert_eq!(selection.drag_anchor(&TextKey::new("r1", 1)), None);

        let spans = registry.resolve((0, 6), (1, 6));
        assert!(selection.set_spans(spans.clone()));
        // An unchanged resolve must not force a repaint.
        assert!(!selection.set_spans(spans));

        assert_eq!(selection.wash_range(&anchor), Some(6..15));
        assert_eq!(selection.wash_range(&TextKey::new("r1", 1)), Some(0..6));
        assert_eq!(selection.wash_range(&TextKey::new("r2", 2)), None);

        // Elements starting a new block join with a paragraph break.
        assert_eq!(
            selection.end_drag(&anchor).as_deref(),
            Some("paragraph\n\nsecond")
        );
        assert_eq!(selection.text(), "paragraph\n\nsecond");

        // A mouse-down outside every text element clears the settled selection.
        selection.clear();
        assert!(selection.is_empty());
        assert_eq!(selection.anchor(), None);
    }

    #[test]
    fn line_oriented_copy_uses_single_newlines_and_preserves_blank_rows() {
        let registry = registry(&[("line-1", "one"), ("line-2", ""), ("line-3", "two")]);
        let mut selection = Selection::default();
        selection.set_spans(registry.resolve((0, 0), (2, 3)));

        // The fixture marks later elements as block starts; Review overrides
        // that flag because every registered element is one logical code line.
        for span in &mut selection.spans {
            span.block_break = false;
        }
        assert_eq!(selection.text(), "one\n\ntwo");
    }

    #[test]
    fn shift_click_extends_a_settled_selection() {
        let registry = sample();
        let mut selection = Selection::default();
        let anchor = TextKey::new("r1", 0);
        selection.begin(anchor.clone(), 6);
        selection.set_spans(registry.resolve((0, 6), (0, 15)));
        selection.end_drag(&anchor);

        // Nothing to extend from an empty selection.
        assert!(!Selection::default().extend_to(&registry, (0, 0)));

        // A click beyond the far edge keeps the anchor and grows.
        assert!(selection.extend_to(&registry, (2, 5)));
        assert_eq!(
            selected(&registry, selection.spans()),
            vec!["paragraph", "second", "third"]
        );

        // A click on the anchor's outer side holds the far edge instead, so
        // the selection still grows to contain it.
        assert!(selection.extend_to(&registry, (0, 2)));
        assert_eq!(
            selected(&registry, selection.spans()),
            vec!["rst paragraph", "second", "third"]
        );

        // A click inside contracts from the fixed end.
        assert!(selection.extend_to(&registry, (0, 10)));
        assert_eq!(
            selected(&registry, selection.spans()),
            vec!["graph", "second", "third"]
        );

        // The extension ends like a drag: release keeps the text.
        assert_eq!(
            selection.end_drag(&TextKey::new("r2", 2)).as_deref(),
            Some("graph\n\nsecond\n\nthird")
        );
    }

    #[test]
    fn shift_click_after_a_word_click_keeps_the_word() {
        let registry = registry(&[("r1", "hello world")]);
        let mut selection = Selection::default();
        let key = TextKey::new("r1", 0);
        selection.begin_with_span(key.clone(), Rc::from("hello world"), 6..11);
        selection.end_drag(&key);

        // The word's start is the stored anchor; clicking before it extends
        // from the word's end so the whole word survives.
        assert!(selection.extend_to(&registry, (0, 2)));
        assert_eq!(selection.wash_range(&key), Some(2..11));
    }

    #[test]
    fn shift_click_without_anchors_in_view_uses_the_visible_edge() {
        let registry = sample();
        let mut selection = Selection::default();
        // The anchor's element is gone from the frame, but a span remains.
        selection.begin(TextKey::new("gone", 0), 0);
        selection.set_spans(registry.resolve((0, 6), (0, 15)));
        selection.end_drag(&TextKey::new("gone", 0));

        assert!(selection.extend_to(&registry, (2, 5)));
        assert_eq!(
            selected(&registry, selection.spans()),
            vec!["paragraph", "second", "third"]
        );
    }

    #[test]
    fn a_click_without_movement_clears_on_release() {
        let mut selection = Selection::default();
        let key = TextKey::new("r1", 0);
        selection.begin(key.clone(), 3);
        assert_eq!(selection.end_drag(&key), None);
        assert!(selection.is_empty());
        assert_eq!(selection.anchor(), None);
    }

    #[test]
    fn a_fallback_press_selects_its_span_when_the_drag_stays_empty() {
        let mut selection = Selection::default();
        let key = TextKey::new("r1", 0);
        let text: Rc<str> = Rc::from("first paragraph");
        selection.begin_with_fallback(
            key.clone(),
            6,
            Span {
                key: key.clone(),
                range: 0..15,
                text,
                block_break: false,
            },
        );

        // The fallback is armed but nothing is selected while the press lasts.
        assert!(selection.is_empty());
        assert!(selection.is_dragging());

        assert!(selection.release());
        assert_eq!(selection.wash_range(&key), Some(0..15));
        assert!(!selection.is_dragging());
    }

    #[test]
    fn a_fallback_press_keeps_what_the_drag_resolved() {
        let registry = sample();
        let mut selection = Selection::default();
        let anchor = TextKey::new("r1", 0);
        let text: Rc<str> = Rc::from("first paragraph");
        selection.begin_with_fallback(
            anchor.clone(),
            6,
            Span {
                key: anchor.clone(),
                range: 0..15,
                text,
                block_break: false,
            },
        );
        selection.set_spans(registry.resolve((0, 6), (1, 6)));

        assert!(selection.release());
        assert_eq!(
            selected(&registry, selection.spans()),
            vec!["paragraph", "second"]
        );
    }

    #[test]
    fn release_without_a_fallback_just_ends_the_drag() {
        let registry = sample();
        let mut selection = Selection::default();
        let anchor = TextKey::new("r1", 0);
        selection.begin(anchor.clone(), 6);
        selection.set_spans(registry.resolve((0, 6), (0, 15)));

        assert!(!selection.release());
        assert_eq!(selection.wash_range(&anchor), Some(6..15));

        // A plain click clears the same way `end_drag` did.
        selection.begin(anchor.clone(), 3);
        assert!(!selection.release());
        assert!(selection.is_empty());
    }

    #[test]
    fn double_and_triple_click_spans() {
        let mut selection = Selection::default();
        let key = TextKey::new("r1", 0);
        let text: Rc<str> = Rc::from("hello world");
        selection.begin_with_span(key.clone(), text.clone(), 6..11);
        assert_eq!(selection.wash_range(&key), Some(6..11));
        assert_eq!(selection.end_drag(&key).as_deref(), Some("world"));
    }

    #[test]
    fn word_ranges_follow_word_characters() {
        let text = "let foo_bar = 12;";
        assert_eq!(word_range(text, 5), 4..11);
        assert_eq!(word_range(text, 4), 4..11);
        assert_eq!(word_range(text, 11), 4..11);
        assert_eq!(word_range(text, 15), 14..16);
        assert_eq!(&text[word_range(text, 12)], "=");
        assert_eq!(word_range(text, 3), 0..3);

        // Mid-character offsets snap down rather than panicking.
        let unicode = "héllo wörld";
        assert_eq!(&unicode[word_range(unicode, 2)], "héllo");
    }

    #[test]
    fn line_ranges_cover_the_surrounding_logical_line() {
        let text = "first line\nsecond line\nthird";
        assert_eq!(line_range(text, 3), 0..10);
        assert_eq!(line_range(text, 15), 11..22);
        assert_eq!(line_range(text, text.len()), 23..28);
    }

    #[test]
    fn registry_positions_survive_rebuilds() {
        let mut registry = SelectionRegistry::default();
        registry.push(RegisteredText {
            key: TextKey::new("row-a", 0),
            text: Rc::from("a"),
            block_break: false,
            annotation_refs: Vec::new(),
            commit_refs: Vec::new(),
            geometry: (),
        });
        assert_eq!(registry.position(&TextKey::new("row-a", 0)), Some(0));
        assert_eq!(registry.position(&TextKey::new("row-b", 0)), None);
        registry.clear();
        assert!(registry.is_empty());
        assert_eq!(registry.position(&TextKey::new("row-a", 0)), None);
    }
}
