//! Find-in-page for the visible transcript.
//!
//! The query is scanned only when it changes. Results carry the virtualized
//! row plus the markdown renderer's stable element ordinal and byte range, so
//! navigation can mount an off-screen message, highlight the exact glyphs,
//! and then reveal the wrapped line without doing search work in a frame.

use regex::{Regex, RegexBuilder};

use super::*;
use crate::md::render::{SearchHighlights, TextSearchMatch};
use crate::md::selection::TextKey;
use crate::ui::ActivationExt;

const MAX_TRANSCRIPT_SEARCH_MATCHES: usize = 20_000;

/// The row flash's fade-out once a deep-linked match lands.
pub(super) const MATCH_FLASH_FADE: Duration = Duration::from_millis(1400);
/// How long a landed match keeps its glyph washes before the state clears —
/// the flash reads as transient without a per-frame fade of painted text.
const MATCH_FLASH_TTL: Duration = Duration::from_secs(5);

/// A landed deep link's highlight state: the matched message's glyph washes
/// — computed once at reveal time, never per frame — plus the row flash and
/// the pending reveal the next frames refine into position.
pub(super) struct TranscriptMatchFlash {
    pub session_id: Uuid,
    pub message_id: Uuid,
    /// Snapshots from reveal time. `transcript_row` re-verifies the id
    /// against the session before trusting `message_index` — a rewind could
    /// have parked another message there.
    pub message_index: usize,
    pub row_index: usize,
    pub highlights: Rc<Vec<TextSearchMatch>>,
    pub active: Option<TextSearchMatch>,
    pub armed_at: Instant,
    /// Set until the first render pass consumes the reveal — like
    /// `TranscriptSearch::pending_reveal`.
    pub pending_reveal: bool,
}

#[derive(Clone)]
struct TranscriptMatchTarget {
    armed_at: Instant,
    message_id: Uuid,
    row_index: usize,
    text: Option<TextSearchMatch>,
}

pub(super) struct TranscriptSearch {
    open: bool,
    query: Entity<TextInput>,
    matches: Vec<TranscriptSearchMatch>,
    matches_by_message: HashMap<usize, Rc<Vec<TextSearchMatch>>>,
    current: Option<usize>,
    limited: bool,
    previous_focus: Option<FocusHandle>,
    generation: u64,
    pending_reveal: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TranscriptSearchMatch {
    message_index: usize,
    row_index: usize,
    message_id: Uuid,
    text: TextSearchMatch,
}

#[derive(Clone)]
struct TranscriptSearchTarget {
    generation: u64,
    current: usize,
    message_id: Uuid,
    row_index: usize,
    key: TextKey,
    range: Range<usize>,
}

impl TranscriptSearch {
    fn target(&self) -> Option<TranscriptSearchTarget> {
        let current = self.current?;
        let found = self.matches.get(current)?;
        Some(TranscriptSearchTarget {
            generation: self.generation,
            current,
            message_id: found.message_id,
            row_index: found.row_index,
            key: TextKey::new(format!("message-{}", found.message_id), found.text.ordinal),
            range: found.text.range.clone(),
        })
    }
}

impl Waku {
    pub(super) fn refresh_transcript_search_localized_text(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &self.transcript_search else {
            return;
        };
        search.query.update(cx, |input, cx| {
            input.set_accessibility_label(tr!("input.find"), cx);
            input.set_placeholder(tr!("input.find"), cx)
        });
    }

    pub(super) fn transcript_search_open(&self) -> bool {
        self.transcript_search
            .as_ref()
            .is_some_and(|search| search.open)
    }

