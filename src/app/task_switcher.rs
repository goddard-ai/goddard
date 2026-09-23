//! Ctrl-Tab switching across Goddard tasks.
//!
//! The task order is snapshotted when Control-Tab opens the overlay. Repeated
//! presses move only the highlight; releasing Control commits once, so a
//! switch never hydrates intermediate transcripts or reshuffles the list
//! underneath the pointer. The snapshot is capped at ten tasks: recently
//! visited first, with the most recently active tasks filling open slots.

use super::*;

const MODAL_WIDTH: f32 = 400.0;
const MODAL_RADIUS: f32 = 17.0;
const MODAL_INSET: f32 = 6.0;
const TITLE_HEIGHT: f32 = 30.0;
const TITLE_INSET_X: f32 = 10.0;
const ROW_HEIGHT: f32 = 34.0;
const ROW_INSET_X: f32 = 10.0;
const ROW_RADIUS: f32 = 10.0;
const PROJECT_NAME_MAX_WIDTH: f32 = 150.0;
const MAX_TASKS: usize = 10;
const WINDOW_MARGIN: f32 = 44.0;
const VERTICAL_BIAS: f32 = 0.08;
const VERTICAL_BIAS_MAX: f32 = 96.0;

/// Runtime-only switcher state. Restoration deliberately seeds only the
/// selected task: opening old windows must not masquerade as user recency.
pub(super) struct TaskSwitcherUi {
    open: bool,
    ordered_session_ids: Vec<Uuid>,
    highlighted_session_id: Option<Uuid>,
    original_session_id: Option<Uuid>,
    recent_session_ids: Vec<Uuid>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    scroll: ScrollHandle,
    generation: u64,
}

impl TaskSwitcherUi {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            ordered_session_ids: Vec::new(),
            highlighted_session_id: None,
            original_session_id: None,
            recent_session_ids: Vec::new(),
            focus,
            previous_focus: None,
            scroll: ScrollHandle::new(),
            generation: 0,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// Project ids in the order their tasks were last activated — the
    /// "recently used" source the project switcher shares.
    pub(super) fn recent_project_ids(&self, sessions: &[AgentSession]) -> Vec<Uuid> {
        let mut seen = HashSet::new();
        self.recent_session_ids
            .iter()
            .filter_map(|session_id| sessions.iter().find(|session| session.id == *session_id))
            .map(|session| session.project_id)
            .filter(|project_id| seen.insert(*project_id))
            .collect()
    }

    pub(super) fn record_access(&mut self, session_id: Uuid) {
        self.recent_session_ids
            .retain(|recent| *recent != session_id);
        self.recent_session_ids.insert(0, session_id);
    }

    pub(super) fn remove(&mut self, session_id: Uuid) {
        self.recent_session_ids
            .retain(|recent| *recent != session_id);
        let highlighted_index = self
            .ordered_session_ids
            .iter()
            .position(|candidate| *candidate == session_id);
        self.ordered_session_ids
            .retain(|candidate| *candidate != session_id);
        if self.highlighted_session_id == Some(session_id) {
            self.highlighted_session_id = highlighted_index.and_then(|index| {
                self.ordered_session_ids
                    .get(index.min(self.ordered_session_ids.len().saturating_sub(1)))
                    .copied()
            });
        }
        self.reveal_highlight();
    }

    fn reveal_highlight(&self) {
        let Some(index) = self.highlighted_session_id.and_then(|highlighted| {
            self.ordered_session_ids
                .iter()
                .position(|candidate| *candidate == highlighted)
        }) else {
            return;
        };
        self.scroll.scroll_to_item(index);
    }

    fn dismiss(&mut self) -> Option<FocusHandle> {
        self.open = false;
        self.ordered_session_ids.clear();
        self.highlighted_session_id = None;
        self.original_session_id = None;
        self.generation = self.generation.wrapping_add(1);
        self.previous_focus.take()
    }
}

