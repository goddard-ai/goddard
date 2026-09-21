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

use crate::persistence::ComposerDraftKey;

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
/// Each card waits this long past its left neighbor before fading in on open.
const CARD_STAGGER: Duration = Duration::from_millis(55);
/// A leaving card outlives its transition by a hair so the timer never cuts
/// the last frames.
const CARD_EXIT_LINGER: Duration = Duration::from_millis(260);
/// How long the composer's fade-in runs on open.
const COMPOSER_TRANSITION: Duration = Duration::from_millis(300);
/// Card transcripts render the lane's row kinds at this fraction of its
/// sizes — the same transcript, small enough to glance across a grid.
const CARD_TEXT_SCALE: f32 = 0.75;

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
    /// Fresh mounts fade in; reordered or surviving cards only slide.
    entering: bool,
    /// How long a fresh mount waits before it starts fading in — the stagger.
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
    /// The draft slot the docked composer's content currently belongs to —
    /// a card's own session draft while targeted, the standing new-task
    /// draft otherwise. Tracked rather than re-resolved: the swap captures
    /// the outgoing text under the key it was loaded from, so a highlight or
    /// selection that moved mid-edit cannot file it under the wrong session.
    pub(super) draft_key: Option<ComposerDraftKey>,
    /// Where an untargeted submission lands. Snapshotted from the last
    /// deliberate card choice — arming or disarming a target — so merely
    /// hovering a card in another project never migrates a half-typed draft.
    pub(super) new_task_project: Option<Uuid>,
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
    /// Each card's transcript list and scrollbar, keyed by session so a
    /// scrolled card keeps its place across opens and reorders.
    card_transcripts: RefCell<HashMap<Uuid, CardTranscript>>,
    /// The folded row-kind vector per card, cached on the lane's own
    /// transcript fingerprint so it is only refolded when the shape changes.
    card_kinds: RefCell<HashMap<Uuid, (u64, Rc<Vec<TranscriptRowKind>>)>>,
    /// The registry card markdown reports selectable text into — separate
    /// from the lane's so ⌘C never picks up card text.
    card_selection: TranscriptSelection,
    /// Card links paint like links but do nothing — cards are read-only.
    card_link_handler: crate::md::render::LinkHandler,
    /// Card bodies parse into their own views: the flatten cache keys on
    /// metrics, and cards render at `CARD_TEXT_SCALE`, so sharing
    /// `message_markdown` would drop every flat each time the lane and a
    /// card alternated scales. Keyed by message id, like the lane's.
    card_markdown: RefCell<HashMap<Uuid, MarkdownView>>,
}

/// One card's scroll surface. `ListState` and the scrollbar are handles into
/// window state, so the entry clones freely.
#[derive(Clone)]
struct CardTranscript {
    rows: ListState,
    scrollbar: Rc<ScrollbarState>,
}