    fn ensure_transcript_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.transcript_search.is_some() {
            return;
        }
        let query = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("input.find"))
                .placeholder(tr!("input.find"))
        });
        cx.subscribe(
            &query,
            |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::Edited => this.refresh_transcript_search(cx),
                InputEvent::Submit(_) => this.navigate_transcript_search(false, cx),
                InputEvent::Focus
                | InputEvent::BackspaceOnEmpty
                | InputEvent::InlineAtomClicked(_)
                | InputEvent::InlineAtomActivated(_) => {}
            },
        )
        .detach();
        self.transcript_search = Some(TranscriptSearch {
            open: false,
            query,
            matches: Vec::new(),
            matches_by_message: HashMap::new(),
            current: None,
            limited: false,
            previous_focus: None,
            generation: 0,
            pending_reveal: false,
        });
    }

    pub(super) fn open_transcript_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .selected_session()
            .is_none_or(|session| session.messages.is_empty())
        {
            cx.propagate();
            return;
        }
        let previous_focus = window.focused(cx);
        let seed = self
            .transcript_selection
            .selection
            .borrow()
            .selected_text()
            .filter(|text| !text.is_empty() && !text.contains('\n'));
        self.ensure_transcript_search(window, cx);

        let search = self
            .transcript_search
            .as_mut()
            .expect("ensure_transcript_search just created it");
        if !search.open {
            search.previous_focus = previous_focus;
        }
        search.open = true;
        let query = search.query.clone();
        if let Some(seed) = seed {
            query.update(cx, |query, cx| query.set_content(seed, cx));
        }
        query.update(cx, |query, cx| query.select_all_text(cx));
        window.focus(&query.read(cx).focus(), cx);
        self.refresh_transcript_search(cx);
        cx.notify();
    }

    pub(super) fn close_transcript_search(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(search) = self.transcript_search.as_mut().filter(|search| search.open) else {
            return;
        };
        search.open = false;
        search.matches.clear();
        search.matches_by_message.clear();
        search.current = None;
        search.limited = false;
        search.pending_reveal = false;
        search.generation = search.generation.wrapping_add(1);
        let previous_focus = search.previous_focus.take();
        if restore_focus && let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    pub(super) fn reset_transcript_search_for_session(&mut self) {
        let Some(search) = self.transcript_search.as_mut() else {
            return;
        };
        search.open = false;
        search.matches.clear();
        search.matches_by_message.clear();
        search.current = None;
        search.limited = false;
        search.previous_focus = None;
        search.pending_reveal = false;
        search.generation = search.generation.wrapping_add(1);
    }

    fn refresh_transcript_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = self.transcript_search.as_ref().filter(|search| search.open) else {
            return;
        };
        let query = search.query.read(cx).content().to_owned();
        self.sync_transcript_rows();
        let origin_row = self.active_transcript_rows().logical_scroll_top().item_ix;
        let row_kinds = self.transcript_row_kinds.borrow().clone();

        let mut matches = Vec::new();
        let mut matches_by_message = HashMap::new();
        let mut limited = false;
        if !query.is_empty() {
            let regex = literal_find_regex(&query);
            if let Some(session) = self.selected_session() {
                for (row_index, kind) in row_kinds.iter().copied().enumerate() {
                    let TranscriptRowKind::Message(message_index) = kind else {
                        continue;
                    };
                    let Some(message) = session.messages.get(message_index) else {
                        continue;
                    };
                    let remaining = MAX_TRANSCRIPT_SEARCH_MATCHES.saturating_sub(matches.len());
                    let (message_matches, message_limited) =
                        if matches!(message.role, MessageRole::User | MessageRole::Assistant) {
                            md::render::markdown_search_matches(
                                message.visible_content(),
                                &regex,
                                remaining,
                            )
                        } else {
                            md::render::plain_search_matches(
                                message.visible_content(),
                                0,
                                &regex,
                                remaining,
                            )
                        };
                    if !message_matches.is_empty() {
                        for text in &message_matches {
                            matches.push(TranscriptSearchMatch {
                                message_index,
                                row_index,
                                message_id: message.id,
                                text: text.clone(),
                            });
                        }
                        matches_by_message.insert(message_index, Rc::new(message_matches));
                    }
                    if message_limited {
                        limited = true;
                        break;
                    }
                }
            }
        }

        let current = (!matches.is_empty()).then(|| {
            matches
                .iter()
                .position(|found| found.row_index >= origin_row)
                .unwrap_or(0)
        });
        let search = self
            .transcript_search
            .as_mut()
            .expect("search remains present while it is refreshed");
        search.matches = matches;
        search.matches_by_message = matches_by_message;
        search.current = current;
        search.limited = limited;
        search.generation = search.generation.wrapping_add(1);
        search.pending_reveal = current.is_some();
        cx.notify();
    }

    pub(super) fn navigate_transcript_search(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let Some(search) = self
            .transcript_search
            .as_mut()
            .filter(|search| search.open && !search.matches.is_empty())
        else {
            return;
        };
        let count = search.matches.len();
        search.current = Some(match search.current {
            Some(current) if backwards => (current + count - 1) % count,
            Some(current) => (current + 1) % count,
            None if backwards => count - 1,
            None => 0,
        });
        search.generation = search.generation.wrapping_add(1);
        search.pending_reveal = true;
        cx.notify();
    }

    pub(super) fn transcript_search_highlights(
        &self,
        message_index: usize,
    ) -> Option<SearchHighlights> {
        if let Some(search) = self.transcript_search.as_ref().filter(|search| search.open) {
            let matches = search.matches_by_message.get(&message_index)?.clone();
            let active = search
                .current
                .and_then(|current| search.matches.get(current))
                .filter(|found| found.message_index == message_index)
                .map(|found| found.text.clone());
            return Some(SearchHighlights { matches, active });
        }
        // A landed deep link paints the matched message's washes without the
        // find bar — while the flash lives, on the message it names.
        let flash = self.transcript_match_flash.as_ref()?;
        if flash.message_index != message_index
            || self.state.selected_session != Some(flash.session_id)
        {
            return None;
        }
        let still_the_same_message = self
            .selected_session()
            .and_then(|session| session.messages.get(message_index))
            .is_some_and(|message| message.id == flash.message_id);
        still_the_same_message.then(|| SearchHighlights {
            matches: flash.highlights.clone(),
            active: flash.active.clone(),
        })
    }

    /// Land the transcript on a deep-linked match: open the fold hiding it,
    /// put its row on screen, and arm the flash the next frames refine into
    /// a centered, glyph-highlighted position. Reports whether the target
    /// resolved — a rewound-away message is not a reveal.
    pub(super) fn reveal_transcript_match(
        &mut self,
        pending: &PendingTranscriptMatch,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = self.selected_session() else {
            return false;
        };
        if session.id != pending.session_id {
            return false;
        }
        let Some(message_index) = session
            .messages
            .iter()
            .position(|message| message.id == pending.message_id)
        else {
            return false;
        };
        let message = &session.messages[message_index];
        let turn_id = message.turn_id;
        let role = message.role;
        let hidden = message.hidden;
        let content = message.visible_content().to_owned();

        self.sync_transcript_rows();
        // A hit folded behind "Worked for …" stays invisible until the
        // disclosure opens — open it so the reveal lands on the text.
        if let Some(turn_id) = turn_id {
            let folded_away = {
                let kinds = self.transcript_row_kinds.borrow();
                !kinds.contains(&TranscriptRowKind::Message(message_index))
                    && kinds.contains(&TranscriptRowKind::TurnFold(turn_id))
            };
            if folded_away {
                self.expanded_turns.insert(turn_id);
                self.sync_transcript_rows();
            }
        }
        let row_index = {
            let kinds = self.transcript_row_kinds.borrow();
            let session = self.selected_session();
            kinds
                .iter()
                .position(|kind| *kind == TranscriptRowKind::Message(message_index))
                // A hidden report prompt's row is the wake marker standing
                // in for it.
                .or_else(|| {
                    kinds.iter().position(|kind| {
                        *kind == TranscriptRowKind::BossTrigger(pending.message_id)
                    })
                })
                .or_else(|| {
                    turn_id.and_then(|turn_id| {
                        kinds
                            .iter()
                            .position(|kind| *kind == TranscriptRowKind::TurnFold(turn_id))
                    })
                })
                .or_else(|| {
                    session.and_then(|session| {
                        turn_id.and_then(|turn_id| {
                            kinds
                                .iter()
                                .position(|kind| row_turn_id(session, *kind) == Some(turn_id))
                        })
                    })
                })
        };
        let Some(row_index) = row_index else {
            return false;
        };

        // `scroll_to` is bounds-independent — the row mounts at the viewport
        // top this frame, and `pending_reveal` centers the match once the
        // row has measured bounds.
        self.transcript_anchor_following.set(false);
        self.transcript_tail_recheck.set(false);
        self.active_transcript_rows().scroll_to(ListOffset {
            item_ix: row_index,
            offset_in_item: Pixels::ZERO,
        });
        self.transcript_is_scrolled.set(true);

        // The same glyph washes ⌘F paints — computed once here so a frame
        // never scans the message again.
        let query = pending.query.trim();
        let (highlights, active) = if query.is_empty() || hidden {
            (Rc::new(Vec::new()), None)
        } else {
            let regex = literal_find_regex(query);
            let (matches, _) = if matches!(role, MessageRole::User | MessageRole::Assistant) {
                md::render::markdown_search_matches(&content, &regex, MAX_TRANSCRIPT_SEARCH_MATCHES)
            } else {
                md::render::plain_search_matches(&content, 0, &regex, MAX_TRANSCRIPT_SEARCH_MATCHES)
            };
            let active = matches.first().cloned();
            (Rc::new(matches), active)
        };

        let armed_at = Instant::now();
        self.transcript_match_flash = Some(TranscriptMatchFlash {
            session_id: pending.session_id,
            message_id: pending.message_id,
            message_index,
            row_index,
            highlights,
            active,
            armed_at,
            pending_reveal: true,
        });
        // The washes expire on a timer — a stale link cannot leave a
        // permanent highlight behind.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MATCH_FLASH_TTL).await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .transcript_match_flash
                    .as_ref()
                    .is_some_and(|flash| flash.armed_at == armed_at)
                {
                    this.transcript_match_flash = None;
                    cx.notify();
                }
            });
        })
        .detach();
        true
    }

    /// The first frame a deep-linked match is on screen: refine the arm-time
    /// top-of-viewport scroll into the real reveal — matched glyphs on the
    /// reveal line, or the row centered when it carries no glyph target.
    pub(super) fn apply_pending_transcript_match_reveal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.transcript_match_flash.as_mut().and_then(|flash| {
            if !flash.pending_reveal || self.state.selected_session != Some(flash.session_id) {
                return None;
            }
            flash.pending_reveal = false;
            Some(TranscriptMatchTarget {
                armed_at: flash.armed_at,
                message_id: flash.message_id,
                row_index: flash.row_index,
                text: flash.active.clone(),
            })
        });
        let Some(target) = target else {
            return;
        };

        if self.transcript_match_target_bounds(&target).is_some() {
            self.reveal_transcript_match_geometry(target, 1, window, cx);
            return;
        }
        self.detach_transcript_search_from_tail();
        self.active_transcript_rows()
            .scroll_to_reveal_item(target.row_index);
        cx.on_next_frame(window, move |this, window, cx| {
            this.reveal_transcript_match_geometry(target, 0, window, cx)
        });
    }

    fn transcript_match_target_bounds(
        &self,
        target: &TranscriptMatchTarget,
    ) -> Option<Bounds<Pixels>> {
        let text = target.text.as_ref()?;
        self.transcript_text_bounds(
            &TextKey::new(format!("message-{}", target.message_id), text.ordinal),
            &text.range,
        )
    }

    fn reveal_transcript_match_geometry(
        &mut self,
        target: TranscriptMatchTarget,
        attempt: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let still_current = self.transcript_match_flash.as_ref().is_some_and(|flash| {
            flash.message_id == target.message_id && flash.armed_at == target.armed_at
        });
        if !still_current {
            return;
        }
        let match_bounds = self.transcript_match_target_bounds(&target);
        let Some(match_bounds) = match_bounds else {
            if target.text.is_some() && attempt < 1 {
                cx.on_next_frame(window, move |this, window, cx| {
                    this.reveal_transcript_match_geometry(target, attempt + 1, window, cx)
                });
            } else {
                // No registered glyph geometry — a bare `?message=` link or
                // text that no painted element claims. Center the row.
                self.center_transcript_match_row(target, 0, window, cx);
            }
            return;
        };
        self.position_transcript_match_bounds(target.message_id, match_bounds, cx);
    }

    fn center_transcript_match_row(
        &mut self,
        target: TranscriptMatchTarget,
        attempt: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.active_transcript_rows();
        let Some(bounds) = rows.bounds_for_item(target.row_index) else {
            if attempt < 3 {
                cx.on_next_frame(window, move |this, window, cx| {
                    this.center_transcript_match_row(target, attempt + 1, window, cx)
                });
            }
            return;
        };
        let viewport = rows.viewport_bounds();
        if viewport.size.height <= Pixels::ZERO {
            return;
        }
        // A row taller than the viewport cannot center — pin its top on the
        // reveal line a glyph reveal would use.
        let target_top = if bounds.size.height >= viewport.size.height {
            viewport.top() + viewport.size.height * 0.35
        } else {
            viewport.top() + (viewport.size.height - bounds.size.height) / 2.0
        };
        let current = rows.scroll_px_offset_for_scrollbar().y;
        let max_offset = rows.max_offset_for_scrollbar().y;
        let next = (current + (target_top - bounds.top())).clamp(-max_offset, Pixels::ZERO);
        if next != current {
            self.detach_transcript_search_from_tail();
            rows.set_offset_from_scrollbar(point(Pixels::ZERO, next));
            cx.notify();
        }
    }

    /// Reveal a pending result without disturbing an already-mounted row.
    ///
    /// `ListState::scroll_to_reveal_item` resets the intra-item offset when the
    /// target is the current row. Transcript messages can be taller than the
    /// viewport, so doing that before the exact glyph reveal produces a
    /// visible one-frame jump to the top of the message. Reuse the previous
    /// frame's registered text geometry whenever it exists; only mount a row
    /// first when the result is genuinely off screen.
    pub(super) fn apply_pending_transcript_search_reveal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.transcript_search.as_mut().and_then(|search| {
            if !search.open || !search.pending_reveal {
                return None;
            }
            search.pending_reveal = false;
            search.target()
        });
        let Some(target) = target else {
            return;
        };

        if self.transcript_search_match_bounds(&target).is_some() {
            self.reveal_transcript_search_geometry(target, 1, window, cx);
            return;
        }

        self.detach_transcript_search_from_tail();
        self.active_transcript_rows()
            .scroll_to_reveal_item(target.row_index);
        cx.on_next_frame(window, move |this, window, cx| {
            this.reveal_transcript_search_geometry(target, 0, window, cx)
        });
    }

    /// The registered glyph bounds one text key paints for `range` — shared
    /// by find-in-page navigation and a deep link's match reveal.
    fn transcript_text_bounds(
        &self,
        key: &TextKey,
        range: &Range<usize>,
    ) -> Option<Bounds<Pixels>> {
        let registry = self.transcript_selection.registry.borrow();
        registry
            .entries()
            .iter()
            .find(|entry| entry.key == *key)
            .and_then(|entry| {
                md::render::text_range_bounds(&entry.geometry, range)
                    .into_iter()
                    .next()
            })
    }

    fn transcript_search_match_bounds(
        &self,
        target: &TranscriptSearchTarget,
    ) -> Option<Bounds<Pixels>> {
        self.transcript_text_bounds(&target.key, &target.range)
    }

    fn detach_transcript_search_from_tail(&self) {
        self.transcript_anchor_following.set(false);
        self.transcript_tail_recheck.set(false);
        self.transcript_is_scrolled.set(true);
    }

    /// Slide a capped user bubble then the transcript so `match_bounds` lands
    /// inside the viewport on the reveal line — the shared tail of find
    /// navigation and deep-link match reveals.
    fn position_transcript_match_bounds(
        &mut self,
        message_id: Uuid,
        mut match_bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        // Reveal inside a capped user bubble before positioning the transcript.
        // Search keeps full text geometry even when those glyphs are clipped.
        if let Some(viewport) = self.user_message_viewports.borrow().get(&message_id) {
            let scroll = &viewport.scroll_handle;
            let bounds = scroll.bounds();
            let margin = px(18.0);
            if scroll.max_offset().y > px(0.5)
                && (match_bounds.top() < bounds.top() + margin
                    || match_bounds.bottom() > bounds.bottom() - margin)
            {
                let current = scroll.offset();
                let target_y = bounds.top() + bounds.size.height * 0.35;
                let next = (current.y + target_y - match_bounds.top())
                    .clamp(-scroll.max_offset().y, Pixels::ZERO);
                if next != current.y {
                    scroll.set_offset(point(current.x, next));
                    match_bounds.origin.y += next - current.y;
                    self.detach_transcript_search_from_tail();
                    cx.notify();
                }
            }
        }

        let rows = self.active_transcript_rows();
        let viewport = rows.viewport_bounds();
        if viewport.size.height <= Pixels::ZERO {
            return;
        }
        let margin = px(24.0);
        if match_bounds.top() >= viewport.top() + margin
            && match_bounds.bottom() <= viewport.bottom() - margin
        {
            return;
        }
        let target_y = viewport.top() + viewport.size.height * 0.35;
        let current = rows.scroll_px_offset_for_scrollbar().y;
        let max_offset = rows.max_offset_for_scrollbar().y;
        let next = (current + (target_y - match_bounds.top())).clamp(-max_offset, Pixels::ZERO);
        if next != current {
            self.detach_transcript_search_from_tail();
            rows.set_offset_from_scrollbar(point(Pixels::ZERO, next));
            cx.notify();
        }
    }

    fn reveal_transcript_search_geometry(
        &mut self,
        target: TranscriptSearchTarget,
        attempt: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let still_current = self.transcript_search.as_ref().is_some_and(|search| {
            search.open
                && search.generation == target.generation
                && search.current == Some(target.current)
        });
        if !still_current {
            return;
        }
        let match_bounds = self.transcript_search_match_bounds(&target);
        let Some(match_bounds) = match_bounds else {
            if attempt < 1 {
                cx.on_next_frame(window, move |this, window, cx| {
                    this.reveal_transcript_search_geometry(target, attempt + 1, window, cx)
                });
            }
            return;
        };
        self.position_transcript_match_bounds(target.message_id, match_bounds, cx);
    }

    pub(super) fn render_transcript_search_bar(
        &self,
        chat_viewport_width: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let search = self
            .transcript_search
            .as_ref()
            .filter(|search| search.open)?;
        let theme = Theme::current(cx);
        let query_empty = search.query.read(cx).content().is_empty();
        let has_matches = !search.matches.is_empty();
        let count_label: Option<SharedString> = if query_empty {
            None
        } else if !has_matches {
            Some(tr!("find.no_results").into())
        } else {
            let current = search.current.map(|index| index + 1).unwrap_or(0);
            let suffix = if search.limited { "+" } else { "" };
            Some(SharedString::from(tr!(
                "find.result_count",
                current = current,
                total = format!("{}{suffix}", search.matches.len())
            )))
        };
        let bar_width = (chat_viewport_width - 24.0).clamp(260.0, 430.0);
        let input_width = (bar_width - 154.0).max(104.0);
        let query = search.query.clone();
        let previous_focus = self.transcript_control_focus("transcript-find-previous", cx);
        let next_focus = self.transcript_control_focus("transcript-find-next", cx);
        let close_focus = self.transcript_control_focus("transcript-find-close", cx);

        let previous = icon_button("transcript-find-previous", "icons/arrow-up.svg", theme)
            .track_focus(&previous_focus)
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .opacity(if has_matches { 1.0 } else { 0.45 })
            .tooltip(|window, cx| Tooltip::new(tr!("find.previous_match")).build(window, cx))
            .when(has_matches, |button| {
                button.on_activation(cx, |this, _, cx| this.navigate_transcript_search(true, cx))
            });
        let next = icon_button("transcript-find-next", "icons/arrow-down.svg", theme)
            .track_focus(&next_focus)
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .opacity(if has_matches { 1.0 } else { 0.45 })
            .tooltip(|window, cx| Tooltip::new(tr!("find.next_match")).build(window, cx))
            .when(has_matches, |button| {
                button.on_activation(cx, |this, _, cx| this.navigate_transcript_search(false, cx))
            });
        let close = icon_button("transcript-find-close", "icons/x.svg", theme)
            .track_focus(&close_focus)
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .tooltip(|window, cx| Tooltip::new(tr!("find.close")).build(window, cx))
            .on_activation(cx, |this, window, cx| {
                this.close_transcript_search(true, window, cx)
            });

        Some(
            div()
                .id("transcript-search-bar")
                .key_context("FindBar")
                .absolute()
                .top(px(8.0))
                .right(px(12.0))
                .w(px(bar_width))
                .max_w_full()
                .rounded(px(10.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.raised)
                .shadow_xs()
                .px(px(6.0))
                .py(px(6.0))
                .flex()
                .items_center()
                .gap(px(5.0))
                // The transcript canvas takes focus on mouse down so Cmd/Ctrl-F
                // routes here instead of to a stale editor focus. Keep that
                // parent listener from stealing focus back from this field.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    TextField::new("transcript-find-query", query)
                        .w(px(input_width))
                        .flex_none(),
                )
                .child(
                    div()
                        .min_w(px(62.0))
                        .flex_1()
                        .text_size(sp(12.5))
                        .whitespace_nowrap()
                        .text_color(if !query_empty && !has_matches {
                            theme.danger
                        } else {
                            theme.text_tertiary
                        })
                        .children(count_label),
                )
                .child(previous)
                .child(next)
                .child(close)
                .into_any_element(),
        )
    }
}