fn ordered_task_ids(
    current: Option<Uuid>,
    recent: &[Uuid],
    sessions: &[AgentSession],
    dormant: &HashSet<Uuid>,
) -> Vec<Uuid> {
    // Dormant tasks never join the list — even the current one — so the
    // switcher can neither highlight nor commit a shelved task.
    let eligible = sessions
        .iter()
        .filter(|session| {
            session.has_started()
                && session.archived_at.is_none()
                && !session.is_side_chat()
                && !dormant.contains(&session.id)
        })
        .collect::<Vec<_>>();
    let valid = eligible
        .iter()
        .map(|session| session.id)
        .collect::<HashSet<_>>();
    let mut seen = HashSet::with_capacity(eligible.len());
    let mut ordered = Vec::with_capacity(eligible.len().min(MAX_TASKS));
    let mut push = |id| {
        if ordered.len() < MAX_TASKS && valid.contains(&id) && seen.insert(id) {
            ordered.push(id);
        }
    };

    if let Some(current) = current {
        push(current);
    }
    for recent in recent {
        push(*recent);
    }
    // Recently visited tasks keep their rank; when they leave slots open, the
    // most recently active tasks fill them so the list still shows ten.
    let mut recently_active = eligible;
    recently_active
        .sort_by_key(|session| std::cmp::Reverse(sidebar::sidebar_session_timestamp(session)));
    for session in recently_active {
        push(session.id);
    }
    ordered
}

pub(super) fn initial_highlight_index(
    ordered: &[Uuid],
    current: Option<Uuid>,
    reverse: bool,
) -> Option<usize> {
    if ordered.is_empty() {
        return None;
    }
    if ordered.first().copied() == current {
        if ordered.len() == 1 {
            return Some(0);
        }
        return Some(if reverse { ordered.len() - 1 } else { 1 });
    }
    Some(if reverse { ordered.len() - 1 } else { 0 })
}

fn task_switcher_title(session: &AgentSession) -> String {
    let title = session.display_title();
    if title == AgentSession::DEFAULT_TITLE {
        tr!("session.new_task")
    } else {
        title.to_owned()
    }
}

impl Waku {
    pub(super) fn switch_task_forward_action(
        &mut self,
        _: &SwitchTaskForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_task_switcher(false, window, cx);
    }

    pub(super) fn switch_task_backward_action(
        &mut self,
        _: &SwitchTaskBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_task_switcher(true, window, cx);
    }

    pub(super) fn select_first_task_action(
        &mut self,
        _: &SelectFirstTask,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_task_switcher_highlight(0, cx);
    }

    pub(super) fn select_last_task_action(
        &mut self,
        _: &SelectLastTask,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let last = self
            .task_switcher
            .ordered_session_ids
            .len()
            .saturating_sub(1);
        self.set_task_switcher_highlight(last, cx);
    }

    pub(super) fn confirm_task_switch_action(
        &mut self,
        _: &ConfirmTaskSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_task_switcher(false, window, cx);
    }

    pub(super) fn cancel_task_switch_action(
        &mut self,
        _: &CancelTaskSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_task_switcher(window, cx);
    }

    pub(super) fn task_switcher_modifiers_changed(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.task_switcher.open && !event.control {
            self.finish_task_switcher(false, window, cx);
        }
    }

    fn cycle_task_switcher(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.task_switcher.open {
            self.open_task_switcher(reverse, window, cx);
            return;
        }

        let Some(current_index) = self
            .task_switcher
            .highlighted_session_id
            .and_then(|current| {
                self.task_switcher
                    .ordered_session_ids
                    .iter()
                    .position(|candidate| *candidate == current)
            })
        else {
            self.cancel_task_switcher(window, cx);
            return;
        };
        let len = self.task_switcher.ordered_session_ids.len();
        if len == 0 {
            self.cancel_task_switcher(window, cx);
            return;
        }
        let next = if reverse {
            (current_index + len - 1) % len
        } else {
            (current_index + 1) % len
        };
        self.set_task_switcher_highlight(next, cx);
    }

