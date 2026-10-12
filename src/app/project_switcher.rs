//! Cmd-N switching across recently used projects in a New Task draft.
//!
//! Mirrors the task switcher: the project order is snapshotted when the
//! overlay opens. On the New Task page it starts with a focused search;
//! another Cmd-N/Shift-Cmd-N enters cycling. Repeated presses move only the
//! highlight, and releasing the platform modifier commits once — so the draft under the pointer never
//! retargets mid-gesture. Over Big Picture the same overlay answers for the
//! standing new-task draft instead. Recency is borrowed from the task
//! switcher's session history: a project ranks by when one of its tasks was
//! last activated. The snapshot is capped at ten projects; when recency
//! leaves slots open, the most recently added projects fill them.

use super::*;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};

const MODAL_WIDTH: f32 = 400.0;
const MODAL_RADIUS: f32 = 17.0;
const MODAL_INSET: f32 = 6.0;
const TITLE_HEIGHT: f32 = 30.0;
const TITLE_INSET_X: f32 = 10.0;
const ROW_HEIGHT: f32 = 34.0;
const ROW_INSET_X: f32 = 10.0;
const ROW_RADIUS: f32 = 10.0;
const PATH_MAX_WIDTH: f32 = 180.0;
const MAX_PROJECTS: usize = 10;
const MAX_SEARCH_RESULTS: usize = 50;
const WINDOW_MARGIN: f32 = 44.0;
const VERTICAL_BIAS: f32 = 0.08;
const VERTICAL_BIAS_MAX: f32 = 96.0;
const TEMPORARY_CHAT_PICKER_ID: Uuid = Uuid::from_u128(u128::MAX);
const NEW_CHAT_PICKER_ID: Uuid = Uuid::from_u128(u128::MAX - 1);

/// What a commit retargets: the draft the ⌘N gesture was opened on, the
/// open Projects page's own project — the ⌘⇧P gesture's — Big Picture's
/// standing new-task draft, the boss chat, whose pick lands on a New
/// Task page like the draft's, or a standalone terminal — the ⌘T
/// gesture's — whose pick becomes the terminal's working directory.
#[derive(Clone, Copy, PartialEq)]
enum ProjectSwitcherTarget {
    Draft,
    ProjectsPage,
    BigPicture,
    BossChat,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BossChatPickerLanding {
    NewTask,
    NoProject,
    BossChat,
}

fn boss_chat_picker_landing(is_boss: bool, is_projectless: bool) -> BossChatPickerLanding {
    if is_boss {
        BossChatPickerLanding::BossChat
    } else if is_projectless {
        BossChatPickerLanding::NoProject
    } else {
        BossChatPickerLanding::NewTask
    }
}

/// Runtime-only switcher state, like its task counterpart: recency lives in
/// the task switcher's session history, so restoration seeds nothing here.
pub(super) struct ProjectSwitcherUi {
    open: bool,
    ordered_project_ids: Vec<Uuid>,
    recent_project_ids: Vec<Uuid>,
    search: Option<Entity<TextInput>>,
    searching: bool,
    cycling: bool,
    highlighted_project_id: Option<Uuid>,
    original_session_id: Option<Uuid>,
    target: ProjectSwitcherTarget,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    scroll: ScrollHandle,
    generation: u64,
}

impl ProjectSwitcherUi {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            ordered_project_ids: Vec::new(),
            recent_project_ids: Vec::new(),
            search: None,
            searching: false,
            cycling: false,
            highlighted_project_id: None,
            original_session_id: None,
            target: ProjectSwitcherTarget::Draft,
            focus,
            previous_focus: None,
            scroll: ScrollHandle::new(),
            generation: 0,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// The draft the switcher was opened on went away; without a window the
    /// previous focus cannot be restored, so just drop the overlay state.
    /// Only the draft target holds a session — the page target's `None`
    /// never matches here.
    pub(super) fn session_removed(&mut self, session_id: Uuid) {
        if self.original_session_id == Some(session_id) {
            self.dismiss();
        }
    }

    /// A project vanishing mid-gesture invalidates the snapshot, so close the
    /// overlay rather than repair selection against a stale row list.
    pub(super) fn project_removed(&mut self, project_id: Uuid) {
        if self.open && self.ordered_project_ids.contains(&project_id) {
            self.dismiss();
        }
    }

    fn reveal_highlight(&self) {
        let Some(index) = self.highlighted_project_id.and_then(|highlighted| {
            self.ordered_project_ids
                .iter()
                .position(|candidate| *candidate == highlighted)
        }) else {
            return;
        };
        self.scroll.scroll_to_item(index);
    }

    pub(super) fn dismiss(&mut self) -> Option<FocusHandle> {
        self.open = false;
        self.ordered_project_ids.clear();
        self.recent_project_ids.clear();
        self.searching = false;
        self.cycling = false;
        self.highlighted_project_id = None;
        self.original_session_id = None;
        self.target = ProjectSwitcherTarget::Draft;
        self.generation = self.generation.wrapping_add(1);
        self.previous_focus.take()
    }
}

pub(super) fn ordered_project_ids(
    current: Option<Uuid>,
    recent: &[Uuid],
    projects: &[Project],
) -> Vec<Uuid> {
    let by_id = projects
        .iter()
        .map(|project| (project.id, project))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::with_capacity(projects.len());
    let mut ordered = Vec::with_capacity(projects.len().min(MAX_PROJECTS));
    let mut push = |id| {
        let Some(project) = by_id.get(&id) else {
            return;
        };
        if ordered.len() < MAX_PROJECTS && !project.is_projectless() && seen.insert(id) {
            ordered.push(id);
        }
    };

    if let Some(current) = current {
        push(current);
    }
    for recent in recent {
        push(*recent);
    }
    // Recently used projects keep their rank; when they leave slots open, the
    // most recently added projects fill them so the list still shows ten.
    let mut recently_added = projects.iter().collect::<Vec<_>>();
    recently_added.sort_by_key(|project| std::cmp::Reverse(project.created_at));
    for project in recently_added {
        push(project.id);
    }
    ordered
}

fn action_ids_for_target(target: ProjectSwitcherTarget) -> &'static [Uuid] {
    match target {
        ProjectSwitcherTarget::Draft | ProjectSwitcherTarget::BossChat => {
            &[NEW_CHAT_PICKER_ID, TEMPORARY_CHAT_PICKER_ID]
        }
        _ => &[],
    }
}

