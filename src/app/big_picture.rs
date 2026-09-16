//! Big Picture mode: a ⌘0 overlay of the sessions most worth a glance.
//!
//! Cards fill as much of a grid as the window affords — up to eight across
//! on a wide screen, further rows when it's tall — tasks waiting on input
//! first, then unread completions, then running work, then whatever the
//! sidebar saw most recently — each carrying a live tail of its transcript.
//! The card set is reconciled every frame the overlay is open, so a session
//! that newly blocks slides in while a settled one fades out rather than the
//! grid jumping.
//!
//! Clicking a card never navigates: it targets the docked composer at that
//! session, and clicking again — or sending — returns it to new-task mode.
//! The arrow keys do the same without a confirm step: they move the highlight
//! and retarget the composer in one motion. Escape peels off the target first,
//! then closes the overlay; clicking the scrim closes it outright.

use gpui::{KeyBinding, actions};

use super::*;

pub const MAX_CARDS: usize = 8;
const CARD_GAP: f32 = 16.0;
const CARD_RADIUS: f32 = 14.0;
/// Distance from the window's side edges to the card grid and composer.
const EDGE_MARGIN: f32 = 32.0;
/// Clears the 48px titlebar — and the traffic lights inside it — plus a
/// comfortable gap before the first card.
const TOP_MARGIN: f32 = 64.0;
/// Air between the card grid's bottom edge and the docked composer.
const CARD_COMPOSER_GAP: f32 = 20.0;
/// Narrowest a card can get before the layout would rather clip than crush
/// the header row — only reached on very small windows.
const CARD_MIN_WIDTH: f32 = 140.0;
/// The width a card wants. Columns are how many of these the window affords:
/// four across a normal screen, up to eight on a wide one.
const CARD_TARGET_WIDTH: f32 = 300.0;
/// The shortest row worth adding. Taller windows stack a second (or further)
/// row of cards instead of stretching one row past readability.
const CARD_MIN_ROW_HEIGHT: f32 = 240.0;
/// The composer lane's height before it has ever been measured — a prompt's
/// single-line footprint, used for the first open's grid math.
const COMPOSER_HEIGHT_FALLBACK: f32 = 96.0;
/// The hint line's footprint below the composer.
const HINT_HEIGHT: f32 = 16.0;
const COMPOSER_BOTTOM_MARGIN: f32 = 28.0;
const HINT_BOTTOM_MARGIN: f32 = 10.0;
const CARD_TRANSITION: Duration = Duration::from_millis(220);
/// Each card waits this long past its left neighbor before rising on open.
const CARD_STAGGER: Duration = Duration::from_millis(55);
/// A leaving card outlives its transition by a hair so the timer never cuts
/// the last frames.
const CARD_EXIT_LINGER: Duration = Duration::from_millis(260);
/// How far the composer travels on open, and how long that takes.
const COMPOSER_RISE: f32 = 28.0;
const COMPOSER_TRANSITION: Duration = Duration::from_millis(300);
const PREVIEW_MESSAGES: usize = 5;
const PREVIEW_SNIPPET_GRAPHEMES: usize = 220;

actions!(
    waku_big_picture,
    [
        DismissBigPicture,
        BigPictureLeft,
        BigPictureRight,
        BigPictureConfirm,
    ]
);

/// ⌘1–⌘9 — arm the composer on the nth visible card. While the overlay owns
/// the keymap this shadows the sidebar's session chords, which have no
/// business retargeting the background session.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = waku_big_picture, no_json)]
pub struct SelectBigPictureCard {
    pub index: usize,
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", DismissBigPicture, Some("BigPicture")),
        KeyBinding::new("left", BigPictureLeft, Some("BigPicture")),
        KeyBinding::new("right", BigPictureRight, Some("BigPicture")),
        KeyBinding::new("enter", BigPictureConfirm, Some("BigPicture")),
    ]);
    for index in 0..9 {
        cx.bind_keys([KeyBinding::new(
            &format!("secondary-{}", index + 1),
            SelectBigPictureCard { index },
            Some("BigPicture"),
        )]);
    }
}

/// A mounted card. Slots outlive their session's rank so an evicted card can
/// fade where it stood instead of vanishing mid-grid.
#[derive(Clone)]
struct BigPictureSlot {
    session_id: Uuid,
    /// Flat row-major position in the grid; only an index or count change
    /// re-runs the slide, so a window resize moves cards without replaying a
    /// transition per frame.
    index: usize,
    left: f32,
    /// Where the in-flight transition started, for FLIP slides on re-sort.
    from_left: f32,
    top: f32,
    from_top: f32,
    /// Fill size this frame; resized live, so a viewport drag never replays.
    width: f32,
    from_width: f32,
    height: f32,
    from_height: f32,
    leaving: bool,
    /// Fresh mounts fade up; reordered or surviving cards only slide.
    entering: bool,
    /// How long a fresh mount waits before it starts rising — the stagger.
    enter_delay: Duration,
    /// Bumped per transition so `with_animation` replays from delta 0.
    anim_seq: u64,
}