    fn open_task_switcher(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let ordered = ordered_task_ids(
            self.state.selected_session,
            &self.task_switcher.recent_session_ids,
            &self.state.sessions,
            &sessions::dormant_session_ids(&self.state.sessions, self.state.dormant_after_days),
        );
        let Some(highlighted_index) =
            initial_highlight_index(&ordered, self.state.selected_session, reverse)
        else {
            return;
        };

        if self.project_switcher.is_open() {
            self.cancel_project_switcher(window, cx);
        }
        if self.command_palette.is_open() {
            self.toggle_command_palette_action(&ToggleCommandPalette, window, cx);
        }
        let open_menus = self
            .menus
            .borrow()
            .values()
            .filter(|menu| menu.is_open())
            .cloned()
            .collect::<Vec<_>>();
        self.task_switcher.previous_focus = if open_menus.is_empty() {
            window.focused(cx)
        } else if self.settings_page.is_some() {
            Some(self.settings_focus.clone())
        } else {
            Some(self.composer_focus(cx))
        };

        self.task_switcher.open = true;
        self.task_switcher.ordered_session_ids = ordered;
        self.task_switcher.highlighted_session_id = self
            .task_switcher
            .ordered_session_ids
            .get(highlighted_index)
            .copied();
        self.task_switcher.original_session_id = self.state.selected_session;
        self.task_switcher.reveal_highlight();
        self.task_switcher.generation = self.task_switcher.generation.wrapping_add(1);
        let generation = self.task_switcher.generation;
        let focus = self.task_switcher.focus.clone();
        let weak = cx.entity().downgrade();

        if !open_menus.is_empty() {
            window.defer(cx, move |window, cx| {
                for menu in open_menus {
                    menu.close(window, cx);
                }
            });
        }

        // Deferred overlays join the dispatch tree after their deferred
        // paint. Two frames guarantees the switcher focus can resolve, while
        // the root modifier listener still catches a very quick Control
        // release.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let should_focus = weak
                    .update(cx, |this, _| {
                        this.task_switcher.open && this.task_switcher.generation == generation
                    })
                    .unwrap_or(false);
                if should_focus {
                    window.focus(&focus, cx);
                }
            });
        });
        cx.notify();
    }

    fn set_task_switcher_highlight(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(session_id) = self.task_switcher.ordered_session_ids.get(index).copied() else {
            return;
        };
        if self.task_switcher.highlighted_session_id == Some(session_id) {
            return;
        }
        self.task_switcher.highlighted_session_id = Some(session_id);
        self.task_switcher.reveal_highlight();
        cx.notify();
    }

    fn finish_task_switcher(
        &mut self,
        pointer_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.task_switcher.open {
            return;
        }
        let selected = self.task_switcher.highlighted_session_id;
        let original = self.task_switcher.original_session_id;
        let previous_focus = self.task_switcher.dismiss();
        let may_commit = pointer_selection || self.state.selected_session == original;
        let mut focus_after = previous_focus;
        if may_commit
            && let Some(selected) = selected
            && self.state.sessions.iter().any(|session| {
                session.id == selected && session.has_started() && session.archived_at.is_none()
            })
        {
            let was_in_settings = self.settings_page.is_some();
            self.settings_page = None;
            self.select_session(selected, cx);
            if was_in_settings {
                focus_after = Some(self.composer_focus(cx));
            }
        }
        if let Some(previous_focus) = focus_after {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_task_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.task_switcher.open {
            return;
        }
        if let Some(previous_focus) = self.task_switcher.dismiss() {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    fn render_task_switcher_entry(
        &self,
        session: &AgentSession,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let session_id = session.id;
        let highlighted = self.task_switcher.highlighted_session_id == Some(session_id);
        let working = matches!(
            session.status,
            SessionStatus::Connecting | SessionStatus::Working
        );
        let project_name = self
            .state
            .projects
            .iter()
            .find(|project| project.id == session.project_id)
            .filter(|project| !project.is_projectless())
            .map(Project::display_name);

        div()
            .id(SharedString::from(format!(
                "task-switcher-entry-{session_id}"
            )))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex_none()
            .px(px(ROW_INSET_X))
            .rounded(px(ROW_RADIUS))
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_default()
            .when(highlighted, |entry| entry.bg(theme.overlay_strong))
            .hover(|entry| entry.bg(theme.overlay))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                let Some(index) = this
                    .task_switcher
                    .ordered_session_ids
                    .iter()
                    .position(|candidate| *candidate == session_id)
                else {
                    return;
                };
                this.set_task_switcher_highlight(index, cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.task_switcher.open {
                    this.task_switcher.highlighted_session_id = Some(session_id);
                    this.finish_task_switcher(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(task_switcher_title(session)),
            )
            .when_some(project_name, |entry, project_name| {
                entry.child(
                    div()
                        .flex_none()
                        .max_w(px(PROJECT_NAME_MAX_WIDTH))
                        .truncate()
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(project_name),
                )
            })
            .when(working, |entry| {
                entry.child(motion::spin_slow(icon(
                    "icons/loader-circle.svg",
                    12.0,
                    status_color(&theme, session.status),
                )))
            })
            .when(session.status == SessionStatus::Background, |entry| {
                entry.child(icon(
                    "icons/hourglass.svg",
                    12.0,
                    status_color(&theme, session.status),
                ))
            })
            .when(session.status == SessionStatus::Waiting, |entry| {
                entry.child(icon(
                    "icons/alert.svg",
                    12.0,
                    status_color(&theme, session.status),
                ))
            })
            .when(session.status == SessionStatus::Failed, |entry| {
                entry.child(icon(
                    "icons/x-bold.svg",
                    12.0,
                    status_color(&theme, session.status),
                ))
            })
            .when(session.status == SessionStatus::Idle, |entry| {
                let unread = self.state.unseen_completions.contains_key(&session_id);
                entry.child(
                    div()
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
                        ),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_task_switcher(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.task_switcher.open || self.task_switcher.ordered_session_ids.is_empty() {
            return None;
        }
        let theme = Theme::current(cx);
        let scroll = self.task_switcher.scroll.clone();
        let focus = self.task_switcher.focus.clone();
        let entries = self
            .task_switcher
            .ordered_session_ids
            .iter()
            .filter_map(|session_id| {
                self.state
                    .sessions
                    .iter()
                    .find(|session| session.id == *session_id)
            })
            .map(|session| self.render_task_switcher_entry(session, cx))
            .collect::<Vec<_>>();
        let viewport_height = f32::from(window.viewport_size().height);
        // Padding below the card pushes the centered layout up by half the
        // padding, so the switcher sits slightly above center like Spotlight.
        let vertical_bias = (viewport_height * VERTICAL_BIAS).min(VERTICAL_BIAS_MAX);
        let list_max_height = (viewport_height
            - WINDOW_MARGIN * 2.0
            - vertical_bias * 2.0
            - TITLE_HEIGHT
            - MODAL_INSET * 2.0)
            .max(ROW_HEIGHT);

        let card = div()
            .id("task-switcher")
            .key_context("TaskSwitcher")
            .track_focus(&focus)
            .w(px(MODAL_WIDTH))
            .p(px(MODAL_INSET))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(MODAL_RADIUS))
            .border(hairline())
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_xl()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(TITLE_HEIGHT))
                    .flex_none()
                    .px(px(TITLE_INSET_X))
                    .flex()
                    .items_center()
                    .text_size(sp(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(tr!("task_switcher.recently_viewed")),
            )
            .child(
                div()
                    .id("task-switcher-list")
                    .flex_none()
                    .max_h(px(list_max_height))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .flex()
                    .flex_col()
                    .children(entries),
            );
        let layer = div()
            .id("task-switcher-layer")
            .absolute()
            .inset_0()
            .occlude()
            .pb(px(vertical_bias * 2.0))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.cancel_task_switcher(window, cx)),
            )
            .child(card);
        Some(gpui::deferred(layer).with_priority(6).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started_session(last_reply_at: u64) -> AgentSession {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.begin_turn("task");
        session.last_reply_at = Some(last_reply_at);
        session
    }

    #[test]
    fn switcher_order_contains_only_the_ten_most_recently_visited_tasks() {
        let current = started_session(10);
        let recent = (0..12).map(|_| started_session(0)).collect::<Vec<_>>();
        let unvisited = started_session(0);
        let missing = Uuid::new_v4();
        let mut sessions = vec![unvisited, current.clone()];
        sessions.extend(recent.iter().cloned());
        let mut recorded_recency = vec![missing];
        recorded_recency.extend(recent.iter().map(|session| session.id));
        let mut expected = vec![current.id];
        expected.extend(recent.iter().take(MAX_TASKS - 1).map(|session| session.id));

        assert_eq!(
            ordered_task_ids(
                Some(current.id),
                &recorded_recency,
                &sessions,
                &HashSet::new()
            ),
            expected
        );
    }

    #[test]
    fn switcher_order_fills_remaining_slots_with_recently_active_tasks() {
        let current = started_session(10);
        let recent = [started_session(20), started_session(30)];
        // Recently active tasks rank below any recently visited one, and an
        // already-listed task is not repeated.
        let active_newest = started_session(90);
        let active_oldest = started_session(5);
        let sessions = vec![
            active_oldest.clone(),
            recent[0].clone(),
            active_newest.clone(),
            current.clone(),
            recent[1].clone(),
        ];
        let recorded_recency = recent.iter().map(|session| session.id).collect::<Vec<_>>();

        assert_eq!(
            ordered_task_ids(
                Some(current.id),
                &recorded_recency,
                &sessions,
                &HashSet::new()
            ),
            vec![
                current.id,
                recent[0].id,
                recent[1].id,
                active_newest.id,
                active_oldest.id,
            ]
        );
    }

    #[test]
    fn first_control_tab_targets_previous_task_and_reverse_wraps() {
        let current = Uuid::new_v4();
        let previous = Uuid::new_v4();
        let oldest = Uuid::new_v4();
        let ordered = [current, previous, oldest];

        assert_eq!(
            initial_highlight_index(&ordered, Some(current), false),
            Some(1)
        );
        assert_eq!(
            initial_highlight_index(&ordered, Some(current), true),
            Some(2)
        );
    }

    #[test]
    fn single_current_task_still_opens_the_switcher() {
        let current = Uuid::new_v4();
        assert_eq!(
            initial_highlight_index(&[current], Some(current), false),
            Some(0)
        );
    }

    #[test]
    fn switcher_omits_dormant_tasks_even_the_current_one() {
        let shelved = started_session(20);
        let live = started_session(10);
        let sessions = vec![shelved.clone(), live.clone()];
        let dormant = HashSet::from([shelved.id]);
        // A dormant current task does not lead the list, and a dormant
        // recent visit is not offered either.
        let ordered = ordered_task_ids(
            Some(shelved.id),
            &[shelved.id, live.id],
            &sessions,
            &dormant,
        );
        assert_eq!(ordered, vec![live.id]);
    }

    #[test]
    fn draft_can_switch_to_the_only_visited_started_task() {
        let draft = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        let started = started_session(10);
        let sessions = vec![draft.clone(), started.clone()];
        let ordered = ordered_task_ids(
            Some(draft.id),
            &[draft.id, started.id],
            &sessions,
            &HashSet::new(),
        );
        assert_eq!(ordered, vec![started.id]);
        assert_eq!(
            initial_highlight_index(&ordered, Some(draft.id), false),
            Some(0)
        );
    }
}
