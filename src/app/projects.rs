//! The Projects page: one surface over a project's issues and pull
//! requests, with an agent composer docked underneath. The local-repo half
//! of that surface — worktrees and branches — lives on the Settings → Git
//! page, which shares this file's per-project state, tables, and menus.
//!
//! The page is scoped to its own project selection — independent of the
//! sidebar's — and claims the main column while open, like the GitHub
//! browser it replaces. Every read is a daemon workspace operation on the
//! background executor; render only paints what has landed. Selection on the
//! Worktrees and Branches lists follows macOS list conventions so rows take
//! bulk actions from a right-click menu or the bar under the table.

use std::collections::HashSet;

use super::*;

use waku_client::{
    GitHubAvailability, PullRequestSummary, RepoBranch, RepoWorktree, WorkItemQueryState,
};

/// The two surfaces' tabs. Issues and Pull Requests read through the `gh`
/// machinery in `github.rs` and form the Projects page; Worktrees and
/// Branches read the local repo and form the Settings → Git page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProjectsTab {
    Worktrees,
    Branches,
    Issues,
    PullRequests,
}

impl ProjectsTab {
    /// The Projects page's tab strip — its ⌘⌥n chords index this list.
    pub const ALL: [Self; 2] = [Self::Issues, Self::PullRequests];

    /// The Settings → Git page's sub-tabs.
    pub const GIT_TABS: [Self; 2] = [Self::Worktrees, Self::Branches];

    /// Whether the tab is one of the Git page's local-repo lists.
    fn is_git_tab(self) -> bool {
        matches!(self, Self::Worktrees | Self::Branches)
    }

    pub fn label(self) -> String {
        match self {
            Self::Worktrees => tr!("projects.tab_worktrees"),
            Self::Branches => tr!("projects.tab_branches"),
            Self::Issues => tr!("github.tab_issues"),
            Self::PullRequests => tr!("github.tab_pull_requests"),
        }
    }

    /// The GitHub-backed tab this is, when it is one.
    fn github_tab(self) -> Option<github::GitHubTab> {
        match self {
            Self::Issues => Some(github::GitHubTab::Issues),
            Self::PullRequests => Some(github::GitHubTab::PullRequests),
            _ => None,
        }
    }
}

/// Stable identity of a selectable Worktrees/Branches row — survives
/// refetches so a selection outlives the data underneath it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum ProjectsRowKey {
    Worktree(PathBuf),
    /// `heads/<name>` for locals, `remotes/<remote>/<name>` for tracking refs.
    Branch(String),
}

impl ProjectsRowKey {
    fn branch(remote: Option<&str>, name: &str) -> Self {
        Self::Branch(match remote {
            Some(remote) => format!("remotes/{remote}/{name}"),
            None => format!("heads/{name}"),
        })
    }

    /// The row-set this key belongs to, so a selection only shows on its tab.
    fn tab(&self) -> ProjectsTab {
        match self {
            Self::Worktree(_) => ProjectsTab::Worktrees,
            Self::Branch(_) => ProjectsTab::Branches,
        }
    }
}

/// One flattened row in the Worktrees or Branches list.
#[derive(Clone, Debug, PartialEq)]
enum ProjectsListRow {
    Worktree {
        index: usize,
    },
    /// Collapsible remote group header; expanding a cold remote fetches it.
    RemoteHeader {
        remote: SharedString,
        count: usize,
        expanded: bool,
    },
    Branch {
        index: usize,
    },
}

/// Indices of the worktrees a (trimmed, lowercased) query keeps — matched
/// against the folder name, branch, and full path. An empty query keeps all.
fn filter_worktree_indices(entries: &[RepoWorktree], filter: &str) -> Vec<usize> {
    let filter = filter.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            if filter.is_empty() {
                return true;
            }
            let name = entry
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            entry
                .branch
                .iter()
                .chain(std::iter::once(&name))
                .any(|text| text.to_lowercase().contains(&filter))
                || entry
                    .path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&filter)
        })
        .map(|(index, _)| index)
        .collect()
}

/// The flattened Branches rows passing `filter`: locals first, then one
/// collapsible group per remote with `origin` leading. A query drops the
/// group headers and lists matches flat.
fn flatten_branch_rows(
    entries: &[RepoBranch],
    filter: &str,
    expanded_remotes: &HashSet<String>,
) -> Vec<ProjectsListRow> {
    let filter = filter.trim().to_lowercase();
    if !filter.is_empty() {
        return entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.name.to_lowercase().contains(&filter)
                    || entry
                        .remote
                        .as_deref()
                        .is_some_and(|remote| remote.to_lowercase().contains(&filter))
            })
            .map(|(index, _)| ProjectsListRow::Branch { index })
            .collect();
    }
    let mut rows = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.remote.is_none() {
            rows.push(ProjectsListRow::Branch { index });
        }
    }
    // Remote names in `origin`-first order, then first-seen.
    let mut remotes: Vec<String> = Vec::new();
    for entry in entries.iter() {
        if let Some(remote) = &entry.remote
            && !remotes.contains(remote)
        {
            remotes.push(remote.clone());
        }
    }
    remotes.sort_by(|a, b| (a != "origin").cmp(&(b != "origin")));
    for remote in remotes {
        let members: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.remote.as_deref() == Some(remote.as_str()))
            .map(|(index, _)| index)
            .collect();
        let expanded = expanded_remotes.contains(&remote);
        rows.push(ProjectsListRow::RemoteHeader {
            remote: SharedString::from(remote.clone()),
            count: members.len(),
            expanded,
        });
        if expanded {
            rows.extend(
                members
                    .into_iter()
                    .map(|index| ProjectsListRow::Branch { index }),
            );
        }
    }
    rows
}

/// macOS list selection applied to `selection`: plain clicks isolate the row
/// and move the anchor — or empty the selection when the row is all of it —
/// ⌘ toggles in place, ⇧ ranges from the anchor through `ordered`, the
/// tab's filtered row order.
fn apply_row_select(
    selection: &mut HashSet<ProjectsRowKey>,
    anchor: &mut Option<ProjectsRowKey>,
    ordered: &[ProjectsRowKey],
    key: ProjectsRowKey,
    modifiers: gpui::Modifiers,
) {
    if modifiers.shift {
        if let Some(anchor_key) = anchor.clone()
            && let (Some(from), Some(to)) = (
                ordered
                    .iter()
                    .position(|candidate| *candidate == anchor_key),
                ordered.iter().position(|candidate| *candidate == key),
            )
        {
            let (from, to) = (from.min(to), from.max(to));
            selection.clear();
            selection.extend(ordered[from..=to].iter().cloned());
            return;
        }
        selection.clear();
        selection.insert(key.clone());
        *anchor = Some(key);
    } else if modifiers.secondary() {
        if !selection.remove(&key) {
            selection.insert(key.clone());
        }
        *anchor = Some(key);
    } else {
        let deselect_only = selection.len() == 1 && selection.contains(&key);
        selection.clear();
        if !deselect_only {
            selection.insert(key.clone());
        }
        *anchor = Some(key);
    }
}

const PROJECTS_ROW_HEIGHT: f32 = 30.0;
const PROJECTS_COLUMN_HEADER_HEIGHT: f32 = 26.0;
const PROJECTS_HEADER_HEIGHT: f32 = 48.0;
const PROJECTS_TOOLBAR_HEIGHT: f32 = 40.0;
const PROJECTS_CONTENT_MAX_WIDTH: f32 = 900.0;
const PROJECTS_CONTENT_MARGIN: f32 = 60.0;
/// Fixed trailing column widths shared by the column header and the rows
/// beneath it, so the table's cells line up.
const PROJECTS_BRANCH_COL: f32 = 160.0;
const PROJECTS_PR_COL: f32 = 56.0;
const PROJECTS_DIVERGENCE_COL: f32 = 96.0;
const PROJECTS_COUNT_COL: f32 = 64.0;
const PROJECTS_UPDATED_COL: f32 = 76.0;
/// Scrollable padding below the last table row, so the list can scroll up
/// off the bottom edge of the window.
const PROJECTS_LIST_BOTTOM_PADDING: f32 = 78.0;

/// Per-project page state: tabs, per-tab filters, fetched tables,
/// selection, and the Projects page's docked composer. Kept in
/// `Waku::projects_page_states` by project id so toggling either surface or
/// switching projects loses nothing.
pub(super) struct ProjectsPageState {
    /// The Projects page's tab — always an Issues/Pull Requests variant.
    pub tab: ProjectsTab,
    /// The Settings → Git page's sub-tab — always a Worktrees/Branches
    /// variant, kept separate so the two surfaces never fight over `tab`.
    pub git_tab: ProjectsTab,
    worktree_filter: Entity<TextInput>,
    branch_filter: Entity<TextInput>,
    issue_filter: Entity<TextInput>,
    pr_filter: Entity<TextInput>,
    pub worktrees: github::GitHubFetch<Rc<Vec<RepoWorktree>>>,
    pub branches: github::GitHubFetch<Rc<Vec<RepoBranch>>>,
    pub selection: HashSet<ProjectsRowKey>,
    /// Anchor for shift-range selection — the last plainly clicked row.
    anchor: Option<ProjectsRowKey>,
    /// Remote groups the user opened; "origin" starts open.
    pub expanded_remotes: HashSet<String>,
    /// A fetch is in flight for this remote; the group header spins.
    fetching_remotes: HashSet<String>,
    /// Remotes already fetched this page's lifetime.
    fetched_remotes: HashSet<String>,
    pub list_state: ListState,
    pub list_scrollbar: Rc<ScrollbarState>,
    /// Lazily created per row identity, like focus handles.
    row_menus: RefCell<HashMap<ProjectsRowKey, ContextMenuHandle>>,
    row_focuses: RefCell<HashMap<ProjectsRowKey, FocusHandle>>,
    selector_menu: ContextMenuHandle,
    work_item_state_menu: ContextMenuHandle,
    /// Drag-resized widths for the Worktrees table's fixed columns
    /// (branch, changes, tasks, updated); the name column stays flexible.
    worktree_col_widths: [f32; 4],
    /// Same for the Branches table (PR, divergence, updated).
    branch_col_widths: [f32; 3],
    col_resize: Rc<column_resize::ColumnResize>,
    /// Sessions bound per worktree path, folded once per frame so row
    /// builders read a map instead of re-scanning the session list.
    session_counts: RefCell<Rc<HashMap<PathBuf, usize>>>,
    /// Open PRs keyed by head branch, folded once per frame for branch rows.
    prs_by_head: RefCell<Rc<HashMap<String, Rc<PullRequestSummary>>>>,
    /// Superseded fetch replies drop instead of overwriting newer state.
    generation: u64,
}

impl ProjectsPageState {
    fn new(window: &mut Window, cx: &mut Context<Waku>) -> Self {
        fn filter_input(
            placeholder: String,
            window: &mut Window,
            cx: &mut Context<Waku>,
        ) -> Entity<TextInput> {
            cx.new(|cx| {
                TextInput::new(window, cx)
                    .accessibility_label(placeholder.clone())
                    .placeholder(placeholder)
                    .clear_on_escape()
            })
        }
        Self {
            tab: ProjectsTab::Issues,
            git_tab: ProjectsTab::Worktrees,
            worktree_filter: filter_input(tr!("projects.filter_worktrees"), window, cx),
            branch_filter: filter_input(tr!("projects.filter_branches"), window, cx),
            issue_filter: filter_input(tr!("projects.filter_issues"), window, cx),
            pr_filter: filter_input(tr!("projects.filter_pull_requests"), window, cx),
            worktrees: github::GitHubFetch::Loading,
            branches: github::GitHubFetch::Loading,
            selection: HashSet::new(),
            anchor: None,
            expanded_remotes: HashSet::from(["origin".to_owned()]),
            fetching_remotes: HashSet::new(),
            fetched_remotes: HashSet::new(),
            list_state: ListState::new(0, ListAlignment::Top, px(PROJECTS_ROW_HEIGHT)),
            list_scrollbar: ScrollbarState::new(),
            row_menus: RefCell::new(HashMap::new()),
            row_focuses: RefCell::new(HashMap::new()),
            selector_menu: ContextMenuHandle::new(cx),
            work_item_state_menu: ContextMenuHandle::new(cx),
            worktree_col_widths: [
                PROJECTS_BRANCH_COL,
                PROJECTS_COUNT_COL,
                PROJECTS_COUNT_COL,
                PROJECTS_UPDATED_COL,
            ],
            branch_col_widths: [
                PROJECTS_PR_COL,
                PROJECTS_DIVERGENCE_COL,
                PROJECTS_UPDATED_COL,
            ],
            col_resize: column_resize::ColumnResize::new(),
            session_counts: RefCell::new(Rc::new(HashMap::new())),
            prs_by_head: RefCell::new(Rc::new(HashMap::new())),
            generation: 0,
        }
    }