/// Runtime-only overlay state; nothing here persists across windows.
pub(super) struct BigPictureUi {
    open: bool,
    slots: Vec<BigPictureSlot>,
    /// Arrow-key position — the card Enter or a click arms as the target.
    highlighted: Option<Uuid>,
    /// The session the docked composer follows up on; `None` starts a task.
    target: Option<Uuid>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    focus_generation: u64,
    anim_seq: u64,
    /// Bumped on every open so the composer's rise replays from delta 0.
    open_seq: u64,
    /// A prompt that arrived before its projectless workspace did; drained by
    /// the completion that materializes the session it belongs to.
    pending_submission: Option<ComposerSubmission>,
    /// Sessions whose transcript hydration this open already requested, so a
    /// persistently failing load cannot re-spawn (and re-toast) every frame.
    hydrate_requested: HashSet<Uuid>,
    /// Card count from the last reconcile; a change means a card entered or
    /// left — which animates geometry — while an unchanged count means any
    /// geometry delta is a viewport resize and stays instant.
    last_row_count: usize,
    /// Blurred snapshot of the frame underneath, captured as the overlay
    /// opens and painted under the scrim.
    backdrop: Option<Arc<gpui::RenderImage>>,
}

impl BigPictureUi {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            slots: Vec::new(),
            highlighted: None,
            target: None,
            focus,
            previous_focus: None,
            focus_generation: 0,
            anim_seq: 0,
            open_seq: 0,
            pending_submission: None,
            hydrate_requested: HashSet::new(),
            last_row_count: 0,
            backdrop: None,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// The card the docked composer follows up on; `None` starts a task.
    pub(super) fn target(&self) -> Option<Uuid> {
        self.target
    }
}

/// Selection priority, left to right: a task parked on its user outranks a
/// finished one they have not read, which outranks one still working; plain
/// recency fills whatever slots remain.
fn big_picture_tier(session: &AgentSession, unseen: &HashMap<Uuid, u64>) -> u8 {
    match session.status {
        SessionStatus::Waiting => 0,
        SessionStatus::Idle if unseen.contains_key(&session.id) => 1,
        status if status.is_busy() => 2,
        _ => 3,
    }
}

/// The card an arrow key lands on: the neighbor of the highlighted one,
/// wrapping at both ends; with nothing highlighted — or a highlight that
/// fell off the row — the edge the arrow faces.
fn big_picture_arrow_target(
    order: &[Uuid],
    highlighted: Option<Uuid>,
    reverse: bool,
) -> Option<Uuid> {
    if order.is_empty() {
        return None;
    }
    let next = match highlighted.and_then(|id| order.iter().position(|candidate| *candidate == id))
    {
        Some(index) if reverse => (index + order.len() - 1) % order.len(),
        Some(index) => (index + 1) % order.len(),
        None if reverse => order.len() - 1,
        None => 0,
    };
    order.get(next).copied()
}

/// The card order for this frame: tier first, then most recently touched.
/// Waiting sessions rank by `updated_at` — the moment they parked — matching
/// `next_unread_session`; everything else ranks by sidebar recency, with the
/// unseen-completion stamp counting as activity. `limit` is how many cards
/// this frame's grid can afford.
fn big_picture_order(
    sessions: &[AgentSession],
    unseen: &HashMap<Uuid, u64>,
    limit: usize,
) -> Vec<Uuid> {
    let mut eligible = sessions
        .iter()
        .filter(|session| session.has_started() && session.archived_at.is_none())
        .collect::<Vec<_>>();
    eligible.sort_by_key(|session| {
        let recency = if session.status == SessionStatus::Waiting {
            session.updated_at
        } else {
            sidebar::sidebar_session_timestamp(session)
                .max(unseen.get(&session.id).copied().unwrap_or(0))
        };
        (
            big_picture_tier(session, unseen),
            std::cmp::Reverse(recency),
            std::cmp::Reverse(session.created_at),
        )
    });
    eligible
        .into_iter()
        .take(limit)
        .map(|session| session.id)
        .collect()
}