impl BigPictureUi {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            slots: Vec::new(),
            highlighted: None,
            target: None,
            draft_key: None,
            new_task_project: None,
            focus,
            previous_focus: None,
            focus_generation: 0,
            anim_seq: 0,
            open_seq: 0,
            pending_submission: None,
            hydrate_requested: HashSet::new(),
            last_row_count: 0,
            backdrop: None,
            card_transcripts: RefCell::new(HashMap::new()),
            card_kinds: RefCell::new(HashMap::new()),
            card_selection: TranscriptSelection::default(),
            card_link_handler: Rc::new(|_, _, _| {}),
            card_markdown: RefCell::new(HashMap::new()),
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
/// Waiting sessions rank by `updated_at` — the moment they parked;
/// everything else ranks by sidebar recency, with the unseen-completion
/// stamp counting as activity. `limit` is how many cards this frame's grid
/// can afford.
fn big_picture_order(
    sessions: &[AgentSession],
    unseen: &HashMap<Uuid, u64>,
    limit: usize,
) -> Vec<Uuid> {
    let mut eligible = sessions
        .iter()
        .filter(|session| {
            session.has_started() && session.archived_at.is_none() && !session.is_side_chat()
        })
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

impl Waku {
    pub(super) fn toggle_big_picture_action(
        &mut self,
        _: &ToggleBigPicture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture.open {
            self.close_big_picture(window, cx);
        } else if self.state.big_picture_enabled {
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
        // Stash the background session's half-typed draft before the overlay
        // takes the composer over — capture resolves against the selection
        // only while `open` is still false.
        self.capture_and_save_current_composer_draft(cx);
        self.big_picture.open = true;
        self.big_picture.slots.clear();
        self.big_picture.target = None;
        self.big_picture.hydrate_requested.clear();
        // Card scroll state survives opens so a card the reader scrolled
        // keeps its place — but only for sessions that still exist.
        {
            let sessions = &self.state.sessions;
            self.big_picture
                .card_transcripts
                .borrow_mut()
                .retain(|id, _| sessions.iter().any(|session| session.id == *id));
            self.big_picture
                .card_kinds
                .borrow_mut()
                .retain(|id, _| sessions.iter().any(|session| session.id == *id));
        }
        // Parsed card bodies are bounded like the lane's `message_markdown`.
        let mut card_markdown = self.big_picture.card_markdown.borrow_mut();
        let cached_bytes: usize = card_markdown.values().map(MarkdownView::source_len).sum();
        if cached_bytes > MAX_CACHED_MESSAGE_SOURCE_BYTES {
            card_markdown.clear();
        }
        drop(card_markdown);
        self.big_picture.focus_generation = self.big_picture.focus_generation.wrapping_add(1);
        self.big_picture.open_seq = self.big_picture.open_seq.wrapping_add(1);
        self.big_picture.last_row_count = 0;
        self.big_picture.highlighted = big_picture_order(
            &self.state.sessions,
            &self.state.unseen_completions,
            MAX_CARDS,
        )
        .first()
        .copied();
        self.big_picture.new_task_project = self.big_picture_new_task_project();
        // The overlay's draft machinery starts unloaded so the first sync
        // restores the new-task draft rather than leaving the background
        // session's text on screen.
        self.big_picture.draft_key = None;
        self.sync_big_picture_draft(cx);
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
        // File the composer's content under whichever overlay slot owned it —
        // a card's session draft or the new-task draft — before the selected
        // session's draft takes the composer back.
        if let Some(key) = self.big_picture.draft_key.take() {
            let draft = self.current_composer_draft(Some(key), cx);
            if self.composer_drafts.set(key, draft) {
                self.schedule_composer_draft_save(cx);
            }
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
        self.restore_selected_composer_draft(cx);
        if let Some(previous_focus) = self.big_picture.previous_focus.take() {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    /// The window-free half of `close_big_picture` — used by the Experiments
    /// opt-out, where a toggle callback has no window to hand focus back to.
    pub(super) fn dismiss_big_picture(&mut self, cx: &mut Context<Self>) {
        if !self.big_picture.open {
            return;
        }
        if let Some(key) = self.big_picture.draft_key.take() {
            let draft = self.current_composer_draft(Some(key), cx);
            if self.composer_drafts.set(key, draft) {
                self.schedule_composer_draft_save(cx);
            }
        }
        self.big_picture.open = false;
        self.big_picture.slots.clear();
        self.big_picture.highlighted = None;
        self.big_picture.target = None;
        self.big_picture.pending_submission = None;
        self.big_picture.backdrop = None;
        self.big_picture.previous_focus = None;
        self.composer.update(cx, |composer, cx| {
            composer.set_placeholder(tr!("input.do_anything"), cx);
        });
        self.restore_selected_composer_draft(cx);
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

    /// The project an untargeted submission lands in — the highlighted card's
    /// first, then whatever the workspace underneath had selected.
    fn big_picture_new_task_project(&self) -> Option<Uuid> {
        self.big_picture
            .highlighted
            .and_then(|id| self.state.sessions.iter().find(|session| session.id == id))
            .map(|session| session.project_id)
            .or_else(|| self.selected_session().map(|session| session.project_id))
            .or_else(|| self.selected_project().map(|project| project.id))
    }

    /// Is `session_id` mounted on the grid right now — so a command like ⌘D
    /// can arm its card instead of exiting to select it.
    pub(super) fn big_picture_card_visible(&self, session_id: Uuid) -> bool {
        self.big_picture
            .slots
            .iter()
            .any(|slot| slot.session_id == session_id && !slot.leaving)
    }

    /// Arm a card programmatically — command-driven targeting like ⌘D, where
    /// no click or arrow moved the highlight first.
    pub(super) fn arm_big_picture_card(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        self.big_picture.highlighted = Some(session_id);
        self.set_big_picture_target(Some(session_id), cx);
    }

    /// The draft slot the armed target implies: a card's own session draft,
    /// or the standing new-task draft when nothing is armed.
    fn big_picture_draft_key(&self) -> Option<ComposerDraftKey> {
        match self.big_picture.target {
            Some(session_id) => self
                .state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(ComposerDraftKey::for_session),
            None => self
                .big_picture
                .new_task_project
                .map(ComposerDraftKey::NewSession),
        }
    }

    /// ⌘N/⌘⇧N inside the overlay: step the untargeted composer's destination
    /// through the same ordering the project switcher uses. An armed card
    /// peels off first — the chord configures the new-task draft.
    pub(super) fn cycle_big_picture_new_task_project(
        &mut self,
        reverse: bool,
        cx: &mut Context<Self>,
    ) {
        if self.big_picture.target.is_some() {
            self.set_big_picture_target(None, cx);
            return;
        }
        let recent = self.task_switcher.recent_project_ids(&self.state.sessions);
        let ordered = project_switcher::ordered_project_ids(
            self.big_picture.new_task_project,
            &recent,
            &self.state.projects,
        );
        let Some(index) = task_switcher::initial_highlight_index(
            &ordered,
            self.big_picture.new_task_project,
            reverse,
        ) else {
            return;
        };
        self.big_picture.new_task_project = ordered.get(index).copied();
        self.sync_big_picture_draft(cx);
        cx.notify();
    }

    /// Point the docked composer at the draft its current target owns: stash
    /// the visible text under the key it was loaded from, then load the new
    /// slot. Every deliberate target move — arrows, clicks, ⌘n, Escape —
    /// comes through here; passive highlight changes do not.
    pub(super) fn sync_big_picture_draft(&mut self, cx: &mut Context<Self>) {
        let next = self.big_picture_draft_key();
        if next == self.big_picture.draft_key {
            return;
        }
        if let Some(previous) = self.big_picture.draft_key {
            let draft = self.current_composer_draft(Some(previous), cx);
            if self.composer_drafts.set(previous, draft) {
                self.schedule_composer_draft_save(cx);
            }
        }
        self.big_picture.draft_key = next;
        let draft = next
            .and_then(|key| self.composer_drafts.get(key))
            .cloned()
            .unwrap_or_default();
        self.apply_composer_draft(next, draft, cx);
    }

    pub(super) fn set_big_picture_target(&mut self, target: Option<Uuid>, cx: &mut Context<Self>) {
        if self.big_picture.target == target {
            return;
        }
        self.big_picture.target = target;
        // The standing new-task destination follows the last deliberate card
        // choice: armed card's project while targeted, the card under the
        // highlight once the target peels off.
        self.big_picture.new_task_project = target
            .or(self.big_picture.highlighted)
            .and_then(|id| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == id)
                    .map(|session| session.project_id)
            })
            .or(self.big_picture.new_task_project);
        self.sync_big_picture_draft(cx);
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
            self.set_big_picture_target(None, cx);
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
                    .big_picture_new_task_project()
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
                self.set_big_picture_target(Some(session_id), cx);
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
        let border = if is_target {
            theme.accent
        } else if is_highlighted {
            theme.border_strong
        } else {
            theme.border
        };
        let body: AnyElement = if !session.detail_loaded {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(sp(11.5))
                .text_color(theme.text_ghost)
                .child(tr!("big_picture.loading"))
                .into_any_element()
        } else {
            self.render_big_picture_transcript(session, cx)
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
                    .border_color(theme.separator)
                    .rounded_t(px(CARD_RADIUS - 1.0))
                    .bg(theme.surface)
                    .child(self.render_session_row_body(session_id, false, cx)),
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
                        // the target resets on close anyway. A press that began
                        // on the card's scrollbar is a scroll, not a commit.
                        if event.click_count == 2
                            && this.big_picture.open
                            && !this.card_scrollbar_engaged(session_id)
                        {
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
                    // The scrollbar is an overlay painted over the card — a
                    // click on its track still lands here. Scrolling a card
                    // must not retarget the composer.
                    if this.big_picture.open && !this.card_scrollbar_engaged(session_id) {
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
                    // sits invisible through its delay, then fades in.
                    let delta = if entering {
                        let delay = enter_delay.as_secs_f32()
                            / (CARD_TRANSITION + enter_delay).as_secs_f32();
                        ((delta - delay) / (1.0 - delay)).clamp(0.0, 1.0)
                    } else {
                        delta
                    };
                    element
                        .left(px(from_left + (left - from_left) * delta))
                        .top(px(from_top + (top - from_top) * delta))
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

    /// Is the pointer on a card's scrollbar track, or mid-drag on its thumb.
    /// The bar is an overlay, so presses it handles still reach the card.
    fn card_scrollbar_engaged(&self, session_id: Uuid) -> bool {
        self.big_picture
            .card_transcripts
            .borrow()
            .get(&session_id)
            .is_some_and(|card| card.scrollbar.engaged())
    }

    /// A card's transcript: the lane's folded row kinds on a virtualized,
    /// bottom-pinned list rendered at `CARD_TEXT_SCALE`. Footer and
    /// changed-files rows are dropped — a card shows conversation and
    /// activity, not actions.
    fn render_big_picture_transcript(
        &self,
        session: &AgentSession,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let session_id = session.id;
        let card = {
            let mut cards = self.big_picture.card_transcripts.borrow_mut();
            cards
                .entry(session_id)
                .or_insert_with(|| {
                    let rows = ListState::new(0, ListAlignment::Bottom, px(2048.0));
                    rows.set_scroll_handler(|_, window, _| window.refresh());
                    CardTranscript {
                        rows,
                        scrollbar: ScrollbarState::new(),
                    }
                })
                .clone()
        };
        let fingerprint = transcript_rows_fingerprint(session, &self.expanded_turns);
        let (kinds, refolded) = {
            let mut cache = self.big_picture.card_kinds.borrow_mut();
            let entry = cache
                .entry(session_id)
                .or_insert_with(|| (0, Rc::new(Vec::new())));
            if entry.0 != fingerprint {
                let mut folded = folded_transcript_row_kinds(session, &self.expanded_turns);
                folded.retain(|kind| {
                    !matches!(
                        kind,
                        TranscriptRowKind::ResponseFooter(..) | TranscriptRowKind::ChangedFiles(_)
                    )
                });
                *entry = (fingerprint, Rc::new(folded));
                (entry.1.clone(), true)
            } else {
                (entry.1.clone(), false)
            }
        };
        let count = kinds.len();
        let current = card.rows.item_count();
        if count > current {
            // Appends keep the card's place — or the tail, while pinned.
            card.rows.splice(current..current, count - current);
            if refolded {
                card.rows.remeasure_items(0..current);
            }
        } else if count < current {
            card.rows.reset(count);
        } else if refolded {
            card.rows.remeasure_items(0..count);
        }
        if count == 0 {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(sp(11.5))
                .text_color(theme.text_ghost)
                .child(tr!("big_picture.empty_transcript"))
                .into_any_element();
        }
        // Streaming grows the tail without moving the fold. Re-measure the
        // last few rows while the session works so fresh text is not clipped
        // — the same tail the lane re-measures on every commit.
        if session.status.is_busy() {
            card.rows
                .remeasure_items(count.saturating_sub(STREAM_REMEASURE_TAIL_ROWS)..count);
        }
        let entity = cx.entity().downgrade();
        let rows = card.rows.clone();
        div()
            .size_full()
            .relative()
            .child(
                rem_scale(CARD_TEXT_SCALE).size_full().child(
                    list(card.rows.clone(), move |index, window, cx| {
                        entity
                            .upgrade()
                            .map(|entity| {
                                entity.update(cx, |this, cx| {
                                    this.big_picture_card_row(session_id, index, window, cx)
                                })
                            })
                            .unwrap_or_else(|| div().into_any_element())
                    })
                    .size_full(),
                ),
            )
            .child(scrollbar::edge_fade(
                rows.clone(),
                scrollbar::FadeEdge::Top,
                theme.raised,
            ))
            .child(scrollbar::edge_fade(
                rows.clone(),
                scrollbar::FadeEdge::Bottom,
                theme.raised,
            ))
            .child(scrollbar::vertical(&rows, &card.scrollbar))
            .into_any_element()
    }

    /// The markdown render context for one card row — the card's own
    /// selection registry and an inert link handler.
    fn card_markdown_ctx<'a>(
        &self,
        row: String,
        palette: &'a MarkdownPalette,
        metrics: MarkdownMetrics,
        animate_streaming: bool,
        cx: &App,
    ) -> MarkdownCtx<'a> {
        MarkdownCtx::new(
            row,
            palette,
            metrics,
            self.big_picture.card_selection.clone(),
        )
        .with_families(crate::fonts::current(cx))
        .with_math_enabled(self.state.render_math)
        .with_guided_reading(self.guided_reading())
        .with_link_handler(self.big_picture.card_link_handler.clone())
        .with_streaming_animation(animate_streaming)
    }

    /// Metrics rescaled to the card's text scale on top of the user's font
    /// size settings. Markdown sizes are `px`, so the `RemScale` wrapper
    /// cannot reach them — they scale here instead.
    fn card_markdown_metrics(&self, metrics: MarkdownMetrics) -> MarkdownMetrics {
        metrics.scaled(
            self.state.ui_font_size * CARD_TEXT_SCALE,
            self.state.code_font_size * CARD_TEXT_SCALE,
        )
    }

    /// One row of a card transcript. `index` is a position in the card's
    /// `card_kinds` vector, synced this frame by `render_big_picture_transcript`.
    fn big_picture_card_row(
        &self,
        session_id: Uuid,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);
        let Some(session) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
        else {
            return div().into_any_element();
        };
        let (row_count, kind, starts_followup_turn) = {
            let cache = self.big_picture.card_kinds.borrow();
            let Some((_, kinds)) = cache.get(&session_id) else {
                return div().into_any_element();
            };
            (
                kinds.len(),
                kinds
                    .get(index)
                    .copied()
                    .unwrap_or(TranscriptRowKind::Message(index)),
                row_starts_followup_turn(session, kinds, index),
            )
        };
        let inner = match kind {
            TranscriptRowKind::Message(message_index) => session
                .messages
                .get(message_index)
                .cloned()
                .map(|message| {
                    let copied = self.copied_message_feedback.contains_key(&message.id);
                    let menu = self.menu_handle(format!("big-picture-message-{}", message.id), cx);
                    let attachment_menus = (0..message.attachments.len())
                        .map(|index| {
                            self.menu_handle(
                                format!("big-picture-message-{}-attachment-{index}", message.id),
                                cx,
                            )
                        })
                        .collect();
                    let attachment_images = message
                        .attachments
                        .iter()
                        .map(|attachment| {
                            if !attachment.is_image {
                                return None;
                            }
                            let Some(reference) = attachment.blob_reference.as_deref() else {
                                return None;
                            };
                            self.image_for_reference(
                                reference,
                                Some(&attachment.path),
                                Some(&attachment.name),
                                cx,
                            )
                        })
                        .collect();
                    let metrics =
                        self.card_markdown_metrics(if message.role == MessageRole::User {
                            MarkdownMetrics::USER_MESSAGE
                        } else {
                            MarkdownMetrics::BODY
                        });
                    let animate_streaming = message.streaming && !cx.reduce_motion();
                    let ctx = self.card_markdown_ctx(
                        format!("big-picture-message-{}", message.id),
                        &palette,
                        metrics,
                        animate_streaming,
                        cx,
                    );
                    let work_item_refs = (message.role == MessageRole::User)
                        .then(|| {
                            self.work_item_refs_for_content(
                                self.workspace_path_for_session(session),
                                message.visible_content(),
                            )
                        })
                        .unwrap_or_default();
                    let mut markdown = self.big_picture.card_markdown.borrow_mut();
                    let view = matches!(message.role, MessageRole::User | MessageRole::Assistant)
                        .then(|| {
                            // A seeded view paints the text that arrived while
                            // the overlay was closed at full opacity instead of
                            // dissolving the whole reply on open.
                            let view = markdown
                                .entry(message.id)
                                .or_insert_with(MarkdownView::seeded);
                            view.set_text(message.visible_content(), message.streaming);
                            &*view
                        });
                    let rendered = render_message(
                        MessageRender {
                            theme: &theme,
                            message: &message,
                            assistant_footer_copy_content: None,
                            assistant_footer_time: None,
                            copied,
                            show_response_token_speed: false,
                            assistant_message_action: None,
                            user_message_action: None,
                            user_message_viewport: None,
                            user_message_expanded: false,
                            user_message_expand_focus: None,
                            message_edit_input: None,
                            attachment_menus,
                            attachment_images,
                            attachments_can_reveal: !self.is_remote_session(session_id),
                            markdown: view,
                            work_item_refs,
                            ctx: &ctx,
                            menu,
                            sent_by_task_link: message
                                .sent_by_task
                                .filter(|id| self.sent_by_task_openable(*id)),
                            waku: cx.entity().downgrade(),
                            composer: self.composer.clone(),
                            landed_notice: None,
                        },
                        cx,
                    );
                    if animate_streaming && view.is_some_and(MarkdownView::is_fading) {
                        motion::pulse_lease(window.current_view(), cx);
                    }
                    rendered
                })
                .unwrap_or_else(|| div().into_any_element()),
            TranscriptRowKind::TurnBlock(block_index) => {
                self.render_card_activities_row(session, block_index, &theme)
            }
            TranscriptRowKind::TurnFold(turn_id) => {
                self.render_card_turn_fold_row(session, turn_id, &theme)
            }
            TranscriptRowKind::WorkingIndicator => {
                self.render_card_working_indicator_row(session, &theme)
            }
            // Folded out of `card_kinds` entirely; the fallback renders nothing.
            TranscriptRowKind::ResponseFooter(..) | TranscriptRowKind::ChangedFiles(_) => {
                div().into_any_element()
            }
        };
        div()
            .id(SharedString::from(format!(
                "big-picture-row-{session_id}-{index}"
            )))
            .w_full()
            .py(px(4.0))
            .when(index == 0, |element| element.pt(px(6.0)))
            .when(starts_followup_turn, |element| {
                element.pt(px(FOLLOWUP_TURN_TOP_GAP * CARD_TEXT_SCALE))
            })
            .when(index + 1 == row_count, |element| element.pb(px(6.0)))
            .child(inner)
            .into_any_element()
    }

    /// A card's tool-activity block as one summary line — the disclosure's
    /// collapsed header — with a spinner while the group is live. Side-chat
    /// panels draw the same compact row.
    pub(super) fn render_card_activities_row(
        &self,
        session: &AgentSession,
        block_index: usize,
        theme: &Theme,
    ) -> AnyElement {
        let Some(block) = session.transcript_blocks.get(block_index) else {
            return div().into_any_element();
        };
        if block.activities.is_empty() {
            return div().into_any_element();
        }
        let last_block = block_index + 1 == session.transcript_blocks.len();
        let live_group = activity_group_is_live(
            session
                .active_turn_id()
                .is_some_and(|turn_id| block.turn_id == Some(turn_id)),
            last_block,
            block.after_message,
            session.messages.len(),
        );
        let live_reasoning_id = (self
            .runtimes
            .get(&session.id)
            .is_some_and(|runtime| runtime.stream_phase == Some(StreamPhase::Reasoning))
            && session.status == SessionStatus::Working
            && last_block)
            .then(|| {
                block
                    .activities
                    .iter()
                    .rev()
                    .find(|activity| activity.reasoning.is_some())
                    .map(|activity| activity.id)
            })
            .flatten();
        let title = activity_header_title(&block.activities, live_group, live_reasoning_id);
        div()
            .h(px(22.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(live_group, |element| {
                element.child(motion::spin_slow(icon(
                    "icons/loader-circle.svg",
                    10.0,
                    status_color(theme, session.status),
                )))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .line_height(sp(16.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(title)),
            )
            .into_any_element()
    }

    /// A card's "Worked for Ns" divider — the lane's fold row minus the
    /// toggle, since cards are read-only. Side-chat panels draw it too.
    pub(super) fn render_card_turn_fold_row(
        &self,
        session: &AgentSession,
        turn_id: Uuid,
        theme: &Theme,
    ) -> AnyElement {
        let expanded = self.expanded_turns.contains(&turn_id);
        let label = turn_fold_label(session, turn_id);
        div()
            .w_full()
            .h(px(24.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(div().h(hairline()).flex_1().bg(theme.separator))
            .child(
                div()
                    .h(px(24.0))
                    .px(px(2.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .text_size(sp(13.5))
                    .line_height(sp(18.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(label))
                    .child(icon(
                        if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        },
                        11.5,
                        theme.affordance_icon(),
                    )),
            )
            .child(div().h(hairline()).flex_1().bg(theme.separator))
            .into_any_element()
    }

    /// The live turn's closing row, drawn from the card's own session. A
    /// side-chat panel shows the same indicator while its session works.
    pub(super) fn render_card_working_indicator_row(
        &self,
        session: &AgentSession,
        theme: &Theme,
    ) -> AnyElement {
        let elapsed = session
            .turns
            .last()
            .filter(|turn| turn.status == TurnStatus::Running)
            .map(|turn| unix_time().saturating_sub(turn.started_at))
            .unwrap_or(0);
        // A parked turn is waiting on detached work, not working.
        let label = if session.status == SessionStatus::Background {
            tr!("transcript.waiting_background")
        } else {
            tr!(
                "transcript.working_for",
                duration = format_working_elapsed(elapsed)
            )
        };
        div()
            .h(px(22.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(thinking::goddard_thinking(theme.text_tertiary))
            .child(
                div()
                    .text_size(sp(13.5))
                    .line_height(sp(18.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(label)),
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
                        .focus_visible(|element| element.bg(theme.focus_highlight()))
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
    /// shortcuts — plus the workspace footer: its project, worktree, and
    /// branch chips follow the armed card, or the standing new-task
    /// destination while nothing is armed.
    fn render_big_picture_composer(&mut self, window: &Window, cx: &mut Context<Self>) -> Div {
        div()
            .w_full()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.render_composer(window, cx))
            .child(self.render_workspace_footer(cx))
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
        let composer_height = self
            .composer_lane_height
            .get()
            .max(COMPOSER_HEIGHT_FALLBACK);
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
        let geometry =
            big_picture_card_geometry(desired.len(), rows, row_width, card_width, card_height);
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
            // The overlay covers the session column's drop group, so it
            // re-declares it: the docked composer card lights up wherever
            // the drag is held and accepts the same drops.
            .group(composer::SESSION_DROP_GROUP)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.stage_dropped_files(paths, window, cx);
            }))
            .on_drop(
                cx.listener(|this, drag: &composer::SidebarSessionDrag, window, cx| {
                    this.stage_session_reference(drag.session_id, &drag.title, window, cx);
                }),
            )
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
            // Chords aimed at chrome the overlay covers — the sidebar and
            // panels, other pickers and pages, the editor's find bar — have
            // no object here. Swallow them: acting on the workspace behind
            // the scrim would be invisible by definition.
            .on_action(cx.listener(|_, _: &ToggleSidebar, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleRightPanel, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleGitPanel, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleTerminals, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &NewTerminal, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleUsagePanel, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleProjectsPage, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &SelectProjectsTab, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &NewProject, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &OpenSettings, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleCommandPalette, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleFileFinder, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &OpenResumePicker, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &RunProjectScript, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &FocusTerminal, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &SaveFile, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &OpenFind, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &OpenFindReplace, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &FindNext, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &FindPrevious, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &CloseFind, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleFindCaseSensitive, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleFindWholeWord, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ToggleFindRegex, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ReplaceAllMatches, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &CopySelection, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &AddToChat, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &NavigateBack, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &NavigateForward, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &SwitchTaskForward, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &SwitchTaskBackward, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &SelectFirstTask, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &SelectLastTask, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &ConfirmTaskSwitch, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &CancelTaskSwitch, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &SelectSidebarSession, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &GoToPreviousTurn, _, cx| cx.stop_propagation()))
            .on_action(cx.listener(|_, _: &GoToNextTurn, _, cx| cx.stop_propagation()))
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
                    .child(
                        div()
                            .relative()
                            .size_full()
                            // Painted before any card so the frame's card
                            // selection registry holds exactly this frame's
                            // card text, in order.
                            .child(md::render::frame_reset(
                                self.big_picture.card_selection.clone(),
                            ))
                            .children(cards),
                    ),
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
                        |element, delta| element.opacity(delta),
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