    fn filter_input(&self, tab: ProjectsTab) -> &Entity<TextInput> {
        match tab {
            ProjectsTab::Worktrees => &self.worktree_filter,
            ProjectsTab::Branches => &self.branch_filter,
            ProjectsTab::Issues => &self.issue_filter,
            ProjectsTab::PullRequests => &self.pr_filter,
        }
    }

    pub(super) fn filter_text(&self, tab: ProjectsTab, cx: &App) -> String {
        self.filter_input(tab).read(cx).content().to_owned()
    }

    /// Selected keys belonging to `tab`'s row-set.
    fn selected_in(&self, tab: ProjectsTab) -> usize {
        self.selection.iter().filter(|key| key.tab() == tab).count()
    }
}

impl Waku {
    /// Open the page on `tab` (or the remembered tab), scoped to the project
    /// the page last showed — then the recency the ⌘⇧P switcher cycles.
    pub(super) fn open_projects_page(
        &mut self,
        tab: Option<ProjectsTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Experimental — the page only opens while its opt-in is on.
        if !self.state.projects_page_enabled {
            return;
        }
        let current = self
            .projects_page
            .filter(|id| self.state.projects.iter().any(|project| project.id == *id));
        let last = || {
            self.last_projects_page_project.filter(|id| {
                self.state
                    .projects
                    .iter()
                    .any(|project| project.id == *id && !project.is_projectless())
            })
        };
        let recent = || {
            self.task_switcher
                .recent_project_ids(&self.state.sessions)
                .into_iter()
                .find(|id| {
                    self.state
                        .projects
                        .iter()
                        .any(|project| project.id == *id && !project.is_projectless())
                })
        };
        let selected = || {
            self.state.selected_project.filter(|id| {
                self.state
                    .projects
                    .iter()
                    .any(|project| project.id == *id && !project.is_projectless())
            })
        };
        let first = || {
            self.state
                .projects
                .iter()
                .find(|project| !project.is_projectless())
                .map(|project| project.id)
        };
        let Some(project_id) = current
            .or_else(last)
            .or_else(recent)
            .or_else(selected)
            .or_else(first)
        else {
            return;
        };
        self.session_navigation.visit(
            self.navigation_location(),
            NavigationLocation::ProjectsPage(project_id),
        );
        self.show_projects_page(project_id, window, cx);
        if let Some(tab) = tab {
            let set_tab = self
                .projects_page_states
                .get_mut(&project_id)
                .is_some_and(|state| state.tab != tab);
            if set_tab {
                self.set_projects_tab(project_id, tab, window, cx);
            }
        }
    }

    /// Put the page on screen scoped to `project_id` — shared by open,
    /// back/forward restores, and in-page project switches. Recording the
    /// move is the caller's job: open and switch `visit`, restores
    /// `go_back`/`go_forward`.
    pub(super) fn show_projects_page(
        &mut self,
        project_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Experimental — a back/forward entry recorded while the opt-in was
        // on must not reopen the page after it is turned off.
        if !self.state.projects_page_enabled {
            return;
        }
        self.settings_page = None;
        self.notifications.open = false;
        // The page claims the main area — a selected terminal gives way and
        // the Terminals group folds, same as picking a chat does. A selected
        // task gives way too: park the transcript's draft, panel, and scroll
        // position the same way a terminal takeover does, and drop any
        // activation still in flight so it cannot hand the area back.
        self.selected_terminal = None;
        if self.state.selected_session.is_some() {
            self.capture_and_save_current_composer_draft(cx);
            self.store_selected_right_panel_state();
            self.store_transcript_scroll_position();
            self.state.selected_session = None;
            self.save();
        }
        self.pending_session_activation = None;
        if self
            .sidebar_collapsed_groups
            .insert(SidebarGroup::Terminals)
        {
            self.sidebar_rows_fingerprint.set(None);
        }
        // The docked composer answers to the page's draft — activation
        // clears `projects_page`, so the page marker lands after the bind.
        self.bind_projects_page_draft(project_id, cx);
        self.projects_page = Some(project_id);
        self.last_projects_page_project = Some(project_id);
        self.projects_ensure_state(project_id, window, cx);
        self.projects_refresh(project_id, cx);
        self.focus_projects_filter(window, cx);
        cx.notify();
    }

    /// Point the open page at another project — recorded like a task switch,
    /// so back returns to the project the page just left.
    pub(super) fn switch_projects_page_project(
        &mut self,
        project_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.projects_page == Some(project_id) {
            return;
        }
        self.session_navigation.visit(
            self.navigation_location(),
            NavigationLocation::ProjectsPage(project_id),
        );
        self.show_projects_page(project_id, window, cx);
    }

    pub(super) fn close_projects_page(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self.projects_page.take() else {
            return;
        };
        // Closing the page is a location change too: the transcript it was
        // covering comes back, and back returns to the page.
        if let Some(location) = self.navigation_location() {
            self.session_navigation
                .visit(Some(NavigationLocation::ProjectsPage(project_id)), location);
        }
        cx.notify();
    }

    /// ⌘⇧P: closed → open the page; open → start (or advance) the
    /// recent-project overlay, which commits on modifier release like the
    /// ⌘N draft switcher.
    pub(super) fn toggle_projects_page_action(
        &mut self,
        _: &ToggleProjectsPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.projects_page.is_none() {
            self.open_projects_page(None, window, cx);
        } else {
            self.cycle_page_project_switcher(false, window, cx);
        }
    }

    /// ⌘⌥1–2: switch the open page's tab, or open the page straight onto it.
    pub(super) fn select_projects_tab_action(
        &mut self,
        action: &SelectProjectsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = ProjectsTab::ALL.get(action.index).copied() else {
            return;
        };
        match self.projects_page {
            None => self.open_projects_page(Some(tab), window, cx),
            Some(project_id) => self.set_projects_tab(project_id, tab, window, cx),
        }
    }

    /// ⌘A selects every filtered row on the surface's active
    /// Worktrees/Branches tab; inside a filter field the TextInput context
    /// still wins and the chord selects its text.
    pub(super) fn select_all_projects_rows_action(
        &mut self,
        _: &SelectAllProjectsRows,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.projects_page else {
            return;
        };
        let Some(tab) = self
            .projects_page_states
            .get(&project_id)
            .map(|state| state.tab)
        else {
            return;
        };
        self.select_all_page_rows(project_id, tab, cx);
    }

    /// The Settings → Git page's ⌘A — the same selection, on `git_tab`.
    fn select_all_git_rows_action(
        &mut self,
        _: &SelectAllProjectsRows,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.settings_git_project else {
            return;
        };
        let Some(tab) = self
            .projects_page_states
            .get(&project_id)
            .map(|state| state.git_tab)
        else {
            return;
        };
        self.select_all_page_rows(project_id, tab, cx);
    }

    fn select_all_page_rows(&mut self, project_id: Uuid, tab: ProjectsTab, cx: &mut Context<Self>) {
        let keys: Vec<ProjectsRowKey> = self
            .projects_list_rows(project_id, tab, cx)
            .iter()
            .filter_map(|row| self.projects_row_key(project_id, row, cx))
            .collect();
        if let Some(state) = self.projects_page_states.get_mut(&project_id) {
            state.selection.extend(keys);
        }
        cx.notify();
    }

    /// ⌘F on the page puts focus in the active tab's filter.
    pub(super) fn focus_projects_filter_action(
        &mut self,
        _: &FocusProjectsFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.projects_page else {
            return;
        };
        let Some(tab) = self
            .projects_page_states
            .get(&project_id)
            .map(|state| state.tab)
        else {
            return;
        };
        self.focus_tab_filter(project_id, tab, window, cx);
    }

    /// The Settings → Git page's ⌘F — its `git_tab`'s filter.
    fn focus_git_filter_action(
        &mut self,
        _: &FocusProjectsFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.settings_git_project else {
            return;
        };
        let Some(tab) = self
            .projects_page_states
            .get(&project_id)
            .map(|state| state.git_tab)
        else {
            return;
        };
        self.focus_tab_filter(project_id, tab, window, cx);
    }

    /// Escape on the page peels one layer at a time: the selection, then the
    /// page itself. An emptied filter's second Escape reaches here through
    /// `clear_on_escape`; an open work-item detail is right-panel chrome and
    /// handles its own Escape.
    pub(super) fn dismiss_projects_layer_action(
        &mut self,
        _: &DismissProjectsLayer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.projects_page else {
            return;
        };
        let cleared = self
            .projects_page_states
            .get_mut(&project_id)
            .is_some_and(|state| {
                let tab = state.tab;
                let before = state.selection.len();
                state.selection.retain(|key| key.tab() != tab);
                if state.selection.len() == before {
                    false
                } else {
                    state.anchor = None;
                    true
                }
            });
        if cleared {
            cx.notify();
            return;
        }
        self.close_projects_page(cx);
        if self.selected_session().is_some() {
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
        }
    }

    /// Escape on the Settings → Git page peels the active sub-tab's
    /// selection; there is no page layer underneath to close — Settings
    /// handles its own exit.
    fn dismiss_git_layer_action(
        &mut self,
        _: &DismissProjectsLayer,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.settings_git_project else {
            return;
        };
        let Some(state) = self.projects_page_states.get_mut(&project_id) else {
            return;
        };
        let tab = state.git_tab;
        let before = state.selection.len();
        state.selection.retain(|key| key.tab() != tab);
        if state.selection.len() != before {
            state.anchor = None;
            cx.notify();
        }
    }