/// One card's resting geometry inside the grid, in grid-local coordinates —
/// row 0 sits under `TOP_MARGIN`.
#[derive(Clone, Copy)]
struct CardGeometry {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

/// Lay out `count` cards row-major over `rows` rows, each row balanced to
/// within one card of the next. Every card takes the grid's uniform size; a
/// short row centers itself rather than hugging the left edge.
fn big_picture_card_geometry(
    count: usize,
    rows: usize,
    row_width: f32,
    card_width: f32,
    card_height: f32,
) -> Vec<CardGeometry> {
    let base = count / rows;
    let extra = count % rows;
    let mut cards = Vec::with_capacity(count);
    for row in 0..rows {
        // Even the rows out: the first `extra` rows carry one card more.
        let row_count = base + usize::from(row < extra);
        let row_span =
            row_count as f32 * card_width + row_count.saturating_sub(1) as f32 * CARD_GAP;
        let row_left = ((row_width - row_span) / 2.0).max(0.0);
        for column in 0..row_count {
            if cards.len() == count {
                break;
            }
            cards.push(CardGeometry {
                left: row_left + column as f32 * (card_width + CARD_GAP),
                top: row as f32 * (card_height + CARD_GAP),
                width: card_width,
                height: card_height,
            });
        }
    }
    cards
}

/// A card's transcript tail: the last few visible messages, newest last,
/// pinned to the bottom of the card by the column's `justify_end`.
fn big_picture_preview_lines(session: &AgentSession) -> Vec<(MessageRole, String)> {
    session
        .messages
        .iter()
        .rev()
        .filter(|message| {
            !message.hidden
                && message.role != MessageRole::System
                && !message.visible_content().trim().is_empty()
        })
        .take(PREVIEW_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| {
            let snippet = message
                .visible_content()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            // Agent text renders whole — a clipped sentence mid-reply reads
            // as a bug. Only user prompts keep a cap: a pasted wall of text
            // would otherwise swallow the card's other lines.
            let snippet = if message.role == MessageRole::User {
                snippet
                    .graphemes(true)
                    .take(PREVIEW_SNIPPET_GRAPHEMES)
                    .collect::<String>()
            } else {
                snippet
            };
            (message.role, snippet)
        })
        .collect()
}

impl Waku {
    pub(super) fn toggle_big_picture_action(
        &mut self,
        _: &ToggleBigPicture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture.open {
            self.close_big_picture(window, cx);
        } else {
            self.open_big_picture(window, cx);
        }
    }