fn picker_ids(project_ids: &[Uuid], target: ProjectSwitcherTarget) -> Vec<Uuid> {
    action_ids_for_target(target)
        .iter()
        .copied()
        .chain(project_ids.iter().copied())
        .collect()
}

fn initial_picker_highlight(project_ids: &[Uuid], target: ProjectSwitcherTarget) -> Option<Uuid> {
    project_ids
        .first()
        .copied()
        .or_else(|| action_ids_for_target(target).first().copied())
}

fn highlight_after_filter(current: Option<Uuid>, rows: &[Uuid], query: &str) -> Option<Uuid> {
    if query.trim().is_empty() {
        current
            .filter(|id| rows.contains(id))
            .or_else(|| rows.first().copied())
    } else {
        rows.first().copied()
    }
}

fn is_picker_action_id(id: Uuid) -> bool {
    id == NEW_CHAT_PICKER_ID || id == TEMPORARY_CHAT_PICKER_ID
}

fn next_project_switcher_index(current: usize, len: usize, reverse: bool) -> Option<usize> {
    (len > 0).then(|| {
        if reverse {
            (current + len - 1) % len
        } else {
            (current + 1) % len
        }
    })
}

fn matching_action_ids(query: &str, target: ProjectSwitcherTarget) -> Vec<Uuid> {
    let query = query.trim();
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
    let mut utf32 = Vec::new();
    action_ids_for_target(target)
        .iter()
        .copied()
        .filter(|id| {
            if query.is_empty() {
                return true;
            }
            let label = if *id == NEW_CHAT_PICKER_ID {
                tr!("project.new_chat")
            } else {
                tr!("project.temporary_chat")
            };
            pattern
                .score(Utf32Str::new(&label, &mut utf32), &mut matcher)
                .is_some()
        })
        .collect()
}