    fn focus_projects_filter(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project_id) = self.projects_page else {
            return;
        };
        let Some(tab) = self
            .projects_page_states
            .get(&project_id)
            .map(|state| state.tab)
        else {
            return;
        };
        self.focus_tab_filter(project_id, tab, window, cx);
    }

    /// Focus `tab`'s filter field on `project_id`'s page state — shared by
    /// the Projects page (`state.tab`) and the Settings → Git page
    /// (`state.git_tab`).
    fn focus_tab_filter(
        &self,
        project_id: Uuid,
        tab: ProjectsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return;
        };
        let focus = state.filter_input(tab).read(cx).focus();
        window.focus(&focus, cx);
    }

    pub(super) fn projects_ensure_state(
        &mut self,
        project_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.projects_page_states
            .entry(project_id)
            .or_insert_with(|| ProjectsPageState::new(window, cx));
        self.github_browsers
            .entry(project_id)
            .or_insert_with(|| github::GitHubBrowser::new(project_id, window, cx));
    }

    /// Switch the page's tab, focusing its filter — a tab's filter is where
    /// the keyboard lands.
    fn set_projects_tab(
        &mut self,
        project_id: Uuid,
        tab: ProjectsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self
            .projects_page_states
            .get_mut(&project_id)
            .is_some_and(|state| {
                if state.tab == tab {
                    return false;
                }
                state.tab = tab;
                state
                    .list_state
                    .reset_with_uniform_height(0, px(PROJECTS_ROW_HEIGHT));
                true
            });
        if !changed {
            return;
        }
        self.projects_refresh(project_id, cx);
        self.focus_projects_filter(window, cx);
        cx.notify();
    }

    /// The Settings → Git page's sub-tab switch — the same shape as
    /// `set_projects_tab`, scoped to `git_tab`.
    fn set_git_tab(
        &mut self,
        project_id: Uuid,
        tab: ProjectsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !tab.is_git_tab() {
            return;
        }
        let changed = self
            .projects_page_states
            .get_mut(&project_id)
            .is_some_and(|state| {
                if state.git_tab == tab {
                    return false;
                }
                state.git_tab = tab;
                state
                    .list_state
                    .reset_with_uniform_height(0, px(PROJECTS_ROW_HEIGHT));
                true
            });
        if !changed {
            return;
        }
        self.projects_refresh(project_id, cx);
        self.focus_tab_filter(project_id, tab, window, cx);
        cx.notify();
    }

    /// The project whose repo the Settings → Git page shows: the page's own
    /// selection, then the Projects page's last project, then the sidebar's,
    /// then the first real project.
    fn resolve_git_settings_project(&mut self) -> Option<Uuid> {
        let valid = |id: &Uuid| {
            self.state
                .projects
                .iter()
                .any(|project| project.id == *id && !project.is_projectless())
        };
        let resolved = self
            .settings_git_project
            .filter(&valid)
            .or_else(|| self.last_projects_page_project.filter(&valid))
            .or_else(|| self.state.selected_project.filter(&valid))
            .or_else(|| {
                self.state
                    .projects
                    .iter()
                    .find(|project| !project.is_projectless())
                    .map(|project| project.id)
            });
        self.settings_git_project = resolved;
        resolved
    }

    /// Point the Settings → Git page at another project; the fetch runs on
    /// the next render once the per-project state exists.
    fn select_git_settings_project(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        if self.settings_git_project == Some(project_id) {
            return;
        }
        self.settings_git_project = Some(project_id);
        self.git_page_refresh_pending = true;
        cx.notify();
    }

    /// Refetch the page's tables for `project_id`: local git data and the
    /// GitHub lists (which also resolve the repo for the tab gating).
    pub(super) fn projects_refresh(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };
        // An offline remote keeps its last-loaded rows rather than flipping
        // to a spinner that can never resolve.
        let Some(workspace) = self.workspace_client_for_project(project_id) else {
            return;
        };
        let Some(state) = self.projects_page_states.get_mut(&project_id) else {
            return;
        };
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        // Refresh keeps showing the rows it is about to replace; the loader
        // is for the first pass only.
        if !matches!(state.worktrees, github::GitHubFetch::Loaded(Some(_))) {
            state.worktrees = github::GitHubFetch::Loading;
        }
        if !matches!(state.branches, github::GitHubFetch::Loaded(Some(_))) {
            state.branches = github::GitHubFetch::Loading;
        }
        cx.notify();
        cx.spawn(async move |waku, cx| {
            let resolved = cx
                .background_executor()
                .spawn(async move {
                    let worktrees = workspace
                        .request(waku_client::WorkspaceOperation::ListWorktrees {
                            cwd: cwd.clone(),
                        })
                        .ok()
                        .and_then(|result| match result {
                            waku_client::WorkspaceResult::RepoWorktrees { entries } => entries,
                            _ => None,
                        });
                    let branches = workspace
                        .request(waku_client::WorkspaceOperation::ListRepoBranches { cwd })
                        .ok()
                        .and_then(|result| match result {
                            waku_client::WorkspaceResult::RepoBranches { entries } => entries,
                            _ => None,
                        });
                    (worktrees, branches)
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let Some(state) = waku.projects_page_states.get_mut(&project_id) else {
                    return;
                };
                if state.generation != generation {
                    return;
                }
                let (worktrees, branches) = resolved;
                state.worktrees = github::GitHubFetch::Loaded(worktrees.map(Rc::new));
                state.branches = github::GitHubFetch::Loaded(branches.map(Rc::new));
                cx.notify();
            });
        })
        .detach();

        self.github_refresh(project_id, cx);
    }

    /// `tab`'s flattened Worktrees/Branches rows — the GitHub tabs have no
    /// row list and return empty.
    fn projects_list_rows(
        &self,
        project_id: Uuid,
        tab: ProjectsTab,
        cx: &App,
    ) -> Vec<ProjectsListRow> {
        if !self.projects_page_states.contains_key(&project_id) {
            return Vec::new();
        }
        match tab {
            ProjectsTab::Worktrees => self
                .projects_filtered_worktrees(project_id, cx)
                .map(|(_, indices)| {
                    indices
                        .into_iter()
                        .map(|index| ProjectsListRow::Worktree { index })
                        .collect()
                })
                .unwrap_or_default(),
            ProjectsTab::Branches => self
                .projects_branch_rows(project_id, cx)
                .map(|(_, rows)| rows)
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The worktree rows passing the tab's filter, as indices into the
    /// fetched list.
    fn projects_filtered_worktrees(
        &self,
        project_id: Uuid,
        cx: &App,
    ) -> Option<(Rc<Vec<RepoWorktree>>, Vec<usize>)> {
        let state = self.projects_page_states.get(&project_id)?;
        let entries = match &state.worktrees {
            github::GitHubFetch::Loaded(Some(entries)) => entries.clone(),
            _ => return None,
        };
        let filter = state.filter_text(ProjectsTab::Worktrees, cx);
        let indices = filter_worktree_indices(&entries, &filter);
        Some((entries, indices))
    }

    /// The flattened Branches rows passing the tab's filter: locals first,
    /// then one collapsible group per remote with `origin` leading. A query
    /// drops the group headers and lists matches flat.
    fn projects_branch_rows(
        &self,
        project_id: Uuid,
        cx: &App,
    ) -> Option<(Rc<Vec<RepoBranch>>, Vec<ProjectsListRow>)> {
        let state = self.projects_page_states.get(&project_id)?;
        let entries = match &state.branches {
            github::GitHubFetch::Loaded(Some(entries)) => entries.clone(),
            _ => return None,
        };
        let filter = state.filter_text(ProjectsTab::Branches, cx);
        let rows = flatten_branch_rows(&entries, &filter, &state.expanded_remotes);
        Some((entries, rows))
    }

    /// A row's selection key within the current tab's row-set.
    fn projects_row_key(
        &self,
        project_id: Uuid,
        row: &ProjectsListRow,
        _cx: &App,
    ) -> Option<ProjectsRowKey> {
        let state = self.projects_page_states.get(&project_id)?;
        match row {
            ProjectsListRow::Worktree { index } => match &state.worktrees {
                github::GitHubFetch::Loaded(Some(entries)) => entries
                    .get(*index)
                    .map(|entry| ProjectsRowKey::Worktree(entry.path.clone())),
                _ => None,
            },
            ProjectsListRow::Branch { index } => match &state.branches {
                github::GitHubFetch::Loaded(Some(entries)) => entries
                    .get(*index)
                    .map(|entry| ProjectsRowKey::branch(entry.remote.as_deref(), &entry.name)),
                _ => None,
            },
            ProjectsListRow::RemoteHeader { .. } => None,
        }
    }

    /// Click selection on a Worktrees/Branches row: plain selects just the
    /// row and moves the anchor, ⌘ toggles it in place, ⇧ ranges from the
    /// anchor through the filtered order. The key's own row-set supplies the
    /// order — the same row may render on either surface.
    fn projects_row_select(
        &mut self,
        project_id: Uuid,
        key: ProjectsRowKey,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let ordered: Vec<ProjectsRowKey> = self
            .projects_list_rows(project_id, key.tab(), cx)
            .iter()
            .filter_map(|row| self.projects_row_key(project_id, row, cx))
            .collect();
        let Some(state) = self.projects_page_states.get_mut(&project_id) else {
            return;
        };
        apply_row_select(
            &mut state.selection,
            &mut state.anchor,
            &ordered,
            key,
            modifiers,
        );
        cx.notify();
    }

    /// Right-clicking an unselected row narrows the selection to it first —
    /// the Finder convention — while a selected row keeps the selection so
    /// the menu acts on all of them. Returns the keys the menu should carry.
    fn projects_menu_target(
        &mut self,
        project_id: Uuid,
        key: &ProjectsRowKey,
        cx: &mut Context<Self>,
    ) -> Vec<ProjectsRowKey> {
        let Some(state) = self.projects_page_states.get_mut(&project_id) else {
            return Vec::new();
        };
        if !state.selection.contains(key) {
            state.selection.clear();
            state.selection.insert(key.clone());
            state.anchor = Some(key.clone());
            cx.notify();
        }
        let tab = key.tab();
        state
            .selection
            .iter()
            .filter(|selected| selected.tab() == tab)
            .cloned()
            .collect()
    }

    // ----- actions shared by menus, the bulk bar, and the composer -----

    /// A draft bound to an existing worktree — the task owns it from there.
    /// Reachable from the Settings → Git page too, so leave settings before
    /// landing on the draft.
    fn projects_new_task_in_worktree(
        &mut self,
        project_id: Uuid,
        path: PathBuf,
        name: String,
        branch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        self.close_projects_page(cx);
        self.bind_new_draft_to_worktree(
            project_id,
            SessionWorkspace::Worktree {
                path,
                name,
                branch,
                base_branch: None,
            },
            window,
            cx,
        );
    }

    /// A draft whose worktree materializes from `base_ref` at first submit.
    /// Reachable from the Settings → Git page too, so leave settings before
    /// landing on the draft.
    fn projects_new_task_on_branch(
        &mut self,
        project_id: Uuid,
        base_ref: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_page = None;
        self.close_projects_page(cx);
        self.create_session_for(project_id, self.state.last_provider, cx);
        self.select_workspace(
            SessionWorkspace::NewWorktree {
                base_branch: Some(base_ref),
            },
            cx,
        );
        let focus = self.composer_focus(cx);
        window.focus(&focus, cx);
    }

    /// `git worktree add` detached at `base_ref`; the list refreshes when it
    /// lands.
    fn projects_create_worktree(
        &mut self,
        project_id: Uuid,
        base_ref: String,
        cx: &mut Context<Self>,
    ) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };
        let Some(workspace) = self.workspace_client_for_project(project_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        let sync_default_branch = self.state.new_worktree_sync_default_branch;
        let sync_branches = self.state.new_worktree_sync_branches.clone();
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    workspace.request(waku_client::WorkspaceOperation::CreateWorktree {
                        project_path: cwd,
                        name: None,
                        base_ref: Some(base_ref),
                        sync_default_branch,
                        sync_branches,
                    })
                })
                .await;
            let _ = waku.update(cx, |waku, cx| match result {
                Ok(_) => waku.projects_refresh(project_id, cx),
                Err(error) => {
                    waku.show_toast(tr!("errors.create_worktree", error = error.to_string()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Remove worktrees after a native confirmation; Git still refuses dirty
    /// ones without force, and per-path failures toast.
    fn projects_remove_worktrees(
        &mut self,
        project_id: Uuid,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = if paths.len() == 1 {
            tr!(
                "projects.confirm_remove_one",
                name = paths[0].display().to_string()
            )
        } else {
            tr!("projects.confirm_remove_many", count = paths.len())
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &message,
            Some(&tr!("projects.confirm_remove_detail")),
            &[
                gpui::PromptButton::cancel(tr!("common.cancel")),
                gpui::PromptButton::ok(tr!("common.remove")),
            ],
            cx,
        );
        cx.spawn(async move |waku, cx| {
            if answer.await.ok() != Some(1) {
                return;
            }
            let _ = waku.update(cx, |waku, cx| {
                let workspace = waku_client::WorkspaceClient::new(waku.daemon.client());
                cx.spawn(async move |waku, cx| {
                    let failures = cx
                        .background_executor()
                        .spawn(async move {
                            let mut failures = Vec::new();
                            for path in paths {
                                if let Err(error) = workspace.request(
                                    waku_client::WorkspaceOperation::RemoveWorktree {
                                        path: path.clone(),
                                        force: false,
                                    },
                                ) {
                                    failures.push(format!("{}: {error}", path.display()));
                                }
                            }
                            failures
                        })
                        .await;
                    let _ = waku.update(cx, |waku, cx| {
                        if !failures.is_empty() {
                            waku.show_toast(tr!(
                                "projects.remove_failed",
                                error = failures.join("; ")
                            ));
                        }
                        if let Some(state) = waku.projects_page_states.get_mut(&project_id) {
                            state.selection.clear();
                            state.anchor = None;
                        }
                        waku.projects_refresh(project_id, cx);
                    });
                })
                .detach();
            });
        })
        .detach();
    }

    fn projects_prune_worktrees(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };
        let Some(workspace) = self.workspace_client_for_project(project_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        cx.spawn(async move |waku, cx| {
            let _ = cx
                .background_executor()
                .spawn(async move {
                    workspace.request(waku_client::WorkspaceOperation::PruneWorktrees { cwd })
                })
                .await;
            let _ = waku.update(cx, |waku, cx| waku.projects_refresh(project_id, cx));
        })
        .detach();
    }

    /// Delete local branches after a native confirmation; Git refuses
    /// branches checked out in a worktree or unmerged, and per-branch
    /// failures toast.
    fn projects_delete_branches(
        &mut self,
        project_id: Uuid,
        names: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };
        let message = if names.len() == 1 {
            tr!("projects.confirm_delete_one", name = names[0].clone())
        } else {
            tr!("projects.confirm_delete_many", count = names.len())
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &message,
            Some(&tr!("projects.confirm_delete_detail")),
            &[
                gpui::PromptButton::cancel(tr!("common.cancel")),
                gpui::PromptButton::ok(tr!("common.delete")),
            ],
            cx,
        );
        cx.spawn(async move |waku, cx| {
            if answer.await.ok() != Some(1) {
                return;
            }
            let _ = waku.update(cx, |waku, cx| {
                let workspace = waku_client::WorkspaceClient::new(waku.daemon.client());
                cx.spawn(async move |waku, cx| {
                    let failures = cx
                        .background_executor()
                        .spawn(async move {
                            match workspace.request(
                                waku_client::WorkspaceOperation::DeleteBranches {
                                    cwd,
                                    names,
                                    force: false,
                                },
                            ) {
                                Ok(waku_client::WorkspaceResult::BranchDeletions { failures }) => {
                                    failures
                                        .into_iter()
                                        .map(|failure| {
                                            format!("{}: {}", failure.name, failure.error)
                                        })
                                        .collect::<Vec<_>>()
                                }
                                Ok(_) => Vec::new(),
                                Err(error) => vec![error.to_string()],
                            }
                        })
                        .await;
                    let _ = waku.update(cx, |waku, cx| {
                        if !failures.is_empty() {
                            waku.show_toast(tr!(
                                "projects.delete_failed",
                                error = failures.join("; ")
                            ));
                        }
                        if let Some(state) = waku.projects_page_states.get_mut(&project_id) {
                            state.selection.clear();
                            state.anchor = None;
                        }
                        waku.projects_refresh(project_id, cx);
                    });
                })
                .detach();
            });
        })
        .detach();
    }

    /// Expanding a remote group fetches it once — remote-tracking refs
    /// render from local state instantly; the fetch freshens them.
    fn projects_fetch_remote(&mut self, project_id: Uuid, remote: String, cx: &mut Context<Self>) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };
        let Some(workspace) = self.workspace_client_for_project(project_id) else {
            self.show_toast(tr!("errors.daemon_disconnected"));
            cx.notify();
            return;
        };
        let Some(state) = self.projects_page_states.get_mut(&project_id) else {
            return;
        };
        if !state.fetched_remotes.insert(remote.clone()) {
            return;
        }
        state.fetching_remotes.insert(remote.clone());
        cx.notify();
        let fetch_remote = remote.clone();
        cx.spawn(async move |waku, cx| {
            let _ = cx
                .background_executor()
                .spawn(async move {
                    workspace.request(waku_client::WorkspaceOperation::FetchRemote {
                        cwd,
                        remote: fetch_remote,
                    })
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                if let Some(state) = waku.projects_page_states.get_mut(&project_id) {
                    state.fetching_remotes.remove(&remote);
                }
                waku.projects_refresh(project_id, cx);
            });
        })
        .detach();
    }

    /// Submit the page's composer: a normal task on the page's project whose
    /// prompt carries the visible context — project, tab, and filter.
    pub(super) fn projects_submit(&mut self, prompt: &str, cx: &mut Context<Self>) {
        if self.projects_page.is_none() {
            return;
        }
        let Some(submission) = self.submission_with_attachments(prompt, cx) else {
            return;
        };
        // Enter and steer already cleared the field; the send-button path
        // needs it done here.
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.submit_projects_page_submission(submission, prompt, cx);
    }

    pub(super) fn submit_projects_page_submission(
        &mut self,
        mut submission: ComposerSubmission,
        typed: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.projects_page else {
            return;
        };
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return;
        };
        let project = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id);
        let Some(project) = project else {
            return;
        };
        let project_name = project.display_name();
        let project_path = project.path.clone();
        let tab = state.tab;
        let filter = state.filter_text(tab, cx);
        let filter = filter.trim().to_owned();

        let mut context = format!("- Project: {project_name} ({})", project_path.display());
        context.push_str(&format!("\n- Tab: {}", tab.label()));
        if !filter.is_empty() {
            context.push_str(&format!("\n- Filter: {filter}"));
        }

        // The stored prompt carries the context block; the bubble keeps the
        // user's own words with the context readable in the same turn.
        let typed = typed.trim();
        submission.prompt = format!(
            "Context for this request:\n{context}\n\n{}",
            submission.prompt
        );
        let display_body = if typed.is_empty() {
            submission.display_content.clone().unwrap_or_default()
        } else {
            typed.to_owned()
        };
        submission.display_content = Some(format!("{context}\n\n{display_body}"));

        self.close_projects_page(cx);
        self.create_session_for(project_id, self.state.last_provider, cx);
        let Some(session_id) = self.state.selected_session else {
            return;
        };
        self.submit_composer_submission_to(session_id, submission, cx);
    }
}