    fn open_big_picture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.is_open() {
            self.toggle_command_palette_action(&ToggleCommandPalette, window, cx);
        }
        if self.file_finder.is_open() {
            self.close_file_finder(window, cx);
        }
        if self.task_switcher.is_open() {
            self.cancel_task_switcher(window, cx);
        }
        if self.project_switcher.is_open() {
            self.cancel_project_switcher(window, cx);
        }
        self.big_picture.previous_focus = window.focused(cx);
        self.big_picture.open = true;
        self.big_picture.slots.clear();
        self.big_picture.target = None;
        self.big_picture.hydrate_requested.clear();
        self.big_picture.focus_generation = self.big_picture.focus_generation.wrapping_add(1);
        self.big_picture.open_seq = self.big_picture.open_seq.wrapping_add(1);
        self.big_picture.last_row_count = 0;
        self.big_picture.highlighted =
            big_picture_order(&self.state.sessions, &self.state.unseen_completions, MAX_CARDS)
                .first()
                .copied();
        self.sync_big_picture_placeholder(cx);
        self.big_picture.backdrop = None;
        // Snapshot the frame before the scrim covers it — capture, repack,
        // and blur all run on the background executor.
        if let Some(window_id) = crate::platform::window_capture_id(window) {
            cx.spawn(async move |waku, cx| {
                let backdrop = cx
                    .background_executor()
                    .spawn(async move { crate::platform::blurred_window_snapshot(window_id) })
                    .await;
                let _ = waku.update(cx, |this, cx| {
                    if this.big_picture.open
                        && let Some(backdrop) = backdrop
                    {
                        this.big_picture.backdrop = Some(backdrop);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let generation = self.big_picture.focus_generation;
        let focus = self.big_picture.focus.clone();
        let weak = cx.entity().downgrade();
        // Deferred overlays join the dispatch tree after their deferred paint,
        // the same two-frame wait the task switcher uses.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let should_focus = weak
                    .update(cx, |this, _| {
                        this.big_picture.open && this.big_picture.focus_generation == generation
                    })
                    .unwrap_or(false);
                if should_focus {
                    window.focus(&focus, cx);
                }
            });
        });
        cx.notify();
    }

    pub(super) fn close_big_picture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.big_picture.open {
            return;
        }
        self.big_picture.open = false;
        self.big_picture.slots.clear();
        self.big_picture.highlighted = None;
        self.big_picture.target = None;
        self.big_picture.pending_submission = None;
        self.big_picture.backdrop = None;
        self.composer.update(cx, |composer, cx| {
            composer.set_placeholder(tr!("input.do_anything"), cx);
        });
        if let Some(previous_focus) = self.big_picture.previous_focus.take() {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    /// Keep the composer hint honest about where Enter sends: a follow-up on
    /// the targeted task, or a brand-new one.
    fn sync_big_picture_placeholder(&self, cx: &mut Context<Self>) {
        let placeholder = match self.big_picture.target {
            Some(session_id) => self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| {
                    tr!(
                        "big_picture.follow_up_placeholder",
                        title = session.display_title()
                    )
                })
                .unwrap_or_else(|| tr!("big_picture.new_task_placeholder")),
            None => tr!("big_picture.new_task_placeholder"),
        };
        self.composer.update(cx, |composer, cx| {
            composer.set_placeholder(placeholder, cx);
        });
    }

    fn set_big_picture_target(&mut self, target: Option<Uuid>, cx: &mut Context<Self>) {
        if self.big_picture.target == target {
            return;
        }
        self.big_picture.target = target;
        self.sync_big_picture_placeholder(cx);
        cx.notify();
    }

    fn toggle_big_picture_target(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.big_picture.highlighted = Some(session_id);
        let target = (self.big_picture.target != Some(session_id)).then_some(session_id);
        self.set_big_picture_target(target, cx);
    }

    /// ⌘n — arm the nth card in row-major order, or peel the target off if
    /// it's already armed. An index past the visible grid does nothing; the
    /// sidebar chord it shadows must never leak through to selection.
    fn select_big_picture_card_action(
        &mut self,
        action: &SelectBigPictureCard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let order = self.big_picture_navigable();
        let Some(session_id) = order.get(action.index).copied() else {
            return;
        };
        self.big_picture.highlighted = Some(session_id);
        let target = (self.big_picture.target != Some(session_id)).then_some(session_id);
        self.set_big_picture_target(target, cx);
    }

    /// The ids arrow keys and Enter walk — the live cards, in row-major
    /// display order.
    fn big_picture_navigable(&self) -> Vec<Uuid> {
        let mut slots = self
            .big_picture
            .slots
            .iter()
            .filter(|slot| !slot.leaving)
            .collect::<Vec<_>>();
        slots.sort_by_key(|slot| slot.index);
        slots.iter().map(|slot| slot.session_id).collect()
    }

    /// Arrows move the highlight *and* retarget the composer — no separate
    /// confirm step. The docked prompt follows whichever card is highlighted.
    fn move_big_picture_highlight(&mut self, reverse: bool, cx: &mut Context<Self>) {
        let order = self.big_picture_navigable();
        let Some(next) = big_picture_arrow_target(&order, self.big_picture.highlighted, reverse)
        else {
            return;
        };
        self.big_picture.highlighted = Some(next);
        self.set_big_picture_target(Some(next), cx);
    }

    /// Escape peels off one layer at a time: an armed target first, the
    /// overlay second.
    pub(super) fn dismiss_big_picture_action(
        &mut self,
        _: &DismissBigPicture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture.target.is_some() {
            self.set_big_picture_target(None, cx);
        } else {
            self.close_big_picture(window, cx);
        }
    }

    pub(super) fn big_picture_left_action(
        &mut self,
        _: &BigPictureLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_big_picture_highlight(true, cx);
    }

    pub(super) fn big_picture_right_action(
        &mut self,
        _: &BigPictureRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_big_picture_highlight(false, cx);
    }

    /// Enter no longer arms the target — arrows and clicks already did. Its
    /// one remaining job is dropping focus into the docked composer.
    pub(super) fn big_picture_confirm_action(
        &mut self,
        _: &BigPictureConfirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    /// Reconcile mounted slots against this frame's ranking. Runs from
    /// `render` — the work is a sort of a sessions-length vec plus, at most,
    /// one spawn per card entering or leaving; steady state touches nothing.
    fn reconcile_big_picture_slots(
        &mut self,
        desired: &[Uuid],
        geometry: &[CardGeometry],
        cx: &mut Context<Self>,
    ) {
        for session_id in desired {
            if self.big_picture.hydrate_requested.insert(*session_id) {
                self.ensure_session_loaded(*session_id, cx);
            }
        }
        let mut seq = self.big_picture.anim_seq;
        // A slot whose session ranked back in before its exit finished just
        // rejoins the row — the animation restart reads as a fade back up.
        for slot in &mut self.big_picture.slots {
            if slot.leaving && desired.contains(&slot.session_id) {
                slot.leaving = false;
                slot.entering = true;
                // A card that never finished leaving rises back where it
                // stands — it skipped the entrance line, so no stagger.
                slot.enter_delay = Duration::ZERO;
                seq += 1;
                slot.anim_seq = seq;
            }
        }
        let exiting = self
            .big_picture
            .slots
            .iter()
            .filter(|slot| !slot.leaving && !desired.contains(&slot.session_id))
            .map(|slot| (slot.session_id, slot.anim_seq))
            .collect::<Vec<_>>();
        for (session_id, _) in exiting {
            seq += 1;
            if let Some(slot) = self
                .big_picture
                .slots
                .iter_mut()
                .find(|slot| slot.session_id == session_id)
            {
                slot.leaving = true;
                slot.anim_seq = seq;
            }
            let slot_seq = seq;
            cx.spawn(async move |waku, cx| {
                cx.background_executor().timer(CARD_EXIT_LINGER).await;
                let _ = waku.update(cx, |this, cx| {
                    this.big_picture.slots.retain(|slot| {
                        !(slot.session_id == session_id
                            && slot.leaving
                            && slot.anim_seq == slot_seq)
                    });
                    cx.notify();
                });
            })
            .detach();
        }
        // A card arriving or leaving changes every card's geometry, so those
        // slides animate. A viewport resize does not — same count, new size,
        // no replayed transition chasing the drag.
        let count_changed = self.big_picture.last_row_count != desired.len();
        for (index, session_id) in desired.iter().enumerate() {
            let CardGeometry {
                left,
                top,
                width,
                height,
            } = geometry[index];
            match self
                .big_picture
                .slots
                .iter_mut()
                .find(|slot| slot.session_id == *session_id)
            {
                Some(slot) if !slot.leaving => {
                    if slot.index != index
                        || (count_changed
                            && (slot.left != left
                                || slot.top != top
                                || slot.width != width
                                || slot.height != height))
                    {
                        slot.from_left = slot.left;
                        slot.from_top = slot.top;
                        slot.from_width = slot.width;
                        slot.from_height = slot.height;
                        slot.index = index;
                        slot.entering = false;
                        seq += 1;
                        slot.anim_seq = seq;
                    }
                    slot.left = left;
                    slot.top = top;
                    slot.width = width;
                    slot.height = height;
                }
                Some(_) => {}
                None => {
                    seq += 1;
                    self.big_picture.slots.push(BigPictureSlot {
                        session_id: *session_id,
                        index,
                        left,
                        from_left: left,
                        top,
                        from_top: top,
                        width,
                        from_width: width,
                        height,
                        from_height: height,
                        leaving: false,
                        entering: true,
                        enter_delay: CARD_STAGGER * index as u32,
                        anim_seq: seq,
                    });
                }
            }
        }
        self.big_picture.anim_seq = seq;
        self.big_picture.last_row_count = desired.len();
        // A targeted or highlighted session can leave the grid — or the task
        // list — under the overlay; the highlight only tracks visible cards.
        if self
            .big_picture
            .highlighted
            .is_some_and(|id| !desired.contains(&id))
        {
            self.big_picture.highlighted = desired.first().copied();
        }
        if self
            .big_picture
            .target
            .is_some_and(|id| !self.state.sessions.iter().any(|s| s.id == id))
        {
            self.big_picture.target = None;
            self.sync_big_picture_placeholder(cx);
        }
    }

    /// Route a composer submit while the overlay is open: follow up on the
    /// targeted session, or start a new task in the highlighted card's
    /// project — the session underneath's, otherwise.
    pub(super) fn submit_big_picture_submission(
        &mut self,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        match self.big_picture.target {
            Some(session_id) => {
                self.submit_composer_submission_to(session_id, submission, cx);
                self.set_big_picture_target(None, cx);
            }
            None => {
                let project = self
                    .big_picture
                    .highlighted
                    .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
                    .map(|session| session.project_id)
                    .or_else(|| self.selected_session().map(|session| session.project_id))
                    .or_else(|| self.selected_project().map(|project| project.id))
                    .and_then(|project_id| {
                        self.state
                            .projects
                            .iter()
                            .find(|project| project.id == project_id)
                    })
                    .map(|project| (project.id, project.is_projectless()));
                match project {
                    Some((project_id, false)) => {
                        self.create_session_for(project_id, self.state.last_provider, cx);
                        if let Some(session_id) = self.state.selected_session {
                            self.submit_composer_submission(submission, cx);
                            // Keep the composer on the task it just started so
                            // a second message follows up instead of spawning
                            // another task.
                            self.big_picture.highlighted = Some(session_id);
                            self.set_big_picture_target(Some(session_id), cx);
                        }
                    }
                    // A projectless workspace is provisioned by the daemon;
                    // the completion drains the stashed prompt once the new
                    // session exists. A reused projectless draft resolves
                    // synchronously, so drain right away too.
                    _ => {
                        self.big_picture.pending_submission = Some(submission);
                        self.create_projectless_session(cx);
                        self.drain_big_picture_pending_submission(cx);
                    }
                }
            }
        }
    }

    /// Sends the stashed prompt once a fresh draft is selected — called both
    /// after `create_projectless_session` (its draft-reuse path resolves
    /// synchronously) and from its async workspace-creation completion.
    pub(super) fn drain_big_picture_pending_submission(&mut self, cx: &mut Context<Self>) {
        let ready = self
            .selected_session()
            .is_some_and(|session| !session.has_started());
        if !ready {
            return;
        }
        if let Some(submission) = self.big_picture.pending_submission.take() {
            self.submit_composer_submission(submission, cx);
            if let Some(session_id) = self.state.selected_session {
                self.big_picture.highlighted = Some(session_id);
                self.big_picture.target = Some(session_id);
                self.sync_big_picture_placeholder(cx);
            }
        }
    }

    pub(super) fn steer_big_picture_submission(
        &mut self,
        submission: ComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        match self.big_picture.target {
            Some(session_id) => {
                self.steer_session_submission(session_id, submission, cx);
                self.set_big_picture_target(None, cx);
            }
            None => self.submit_big_picture_submission(submission, cx),
        }
    }

    fn render_big_picture_card(
        &self,
        session: &AgentSession,
        slot: &BigPictureSlot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let session_id = session.id;
        let is_target = self.big_picture.target == Some(session_id);
        let is_highlighted = self.big_picture.highlighted == Some(session_id);
        let status = session.status;
        let leaving = slot.leaving;
        let entering = slot.entering;
        let left = slot.left;
        let from_left = slot.from_left;
        let top = slot.top;
        let from_top = slot.from_top;
        let width = slot.width;
        let from_width = slot.from_width;
        let height = slot.height;
        let from_height = slot.from_height;
        let enter_delay = slot.enter_delay;
        let anim_id =
            SharedString::from(format!("big-picture-card-{session_id}-{}", slot.anim_seq));
        let preview = if session.detail_loaded {
            big_picture_preview_lines(session)
        } else {
            Vec::new()
        };
        // The newest in-flight activity — what the agent is doing right now —
        // pinned below the message tail like the transcript's live group.
        let active_tool = session
            .status
            .is_busy()
            .then(|| {
                session
                    .transcript_blocks
                    .iter()
                    .rev()
                    .flat_map(|block| block.activities.iter().rev())
                    .find(|activity| !activity.complete)
            })
            .flatten();
        let border = if is_target {
            theme.accent
        } else if is_highlighted {
            theme.border_strong
        } else {
            theme.border
        };
        let body: AnyElement = if !session.detail_loaded {
            div()
                .text_size(sp(11.5))
                .text_color(theme.text_ghost)
                .child(tr!("big_picture.loading"))
                .into_any_element()
        } else if preview.is_empty() && active_tool.is_none() {
            div()
                .text_size(sp(11.5))
                .text_color(theme.text_ghost)
                .child(tr!("big_picture.empty_transcript"))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .justify_end()
                .gap(px(6.0))
                .size_full()
                .children(preview.into_iter().map(|(role, snippet)| {
                    div()
                        .text_size(sp(11.5))
                        .line_height(sp(15.0))
                        .whitespace_normal()
                        .text_color(match role {
                            MessageRole::User => theme.text,
                            _ => theme.text_secondary,
                        })
                        .when(role == MessageRole::User, |element| {
                            element
                                .bg(theme.overlay)
                                .rounded(px(8.0))
                                .px(px(8.0))
                                .py(px(4.0))
                        })
                        .child(snippet)
                        .into_any_element()
                }))
                .when_some(active_tool, |element, activity| {
                    let title = activity.reasoning.as_ref().map_or_else(
                        || activity_display_title(activity),
                        |reasoning| reasoning_activity_title(reasoning, true),
                    );
                    element.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(motion::spin_slow(icon(
                                "icons/loader-circle.svg",
                                10.0,
                                status_color(&theme, status),
                            )))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(11.5))
                                    .text_color(theme.text_secondary)
                                    .child(title),
                            ),
                    )
                })
                .into_any_element()
        };
        let mut card = div()
            .id(SharedString::from(format!("big-picture-card-{session_id}")))
            .size_full()
            .rounded(px(CARD_RADIUS))
            .border(hairline())
            .border_color(border)
            .when(is_target, |element| element.shadow_md())
            .bg(theme.raised)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .cursor_default()
            .child(
                // The same two-line row the sidebar uses, wrapped in the
                // card's header chrome.
                div()
                    .flex_none()
                    .px(px(8.0))
                    .py(px(7.0))
                    .border_b(hairline())
                    .border_color(theme.border)
                    .rounded_t(px(CARD_RADIUS - 1.0))
                    .bg(theme.surface)
                    .child(self.render_session_row_body(session_id, false, false, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(12.0))
                    .overflow_hidden()
                    .child(body),
            );
        if self.session_rename == Some(session_id) {
            card = card.on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                if this.session_rename == Some(session_id) {
                    this.commit_session_rename(cx);
                }
            }));
        }
        if !leaving {
            card = card
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        // Double-click commits: leave the overlay on the card's
                        // session. The first click's target toggle is harmless —
                        // the target resets on close anyway.
                        if event.click_count == 2 && this.big_picture.open {
                            this.close_big_picture(window, cx);
                            this.select_session(session_id, cx);
                        }
                    }),
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.big_picture.highlighted != Some(session_id) {
                        this.big_picture.highlighted = Some(session_id);
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.big_picture.open {
                        this.toggle_big_picture_target(session_id, cx);
                        cx.stop_propagation();
                    }
                }));
        }
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .child(card)
            .with_animation(
                anim_id,
                Animation::new(if entering {
                    CARD_TRANSITION + enter_delay
                } else {
                    CARD_TRANSITION
                })
                .with_easing(ease_out_quint()),
                move |element, delta| {
                    // The stagger folds into the animation's span: the card
                    // sits invisible through its delay, then rises.
                    let delta = if entering {
                        let delay = enter_delay.as_secs_f32()
                            / (CARD_TRANSITION + enter_delay).as_secs_f32();
                        ((delta - delay) / (1.0 - delay)).clamp(0.0, 1.0)
                    } else {
                        delta
                    };
                    let mut top = from_top + (top - from_top) * delta;
                    if leaving {
                        top += 10.0 * delta;
                    } else if entering {
                        top += 14.0 * (1.0 - delta);
                    }
                    element
                        .left(px(from_left + (left - from_left) * delta))
                        .top(px(top))
                        .w(px(from_width + (width - from_width) * delta))
                        .h(px(from_height + (height - from_height) * delta))
                        .opacity(if leaving {
                            1.0 - delta
                        } else if entering {
                            delta
                        } else {
                            1.0
                        })
                },
            )
            .into_any_element()
    }

    /// The chip the shared composer card shows above the field while a card
    /// is targeted — the destination stays visible even once a draft hides
    /// the placeholder.
    pub(super) fn render_big_picture_target_chip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.big_picture.is_open() {
            return None;
        }
        let theme = Theme::current(cx);
        let title = self.big_picture.target.and_then(|session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.display_title().to_owned())
        })?;
        let clear_target_focus = self.transcript_control_focus("big-picture-clear-target", cx);
        Some(
            div()
                .flex_none()
                .mx(px(10.0))
                .mb(px(6.0))
                .h(px(24.0))
                .pl(px(8.0))
                .pr(px(4.0))
                .rounded(px(7.0))
                .bg(theme.accent.opacity(0.12))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon("icons/corner-down-right.svg", 11.0, theme.accent))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(sp(11.5))
                        .text_color(theme.accent)
                        .child(tr!("big_picture.replying_to", title = title)),
                )
                .child(
                    div()
                        .id("big-picture-clear-target")
                        .track_focus(&clear_target_focus)
                        .tab_index(0)
                        .size(px(18.0))
                        .flex_none()
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_default()
                        .hover(|element| element.bg(theme.overlay_strong))
                        .focus_visible(|element| {
                            element.border(hairline()).border_color(theme.accent)
                        })
                        .child(icon("icons/x.svg", 10.0, theme.accent))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_big_picture_target(None, cx);
                            cx.stop_propagation();
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.set_big_picture_target(None, cx);
                                cx.stop_propagation();
                            }
                        })),
                )
                .into_any_element(),
        )
    }

    /// The same composer card the session column docks — same controls, same
    /// shortcuts. Only the submit routing and the target chip differ.
    fn render_big_picture_composer(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        div()
            .w_full()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.render_composer(window, cx))
    }

    pub(super) fn render_big_picture(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.big_picture.open {
            return None;
        }
        let theme = Theme::current(cx);
        let viewport = window.viewport_size();
        // The grid fills what the window affords: columns come from width at
        // the card's target size, extra rows from leftover height. The composer
        // lane's last measured height stands in for the overlay's docked one —
        // the same card renders in both places.
        let row_width = (f32::from(viewport.width) - EDGE_MARGIN * 2.0).max(0.0);
        let composer_height = self.composer_lane_height.get().max(COMPOSER_HEIGHT_FALLBACK);
        let grid_height = (f32::from(viewport.height)
            - TOP_MARGIN
            - CARD_COMPOSER_GAP
            - composer_height
            - COMPOSER_BOTTOM_MARGIN
            - HINT_HEIGHT
            - HINT_BOTTOM_MARGIN)
            .max(0.0);
        let columns = (((row_width + CARD_GAP) / (CARD_TARGET_WIDTH + CARD_GAP)) as usize)
            .clamp(1, MAX_CARDS);
        let max_rows = (((grid_height + CARD_GAP) / (CARD_MIN_ROW_HEIGHT + CARD_GAP)) as usize)
            .clamp(1, MAX_CARDS);
        let desired = big_picture_order(
            &self.state.sessions,
            &self.state.unseen_completions,
            (columns * max_rows).min(MAX_CARDS),
        );
        let rows = desired.len().div_ceil(columns).max(1);
        let card_width =
            ((row_width - CARD_GAP * (columns - 1) as f32) / columns as f32).max(CARD_MIN_WIDTH);
        let card_height = (grid_height - CARD_GAP * (rows - 1) as f32) / rows as f32;
        let geometry = big_picture_card_geometry(
            desired.len(),
            rows,
            row_width,
            card_width,
            card_height,
        );
        self.reconcile_big_picture_slots(&desired, &geometry, cx);
        let slots = self.big_picture.slots.clone();
        let cards = slots
            .iter()
            .filter_map(|slot| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == slot.session_id)
                    .map(|session| (session, slot))
            })
            .map(|(session, slot)| self.render_big_picture_card(session, slot, cx))
            .collect::<Vec<_>>();
        let focus = self.big_picture.focus.clone();
        let backdrop = self.big_picture.backdrop.clone();
        let scrim = theme.backdrop().opacity(0.72);
        let layer = div()
            .id("big-picture-layer")
            .key_context("BigPicture")
            .track_focus(&focus)
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            // The blurred frame snapshot paints first; the scrim dims it.
            .children(backdrop.map(|image| {
                img(image)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Fill)
            }))
            .child(div().absolute().inset_0().bg(scrim))
            .on_action(cx.listener(Self::toggle_big_picture_action))
            .on_action(cx.listener(Self::dismiss_big_picture_action))
            .on_action(cx.listener(Self::big_picture_left_action))
            .on_action(cx.listener(Self::big_picture_right_action))
            .on_action(cx.listener(Self::big_picture_confirm_action))
            .on_action(cx.listener(Self::select_big_picture_card_action))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_big_picture(window, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .pt(px(TOP_MARGIN))
                    .px(px(EDGE_MARGIN))
                    .pb(px(CARD_COMPOSER_GAP))
                    .child(div().relative().size_full().children(cards)),
            )
            .child(
                // Composer and hint rise together on every open.
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .pb(px(COMPOSER_BOTTOM_MARGIN))
                            .px(px(EDGE_MARGIN))
                            .flex()
                            .justify_center()
                            .child(self.render_big_picture_composer(window, cx)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .pb(px(HINT_BOTTOM_MARGIN))
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_ghost)
                                    .child(tr!("big_picture.hint")),
                            ),
                    )
                    .with_animation(
                        SharedString::from(format!(
                            "big-picture-chrome-{}",
                            self.big_picture.open_seq
                        )),
                        Animation::new(COMPOSER_TRANSITION).with_easing(ease_out_quint()),
                        |element, delta| {
                            element
                                .top(px(COMPOSER_RISE * (1.0 - delta)))
                                .opacity(delta)
                        },
                    ),
            );
        Some(
            gpui::deferred(motion::fade_in("big-picture-layer-enter", layer))
                .with_priority(7)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(status: SessionStatus, last_reply_at: u64, updated_at: u64) -> AgentSession {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("task");
        session.status = status;
        session.last_reply_at = Some(last_reply_at);
        session.updated_at = updated_at;
        session
    }

    #[test]
    fn waiting_outranks_unread_which_outranks_running() {
        let running = session(SessionStatus::Working, 100, 100);
        let unread_idle = session(SessionStatus::Idle, 50, 50);
        let waiting = session(SessionStatus::Waiting, 10, 10);
        let settled = session(SessionStatus::Idle, 90, 90);
        let mut unseen = HashMap::new();
        unseen.insert(unread_idle.id, 40);
        let sessions = vec![
            running.clone(),
            settled.clone(),
            unread_idle.clone(),
            waiting.clone(),
        ];

        assert_eq!(
            big_picture_order(&sessions, &unseen, MAX_CARDS),
            vec![waiting.id, unread_idle.id, running.id, settled.id]
        );
    }

    #[test]
    fn within_a_tier_the_most_recently_updated_wins() {
        let older_wait = session(SessionStatus::Waiting, 10, 60);
        let newer_wait = session(SessionStatus::Waiting, 10, 90);
        let sessions = vec![older_wait.clone(), newer_wait.clone()];

        assert_eq!(
            big_picture_order(&sessions, &HashMap::new(), MAX_CARDS),
            vec![newer_wait.id, older_wait.id]
        );
    }

    #[test]
    fn the_grid_caps_at_the_frame_limit_and_skips_unstarted_drafts() {
        let draft = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let mut sessions = vec![draft];
        for index in 0..10 {
            sessions.push(session(SessionStatus::Idle, index as u64 + 1, 0));
        }
        let ordered = big_picture_order(&sessions, &HashMap::new(), 4);

        assert_eq!(ordered.len(), 4);
        assert!(
            !sessions[0..1]
                .iter()
                .any(|draft| ordered.contains(&draft.id))
        );
        assert_eq!(
            big_picture_order(&sessions, &HashMap::new(), MAX_CARDS).len(),
            MAX_CARDS
        );
    }

    #[test]
    fn unseen_stamp_counts_as_recency_for_idle_tasks() {
        let mut unseen = HashMap::new();
        let stale_unread = session(SessionStatus::Idle, 10, 10);
        let recent_read = session(SessionStatus::Idle, 80, 80);
        unseen.insert(stale_unread.id, 100);
        let sessions = vec![stale_unread.clone(), recent_read.clone()];

        // The unread task still leads on tier, not recency.
        assert_eq!(
            big_picture_order(&sessions, &unseen, MAX_CARDS),
            vec![stale_unread.id, recent_read.id]
        );
    }

    #[test]
    fn geometry_balances_rows_and_centers_the_short_one() {
        // Seven cards over two rows: 4 + 3, both rows centered on the grid.
        let cards = big_picture_card_geometry(7, 2, 1000.0, 235.0, 400.0);
        assert_eq!(cards.len(), 7);
        let full_row_span = 4.0 * 235.0 + 3.0 * CARD_GAP;
        assert_eq!(cards[0].left, (1000.0 - full_row_span) / 2.0);
        assert_eq!(cards[0].top, 0.0);
        assert_eq!(cards[4].top, 416.0);
        // The three-card row is centered: equal margins on both edges.
        let short_row_span = 3.0 * 235.0 + 2.0 * CARD_GAP;
        assert_eq!(cards[4].left, (1000.0 - short_row_span) / 2.0);
    }

    #[test]
    fn arrows_walk_the_row_and_wrap_at_both_ends() {
        let order = (0..3).map(|_| Uuid::new_v4()).collect::<Vec<_>>();

        // Nothing highlighted: each arrow starts at the edge it faces.
        assert_eq!(
            big_picture_arrow_target(&order, None, false),
            Some(order[0])
        );
        assert_eq!(big_picture_arrow_target(&order, None, true), Some(order[2]));

        assert_eq!(
            big_picture_arrow_target(&order, Some(order[0]), false),
            Some(order[1])
        );
        assert_eq!(
            big_picture_arrow_target(&order, Some(order[2]), false),
            Some(order[0])
        );
        assert_eq!(
            big_picture_arrow_target(&order, Some(order[0]), true),
            Some(order[2])
        );

        // A highlight that fell off the row restarts at an edge.
        let gone = Uuid::new_v4();
        assert_eq!(
            big_picture_arrow_target(&order, Some(gone), false),
            Some(order[0])
        );

        // An empty row offers nothing to land on.
        assert_eq!(big_picture_arrow_target(&[], Some(order[0]), false), None);
    }
}
