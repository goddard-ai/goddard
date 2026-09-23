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

/// What a commit retargets: the draft the ⌘N gesture was opened on, the
/// open Projects page's own project — the ⌘⇧P gesture's — or Big
/// Picture's standing new-task draft.
#[derive(Clone, Copy, PartialEq)]
enum ProjectSwitcherTarget {
    Draft,
    ProjectsPage,
    BigPicture,
}

/// Runtime-only switcher state, like its task counterpart: recency lives in
/// the task switcher's session history, so restoration seeds nothing here.
pub(super) struct ProjectSwitcherUi {
    open: bool,
    ordered_project_ids: Vec<Uuid>,
    recent_project_ids: Vec<Uuid>,
    search: Option<Entity<TextInput>>,
    searching: bool,
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
    // Every projectless workspace displays as "No project" and hides its
    // ephemeral path, so the first one reached stands in for all of them.
    let mut projectless_seen = false;
    let mut ordered = Vec::with_capacity(projects.len().min(MAX_PROJECTS));
    let mut push = |id| {
        let Some(project) = by_id.get(&id) else {
            return;
        };
        if ordered.len() < MAX_PROJECTS
            && !(project.is_projectless() && projectless_seen)
            && seen.insert(id)
        {
            projectless_seen |= project.is_projectless();
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
    let mut projectless_seen = false;
    let mut scored = projects
        .iter()
        .filter_map(|project| {
            if project.is_projectless() && std::mem::replace(&mut projectless_seen, true) {
                return None;
            }
            let path = if project.is_projectless() {
                String::new()
            } else {
                settings::abbreviate_home_path(&project.path, home)
            };
            let text = format!("{} {path} directory folder project", project.display_name());
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
        if self.project_switcher.open && !self.project_switcher.searching && !event.secondary() {
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
            if self.project_switcher.searching && window.modifiers().secondary() {
                self.project_switcher.searching = false;
                self.project_switcher.ordered_project_ids =
                    self.project_switcher.recent_project_ids.clone();
                self.project_switcher.highlighted_project_id =
                    self.project_switcher.ordered_project_ids.first().copied();
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
        let next = if reverse {
            (current_index + len - 1) % len
        } else {
            (current_index + 1) % len
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
            self.project_switcher.searching = true;
            self.project_switcher.recent_project_ids =
                self.project_switcher.ordered_project_ids.clone();
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
            search.update(cx, |input, cx| input.clear(cx));
            self.filter_project_switcher("", cx);
        }
        opened
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

    fn open_switcher(
        &mut self,
        current_project: Option<Uuid>,
        original_session_id: Option<Uuid>,
        target: ProjectSwitcherTarget,
        // The page has no projectless scope; its cycling list excludes them.
        exclude_projectless: bool,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let recent = self.task_switcher.recent_project_ids(&self.state.sessions);
        let projects: Vec<Project> = self
            .state
            .projects
            .iter()
            .filter(|project| !exclude_projectless || !project.is_projectless())
            .cloned()
            .collect();
        let ordered = ordered_project_ids(current_project, &recent, &projects);
        let Some(highlighted_index) =
            task_switcher::initial_highlight_index(&ordered, current_project, reverse)
        else {
            return false;
        };

        if self.task_switcher.is_open() {
            self.cancel_task_switcher(window, cx);
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
        self.project_switcher.previous_focus = if open_menus.is_empty() {
            window.focused(cx)
        } else if self.settings_page.is_some() {
            Some(self.settings_focus.clone())
        } else {
            Some(self.composer_focus(cx))
        };

        self.project_switcher.open = true;
        self.project_switcher.ordered_project_ids = ordered;
        self.project_switcher.highlighted_project_id = self
            .project_switcher
            .ordered_project_ids
            .get(highlighted_index)
            .copied();
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
        self.project_switcher.ordered_project_ids = filtered_project_ids(
            &self.project_switcher.recent_project_ids,
            &self.state.projects,
            self.home_directory.as_deref(),
            query,
        );
        self.project_switcher.highlighted_project_id =
            self.project_switcher.ordered_project_ids.first().copied();
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
        let may_commit = pointer_selection || self.state.selected_session == original;
        let mut focus_after = previous_focus;
        // A projectless pick always commits: the collapsed row stands for a
        // fresh workspace, so "already on it" is never true — even a draft
        // bound to that project row must be moved off its spent directory.
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
            && self
                .selected_session()
                .is_some_and(|session| !session.has_started())
            && (projectless_target
                || self
                    .selected_session()
                    .is_some_and(|session| session.project_id != project_id))
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
                    .child(project.display_name()),
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
        let entries = self
            .project_switcher
            .ordered_project_ids
            .iter()
            .filter_map(|project_id| {
                self.state
                    .projects
                    .iter()
                    .find(|project| project.id == *project_id)
            })
            .map(|project| self.render_project_switcher_entry(project, cx))
            .collect::<Vec<_>>();
        let viewport_height = f32::from(window.viewport_size().height);
        // Padding below the card pushes the centered layout up by half the
        // padding, so the switcher sits slightly above center like Spotlight.
        let vertical_bias = (viewport_height * VERTICAL_BIAS).min(VERTICAL_BIAS_MAX);
        let search_height = if self.project_switcher.searching {
            44.0
        } else {
            0.0
        };
        let title = if self.project_switcher.searching
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
                    .h(px(TITLE_HEIGHT))
                    .flex_none()
                    .px(px(TITLE_INSET_X))
                    .flex()
                    .items_center()
                    .text_size(sp(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(title),
            )
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
    fn switcher_order_collapses_projectless_projects_to_a_single_entry() {
        let root = crate::projectless::workspace_root().expect("default projectless root");
        let projectless = |name, created_at| Project {
            id: Uuid::new_v4(),
            name: String::new(),
            path: root.join("2026-08-08").join(name),
            bookmark: None,
            created_at,
            temporary: false,
            starred: false,
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
        };
        let projects = vec![first.clone(), second.clone(), ordinary.clone()];

        // The most recently added projectless project keeps the one row.
        assert_eq!(
            ordered_project_ids(None, &[], &projects),
            vec![ordinary.id, second.id]
        );
        // A projectless draft's own project wins the slot instead.
        assert_eq!(
            ordered_project_ids(Some(first.id), &[second.id], &projects),
            vec![first.id, ordinary.id]
        );
    }
}