impl Waku {
    // ----- rendering -----

    /// The Issues/Pull Requests tabs stay enabled while the repo is
    /// unresolved or `gh` itself is the problem — those hint states are how
    /// a user fixes it — and disable once `gh` answers "not a GitHub repo".
    fn projects_github_enabled(&self, project_id: Uuid) -> bool {
        self.state.github_enabled
            && !matches!(
            self.github_browsers
                .get(&project_id)
                .and_then(|browser| browser.repo.as_ref()),
            Some((None, GitHubAvailability::Ready))
        )
    }

    pub(super) fn render_projects_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(project_id) = self.projects_page else {
            return div().into_any_element();
        };
        self.projects_ensure_state(project_id, window, cx);
        let github_enabled = self.projects_github_enabled(project_id);
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        // The page's tabs are the GitHub pair; a non-GitHub repo disables
        // them but the tab still shows its hint, and a git sub-tab held over
        // from shared state falls back to Issues.
        let tab = if state.tab.is_git_tab() {
            ProjectsTab::Issues
        } else {
            state.tab
        };
        let missing = self.missing_projects.contains(&project_id);
        let content = if missing {
            self.render_projects_missing(project_id, cx)
        } else {
            match tab {
                ProjectsTab::Worktrees | ProjectsTab::Branches => {
                    self.render_projects_table(project_id, tab, window, cx)
                }
                tab => {
                    let github_tab = tab.github_tab().unwrap_or(github::GitHubTab::PullRequests);
                    let filter = self
                        .projects_page_states
                        .get(&project_id)
                        .map(|state| state.filter_text(tab, cx))
                        .unwrap_or_default();
                    self.render_github_list(project_id, github_tab, &filter, window, cx)
                }
            }
        };