// Empty search keeps the cycling snapshot; a query searches every known project
// with the same fuzzy name/path matching as “New task in…”.
fn filtered_project_ids(
    recent: &[Uuid],
    projects: &[Project],
    home: Option<&Path>,
    query: &str,
) -> Vec<Uuid> {
    let query = query.trim();
    if query.is_empty() {
        return recent.to_vec();
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
    let mut utf32 = Vec::new();
    let mut scored = projects
        .iter()
        .filter_map(|project| {
            if project.is_projectless() {
                return None;
            }
            let path = settings::abbreviate_home_path(&project.path, home);
            let name = project.display_name();
            let text = format!("{name} {path} directory folder project");
            pattern
                .score(Utf32Str::new(&text, &mut utf32), &mut matcher)
                .map(|score| (score, project.id))
        })
        .collect::<Vec<_>>();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .take(MAX_SEARCH_RESULTS)
        .map(|(_, id)| id)
        .collect()
}

impl Waku {
    pub(super) fn switch_project_forward_action(
        &mut self,
        _: &SwitchProjectForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_project_switcher(false, window, cx);
    }

    pub(super) fn switch_project_backward_action(
        &mut self,
        _: &SwitchProjectBackward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_project_switcher(true, window, cx);
    }

    pub(super) fn select_first_project_action(
        &mut self,
        _: &SelectFirstProject,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_project_switcher_highlight(0, cx);
    }

    pub(super) fn select_last_project_action(
        &mut self,
        _: &SelectLastProject,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let last = self
            .project_switcher
            .ordered_project_ids
            .len()
            .saturating_sub(1);
        self.set_project_switcher_highlight(last, cx);
    }

    pub(super) fn confirm_project_switch_action(
        &mut self,
        _: &ConfirmProjectSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_project_switcher(false, window, cx);
    }

    pub(super) fn cancel_project_switch_action(
        &mut self,
        _: &CancelProjectSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_project_switcher(window, cx);
    }

    pub(super) fn project_switcher_modifiers_changed(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_switcher.open
            && (!self.project_switcher.searching || self.project_switcher.cycling)
            && !event.secondary()
        {
            self.finish_project_switcher(false, window, cx);
        }
    }

    fn cycle_project_switcher(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_switcher.open {
            if self.project_switcher.searching
                && !self.project_switcher.cycling
                && window.modifiers().secondary()
            {
                self.project_switcher.cycling = true;
                self.project_switcher.ordered_project_ids = picker_ids(
                    &self.project_switcher.recent_project_ids,
                    self.project_switcher.target,
                );
                self.project_switcher.highlighted_project_id = self
                    .project_switcher
                    .recent_project_ids
                    .first()
                    .copied()
                    .or_else(|| self.project_switcher.ordered_project_ids.first().copied());
                window.focus(&self.project_switcher.focus, cx);
            }
            self.advance_project_switcher(reverse, window, cx);
            return;
        }
        // ⌘⇧N never opens the switcher — the chord is "New task in…" and
        // only ever reverse-cycles the overlay ⌘N already raised.
        if reverse {
            cx.propagate();
            return;
        }
        // Over Big Picture the ⌘N chord's "which project" job belongs to the
        // overlay's own new-task draft: an armed card peels off first, then
        // the switcher answers it for that draft instead of the session
        // underneath.
        if self.big_picture.is_open() {
            if self.big_picture.target().is_some() {
                self.set_big_picture_target(None, cx);
                return;
            }
            if !self.open_big_picture_project_switcher(window, cx) {
                cx.propagate();
            }
            return;
        }
        // ⌘N from a deliverable's preview page lands on the boss chat it
        // borrows — the page isn't a session of its own, so "new chat"
        // from it means the chat underneath, not the project picker over it.
        if self.live_deliverable_page().is_some()
            && let Some(key) = self.boss_chat_key()
        {
            self.settings_page = None;
            self.chat_with_boss(key, cx);
            return;
        }
        // ⌘N lands wherever the user worked most recently: a boss chat
        // messaged since the last New Task start wins over the page. On
        // the boss chat itself the chord asks "which project" — the
        // picker's commit lands on the New Task page.
        if self.boss_chat_key().is_some() && self.session_surface_active() {
            if self.open_boss_chat_project_switcher(window, cx) {
                return;
            }
        } else if let Some(key) = self.boss_message_outranks_new_task() {
            self.settings_page = None;
            self.chat_with_boss(key, cx);
            return;
        }
        // The chord shares ⌘N with New Session; when no draft can take
        // the switcher, let the keystroke fall through to it.
        if !self.open_project_switcher(reverse, window, cx) {
            cx.propagate();
        }
    }

    /// The ⌘⇧P gesture's second press onward: same overlay and ordering as
    /// ⌘N, but a commit retargets the open Projects page instead of a draft.
    pub(super) fn cycle_page_project_switcher(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.project_switcher.open {
            if !self.open_page_project_switcher(reverse, window, cx) {
                cx.propagate();
            }
            return;
        }
        self.advance_project_switcher(reverse, window, cx);
    }

    fn advance_project_switcher(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current_index) =
            self.project_switcher
                .highlighted_project_id
                .and_then(|current| {
                    self.project_switcher
                        .ordered_project_ids
                        .iter()
                        .position(|candidate| *candidate == current)
                })
        else {
            if !self.project_switcher.searching {
                self.cancel_project_switcher(window, cx);
            }
            return;
        };
        let len = self.project_switcher.ordered_project_ids.len();
        if len == 0 {
            self.cancel_project_switcher(window, cx);
            return;
        }
        let Some(next) = next_project_switcher_index(current_index, len, reverse) else {
            self.cancel_project_switcher(window, cx);
            return;
        };
        self.set_project_switcher_highlight(next, cx);
    }

    /// Only the New Task page's own draft can retarget its project — the
    /// switcher answers "which project" for the draft on screen. On any
    /// other surface (a page, a full-width terminal, Settings) the open
    /// fails and the chord falls through to New Session, which navigates
    /// to the New Task page. The draft's own project pins the head of the
    /// list; the first press opens search, and another press starts cycling.
    fn open_project_switcher(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.session_surface_active() {
            return false;
        }
        let Some(current_project) = self
            .selected_session()
            .filter(|session| !session.has_started() && !session.is_busy())
            .map(|session| session.project_id)
        else {
            return false;
        };
        let opened = self.open_switcher(
            Some(current_project),
            self.state.selected_session,
            ProjectSwitcherTarget::Draft,
            false,
            reverse,
            window,
            cx,
        );
        if opened {
            self.begin_project_switcher_search(window, cx);
        }
        opened
    }

    /// ⌘N over the boss chat: the same searching switcher the New Task
    /// page raises, headed by the boss's own row. Its project picks land
    /// on the New Task page; its boss pick stays on the chat.
    fn open_boss_chat_project_switcher(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(key) = self.boss_chat_key() else {
            return false;
        };
        let current = self.boss_ui.states.get(&key).map(|state| state.identity.id);
        if !self.open_switcher(
            current,
            self.state.selected_session,
            ProjectSwitcherTarget::BossChat,
            false,
            false,
            window,
            cx,
        ) {
            return false;
        }
        self.begin_project_switcher_search(window, cx);
        true
    }

    /// ⌘T over the boss chat: the switcher asks which project the new
    /// floating terminal belongs to — the pick opens the terminal at the
    /// project's root. With no registered projects the open fails and the
    /// chord keeps its usual directory guess.
    pub(super) fn open_terminal_project_switcher(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let opened = self.open_switcher(
            None,
            self.state.selected_session,
            ProjectSwitcherTarget::Terminal,
            false,
            false,
            window,
            cx,
        );
        if opened {
            self.begin_project_switcher_search(window, cx);
        }
        opened
    }

    /// The first ⌘N opens the switcher searching — a focused field over the
    /// recency list — which the boss chat and the New Task page share.
    /// ⌘T's terminal pick reuses the same searching overlay.
    fn begin_project_switcher_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_switcher.searching = true;
        self.project_switcher.recent_project_ids = self
            .project_switcher
            .ordered_project_ids
            .iter()
            .copied()
            .filter(|id| !is_picker_action_id(*id))
            .collect();
        let terminal_target = self.project_switcher.target == ProjectSwitcherTarget::Terminal;
        let search = self
            .project_switcher
            .search
            .get_or_insert_with(|| {
                let search = cx.new(|cx| {
                    TextInput::new(window, cx)
                        .clear_on_escape()
                        .placeholder(tr!("command_palette.new_task_in_placeholder"))
                        .accessibility_label(tr!("command_palette.new_task_in_placeholder"))
                });
                cx.subscribe(&search, |this, search, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Edited) && this.project_switcher.searching {
                        let query = search.read(cx).content().to_owned();
                        this.filter_project_switcher(&query, cx);
                    }
                })
                .detach();
                search
            })
            .clone();
        search.update(cx, |input, cx| {
            let label = if terminal_target {
                tr!("project_switcher.terminal_placeholder")
            } else {
                tr!("command_palette.new_task_in_placeholder")
            };
            input.set_placeholder(label.clone(), cx);
            input.set_accessibility_label(label, cx);
            input.clear(cx);
        });
        self.filter_project_switcher("", cx);
    }

    /// ⌘N over Big Picture: the same overlay, answering "which project" for
    /// the standing new-task draft — the pick the untargeted composer
    /// submits into — headed by the draft's current destination.
    fn open_big_picture_project_switcher(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.open_switcher(
            self.big_picture.new_task_project,
            None,
            ProjectSwitcherTarget::BigPicture,
            false,
            false,
            window,
            cx,
        )
    }

    /// ⌘⇧P's switcher: only while the Projects page is open, headed by the
    /// page's own project selection.
    fn open_page_project_switcher(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(current_project) = self.projects_page else {
            return false;
        };
        self.open_switcher(
            Some(current_project),
            None,
            ProjectSwitcherTarget::ProjectsPage,
            true,
            reverse,
            window,
            cx,
        )
    }

    /// The project pool a switcher target lists. The ⌘T terminal picker's
    /// rows are the registered durable projects — the same source the
    /// composer's `@` project mentions draw from — while the task-flavored
    /// targets also row in each live boss chat as a destination.
    fn project_switcher_pool(
        &self,
        target: ProjectSwitcherTarget,
        exclude_projectless: bool,
    ) -> Vec<Project> {
        if target == ProjectSwitcherTarget::Terminal {
            return self
                .state
                .projects
                .iter()
                .filter(|project| {
                    !project.temporary
                        && !project.is_projectless()
                        && !project.is_friends()
                        && !self.boss_ui.projects.contains_key(&project.id)
                })
                .cloned()
                .collect();
        }
        let mut projects: Vec<Project> = self
            .state
            .projects
            .iter()
            .filter(|project| !exclude_projectless || !project.is_projectless())
            .cloned()
            .collect();
        // A New Task destination can also be the boss's own page: each live
        // boss chat rows in like a project, ranked by its chat's recency.
        if matches!(
            target,
            ProjectSwitcherTarget::Draft | ProjectSwitcherTarget::BossChat
        ) {
            projects.extend(self.boss_switcher_projects());
        }
        projects
    }

    fn open_switcher(
        &mut self,
        current_project: Option<Uuid>,
        original_session_id: Option<Uuid>,
        target: ProjectSwitcherTarget,
        // The page has no projectless scope; its cycling list excludes them.
        exclude_projectless: bool,
        _reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let recent = self.task_switcher.recent_project_ids(&self.state.sessions);
        let projects = self.project_switcher_pool(target, exclude_projectless);
        let project_ids = ordered_project_ids(current_project, &recent, &projects);
        let ordered = picker_ids(&project_ids, target);
        let Some(highlighted) = initial_picker_highlight(&project_ids, target) else {
            return false;
        };

        if self.task_switcher.is_open() {
            self.cancel_task_switcher(window, cx);
        }
        if self.command_palette.is_open() {
            self.toggle_command_palette_action(&ToggleCommandPalette, window, cx);
        }
        let keyboard_options_open = self.keyboard_options_is_open();
        let open_menus = self
            .menus
            .borrow()
            .values()
            .filter(|menu| menu.is_open())
            .cloned()
            .collect::<Vec<_>>();
        self.project_switcher.previous_focus = if open_menus.is_empty() && !keyboard_options_open {
            window.focused(cx)
        } else if self.settings_page.is_some() {
            Some(self.settings_focus.clone())
        } else {
            Some(self.composer_focus(cx))
        };
        if keyboard_options_open {
            self.dismiss_keyboard_options(false, window, cx);
        }

        self.project_switcher.open = true;
        self.project_switcher.cycling = false;
        self.project_switcher.ordered_project_ids = ordered;
        self.project_switcher.highlighted_project_id = Some(highlighted);
        self.project_switcher.original_session_id = original_session_id;
        self.project_switcher.target = target;
        self.project_switcher.reveal_highlight();
        self.project_switcher.generation = self.project_switcher.generation.wrapping_add(1);
        let generation = self.project_switcher.generation;
        let focus = self.project_switcher.focus.clone();
        let weak = cx.entity().downgrade();

        if !open_menus.is_empty() {
            window.defer(cx, move |window, cx| {
                for menu in open_menus {
                    menu.close(window, cx);
                }
            });
        }

        // Deferred overlays join the dispatch tree after their deferred paint.
        // Two frames guarantees the switcher focus can resolve, while the root
        // modifier listener still catches a very quick modifier release.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let focus = weak
                    .update(cx, |this, cx| {
                        (this.project_switcher.open
                            && this.project_switcher.generation == generation)
                            .then(|| {
                                if this.project_switcher.searching {
                                    this.project_switcher
                                        .search
                                        .as_ref()
                                        .unwrap()
                                        .read(cx)
                                        .focus()
                                } else {
                                    focus
                                }
                            })
                    })
                    .ok()
                    .flatten();
                if let Some(focus) = focus {
                    window.focus(&focus, cx);
                }
            });
        });
        cx.notify();
        true
    }

    fn filter_project_switcher(&mut self, query: &str, cx: &mut Context<Self>) {
        let projects = self.project_switcher_pool(self.project_switcher.target, false);
        let project_ids = filtered_project_ids(
            &self.project_switcher.recent_project_ids,
            &projects,
            self.home_directory.as_deref(),
            query,
        );
        let ordered = matching_action_ids(query, self.project_switcher.target)
            .into_iter()
            .chain(project_ids)
            .collect();
        self.project_switcher.ordered_project_ids = ordered;
        self.project_switcher.highlighted_project_id = highlight_after_filter(
            self.project_switcher.highlighted_project_id,
            &self.project_switcher.ordered_project_ids,
            query,
        );
        self.project_switcher.reveal_highlight();
        cx.notify();
    }

    fn set_project_switcher_highlight(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(project_id) = self
            .project_switcher
            .ordered_project_ids
            .get(index)
            .copied()
        else {
            return;
        };
        if self.project_switcher.highlighted_project_id == Some(project_id) {
            return;
        }
        self.project_switcher.highlighted_project_id = Some(project_id);
        self.project_switcher.reveal_highlight();
        cx.notify();
    }

    fn finish_project_switcher(
        &mut self,
        pointer_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.project_switcher.open {
            return;
        }
        let selected = self.project_switcher.highlighted_project_id;
        if self.project_switcher.searching && selected.is_none() {
            return;
        }
        let original = self.project_switcher.original_session_id;
        let target = self.project_switcher.target;
        let previous_focus = self.project_switcher.dismiss();
        if target == ProjectSwitcherTarget::Draft
            && !pointer_selection
            && self.state.selected_session != original
        {
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        if selected == Some(NEW_CHAT_PICKER_ID)
            && matches!(
                target,
                ProjectSwitcherTarget::Draft | ProjectSwitcherTarget::BossChat
            )
        {
            self.settings_page = None;
            if target == ProjectSwitcherTarget::BossChat {
                self.create_projectless_session(cx);
            } else {
                self.create_projectless_session_from_composer(cx);
            }
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        if selected == Some(TEMPORARY_CHAT_PICKER_ID)
            && matches!(
                target,
                ProjectSwitcherTarget::Draft | ProjectSwitcherTarget::BossChat
            )
        {
            self.settings_page = None;
            self.create_incognito_projectless_session(cx);
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        // A Big Picture commit retargets the overlay's standing new-task
        // draft — the destination its untargeted composer submits into —
        // never the session idling underneath the scrim.
        if target == ProjectSwitcherTarget::BigPicture {
            if self.big_picture.is_open()
                && let Some(project_id) = selected
                && self
                    .state
                    .projects
                    .iter()
                    .any(|project| project.id == project_id)
            {
                self.big_picture.new_task_project = Some(project_id);
                self.sync_big_picture_draft(cx);
            }
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        // Page commits have no draft to guard — the highlighted project just
        // becomes the page's scope, and its tables refresh.
        if target == ProjectSwitcherTarget::ProjectsPage {
            if let Some(project_id) = selected
                && self
                    .state
                    .projects
                    .iter()
                    .any(|project| project.id == project_id)
            {
                self.switch_projects_page_project(project_id, window, cx);
            }
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        // A terminal commit spawns the floating terminal at the picked
        // project's root — no draft or page to retarget.
        if target == ProjectSwitcherTarget::Terminal {
            if let Some(project_id) = selected
                && let Some(project) = self
                    .state
                    .projects
                    .iter()
                    .find(|project| project.id == project_id)
            {
                let root = project.path.clone();
                if let Some(terminal_id) = self.create_terminal(root, None, None, cx) {
                    self.open_docked_terminal(terminal_id, window, cx);
                    // The card owns focus now; restoring the switcher's
                    // previous focus would send typing back to the composer.
                    return;
                }
            }
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        // A boss row is a destination, not a project pick — committing it
        // opens that boss's chat from either New Task flavored target.
        if let Some(project_id) = selected
            && let Some(key) = self.boss_key_for_project(project_id)
        {
            self.chat_with_boss(key, cx);
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        // A switcher raised over the boss chat commits to the New Task
        // page: the picked project's remembered draft, or a fresh one —
        // the same landing ⌘N makes anywhere else.
        if target == ProjectSwitcherTarget::BossChat {
            self.settings_page = None;
            if let Some(project_id) = selected
                && self
                    .state
                    .projects
                    .iter()
                    .any(|project| project.id == project_id)
            {
                let landing = boss_chat_picker_landing(
                    self.boss_key_for_project(project_id).is_some(),
                    self.state
                        .projects
                        .iter()
                        .any(|project| project.id == project_id && project.is_projectless()),
                );
                match landing {
                    BossChatPickerLanding::BossChat => {
                        if let Some(key) = self.boss_key_for_project(project_id) {
                            self.chat_with_boss(key, cx);
                        }
                    }
                    BossChatPickerLanding::NoProject => self.create_projectless_session(cx),
                    BossChatPickerLanding::NewTask => {
                        if let Some(session_id) = self.session_navigation.remembered_new_task(
                            &self.state.sessions,
                            project_id,
                            |session| self.session_is_boss_managed(session),
                        )
                        {
                            self.select_session(session_id, cx);
                        } else {
                            self.create_session_for(project_id, self.state.last_provider, cx);
                        }
                    }
                }
            }
            if let Some(previous_focus) = previous_focus {
                window.focus(&previous_focus, cx);
            }
            cx.notify();
            return;
        }
        let may_commit = pointer_selection || self.state.selected_session == original;
        let mut focus_after = previous_focus;
        // A projectless pick always commits: the collapsed New chat row
        // stands for a fresh workspace, so "already on it" is never true.
        let projectless_target = selected.is_some_and(|project_id| {
            self.state
                .projects
                .iter()
                .any(|project| project.id == project_id && project.is_projectless())
        });
        if may_commit
            && let Some(project_id) = selected
            && self
                .state
                .projects
                .iter()
                .any(|project| project.id == project_id)
            && self.selected_session().is_some_and(|session| {
                // A started task commits through the switch path — the
                // same route the composer chip takes — not the draft
                // retarget below.
                !session.has_started() || self.can_switch_session_project(session.id)
            })
            && (projectless_target
                || self
                    .selected_session()
                    .is_some_and(|session| session.project_id != project_id))
            && (!projectless_target
                || self
                    .selected_session()
                    .is_some_and(|session| !session.has_started()))
        {
            let was_in_settings = self.settings_page.is_some();
            self.settings_page = None;
            self.select_project_from_composer(project_id, window, cx);
            if was_in_settings {
                focus_after = Some(self.composer_focus(cx));
            }
        }
        if let Some(previous_focus) = focus_after {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_project_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.project_switcher.open {
            return;
        }
        if let Some(previous_focus) = self.project_switcher.dismiss() {
            window.focus(&previous_focus, cx);
        }
        cx.notify();
    }

    /// A boss row: the same entry chrome a project takes — hover,
    /// highlight, pointer commit — with the boss's face and name where a
    /// project's icon and path would sit, and its group for the path slot.
    fn render_project_switcher_boss_entry(
        &self,
        key: waku_client::DaemonKey,
        project_id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.boss_ui.states.get(&key) else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let highlighted = self.project_switcher.highlighted_project_id == Some(project_id);
        div()
            .id(SharedString::from(format!("project-switcher-boss-{key:?}")))
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
                    .project_switcher
                    .ordered_project_ids
                    .iter()
                    .position(|candidate| *candidate == project_id)
                else {
                    return;
                };
                this.set_project_switcher_highlight(index, cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.project_switcher.open {
                    this.project_switcher.highlighted_project_id = Some(project_id);
                    this.finish_project_switcher(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.boss_avatar(&state.identity, 16.0, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(state.identity.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("boss.group")),
            )
            .into_any_element()
    }

    fn render_project_switcher_entry(
        &self,
        project: &Project,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let project_id = project.id;
        let highlighted = self.project_switcher.highlighted_project_id == Some(project_id);
        let path = (!project.is_projectless())
            .then(|| settings::abbreviate_home_path(&project.path, self.home_directory.as_deref()));

        div()
            .id(SharedString::from(format!(
                "project-switcher-entry-{project_id}"
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
                    .project_switcher
                    .ordered_project_ids
                    .iter()
                    .position(|candidate| *candidate == project_id)
                else {
                    return;
                };
                this.set_project_switcher_highlight(index, cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.project_switcher.open {
                    this.project_switcher.highlighted_project_id = Some(project_id);
                    this.finish_project_switcher(true, window, cx);
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
                    .child(if project.is_projectless() {
                        tr!("project.new_chat")
                    } else {
                        project.display_name()
                    }),
            )
            .when_some(path, |entry, path| {
                entry.child(
                    div()
                        .flex_none()
                        .max_w(px(PATH_MAX_WIDTH))
                        .truncate()
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(path),
                )
            })
            .into_any_element()
    }

    fn render_temporary_chat_entry(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let highlighted =
            self.project_switcher.highlighted_project_id == Some(TEMPORARY_CHAT_PICKER_ID);
        div()
            .id("project-switcher-temporary-chat")
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
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                if let Some(index) = this
                    .project_switcher
                    .ordered_project_ids
                    .iter()
                    .position(|candidate| *candidate == TEMPORARY_CHAT_PICKER_ID)
                {
                    this.set_project_switcher_highlight(index, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                if this.project_switcher.open {
                    this.project_switcher.highlighted_project_id = Some(TEMPORARY_CHAT_PICKER_ID);
                    this.finish_project_switcher(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(crate::ui::icon(
                "icons/ghost.svg",
                14.0,
                theme.text_secondary,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(tr!("project.temporary_chat")),
            )
            .into_any_element()
    }

    fn render_new_chat_entry(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let highlighted = self.project_switcher.highlighted_project_id == Some(NEW_CHAT_PICKER_ID);
        div()
            .id("project-switcher-new-chat")
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
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                if let Some(index) = this
                    .project_switcher
                    .ordered_project_ids
                    .iter()
                    .position(|candidate| *candidate == NEW_CHAT_PICKER_ID)
                {
                    this.set_project_switcher_highlight(index, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                if this.project_switcher.open {
                    this.project_switcher.highlighted_project_id = Some(NEW_CHAT_PICKER_ID);
                    this.finish_project_switcher(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(crate::ui::icon(
                "icons/plus.svg",
                14.0,
                theme.text_secondary,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(tr!("project.new_chat")),
            )
            .into_any_element()
    }

    pub(super) fn render_project_switcher(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.project_switcher.open {
            return None;
        }
        let theme = Theme::current(cx);
        let scroll = self.project_switcher.scroll.clone();
        let focus = self.project_switcher.focus.clone();
        let group_label = if self.project_switcher.searching
            && self
                .project_switcher
                .search
                .as_ref()
                .is_some_and(|search| !search.read(cx).content().trim().is_empty())
        {
            tr!("command_palette.project")
        } else {
            tr!("project_switcher.recently_used")
        };
        let mut entries = self
            .project_switcher
            .ordered_project_ids
            .iter()
            .filter_map(|project_id| {
                if *project_id == NEW_CHAT_PICKER_ID {
                    return Some(self.render_new_chat_entry(cx));
                }
                if let Some(project) = self
                    .state
                    .projects
                    .iter()
                    .find(|project| project.id == *project_id)
                {
                    return Some(self.render_project_switcher_entry(project, cx));
                }
                if *project_id == TEMPORARY_CHAT_PICKER_ID {
                    return Some(self.render_temporary_chat_entry(cx));
                }
                self.boss_key_for_project(*project_id)
                    .map(|key| self.render_project_switcher_boss_entry(key, *project_id, cx))
            })
            .collect::<Vec<_>>();
        let action_count = self
            .project_switcher
            .ordered_project_ids
            .iter()
            .take_while(|id| is_picker_action_id(**id))
            .count();
        if entries.len() > action_count {
            entries.insert(
                action_count,
                div()
                    .h(px(TITLE_HEIGHT))
                    .flex_none()
                    .px(px(TITLE_INSET_X))
                    .flex()
                    .items_center()
                    .text_size(sp(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(group_label.clone())
                    .into_any_element(),
            );
        }
        let viewport_height = f32::from(window.viewport_size().height);
        // Padding below the card pushes the centered layout up by half the
        // padding, so the switcher sits slightly above center like Spotlight.
        let vertical_bias = (viewport_height * VERTICAL_BIAS).min(VERTICAL_BIAS_MAX);
        let search_height = if self.project_switcher.searching {
            44.0
        } else {
            0.0
        };
        let list_max_height = (viewport_height
            - WINDOW_MARGIN * 2.0
            - vertical_bias * 2.0
            - TITLE_HEIGHT
            - search_height
            - MODAL_INSET * 2.0)
            .max(ROW_HEIGHT);

        let card = div()
            .id("project-switcher")
            .key_context("ProjectSwitcher")
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
            .when(self.project_switcher.searching, |card| {
                card.child(
                    div()
                        .px(px(TITLE_INSET_X))
                        .h(px(search_height))
                        .flex_none()
                        .flex()
                        .items_center()
                        .child(self.project_switcher.search.as_ref().unwrap().clone()),
                )
            })
            .child(
                div()
                    .id("project-switcher-list")
                    .flex_none()
                    .max_h(px(list_max_height))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .flex()
                    .flex_col()
                    .children(entries)
                    .when(
                        self.project_switcher.ordered_project_ids.is_empty(),
                        |list| {
                            list.child(
                                div()
                                    .p(px(ROW_INSET_X))
                                    .text_color(theme.text_tertiary)
                                    .child(tr!("command_palette.no_results")),
                            )
                        },
                    ),
            );
        let layer = div()
            .id("project-switcher-layer")
            .absolute()
            .inset_0()
            .occlude()
            .pb(px(vertical_bias * 2.0))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.cancel_project_switcher(window, cx)),
            )
            .child(motion::modal_enter("project-switcher-card-enter", card));
        // Priority must clear Big Picture's 7 — ⌘N opens the overlay on top
        // of it now — and ties the menu layer's 8, which a switcher open
        // closes anyway.
        Some(gpui::deferred(layer).with_priority(8).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boss_chat_picker_routes_project_no_project_and_boss_selections() {
        assert_eq!(
            boss_chat_picker_landing(false, false),
            BossChatPickerLanding::NewTask
        );
        assert_eq!(
            boss_chat_picker_landing(false, true),
            BossChatPickerLanding::NoProject
        );
        assert_eq!(
            boss_chat_picker_landing(true, false),
            BossChatPickerLanding::BossChat
        );
    }

    #[test]
    fn search_finds_projects_outside_recents_by_fuzzy_name_or_path() {
        let mut projects = (0..12)
            .map(|index| Project {
                id: Uuid::new_v4(),
                name: format!("Project {index}"),
                path: PathBuf::from(format!("/home/dev/repository-{index}")),
                bookmark: None,
                created_at: index,
                temporary: false,
                starred: false,
                submissions_enabled: false,
                qa_branch: None,
                friend_peer_id: None,
                kind: None,
                resolved_name: None,
            })
            .collect::<Vec<_>>();
        projects[0].name = "Forgotten Orchard".into();
        projects[0].path = PathBuf::from("/home/dev/archived-orchard");
        let recent = ordered_project_ids(None, &[], &projects);
        assert!(!recent.contains(&projects[0].id));
        assert_eq!(filtered_project_ids(&recent, &projects, None, "  "), recent);
        assert_eq!(
            filtered_project_ids(&recent, &projects, None, "FGORCH"),
            vec![projects[0].id]
        );
        assert_eq!(
            filtered_project_ids(
                &recent,
                &projects,
                Some(Path::new("/home")),
                "~/dev/archived-orchard"
            ),
            vec![projects[0].id]
        );
        assert!(filtered_project_ids(&recent, &projects, None, "no-such-project").is_empty());
        assert_eq!(filtered_project_ids(&recent, &projects, None, ""), recent);
    }

    #[test]
    fn switcher_order_contains_only_the_ten_most_recently_used_projects() {
        let project = |created_at| Project {
            id: Uuid::new_v4(),
            name: String::new(),
            path: PathBuf::new(),
            bookmark: None,
            created_at,
            temporary: false,
            starred: false,
            submissions_enabled: false,
            qa_branch: None,
            friend_peer_id: None,
            kind: None,
            resolved_name: None,
        };
        let current = project(0);
        let recent = (0..12).map(|_| project(0)).collect::<Vec<_>>();
        let removed = project(0);
        let mut projects = vec![current.clone()];
        projects.extend(recent.iter().cloned());
        let mut recorded_recency = vec![removed.id];
        recorded_recency.extend(recent.iter().map(|project| project.id));
        let mut expected = vec![current.id];
        expected.extend(
            recent
                .iter()
                .take(MAX_PROJECTS - 1)
                .map(|project| project.id),
        );

        assert_eq!(
            ordered_project_ids(Some(current.id), &recorded_recency, &projects),
            expected
        );
    }

    #[test]
    fn switcher_order_fills_remaining_slots_with_recently_added_projects() {
        let project = |created_at| Project {
            id: Uuid::new_v4(),
            name: String::new(),
            path: PathBuf::new(),
            bookmark: None,
            created_at,
            temporary: false,
            starred: false,
            submissions_enabled: false,
            qa_branch: None,
            friend_peer_id: None,
            kind: None,
            resolved_name: None,
        };
        let current = project(10);
        let recent = [project(20), project(30)];
        // Recently added projects rank below any recently used one, and an
        // already-listed project is not repeated.
        let added_newest = project(90);
        let added_oldest = project(5);
        let projects = vec![
            added_oldest.clone(),
            recent[0].clone(),
            added_newest.clone(),
            current.clone(),
            recent[1].clone(),
        ];
        let recorded_recency = recent.iter().map(|project| project.id).collect::<Vec<_>>();

        assert_eq!(
            ordered_project_ids(Some(current.id), &recorded_recency, &projects),
            vec![
                current.id,
                recent[0].id,
                recent[1].id,
                added_newest.id,
                added_oldest.id,
            ]
        );
    }

    #[test]
    fn projectless_workspaces_never_enter_the_recent_project_list() {
        let root = crate::projectless::workspace_root().expect("default projectless root");
        let projectless = |name, created_at| Project {
            id: Uuid::new_v4(),
            name: String::new(),
            path: root.join("2026-08-08").join(name),
            bookmark: None,
            created_at,
            temporary: false,
            starred: false,
            submissions_enabled: false,
            qa_branch: None,
            friend_peer_id: None,
            kind: None,
            resolved_name: None,
        };
        let first = projectless("first", 10);
        let second = projectless("second", 20);
        let ordinary = Project {
            id: Uuid::new_v4(),
            name: String::new(),
            path: PathBuf::from("/tmp/dev/ordinary"),
            bookmark: None,
            created_at: 30,
            temporary: false,
            starred: false,
            submissions_enabled: false,
            qa_branch: None,
            friend_peer_id: None,
            kind: None,
            resolved_name: None,
        };
        let projects = vec![first.clone(), second.clone(), ordinary.clone()];

        assert_eq!(ordered_project_ids(None, &[], &projects), vec![ordinary.id]);
        assert_eq!(
            ordered_project_ids(Some(first.id), &[second.id], &projects),
            vec![ordinary.id]
        );
        assert_eq!(
            filtered_project_ids(
                &[first.id, second.id, ordinary.id],
                &projects,
                None,
                "new chat"
            ),
            Vec::<Uuid>::new()
        );
    }

    #[test]
    fn projectless_actions_precede_recents_but_default_to_first_recent() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let projects = [first, second];
        let rows = picker_ids(&projects, ProjectSwitcherTarget::Draft);
        assert_eq!(
            rows,
            [NEW_CHAT_PICKER_ID, TEMPORARY_CHAT_PICKER_ID, first, second]
        );
        assert_eq!(
            initial_picker_highlight(&projects, ProjectSwitcherTarget::Draft),
            Some(first)
        );
        assert_eq!(highlight_after_filter(Some(first), &rows, ""), Some(first));
        assert_eq!(
            highlight_after_filter(Some(first), &[TEMPORARY_CHAT_PICKER_ID], "temporary"),
            Some(TEMPORARY_CHAT_PICKER_ID)
        );
        let first_recent_index = rows.iter().position(|id| *id == first).unwrap();
        let temporary_index =
            next_project_switcher_index(first_recent_index, rows.len(), true).unwrap();
        let new_chat_index =
            next_project_switcher_index(temporary_index, rows.len(), true).unwrap();
        assert_eq!(rows[temporary_index], TEMPORARY_CHAT_PICKER_ID);
        assert_eq!(rows[new_chat_index], NEW_CHAT_PICKER_ID);
    }

    #[test]
    fn projectless_action_labels_match_the_picker_query() {
        assert_eq!(
            matching_action_ids("New chat", ProjectSwitcherTarget::Draft),
            [NEW_CHAT_PICKER_ID]
        );
        assert_eq!(
            matching_action_ids("Temporary chat", ProjectSwitcherTarget::Draft),
            [TEMPORARY_CHAT_PICKER_ID]
        );
        assert_eq!(
            matching_action_ids("", ProjectSwitcherTarget::Draft),
            [NEW_CHAT_PICKER_ID, TEMPORARY_CHAT_PICKER_ID]
        );
        assert!(matching_action_ids("Temporary chat", ProjectSwitcherTarget::Terminal).is_empty());
    }
}