fn literal_find_regex(query: &str) -> Regex {
    RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
        .expect("an escaped literal is always a valid regex")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_find_is_case_insensitive_and_unicode_safe() {
        let regex = literal_find_regex("waku");
        let (matches, limited) =
            md::render::markdown_search_matches("Waku **waku** WAKU", &regex, 20);
        assert!(!limited);
        assert_eq!(
            matches
                .iter()
                .map(|found| found.range.clone())
                .collect::<Vec<_>>(),
            vec![0..4, 5..9, 10..14]
        );

        let regex = literal_find_regex("界");
        let (matches, _) = md::render::markdown_search_matches("世**界** 世界", &regex, 20);
        assert_eq!(matches.len(), 2);
        assert!(
            matches
                .iter()
                .all(|found| found.range.start < found.range.end)
        );
    }

    #[test]
    fn markdown_matches_use_the_renderers_element_ordinals() {
        let regex = literal_find_regex("needle");
        let source = "first\n\n> needle\n\n```txt\nneedle\n```";
        let (matches, limited) = md::render::markdown_search_matches(source, &regex, 20);
        assert!(!limited);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].ordinal, 1 << 16);
        assert_eq!(matches[1].ordinal, 2 << 16);
    }

    #[test]
    fn transcript_search_cap_reports_more_results() {
        let regex = literal_find_regex("x");
        let (matches, limited) = md::render::markdown_search_matches("xxxx", &regex, 3);
        assert_eq!(matches.len(), 3);
        assert!(limited);
    }
}