        div()
            .key_context("ProjectsPage")
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::select_all_projects_rows_action))
            .on_action(cx.listener(Self::focus_projects_filter_action))
            .on_action(cx.listener(Self::dismiss_projects_layer_action))
            .child(self.render_projects_header(project_id, tab, github_enabled, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(
                        PROJECTS_CONTENT_MAX_WIDTH + PROJECTS_CONTENT_MARGIN * 2.0
                    ))
                    .mx_auto()
                    .px(px(PROJECTS_CONTENT_MARGIN))
                    .flex()
                    .flex_col()
                    .when(!missing, |element| {
                        element.child(self.render_projects_toolbar(project_id, tab, cx))
                    })
                    .child(content)
                    .children(self.render_projects_bulk_bar(project_id, tab, cx))
                    .child(self.render_projects_composer(project_id, window, cx)),
            )
            .into_any_element()
    }

    /// The page's missing-folder state: what the reconciliation pass found,
    /// the stale path it recorded, and the picker that repoints the project.
    fn render_projects_missing(&mut self, project_id: Uuid, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let path = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| {
                settings::abbreviate_home_path(&project.path, self.home_directory.as_deref())
            })
            .unwrap_or_default();
        let locate = div()
            .id("projects-locate-folder")
            .tab_index(0)
            .h(px(30.0))
            .px(px(14.0))
            .rounded_full()
            .flex()
            .items_center()
            .cursor_default()
            .bg(theme.inverse)
            .text_color(theme.on_inverse)
            .text_size(sp(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
            .hover(|element| element.opacity(0.9))
            .active(|element| element.opacity(0.8))
            .child(tr!("project.locate"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.relocate_project(project_id, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.relocate_project(project_id, cx);
                    cx.stop_propagation();
                }
            }));
        div()
            .flex_1()
            .min_h_0()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .child(icon("icons/folder.svg", 16.0, theme.text_tertiary))
            .child(
                div()
                    .text_size(sp(13.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("project.folder_missing")),
            )
            .child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_tertiary)
                    .child(path),
            )
            .child(locate)
            .into_any_element()
    }

    /// The page's top line: the project selector left of the tab strip.
    fn render_projects_header(
        &mut self,
        project_id: Uuid,
        tab: ProjectsTab,
        github_enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let project_name = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.display_name())
            .unwrap_or_else(|| tr!("sidebar.unknown_project"));

        let selector_menu = self
            .projects_page_states
            .get(&project_id)
            .map(|state| state.selector_menu.clone())
            .unwrap_or_else(|| ContextMenuHandle::new(cx));
        let selector_open = selector_menu.is_open();
        let weak = cx.entity().downgrade();
        let selector = dropdown_menu(
            div()
                .id("projects-selector")
                .h(px(26.0))
                .pl(px(9.0))
                .pr(px(7.0))
                .rounded(px(7.0))
                .flex()
                .items_center()
                .gap(px(5.0))
                .cursor_default()
                .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                .when(selector_open, |element| element.bg(theme.overlay_strong))
                .hover(|style| style.bg(theme.overlay))
                .child(
                    div()
                        .text_size(sp(16.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .max_w(px(240.0))
                        .truncate()
                        .child(project_name),
                )
                .child(icon("icons/chevron-down.svg", 11.0, theme.text_tertiary)),
            "projects-selector-menu",
            &selector_menu,
            MenuAlign::BelowLeft,
            move |cx| {
                let items = weak
                    .update(cx, |this, _| {
                        this.state
                            .projects
                            .iter()
                            .filter(|project| !project.is_projectless())
                            .map(|project| (project.id, project.display_name()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                items
                    .into_iter()
                    .map(|(id, label)| {
                        let weak = weak.clone();
                        MenuItem::new(label, move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.switch_projects_page_project(id, window, cx);
                            });
                        })
                        .selected(id == project_id)
                    })
                    .collect()
            },
        );

        div()
            .flex_none()
            .h(px(PROJECTS_HEADER_HEIGHT))
            .w_full()
            .px(px(14.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .border_b(hairline())
            .border_color(theme.separator)
            .child(selector)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .p(px(2.0))
                    .bg(theme.inset)
                    .children(ProjectsTab::ALL.into_iter().map(|candidate| {
                        let enabled = github_enabled || candidate.github_tab().is_none();
                        self.projects_tab_button(project_id, candidate, tab, enabled, false, cx)
                    })),
            )
            .child(div().flex_1())
            .into_any_element()
    }

    /// One segmented-control button. `for_git` swaps the click target to
    /// the Settings → Git page's `git_tab`; the Projects page drives `tab`.
    fn projects_tab_button(
        &mut self,
        project_id: Uuid,
        candidate: ProjectsTab,
        current: ProjectsTab,
        enabled: bool,
        for_git: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let selected = candidate == current;
        let id_prefix = if for_git { "git-settings" } else { "projects" };
        div()
            .id(SharedString::from(format!("{id_prefix}-tab-{candidate:?}")))
            .h(px(20.0))
            .px(px(10.0))
            .rounded(px(5.0))
            .flex()
            .items_center()
            .text_size(sp(15.0))
            .when(!enabled, |element| element.text_color(theme.text_ghost))
            .when(enabled, |element| {
                let element = element
                    .cursor_default()
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent));
                if selected {
                    element.bg(theme.surface).text_color(theme.text)
                } else {
                    element
                        .text_color(theme.text_secondary)
                        .hover(|style| style.text_color(theme.text))
                }
            })
            .when(!enabled, |element| {
                element.tooltip(Tooltip::text(tr!("projects.not_a_github_repo")))
            })
            .child(candidate.label())
            .when(enabled, |element| {
                element.on_click(cx.listener(move |this, _, window, cx| {
                    if for_git {
                        this.set_git_tab(project_id, candidate, window, cx);
                    } else {
                        this.set_projects_tab(project_id, candidate, window, cx);
                    }
                }))
            })
    }

    /// The line under the header: the tab's filter field (initial focus)
    /// plus per-tab controls — the work-item state picker on the GitHub
    /// tabs, a refresh everywhere.
    fn render_projects_toolbar(
        &mut self,
        project_id: Uuid,
        tab: ProjectsTab,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        let filter = state.filter_input(tab).clone();

        // Open/closed/all for the GitHub tabs, applied server-side.
        let state_picker = tab.github_tab().map(|_| {
            let query_state = self
                .github_browsers
                .get(&project_id)
                .map(|browser| browser.query_state)
                .unwrap_or(WorkItemQueryState::Open);
            let state_label = match query_state {
                WorkItemQueryState::Open => tr!("github.state_open"),
                WorkItemQueryState::Closed => tr!("github.state_closed"),
                WorkItemQueryState::All => tr!("github.state_all"),
            };
            let menu = self
                .projects_page_states
                .get(&project_id)
                .map(|state| state.work_item_state_menu.clone())
                .unwrap_or_else(|| ContextMenuHandle::new(cx));
            let menu_open = menu.is_open();
            let weak = cx.entity().downgrade();
            dropdown_menu(
                div()
                    .id("projects-state-picker")
                    .h(px(24.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .cursor_default()
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .when(menu_open, |element| element.bg(theme.overlay_strong))
                    .hover(|style| style.bg(theme.overlay))
                    .child(
                        div()
                            .text_size(sp(15.0))
                            .text_color(theme.text_secondary)
                            .child(state_label),
                    )
                    .child(icon("icons/chevron-down.svg", 11.0, theme.text_tertiary)),
                "projects-state-menu",
                &menu,
                MenuAlign::BelowLeft,
                move |_| {
                    [
                        (WorkItemQueryState::Open, tr!("github.state_open")),
                        (WorkItemQueryState::Closed, tr!("github.state_closed")),
                        (WorkItemQueryState::All, tr!("github.state_all")),
                    ]
                    .into_iter()
                    .map(|(state, label)| {
                        let weak = weak.clone();
                        MenuItem::new(label, move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.github_set_query_state(project_id, state, cx);
                            });
                        })
                        .selected(state == query_state)
                    })
                    .collect()
                },
            )
            .into_any_element()
        });

        div()
            .flex_none()
            .h(px(PROJECTS_TOOLBAR_HEIGHT))
            .w_full()
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .border_b(hairline())
            .border_color(theme.separator)
            .child(
                TextField::new("projects-filter", filter)
                    .icon("icons/search.svg", 12.0)
                    .w(px(220.0))
                    .flex_none(),
            )
            .child(div().flex_1())
            .children(state_picker)
            .child(
                div()
                    .id("projects-refresh")
                    .w(px(24.0))
                    .h(px(24.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .hover(|style| style.bg(theme.overlay))
                    .active(|style| style.bg(theme.overlay_strong))
                    .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                    .tooltip(Tooltip::text(tr!("github.refresh")))
                    .child(icon("icons/rotate-cw.svg", 13.0, theme.text_secondary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.projects_refresh(project_id, cx);
                    })),
            )
            .into_any_element()
    }

    /// A Worktrees/Branches table, virtualized over `tab`'s flattened
    /// rows — shared by the Projects page's former tabs and the Settings →
    /// Git page that hosts them now. The two per-frame folds — sessions
    /// bound per worktree and open PRs per head branch — land here so row
    /// builders read maps.
    fn render_projects_table(
        &mut self,
        project_id: Uuid,
        tab: ProjectsTab,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let (loading, missing) = match self.projects_page_states.get(&project_id) {
            Some(state) => match tab {
                ProjectsTab::Worktrees => (
                    matches!(state.worktrees, github::GitHubFetch::Loading),
                    matches!(state.worktrees, github::GitHubFetch::Loaded(None)),
                ),
                ProjectsTab::Branches => (
                    matches!(state.branches, github::GitHubFetch::Loading),
                    matches!(state.branches, github::GitHubFetch::Loaded(None)),
                ),
                _ => return div().into_any_element(),
            },
            None => return div().into_any_element(),
        };
        if loading {
            return github::github_centered(
                icon("icons/loader-circle.svg", 16.0, theme.text_tertiary).into_any_element(),
                tr!("github.loading"),
                &theme,
            );
        }
        if missing {
            return github::github_centered(
                icon("icons/alert.svg", 16.0, theme.text_tertiary).into_any_element(),
                tr!("projects.not_a_repo"),
                &theme,
            );
        }

        // Per-frame folds for the row builders.
        let mut session_counts: HashMap<PathBuf, usize> = HashMap::new();
        for session in &self.state.sessions {
            if session.archived_at.is_some() {
                continue;
            }
            if let Some(path) = session.workspace.path() {
                *session_counts.entry(path.to_path_buf()).or_default() += 1;
            }
        }
        let prs_by_head: HashMap<String, Rc<PullRequestSummary>> = match self
            .github_browsers
            .get(&project_id)
            .map(|browser| &browser.pull_requests)
        {
            Some(github::GitHubFetch::Loaded(Some(entries))) => entries
                .iter()
                .filter_map(|pr| {
                    pr.head_branch
                        .clone()
                        .map(|head| (head, Rc::new(pr.clone())))
                })
                .collect(),
            _ => HashMap::new(),
        };
        if let Some(state) = self.projects_page_states.get(&project_id) {
            *state.session_counts.borrow_mut() = Rc::new(session_counts);
            *state.prs_by_head.borrow_mut() = Rc::new(prs_by_head);
        }

        let rows = self.projects_list_rows(project_id, tab, cx);
        if rows.is_empty() {
            let filter_empty = self
                .projects_page_states
                .get(&project_id)
                .map(|state| {
                    // Drop the stale extent so the page-level scrollbar
                    // can't phantom over the empty state.
                    state
                        .list_state
                        .reset_with_uniform_height(0, px(PROJECTS_ROW_HEIGHT));
                    state.filter_text(tab, cx).trim().is_empty()
                })
                .unwrap_or(true);
            return github::github_centered(
                icon("icons/git-branch.svg", 16.0, theme.text_tertiary).into_any_element(),
                if filter_empty {
                    match tab {
                        ProjectsTab::Worktrees => tr!("projects.no_worktrees"),
                        _ => tr!("projects.no_branches"),
                    }
                } else {
                    tr!("github.no_matches")
                },
                &theme,
            );
        }

        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        let list_state = state.list_state.clone();
        if list_state.item_count() != rows.len() {
            list_state.reset_with_uniform_height(rows.len(), px(PROJECTS_ROW_HEIGHT));
        }
        let rows = Rc::new(rows);
        let entity = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(projects_column_header(project_id, tab, state, &theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(
                        list(list_state.clone(), move |index, _window, cx| {
                            let Some(row) = rows.get(index) else {
                                return div().into_any_element();
                            };
                            let row = match row {
                                ProjectsListRow::Worktree { index } => {
                                    ProjectsListRow::Worktree { index: *index }
                                }
                                ProjectsListRow::RemoteHeader {
                                    remote,
                                    count,
                                    expanded,
                                } => ProjectsListRow::RemoteHeader {
                                    remote: remote.clone(),
                                    count: *count,
                                    expanded: *expanded,
                                },
                                ProjectsListRow::Branch { index } => {
                                    ProjectsListRow::Branch { index: *index }
                                }
                            };
                            entity
                                .upgrade()
                                .map(|entity| {
                                    entity.update(cx, |this, cx| {
                                        this.render_projects_row(project_id, row, cx)
                                    })
                                })
                                .unwrap_or_else(|| div().into_any_element())
                        })
                        .pb(px(PROJECTS_LIST_BOTTOM_PADDING))
                        .size_full(),
                    ),
            )
            .into_any_element()
    }

    fn render_projects_row(
        &mut self,
        project_id: Uuid,
        row: ProjectsListRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            ProjectsListRow::RemoteHeader {
                remote,
                count,
                expanded,
            } => self.render_projects_remote_header(project_id, remote, count, expanded, cx),
            ProjectsListRow::Worktree { index } => {
                self.render_projects_worktree_row(project_id, index, cx)
            }
            ProjectsListRow::Branch { index } => {
                self.render_projects_branch_row(project_id, index, cx)
            }
        }
    }

    /// The collapsible group header for one remote's tracking refs.
    fn render_projects_remote_header(
        &mut self,
        project_id: Uuid,
        remote: SharedString,
        count: usize,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let fetching = self
            .projects_page_states
            .get(&project_id)
            .is_some_and(|state| state.fetching_remotes.contains(remote.as_str()));
        let header_remote = remote.clone();
        div()
            .id(SharedString::from(format!("projects-remote-{remote}")))
            .w_full()
            .h(px(PROJECTS_ROW_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_default()
            .border_b(hairline())
            .border_color(theme.separator)
            .bg(theme.inset)
            .hover(|style| style.bg(theme.overlay))
            .child(icon(
                if expanded {
                    "icons/chevron-down.svg"
                } else {
                    "icons/chevron-right.svg"
                },
                10.0,
                theme.text_tertiary,
            ))
            .child(icon("icons/globe.svg", 12.0, theme.text_tertiary))
            .child(
                div()
                    .text_size(sp(14.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_secondary)
                    .child(remote.clone()),
            )
            .child(
                div()
                    .text_size(sp(14.0))
                    .text_color(theme.text_tertiary)
                    .child(format!("{count}")),
            )
            .when(fetching, |element| {
                element.child(motion::spin(icon(
                    "icons/loader-circle.svg",
                    11.0,
                    theme.text_tertiary,
                )))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                let collapsed = this
                    .projects_page_states
                    .get_mut(&project_id)
                    .map(|state| {
                        if !state.expanded_remotes.remove(header_remote.as_str()) {
                            state.expanded_remotes.insert(header_remote.to_string());
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if collapsed {
                    this.projects_fetch_remote(project_id, header_remote.to_string(), cx);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// Frame shared by selectable rows: identity, focus, selection tint,
    /// click modifiers, and the right-click menu.
    fn projects_row_frame(
        &mut self,
        project_id: Uuid,
        key: ProjectsRowKey,
        content: Div,
        menu: impl Fn(&mut Waku, Vec<ProjectsRowKey>, &mut Context<Waku>) -> Vec<MenuItem> + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        let selected = state.selection.contains(&key);
        let focus = state
            .row_focuses
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let handle = state
            .row_menus
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| ContextMenuHandle::new(cx))
            .clone();
        let id = SharedString::from(format!("projects-row-{key:?}"));
        let click_key = key.clone();
        let menu_key = key.clone();
        let key_key = key.clone();
        let entity = cx.entity().downgrade();

        let row = content
            .id(id)
            .track_focus(&focus)
            .tab_index(0)
            .tab_stop(true)
            .w_full()
            .h(px(PROJECTS_ROW_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .border_b(hairline())
            .border_color(theme.separator)
            .when(selected, |element| element.bg(theme.overlay_strong))
            .hover(|style| style.bg(theme.overlay))
            .focus_visible(|style| style.bg(theme.overlay))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.projects_row_select(project_id, click_key.clone(), event.modifiers(), cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.projects_row_select(
                        project_id,
                        key_key.clone(),
                        gpui::Modifiers::default(),
                        cx,
                    );
                    cx.stop_propagation();
                }
            }));

        context_menu(
            row,
            format!("projects-row-menu-{key:?}"),
            &handle,
            move |cx| {
                entity
                    .update(cx, |this, cx| {
                        let targets = this.projects_menu_target(project_id, &menu_key, cx);
                        menu(this, targets, cx)
                    })
                    .unwrap_or_default()
            },
        )
    }

    fn render_projects_worktree_row(
        &mut self,
        project_id: Uuid,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        let github::GitHubFetch::Loaded(Some(entries)) = &state.worktrees else {
            return div().into_any_element();
        };
        let Some(entry) = entries.get(index) else {
            return div().into_any_element();
        };
        let key = ProjectsRowKey::Worktree(entry.path.clone());
        let name = entry
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.path.display().to_string());
        let sessions = state
            .session_counts
            .borrow()
            .get(&entry.path)
            .copied()
            .unwrap_or(0);

        // Column cells share widths with `projects_column_header` — every
        // cell renders even when empty so the columns stay put.
        let mut name_cell = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(15.5))
                    .text_color(theme.text)
                    .child(name),
            );
        if entry.is_main {
            name_cell = name_cell.child(projects_badge(tr!("projects.main_checkout"), &theme));
        }

        let branch_cell = div()
            .flex_none()
            .w(px(state.worktree_col_widths[0]))
            .min_w_0()
            .flex()
            .items_center()
            .child(match &entry.branch {
                Some(branch) => projects_badge(branch.clone(), &theme)
                    .min_w_0()
                    .truncate()
                    .into_any_element(),
                None => div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(14.0))
                    .text_color(theme.text_tertiary)
                    .child(format!("@{}", &entry.head[..entry.head.len().min(8)]))
                    .into_any_element(),
            });

        let dirty = entry.dirty_files.unwrap_or(0);
        let mut changes_cell = div()
            .flex_none()
            .w(px(state.worktree_col_widths[1]))
            .flex()
            .items_center()
            .gap(px(4.0));
        if dirty > 0 {
            changes_cell = changes_cell
                .child(icon("icons/circle-dot.svg", 10.0, theme.warning))
                .child(
                    div()
                        .text_size(sp(14.0))
                        .text_color(theme.warning)
                        .child(format!("{dirty}")),
                );
        }

        let mut sessions_cell = div()
            .flex_none()
            .w(px(state.worktree_col_widths[2]))
            .flex()
            .items_center()
            .gap(px(4.0));
        if sessions > 0 {
            sessions_cell = sessions_cell
                .child(icon("icons/message-square.svg", 10.0, theme.text_tertiary))
                .child(
                    div()
                        .text_size(sp(14.0))
                        .text_color(theme.text_tertiary)
                        .child(format!("{sessions}")),
                );
        }

        let updated_cell = div()
            .flex_none()
            .w(px(state.worktree_col_widths[3]))
            .min_w_0()
            .truncate()
            .text_size(sp(14.0))
            .text_color(theme.text_tertiary)
            .child(
                entry
                    .last_commit_at
                    .map(|at| sidebar::format_time_ago(unix_time().saturating_sub(at)))
                    .unwrap_or_default(),
            );

        let row = div()
            .min_w_0()
            .child(icon("icons/folder.svg", 13.0, theme.text_secondary))
            .child(name_cell)
            .child(branch_cell)
            .child(changes_cell)
            .child(sessions_cell)
            .child(updated_cell);

        self.projects_row_frame(
            project_id,
            key,
            row,
            move |this, targets, cx| this.projects_worktree_menu(project_id, targets, cx),
            cx,
        )
    }

    fn render_projects_branch_row(
        &mut self,
        project_id: Uuid,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        let github::GitHubFetch::Loaded(Some(entries)) = &state.branches else {
            return div().into_any_element();
        };
        let Some(entry) = entries.get(index) else {
            return div().into_any_element();
        };
        let key = ProjectsRowKey::branch(entry.remote.as_deref(), &entry.name);
        let remote = entry.remote.clone();
        let name = entry.name.clone();
        let subject = entry.last_commit_subject.clone();
        let at = entry.last_commit_at;
        let checked_out_in = entry.checked_out_in.clone();
        let ahead = entry.ahead;
        let behind = entry.behind;
        let pr = state.prs_by_head.borrow().get(&name).cloned();

        // Column cells share widths with `projects_column_header` — every
        // cell renders even when empty so the columns stay put. The name and
        // subject cells split the remaining width evenly.
        let mut name_cell = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(15.5))
                    .text_color(theme.text)
                    .child(match &remote {
                        Some(remote) => format!("{remote}/{name}"),
                        None => name.clone(),
                    }),
            );
        if let Some(path) = &checked_out_in {
            name_cell = name_cell.child(
                div()
                    .id(SharedString::from(format!("projects-checked-out-{key:?}")))
                    .flex_none()
                    .flex()
                    .items_center()
                    .tooltip(Tooltip::text(tr!(
                        "projects.checked_out_in",
                        path = path.display().to_string()
                    )))
                    .child(icon("icons/folder-open.svg", 11.0, theme.accent)),
            );
        }

        let mut pr_cell = div()
            .flex_none()
            .w(px(state.branch_col_widths[0]))
            .min_w_0()
            .flex()
            .items_center();
        if let Some(pr) = pr {
            let (color, url) = (
                sidebar::sidebar_pull_request_color(&theme, sidebar::pull_request_class(&pr)),
                pr.url.clone(),
            );
            pr_cell = pr_cell.child(
                div()
                    .id(SharedString::from(format!("projects-pr-{}", pr.number)))
                    .flex_none()
                    .text_size(sp(14.0))
                    .text_color(color)
                    .cursor_default()
                    .tooltip(Tooltip::text(tr!("projects.open_pull_request")))
                    .child(format!("#{}", pr.number))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.open_url(&url);
                        cx.stop_propagation();
                    })),
            );
        }

        let mut divergence_cell = div()
            .flex_none()
            .w(px(state.branch_col_widths[1]))
            .min_w_0()
            .flex()
            .items_center();
        if ahead.unwrap_or(0) > 0 || behind.unwrap_or(0) > 0 {
            divergence_cell = divergence_cell.child(
                div()
                    .text_size(sp(14.0))
                    .text_color(theme.text_tertiary)
                    .child(format!("↑{} ↓{}", ahead.unwrap_or(0), behind.unwrap_or(0))),
            );
        }

        let subject_cell = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(sp(14.5))
            .text_color(theme.text_tertiary)
            .child(subject.unwrap_or_default());

        let updated_cell = div()
            .flex_none()
            .w(px(state.branch_col_widths[2]))
            .min_w_0()
            .truncate()
            .text_size(sp(14.0))
            .text_color(theme.text_tertiary)
            .child(
                at.map(|at| sidebar::format_time_ago(unix_time().saturating_sub(at)))
                    .unwrap_or_default(),
            );

        let row = div()
            .min_w_0()
            .child(icon("icons/git-branch.svg", 13.0, theme.text_secondary))
            .child(name_cell)
            .child(pr_cell)
            .child(divergence_cell)
            .child(subject_cell)
            .child(updated_cell);

        self.projects_row_frame(
            project_id,
            key,
            row,
            move |this, targets, cx| this.projects_branch_menu(project_id, targets, cx),
            cx,
        )
    }

    // ----- context menus -----

    /// The worktree row's menu, applied to every selected worktree for the
    /// plural actions.
    fn projects_worktree_menu(
        &mut self,
        project_id: Uuid,
        targets: Vec<ProjectsRowKey>,
        cx: &mut Context<Self>,
    ) -> Vec<MenuItem> {
        let paths: Vec<PathBuf> = targets
            .iter()
            .filter_map(|key| match key {
                ProjectsRowKey::Worktree(path) => Some(path.clone()),
                _ => None,
            })
            .collect();
        if paths.is_empty() {
            return Vec::new();
        }
        let weak = cx.entity().downgrade();
        let mut items: Vec<MenuItem> = Vec::new();

        if paths.len() == 1 {
            let entry = self
                .projects_page_states
                .get(&project_id)
                .and_then(|state| match &state.worktrees {
                    github::GitHubFetch::Loaded(Some(entries)) => {
                        entries.iter().find(|entry| entry.path == paths[0]).cloned()
                    }
                    _ => None,
                });
            let path = paths[0].clone();
            let path_label = path.clone();

            let new_task_weak = weak.clone();
            let task_path = path.clone();
            let task_name = entry
                .as_ref()
                .map(|entry| {
                    entry
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let task_branch = entry.as_ref().and_then(|entry| entry.branch.clone());
            let is_main = entry.as_ref().is_some_and(|entry| entry.is_main);
            items.push(
                MenuItem::new(tr!("projects.new_task_in_worktree"), move |window, cx| {
                    let _ = new_task_weak.update(cx, |this, cx| {
                        this.projects_new_task_in_worktree(
                            project_id,
                            task_path.clone(),
                            task_name.clone(),
                            task_branch.clone(),
                            window,
                            cx,
                        );
                    });
                })
                .icon("icons/plus.svg")
                .disabled(is_main),
            );

            if !self.is_remote_path(&path) && !self.open_in_apps.is_empty() {
                let apps = self.open_in_apps.clone();
                let open_path = path.clone();
                items.push(MenuItem::Submenu {
                    label: tr!("projects.open_in").into(),
                    value: None,
                    items: Rc::new(move |_cx| {
                        apps.iter()
                            .map(|app| {
                                let weak = weak.clone();
                                let path = open_path.clone();
                                let app_id = app.id;
                                MenuItem::new(app.label, move |_, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.open_workspace_in_app(&path, app_id, cx);
                                    });
                                })
                                .image(app.icon.clone())
                            })
                            .collect()
                    }),
                });
            }
            items.push(
                MenuItem::new(tr!("projects.reveal_in_finder"), move |_, cx| {
                    cx.reveal_path(&path_label);
                })
                .icon("icons/folder-open.svg"),
            );
        }

        items.push({
            let label = if paths.len() == 1 {
                tr!("projects.copy_path")
            } else {
                tr!("projects.copy_paths")
            };
            let copied = paths.clone();
            MenuItem::new(label, move |_, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                    copied
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ));
            })
            .icon("icons/copy.svg")
        });

        let contains_main = self
            .projects_page_states
            .get(&project_id)
            .and_then(|state| match &state.worktrees {
                github::GitHubFetch::Loaded(Some(entries)) => Some(
                    entries
                        .iter()
                        .any(|entry| entry.is_main && paths.contains(&entry.path)),
                ),
                _ => None,
            })
            .unwrap_or(false);
        items.push(MenuItem::Separator);
        items.push({
            let label = if paths.len() == 1 {
                tr!("projects.remove_worktree")
            } else {
                tr!("projects.remove_worktrees", count = paths.len())
            };
            let remove_paths = paths.clone();
            let remove_weak = cx.entity().downgrade();
            MenuItem::new(label, move |window, cx| {
                let _ = remove_weak.update(cx, |this, cx| {
                    this.projects_remove_worktrees(project_id, remove_paths.clone(), window, cx);
                });
            })
            .icon("icons/trash.svg")
            .disabled(contains_main)
        });
        items.push({
            let prune_weak = cx.entity().downgrade();
            MenuItem::new(tr!("projects.prune_stale"), move |_, cx| {
                let _ = prune_weak.update(cx, |this, cx| {
                    this.projects_prune_worktrees(project_id, cx);
                });
            })
        });
        items
    }

    /// The branch row's menu; remote-tracking refs take the same "new
    /// task/worktree" actions against their `remote/name` ref but not delete.
    fn projects_branch_menu(
        &mut self,
        project_id: Uuid,
        targets: Vec<ProjectsRowKey>,
        cx: &mut Context<Self>,
    ) -> Vec<MenuItem> {
        let weak = cx.entity().downgrade();
        let mut items: Vec<MenuItem> = Vec::new();
        let single = (targets.len() == 1).then(|| targets[0].clone());

        if let Some(ProjectsRowKey::Branch(full)) = &single {
            // `full` is `heads/<name>` or `remotes/<remote>/<name>`; the base
            // ref for creation is the local name or `remote/name`.
            let base_ref = full
                .strip_prefix("heads/")
                .map(str::to_owned)
                .or_else(|| full.strip_prefix("remotes/").map(str::to_owned))
                .unwrap_or_else(|| full.clone());
            let remote_ref = full.starts_with("remotes/");
            let display = base_ref.clone();

            let worktree_weak = weak.clone();
            let worktree_ref = base_ref.clone();
            items.push(
                MenuItem::new(
                    tr!("projects.new_worktree_from", name = display.clone()),
                    move |_, cx| {
                        let _ = worktree_weak.update(cx, |this, cx| {
                            this.projects_create_worktree(project_id, worktree_ref.clone(), cx);
                        });
                    },
                )
                .icon("icons/folder.svg"),
            );
            let task_weak = weak.clone();
            let task_ref = base_ref.clone();
            items.push(
                MenuItem::new(
                    tr!("projects.new_task_on", name = display.clone()),
                    move |window, cx| {
                        let _ = task_weak.update(cx, |this, cx| {
                            this.projects_new_task_on_branch(
                                project_id,
                                task_ref.clone(),
                                window,
                                cx,
                            );
                        });
                    },
                )
                .icon("icons/plus.svg"),
            );

            if let Some(pr) = self
                .projects_page_states
                .get(&project_id)
                .and_then(|state| state.prs_by_head.borrow().get(&base_ref).cloned())
            {
                let url = pr.url.clone();
                items.push(
                    MenuItem::new(
                        tr!("projects.view_pull_request", number = pr.number),
                        move |_, cx| {
                            cx.open_url(&url);
                        },
                    )
                    .icon("icons/git-pull-request-arrow.svg"),
                );
            }

            items.push({
                let name = base_ref.clone();
                MenuItem::new(tr!("projects.copy_branch_name"), move |_, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(name.clone()));
                })
                .icon("icons/copy.svg")
            });
            if !remote_ref {
                items.push(MenuItem::Separator);
            }
        }

        let deletable: Vec<String> = targets
            .iter()
            .filter_map(|key| match key {
                ProjectsRowKey::Branch(full) => full.strip_prefix("heads/").map(str::to_owned),
                _ => None,
            })
            .collect();
        if !deletable.is_empty() {
            let label = if deletable.len() == 1 {
                tr!("projects.delete_branch")
            } else {
                tr!("projects.delete_branches", count = deletable.len())
            };
            let delete_weak = cx.entity().downgrade();
            items.push(
                MenuItem::new(label, move |window, cx| {
                    let _ = delete_weak.update(cx, |this, cx| {
                        this.projects_delete_branches(project_id, deletable.clone(), window, cx);
                    });
                })
                .icon("icons/trash.svg"),
            );
        }
        items
    }

    // ----- bulk bar + composer -----

    /// The selection bar stacked under the table — only while the shown
    /// tab's row-set has a selection.
    fn render_projects_bulk_bar(
        &mut self,
        project_id: Uuid,
        tab: ProjectsTab,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        let state = self.projects_page_states.get(&project_id)?;
        let count = state.selected_in(tab);
        if count == 0 {
            return None;
        }

        let action = match tab {
            ProjectsTab::Worktrees => {
                let contains_main = match &state.worktrees {
                    github::GitHubFetch::Loaded(Some(entries)) => entries.iter().any(|entry| {
                        entry.is_main
                            && state
                                .selection
                                .contains(&ProjectsRowKey::Worktree(entry.path.clone()))
                    }),
                    _ => false,
                };
                let label = if count == 1 {
                    tr!("projects.remove_worktree")
                } else {
                    tr!("projects.remove_worktrees", count = count)
                };
                Some((
                    label,
                    contains_main,
                    "icons/trash.svg",
                    ProjectsTab::Worktrees,
                ))
            }
            ProjectsTab::Branches => {
                let deletable = state.selection.iter().any(
                    |key| matches!(key, ProjectsRowKey::Branch(full) if full.starts_with("heads/")),
                );
                let label = if count == 1 {
                    tr!("projects.delete_branch")
                } else {
                    tr!("projects.delete_branches", count = count)
                };
                Some((label, !deletable, "icons/trash.svg", ProjectsTab::Branches))
            }
            _ => None,
        };

        Some(
            div()
                .flex_none()
                .h(px(36.0))
                .w_full()
                .px(px(14.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .border_t(hairline())
                .border_color(theme.separator)
                .bg(theme.raised)
                .child(
                    div()
                        .text_size(sp(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(tr!("projects.selected_count", count = count)),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id("projects-clear-selection")
                        .h(px(22.0))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .cursor_default()
                        .text_size(sp(14.5))
                        .text_color(theme.text_secondary)
                        .hover(|style| style.bg(theme.overlay).text_color(theme.text))
                        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                        .child(tr!("projects.clear_selection"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(state) = this.projects_page_states.get_mut(&project_id) {
                                state.selection.clear();
                                state.anchor = None;
                            }
                            cx.notify();
                        })),
                )
                .children(action.map(|(label, disabled, icon_path, action_tab)| {
                    div()
                        .id("projects-bulk-action")
                        .h(px(22.0))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .text_size(sp(14.5))
                        .when(disabled, |element| element.text_color(theme.text_ghost))
                        .when(!disabled, |element| {
                            element
                                .cursor_default()
                                .text_color(theme.danger)
                                .hover(|style| style.bg(theme.danger_soft))
                                .focus_visible(|style| {
                                    style.border(hairline()).border_color(theme.accent)
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let keys: Vec<ProjectsRowKey> = this
                                        .projects_page_states
                                        .get(&project_id)
                                        .map(|state| {
                                            state
                                                .selection
                                                .iter()
                                                .filter(|key| key.tab() == action_tab)
                                                .cloned()
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    match action_tab {
                                        ProjectsTab::Worktrees => {
                                            let paths = keys
                                                .iter()
                                                .filter_map(|key| match key {
                                                    ProjectsRowKey::Worktree(path) => {
                                                        Some(path.clone())
                                                    }
                                                    _ => None,
                                                })
                                                .collect();
                                            this.projects_remove_worktrees(
                                                project_id, paths, window, cx,
                                            );
                                        }
                                        ProjectsTab::Branches => {
                                            let names = keys
                                                .iter()
                                                .filter_map(|key| match key {
                                                    ProjectsRowKey::Branch(full) => full
                                                        .strip_prefix("heads/")
                                                        .map(str::to_owned),
                                                    _ => None,
                                                })
                                                .collect();
                                            this.projects_delete_branches(
                                                project_id, names, window, cx,
                                            );
                                        }
                                        _ => {}
                                    }
                                }))
                        })
                        .child(icon(
                            icon_path,
                            11.0,
                            if disabled {
                                theme.text_ghost
                            } else {
                                theme.danger
                            },
                        ))
                        .child(label)
                }))
                .into_any_element(),
        )
    }

    /// The docked composer: the page's context chips over the same card the
    /// chat column mounts — one shared `ComposerInput`, so drafts, paste,
    /// attachments, and the controls row all behave identically.
    fn render_projects_composer(
        &mut self,
        project_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(state) = self.projects_page_states.get(&project_id) else {
            return div().into_any_element();
        };
        let tab = state.tab;
        let filter = state.filter_text(tab, cx).trim().to_owned();
        let selected = state.selected_in(tab);
        let project_name = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.display_name())
            .unwrap_or_default();

        let chip = |id: &'static str,
                    label: String,
                    removable: bool,
                    cx: &mut Context<Self>|
         -> AnyElement {
            div()
                .id(id)
                .h(px(22.0))
                .px(px(7.0))
                .rounded(px(6.0))
                .bg(theme.inset)
                .flex()
                .items_center()
                .gap(px(5.0))
                .text_size(sp(14.0))
                .text_color(theme.text_secondary)
                .child(label)
                .when(removable, |element| {
                    element
                        .child(icon("icons/x.svg", 9.0, theme.text_tertiary))
                        .cursor_default()
                        .hover(|style| style.bg(theme.overlay))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(state) = this.projects_page_states.get_mut(&project_id) {
                                match id {
                                    "projects-chip-filter" => {
                                        let tab = state.tab;
                                        state.filter_input(tab).update(cx, |input, cx| {
                                            input.clear(cx);
                                        });
                                    }
                                    "projects-chip-selection" => {
                                        state.selection.clear();
                                        state.anchor = None;
                                    }
                                    _ => {}
                                }
                            }
                            cx.notify();
                        }))
                })
                .into_any_element()
        };

        let mut chips = div()
            .flex_none()
            .w_full()
            .px(px(10.0))
            .pb(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .flex_wrap()
            .child(chip("projects-chip-project", project_name, false, cx))
            .child(chip("projects-chip-tab", tab.label(), false, cx));
        if !filter.is_empty() {
            chips = chips.child(chip(
                "projects-chip-filter",
                tr!("projects.chip_filter", filter = filter.clone()),
                true,
                cx,
            ));
        }
        if selected > 0 {
            chips = chips.child(chip(
                "projects-chip-selection",
                tr!("projects.chip_selected", count = selected),
                true,
                cx,
            ));
        }

        // The same card the chat column docks — chips carry the page's
        // context above it. Big Picture remounts the one composer entity
        // inside its own layer; mounting it here too would collide.
        div()
            .flex_none()
            .w_full()
            .pt(px(4.0))
            .flex()
            .flex_col()
            .child(chips)
            .when(!self.big_picture.is_open(), |element| {
                element
                    .child(self.render_composer(window, cx))
                    .child(self.render_workspace_footer(cx))
            })
            .into_any_element()
    }

    // ----- Settings → Git page -----

    /// The Settings → Git page: the Worktrees/Branches half of the old
    /// Projects page — same per-project state, tables, selection, and
    /// menus — without the docked composer.
    pub(super) fn render_git_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(project_id) = self.resolve_git_settings_project() else {
            return github::github_centered(
                icon("icons/git-branch.svg", 16.0, theme.text_tertiary).into_any_element(),
                tr!("settings.git_no_projects"),
                &theme,
            );
        };
        self.projects_ensure_state(project_id, window, cx);
        if std::mem::take(&mut self.git_page_refresh_pending) {
            self.projects_refresh(project_id, cx);
        }
        let tab = self
            .projects_page_states
            .get(&project_id)
            .map(|state| state.git_tab)
            .unwrap_or(ProjectsTab::Worktrees);
        let missing = self.missing_projects.contains(&project_id);
        let content = if missing {
            self.render_projects_missing(project_id, cx)
        } else {
            self.render_projects_table(project_id, tab, window, cx)
        };

        div()
            .key_context("GitSettingsPage")
            .mt(px(15.0))
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::select_all_git_rows_action))
            .on_action(cx.listener(Self::focus_git_filter_action))
            .on_action(cx.listener(Self::dismiss_git_layer_action))
            .child(self.render_git_settings_header(project_id, tab, cx))
            .when(!missing, |element| {
                element.child(self.render_projects_toolbar(project_id, tab, cx))
            })
            .child(content)
            .children(self.render_projects_bulk_bar(project_id, tab, cx))
            .into_any_element()
    }

    /// The Git page's top row: the project selector left of the
    /// Worktrees/Branches sub-tab strip.
    fn render_git_settings_header(
        &mut self,
        project_id: Uuid,
        tab: ProjectsTab,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let project_name = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.display_name())
            .unwrap_or_else(|| tr!("sidebar.unknown_project"));
        let menu_handle = self.menu_handle("git-settings-project-selector", cx);
        let selector_open = menu_handle.is_open();
        let weak = cx.entity().downgrade();
        let selector = dropdown_menu(
            MenuChip::new("git-settings-project-selector")
                .icon("icons/folder.svg", theme.text_tertiary)
                .label(project_name)
                .outlined()
                .selected(selector_open)
                .max_w(px(220.0))
                .flex_none(),
            "git-settings-project-selector-menu",
            &menu_handle,
            MenuAlign::BelowLeft,
            move |cx| {
                let items = weak
                    .update(cx, |this, _| {
                        this.state
                            .projects
                            .iter()
                            .filter(|project| !project.is_projectless())
                            .map(|project| (project.id, project.display_name()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                items
                    .into_iter()
                    .map(|(id, label)| {
                        let weak = weak.clone();
                        MenuItem::new(label, move |_, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.select_git_settings_project(id, cx);
                            });
                        })
                        .selected(id == project_id)
                    })
                    .collect()
            },
        );

        div()
            .flex_none()
            .w_full()
            .pb(px(10.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(selector)
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .p(px(2.0))
                    .bg(theme.inset)
                    .children(ProjectsTab::GIT_TABS.into_iter().map(|candidate| {
                        self.projects_tab_button(project_id, candidate, tab, true, true, cx)
                    })),
            )
            .into_any_element()
    }

    /// The Settings → Git page's overlay scrollbar, rendered by the
    /// settings column at the window's right edge — the table itself is
    /// width-capped and centered, so mounting the bar inside it would pull
    /// it off the edge. `None` while the page shows anything but the row
    /// list, so a stale extent can't phantom over a centered state.
    pub(super) fn render_git_settings_scrollbar(&mut self) -> Option<AnyElement> {
        let project_id = self.resolve_git_settings_project()?;
        if self.missing_projects.contains(&project_id) {
            return None;
        }
        let state = self.projects_page_states.get(&project_id)?;
        let rows_ready = match state.git_tab {
            ProjectsTab::Worktrees => {
                matches!(state.worktrees, github::GitHubFetch::Loaded(Some(_)))
            }
            ProjectsTab::Branches => {
                matches!(state.branches, github::GitHubFetch::Loaded(Some(_)))
            }
            _ => false,
        };
        if !rows_ready || state.list_state.item_count() == 0 {
            return None;
        }
        Some(
            scrollbar::vertical(
                &PaddedListScroll {
                    state: state.list_state.clone(),
                    bottom: px(PROJECTS_LIST_BOTTOM_PADDING),
                },
                &state.list_scrollbar,
            )
            .into_any_element(),
        )
    }
}

