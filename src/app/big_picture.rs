//! Big Picture mode: a ⌘0 overlay of the sessions most worth a glance.
//!
//! Up to four cards sit side by side — tasks waiting on input first, then
//! unread completions, then running work, then whatever the sidebar saw most
//! recently — each carrying a live tail of its transcript. The card set is
//! reconciled every frame the overlay is open, so a session that newly blocks
//! slides in while a settled one fades out rather than the row jumping.
//!
//! Clicking a card never navigates: it targets the docked composer at that
//! session, and clicking again — or sending — returns it to new-task mode.
//! The arrow keys do the same without a confirm step: they move the highlight
//! and retarget the composer in one motion. Escape peels off the target first,
//! then closes the overlay; clicking the scrim closes it outright.

use gpui::{KeyBinding, actions};

use super::*;

pub const MAX_CARDS: usize = 4;
const CARD_WIDTH: f32 = 360.0;
const CARD_MIN_WIDTH: f32 = 260.0;
const CARD_GAP: f32 = 16.0;
const CARD_RADIUS: f32 = 14.0;
const CARD_HEIGHT_RATIO: f32 = 0.56;
const CARD_HEIGHT_MIN: f32 = 300.0;
const CARD_HEIGHT_MAX: f32 = 540.0;
const ROW_MARGIN_X: f32 = 48.0;
const COMPOSER_WIDTH: f32 = 560.0;
const COMPOSER_BOTTOM_MARGIN: f32 = 28.0;
const HINT_BOTTOM_MARGIN: f32 = 10.0;
const CARD_TRANSITION: Duration = Duration::from_millis(220);
/// A leaving card outlives its transition by a hair so the timer never cuts
/// the last frames.
const CARD_EXIT_LINGER: Duration = Duration::from_millis(260);
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

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", DismissBigPicture, Some("BigPicture")),
        KeyBinding::new("left", BigPictureLeft, Some("BigPicture")),
        KeyBinding::new("right", BigPictureRight, Some("BigPicture")),
        KeyBinding::new("enter", BigPictureConfirm, Some("BigPicture")),
    ]);
}