/// The pinned column header above a Worktrees/Branches table. Its cells use
/// the same padding, gaps, and widths as the rows so each label sits over
/// its column. Fixed columns carry a drag handle on their trailing edge.
fn projects_column_header(
    project_id: Uuid,
    tab: ProjectsTab,
    state: &ProjectsPageState,
    theme: &Theme,
    cx: &mut Context<Waku>,
) -> Div {
    let set_width =
        move |this: &mut Waku, column: usize, width: f32, cx: &mut Context<Waku>| {
            if let Some(state) = this.projects_page_states.get_mut(&project_id) {
                let widths = match tab {
                    ProjectsTab::Worktrees => Some(&mut state.worktree_col_widths[..]),
                    ProjectsTab::Branches => Some(&mut state.branch_col_widths[..]),
                    _ => None,
                };
                if let Some(slot) = widths.and_then(|widths| widths.get_mut(column)) {
                    *slot = width;
                    cx.notify();
                }
            }
        };
    let label = |text: String| {
        div()
            .min_w_0()
            .truncate()
            .text_size(sp(13.0))
            .text_color(theme.text_tertiary)
            .child(text)
    };
    let cell = |index: usize, width: f32, text: String, cx: &mut Context<Waku>| {
        div()
            .flex_none()
            .w(px(width))
            .min_w_0()
            .relative()
            .flex()
            .items_center()
            .child(label(text))
            .child(column_resize::column_resize_handle(
                SharedString::from(format!("projects-col-{tab:?}-{index}")),
                &state.col_resize,
                index,
                width,
                theme,
                cx,
                set_width,
            ))
    };
    let row = div()
        .flex_none()
        .w_full()
        .h(px(PROJECTS_COLUMN_HEADER_HEIGHT))
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .border_b(hairline())
        .border_color(theme.separator)
        // Covers the rows' leading icon so labels align with cell text.
        .child(div().flex_none().w(px(13.0)));
    let row = match tab {
        ProjectsTab::Worktrees => {
            let widths = state.worktree_col_widths;
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(label(tr!("projects.col_name"))),
            )
            .child(cell(0, widths[0], tr!("projects.col_branch"), cx))
            .child(cell(1, widths[1], tr!("projects.col_changes"), cx))
            .child(cell(2, widths[2], tr!("projects.col_tasks"), cx))
            .child(cell(3, widths[3], tr!("projects.col_updated"), cx))
        }
        ProjectsTab::Branches => {
            let widths = state.branch_col_widths;
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(label(tr!("projects.col_branch"))),
            )
            .child(cell(0, widths[0], tr!("projects.col_pull_request"), cx))
            .child(cell(1, widths[1], tr!("projects.col_divergence"), cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(label(tr!("projects.col_last_commit"))),
            )
            .child(cell(2, widths[2], tr!("projects.col_updated"), cx))
        }
        _ => row,
    };
    div()
        .relative()
        .child(row)
        .child(column_resize::column_resize_listeners(
            &state.col_resize,
            cx,
            set_width,
        ))
}