/// A mounted card. Slots outlive their session's rank so an evicted card can
/// fade where it stood instead of vanishing mid-row.
#[derive(Clone)]
struct BigPictureSlot {
    session_id: Uuid,
    /// Column the card rests at; only an index change re-runs the slide, so a
    /// window resize moves cards without replaying a transition per frame.
    index: usize,
    left: f32,
    /// Where the in-flight transition started, for FLIP slides on re-sort.
    from_left: f32,
    leaving: bool,
    /// Fresh mounts fade up; reordered or surviving cards only slide.
    entering: bool,
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
    /// A prompt that arrived before its projectless workspace did; drained by
    /// the completion that materializes the session it belongs to.
    pending_submission: Option<ComposerSubmission>,
    /// Sessions whose transcript hydration this open already requested, so a
    /// persistently failing load cannot re-spawn (and re-toast) every frame.
    hydrate_requested: HashSet<Uuid>,
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
            pending_submission: None,
            hydrate_requested: HashSet::new(),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
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

/// The card order for this frame: tier first, then most recently touched.
/// Waiting sessions rank by `updated_at` — the moment they parked — matching
/// `next_unread_session`; everything else ranks by sidebar recency, with the
/// unseen-completion stamp counting as activity.
fn big_picture_order(sessions: &[AgentSession], unseen: &HashMap<Uuid, u64>) -> Vec<Uuid> {
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
        .take(MAX_CARDS)
        .map(|session| session.id)
        .collect()
}

fn big_picture_status_label(session: &AgentSession, unseen: &HashMap<Uuid, u64>) -> String {
    match session.status {
        SessionStatus::Waiting => tr!("big_picture.status.waiting"),
        SessionStatus::Idle if unseen.contains_key(&session.id) => {
            tr!("big_picture.status.unread")
        }
        SessionStatus::Connecting => tr!("big_picture.status.starting"),
        SessionStatus::Working => tr!("big_picture.status.working"),
        SessionStatus::Background => tr!("big_picture.status.background"),
        SessionStatus::Failed => tr!("big_picture.status.failed"),
        SessionStatus::Idle => tr!("big_picture.status.idle"),
    }
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
        self.big_picture.highlighted =
            big_picture_order(&self.state.sessions, &self.state.unseen_completions)
                .first()
                .copied();
        self.sync_big_picture_placeholder(cx);
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

    /// The ids arrow keys and Enter walk — the live cards, in display order.
    fn big_picture_navigable(&self) -> Vec<Uuid> {
        let mut slots = self
            .big_picture
            .slots
            .iter()
            .filter(|slot| !slot.leaving)
            .collect::<Vec<_>>();
        slots.sort_by_key(|slot| slot.left as i64);
        slots.iter().map(|slot| slot.session_id).collect()
    }

    /// Arrows move the highlight *and* retarget the composer — no separate
    /// confirm step. The docked prompt follows whichever card is highlighted.
    fn move_big_picture_highlight(&mut self, reverse: bool, cx: &mut Context<Self>) {
        let order = self.big_picture_navigable();
        if order.is_empty() {
            return;
        }
        let next = match self
            .big_picture
            .highlighted
            .and_then(|id| order.iter().position(|candidate| *candidate == id))
        {
            Some(index) if reverse => (index + order.len() - 1) % order.len(),
            Some(index) => (index + 1) % order.len(),
            None if reverse => order.len() - 1,
            None => 0,
        };
        self.big_picture.highlighted = order.get(next).copied();
        self.set_big_picture_target(self.big_picture.highlighted, cx);
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
    fn reconcile_big_picture_slots(&mut self, card_width: f32, cx: &mut Context<Self>) {
        let desired = big_picture_order(&self.state.sessions, &self.state.unseen_completions);
        for session_id in &desired {
            if self.big_picture.hydrate_requested.insert(*session_id) {
                self.ensure_session_loaded(*session_id, cx);
            }
        }
        let mut seq = self.big_picture.anim_seq;
        let step = card_width + CARD_GAP;
        // A slot whose session ranked back in before its exit finished just
        // rejoins the row — the animation restart reads as a fade back up.
        for slot in &mut self.big_picture.slots {
            if slot.leaving && desired.contains(&slot.session_id) {
                slot.leaving = false;
                slot.entering = true;
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
        for (index, session_id) in desired.iter().enumerate() {
            let left = index as f32 * step;
            match self
                .big_picture
                .slots
                .iter_mut()
                .find(|slot| slot.session_id == *session_id)
            {
                Some(slot) if !slot.leaving => {
                    if slot.index != index {
                        slot.from_left = slot.left;
                        slot.index = index;
                        slot.entering = false;
                        seq += 1;
                        slot.anim_seq = seq;
                    }
                    slot.left = left;
                }
                Some(_) => {}
                None => {
                    seq += 1;
                    self.big_picture.slots.push(BigPictureSlot {
                        session_id: *session_id,
                        index,
                        left,
                        from_left: left,
                        leaving: false,
                        entering: true,
                        anim_seq: seq,
                    });
                }
            }
        }
        self.big_picture.anim_seq = seq;
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
        card_width: f32,
        card_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let session_id = session.id;
        let is_target = self.big_picture.target == Some(session_id);
        let is_highlighted = self.big_picture.highlighted == Some(session_id);
        let unread = self.state.unseen_completions.contains_key(&session_id);
        let status = session.status;
        let leaving = slot.leaving;
        let entering = slot.entering;
        let left = slot.left;
        let from_left = slot.from_left;
        let anim_id =
            SharedString::from(format!("big-picture-card-{session_id}-{}", slot.anim_seq));
        let project_name = self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id)
            .filter(|project| !project.is_projectless())
            .map(Project::display_name);
        let status_label = big_picture_status_label(session, &self.state.unseen_completions);
        let time_ago = session
            .last_reply_at
            .map(|at| sidebar::format_time_ago(unix_time().saturating_sub(at)));
        let status_glyph = match status {
            SessionStatus::Connecting | SessionStatus::Working => motion::spin_slow(icon(
                "icons/loader-circle.svg",
                12.0,
                status_color(&theme, status),
            )),
            SessionStatus::Background => {
                icon("icons/hourglass.svg", 12.0, status_color(&theme, status)).into_any_element()
            }
            SessionStatus::Waiting => {
                icon("icons/alert.svg", 12.0, status_color(&theme, status)).into_any_element()
            }
            SessionStatus::Failed => {
                icon("icons/x.svg", 12.0, status_color(&theme, status)).into_any_element()
            }
            SessionStatus::Idle => div()
                .flex_none()
                .size(px(12.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(if unread { 7.0 } else { 4.0 }))
                        .rounded_full()
                        .bg(if unread { theme.info } else { theme.text_ghost }),
                )
                .into_any_element(),
        };
        let title = session.display_title().to_owned();
        let title = if title == AgentSession::DEFAULT_TITLE {
            tr!("session.new_task")
        } else {
            title
        };
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
                div()
                    .flex_none()
                    .px(px(14.0))
                    .pt(px(12.0))
                    .pb(px(10.0))
                    .border_b(hairline())
                    .border_color(theme.border)
                    .rounded_t(px(CARD_RADIUS - 1.0))
                    .bg(theme.surface)
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_glyph)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(sp(13.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(title),
                            )
                            .children(time_ago.map(|ago| {
                                div()
                                    .flex_none()
                                    .text_size(sp(11.0))
                                    .text_color(theme.text_ghost)
                                    .child(ago)
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(20.0))
                            .text_size(sp(11.0))
                            .text_color(status_color(&theme, status))
                            .child(status_label)
                            .children(project_name.map(|name| {
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme.text_tertiary)
                                    .child(format!("· {name}"))
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(12.0))
                    .overflow_hidden()
                    .child(body),
            );
        if !leaving {
            card = card
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
            .top_0()
            .left(px(left))
            .w(px(card_width))
            .h(px(card_height))
            .child(card)
            .with_animation(
                anim_id,
                Animation::new(CARD_TRANSITION).with_easing(ease_out_quint()),
                move |element, delta| {
                    let element = element
                        .left(px(from_left + (left - from_left) * delta))
                        .opacity(if leaving {
                            1.0 - delta
                        } else if entering {
                            delta
                        } else {
                            1.0
                        });
                    if leaving {
                        element.top(px(10.0 * delta))
                    } else if entering {
                        element.top(px(14.0 * (1.0 - delta)))
                    } else {
                        element
                    }
                },
            )
            .into_any_element()
    }

    fn render_big_picture_composer(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let target = self.big_picture.target.and_then(|session_id| {
            self.state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| (session.id, session.display_title().to_owned()))
        });
        let has_draft = !self.composer.read(cx).content(cx).trim().is_empty()
            || !self.composer_attachments.is_empty();
        let can_send = has_draft && !self.model_picker_has_no_providers();
        let clear_target_focus = self.transcript_control_focus("big-picture-clear-target", cx);
        let send_focus = self.transcript_control_focus("big-picture-send", cx);
        div()
            .id("big-picture-composer")
            .w(px(COMPOSER_WIDTH))
            .flex_none()
            .rounded(px(16.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.composer)
            .py(px(10.0))
            .shadow_xl()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(target.map(|(_, title)| {
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
                            .focus_visible(|element| element.border(hairline()).border_color(theme.accent))
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
                    .into_any_element()
            }))
            .when(!self.composer_attachments.is_empty(), |card| {
                card.child(self.render_composer_attachments(cx))
            })
            .child(div().pt(px(2.0)).child(self.composer.clone()))
            .child(
                div()
                    .mt(px(8.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(sp(11.0))
                            .text_color(theme.text_ghost)
                            .child(tr!("big_picture.hint")),
                    )
                    .child(
                        div()
                            .id("big-picture-send")
                            .track_focus(&send_focus)
                            .tab_index(0)
                            .w(px(26.0))
                            .h(px(26.0))
                            .flex_none()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if can_send {
                                theme.inverse
                            } else {
                                theme.overlay_strong
                            })
                            .when(can_send, |element| {
                                element
                                    .cursor_default()
                                    .hover(|element| element.opacity(0.9))
                                    .active(|element| element.opacity(0.8))
                            })
                            .focus_visible(|element| element.border(hairline()).border_color(theme.accent))
                            .child(icon(
                                "icons/arrow-up.svg",
                                16.0,
                                if can_send {
                                    theme.on_inverse
                                } else {
                                    theme.text_ghost
                                },
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let prompt = this.composer.read(cx).content(cx).to_owned();
                                if let Some(submission) =
                                    this.submission_with_attachments(&prompt, cx)
                                {
                                    this.composer.update(cx, |input, cx| input.clear(cx));
                                    this.submit_big_picture_submission(submission, cx);
                                }
                            }))
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    let prompt = this.composer.read(cx).content(cx).to_owned();
                                    if let Some(submission) =
                                        this.submission_with_attachments(&prompt, cx)
                                    {
                                        this.composer.update(cx, |input, cx| input.clear(cx));
                                        this.submit_big_picture_submission(submission, cx);
                                    }
                                    cx.stop_propagation();
                                }
                            })),
                    ),
            )
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
        let viewport_width = f32::from(viewport.width);
        let viewport_height = f32::from(viewport.height);
        let card_width = ((viewport_width - ROW_MARGIN_X * 2.0 - CARD_GAP * 3.0) / 4.0)
            .clamp(CARD_MIN_WIDTH, CARD_WIDTH);
        let card_height =
            (viewport_height * CARD_HEIGHT_RATIO).clamp(CARD_HEIGHT_MIN, CARD_HEIGHT_MAX);
        self.reconcile_big_picture_slots(card_width, cx);
        let count = self
            .big_picture
            .slots
            .iter()
            .filter(|slot| !slot.leaving)
            .count();
        let row_width = if count == 0 {
            0.0
        } else {
            count as f32 * card_width + (count - 1) as f32 * CARD_GAP
        };
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
            .map(|(session, slot)| {
                self.render_big_picture_card(session, slot, card_width, card_height, cx)
            })
            .collect::<Vec<_>>();
        let focus = self.big_picture.focus.clone();
        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.45)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.2)
        };
        let layer = div()
            .id("big-picture-layer")
            .key_context("BigPicture")
            .track_focus(&focus)
            .absolute()
            .inset_0()
            .occlude()
            .bg(scrim)
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::toggle_big_picture_action))
            .on_action(cx.listener(Self::dismiss_big_picture_action))
            .on_action(cx.listener(Self::big_picture_left_action))
            .on_action(cx.listener(Self::big_picture_right_action))
            .on_action(cx.listener(Self::big_picture_confirm_action))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_big_picture(window, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .relative()
                            .w(px(row_width.max(card_width)))
                            .h(px(card_height))
                            .children(cards),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .pb(px(COMPOSER_BOTTOM_MARGIN))
                    .px(px(24.0))
                    .flex()
                    .justify_center()
                    .child(self.render_big_picture_composer(cx)),
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
            big_picture_order(&sessions, &unseen),
            vec![waiting.id, unread_idle.id, running.id, settled.id]
        );
    }

    #[test]
    fn within_a_tier_the_most_recently_updated_wins() {
        let older_wait = session(SessionStatus::Waiting, 10, 60);
        let newer_wait = session(SessionStatus::Waiting, 10, 90);
        let sessions = vec![older_wait.clone(), newer_wait.clone()];

        assert_eq!(
            big_picture_order(&sessions, &HashMap::new()),
            vec![newer_wait.id, older_wait.id]
        );
    }

    #[test]
    fn the_grid_caps_at_four_and_skips_unstarted_drafts() {
        let draft = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let mut sessions = vec![draft];
        for index in 0..6 {
            sessions.push(session(SessionStatus::Idle, index as u64 + 1, 0));
        }
        let ordered = big_picture_order(&sessions, &HashMap::new());

        assert_eq!(ordered.len(), MAX_CARDS);
        assert!(
            !sessions[0..1]
                .iter()
                .any(|draft| ordered.contains(&draft.id))
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
            big_picture_order(&sessions, &unseen),
            vec![stale_unread.id, recent_read.id]
        );
    }
}