/// A `list()`'s own padding joins its scroll extent, but
/// `ListState::max_offset_for_scrollbar` reports only the measured items —
/// and clamps before padding can be added back — so a padded list bottoms
/// out its thumb early. This wraps the state so the scrollbar's travel
/// covers the padded extent exactly.
#[derive(Clone)]
struct PaddedListScroll {
    state: ListState,
    bottom: Pixels,
}

impl scrollbar::Scrollable for PaddedListScroll {
    fn viewport_height(&self) -> Pixels {
        self.state.viewport_bounds().size.height
    }

    fn max_offset(&self) -> Pixels {
        let items = px(PROJECTS_ROW_HEIGHT * self.state.item_count() as f32);
        (items + self.bottom - self.viewport_height()).max(Pixels::ZERO)
    }

    fn scrolled(&self) -> Pixels {
        -self.state.scroll_px_offset_for_scrollbar().y
    }

    fn scroll_to(&self, offset: Pixels) {
        self.state
            .set_offset_from_scrollbar(point(Pixels::ZERO, -offset));
    }
}

/// A small rounded label used for a worktree's branch/main badges.
fn projects_badge(label: String, theme: &Theme) -> Div {
    div()
        .flex_none()
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(4.0))
        .bg(theme.overlay)
        .flex()
        .items_center()
        .text_size(sp(13.5))
        .text_color(theme.text_secondary)
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(path: &str, branch: Option<&str>) -> RepoWorktree {
        RepoWorktree {
            path: PathBuf::from(path),
            head: "a".repeat(40),
            branch: branch.map(str::to_owned),
            is_main: false,
            dirty_files: None,
            ahead: None,
            behind: None,
            last_commit_at: None,
        }
    }

    fn branch(name: &str, remote: Option<&str>) -> RepoBranch {
        RepoBranch {
            name: name.to_owned(),
            remote: remote.map(str::to_owned),
            sha: "a".repeat(40),
            upstream: None,
            ahead: None,
            behind: None,
            last_commit_at: None,
            last_commit_subject: None,
            checked_out_in: None,
        }
    }

    #[test]
    fn branch_rows_group_remotes_with_origin_first() {
        let entries = vec![
            branch("main", None),
            branch("topic", Some("upstream")),
            branch("main", Some("origin")),
            branch("dev", Some("upstream")),
            branch("release", Some("origin")),
        ];
        let expanded = HashSet::from(["origin".to_owned()]);
        let rows = flatten_branch_rows(&entries, "", &expanded);
        assert_eq!(
            rows,
            vec![
                ProjectsListRow::Branch { index: 0 },
                ProjectsListRow::RemoteHeader {
                    remote: "origin".into(),
                    count: 2,
                    expanded: true,
                },
                // Expanded remotes list their members; collapsed ones don't.
                ProjectsListRow::Branch { index: 2 },
                ProjectsListRow::Branch { index: 4 },
                ProjectsListRow::RemoteHeader {
                    remote: "upstream".into(),
                    count: 2,
                    expanded: false,
                },
            ]
        );
    }

    #[test]
    fn branch_filter_flattens_groups_and_matches_remote_names() {
        let entries = vec![
            branch("main", None),
            branch("topic", Some("upstream")),
            branch("dev", Some("upstream")),
        ];
        let expanded = HashSet::new();
        // A query drops the headers and returns matching rows flat.
        let rows = flatten_branch_rows(&entries, "up", &expanded);
        assert_eq!(
            rows,
            vec![
                ProjectsListRow::Branch { index: 1 },
                ProjectsListRow::Branch { index: 2 },
            ]
        );
        let rows = flatten_branch_rows(&entries, "main", &expanded);
        assert_eq!(rows, vec![ProjectsListRow::Branch { index: 0 }]);
    }

    #[test]
    fn worktree_filter_matches_folder_branch_and_path() {
        let entries = vec![
            worktree("/repo", Some("main")),
            worktree("/repo/../repo-linked", Some("feature/ux")),
        ];
        assert_eq!(filter_worktree_indices(&entries, ""), vec![0, 1]);
        assert_eq!(filter_worktree_indices(&entries, "linked"), vec![1]);
        assert_eq!(filter_worktree_indices(&entries, "ux"), vec![1]);
        assert_eq!(
            filter_worktree_indices(&entries, "missing"),
            Vec::<usize>::new()
        );
    }

    fn modifiers(shift: bool, secondary: bool) -> gpui::Modifiers {
        gpui::Modifiers {
            shift,
            platform: secondary,
            ..Default::default()
        }
    }

    #[test]
    fn row_select_follows_macos_list_conventions() {
        let ordered: Vec<ProjectsRowKey> = (0..5)
            .map(|index| ProjectsRowKey::branch(None, &format!("b{index}")))
            .collect();
        let mut selection = HashSet::new();
        let mut anchor = None;

        // A plain click isolates the row and moves the anchor.
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[1].clone(),
            modifiers(false, false),
        );
        assert_eq!(selection, HashSet::from([ordered[1].clone()]));
        assert_eq!(anchor, Some(ordered[1].clone()));

        // ⇧ ranges from the anchor through the row, replacing the selection.
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[3].clone(),
            modifiers(true, false),
        );
        assert_eq!(
            selection,
            HashSet::from([ordered[1].clone(), ordered[2].clone(), ordered[3].clone()])
        );

        // ⌘ toggles a row without disturbing the rest.
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[2].clone(),
            modifiers(false, true),
        );
        assert_eq!(
            selection,
            HashSet::from([ordered[1].clone(), ordered[3].clone()])
        );
        assert_eq!(anchor, Some(ordered[2].clone()));

        // ⇧ with no anchor — or one filtered out — behaves like a click.
        let mut anchor = None;
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[4].clone(),
            modifiers(true, false),
        );
        assert_eq!(selection, HashSet::from([ordered[4].clone()]));
    }

    #[test]
    fn row_select_deselects_the_only_selected_row() {
        let ordered: Vec<ProjectsRowKey> = (0..3)
            .map(|index| ProjectsRowKey::branch(None, &format!("b{index}")))
            .collect();
        let mut selection = HashSet::new();
        let mut anchor = None;

        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[1].clone(),
            modifiers(false, false),
        );
        assert_eq!(selection, HashSet::from([ordered[1].clone()]));

        // Clicking the only selected row again clears the selection but
        // keeps the anchor — ⇧ from it still ranges through the list.
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[1].clone(),
            modifiers(false, false),
        );
        assert!(selection.is_empty());
        assert_eq!(anchor, Some(ordered[1].clone()));

        // With a multi-row selection, a plain click still isolates.
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[0].clone(),
            modifiers(false, false),
        );
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[1].clone(),
            modifiers(false, true),
        );
        apply_row_select(
            &mut selection,
            &mut anchor,
            &ordered,
            ordered[1].clone(),
            modifiers(false, false),
        );
        assert_eq!(selection, HashSet::from([ordered[1].clone()]));
    }
}
