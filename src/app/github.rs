//! A project's GitHub lists and details — the machinery behind the Projects
//! page's Issues and Pull Requests tabs.
//!
//! Every read goes through the daemon's `gh` operations on a background
//! executor — the render path only paints what has already landed, and
//! `GitHubFetch::Loaded(None)` ("the host could not answer") renders
//! differently from an empty `Some`. State is keyed by project in
//! `Waku::github_browsers`; the page owns tab and filter state and hands
//! them in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::*;
use waku_client::{
    GitHubAvailability, GitHubRepoRef, IssueDetail, IssueState, IssueSummary, PullRequestDetail,
    PullRequestSummary, WorkItemComment, WorkItemQueryState,
};

/// The list the browser is showing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GitHubTab {
    PullRequests,
    Issues,
}

/// What a detail view is open on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum GitHubItemKind {
    PullRequest,
    Issue,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct GitHubDetailRef {
    pub kind: GitHubItemKind,
    pub number: u64,
}

/// The daemon's answer for a detail read, kind-tagged so one map serves
/// both. Kept `Send` — the background executor hands it back unwrapped and
/// the UI side puts it behind `Rc`.
pub(super) enum GitHubItemDetail {
    PullRequest(PullRequestDetail),
    Issue(IssueDetail),
}

/// A read's lifecycle. `Loading` doubles as "requested, nothing back yet";
/// `Loaded(None)` is the "host could not answer" contract from waku-core.
#[derive(Clone)]
pub(super) enum GitHubFetch<T> {
    Loading,
    Loaded(Option<T>),
}

/// One row in the filtered list: an `Rc` bump per frame instead of cloning a
/// summary's strings.
#[derive(Clone)]
pub(super) enum GitHubListRow {
    PullRequest {
        entries: Rc<Vec<PullRequestSummary>>,
        index: usize,
        detail: GitHubDetailRef,
    },
    Issue {
        entries: Rc<Vec<IssueSummary>>,
        index: usize,
        detail: GitHubDetailRef,
    },
}

impl GitHubListRow {
    fn detail(&self) -> GitHubDetailRef {
        match self {
            Self::PullRequest { detail, .. } | Self::Issue { detail, .. } => *detail,
        }
    }
}

/// One project's GitHub state: the resolved repo, fetched lists, and open
/// detail — what the Projects page's Issues/Pull Requests tabs paint. Kept
/// in `Waku::github_browsers` by project id so switching projects or leaving
/// the page does not lose position.
pub(super) struct GitHubBrowser {
    /// Open/closed/all — applied server-side by `gh list`.
    pub query_state: WorkItemQueryState,
    /// `None` until the resolve lands; `Some((None, availability))` says why a
    /// project is not a readable GitHub repo.
    pub repo: Option<(Option<GitHubRepoRef>, GitHubAvailability)>,
    pub pull_requests: GitHubFetch<Rc<Vec<PullRequestSummary>>>,
    pub issues: GitHubFetch<Rc<Vec<IssueSummary>>>,
    /// The open detail, if any; fetch results live in `details`.
    pub detail: Option<GitHubDetailRef>,
    pub details: HashMap<GitHubDetailRef, GitHubFetch<Rc<GitHubItemDetail>>>,
    /// Row focus handles keyed by item so a virtualized row can take tab
    /// focus and open on Enter.
    pub row_focuses: RefCell<HashMap<GitHubDetailRef, FocusHandle>>,
    /// Row context-menu handles keyed the same way.
    pub row_menus: RefCell<HashMap<GitHubDetailRef, ContextMenuHandle>>,
    pub detail_focus: FocusHandle,
    pub list_state: ListState,
    pub detail_scroll: ScrollHandle,
    pub list_scrollbar: Rc<ScrollbarState>,
    pub detail_scrollbar: Rc<ScrollbarState>,
    /// Incremented whenever a fresh fetch set starts; replies from older
    /// generations drop instead of overwriting newer state.
    pub generation: u64,
    /// Parsed markdown per detail body/comment, keyed `body` / `comment-<i>`.
    pub markdown: RefCell<HashMap<Rc<str>, MarkdownView>>,
    pub markdown_selection: TranscriptSelection,
}

const GITHUB_LIST_ROW_HEIGHT: f32 = 30.0;

impl GitHubBrowser {
    pub(super) fn new(cx: &mut Context<Waku>) -> Self {
        Self {
            query_state: WorkItemQueryState::Open,
            repo: None,
            pull_requests: GitHubFetch::Loading,
            issues: GitHubFetch::Loading,
            detail: None,
            details: HashMap::new(),
            row_focuses: RefCell::new(HashMap::new()),
            row_menus: RefCell::new(HashMap::new()),
            detail_focus: cx.focus_handle(),
            list_state: ListState::new(0, ListAlignment::Top, px(64.0)),
            detail_scroll: ScrollHandle::new(),
            list_scrollbar: ScrollbarState::new(),
            detail_scrollbar: ScrollbarState::new(),
            generation: 0,
            markdown: RefCell::new(HashMap::new()),
            markdown_selection: TranscriptSelection::default(),
        }
    }
}

impl Waku {
    /// Refetch the repo identity (when unknown) and both lists for the
    /// browser's current state filter. Explicit refresh reuses this path.
    pub(super) fn github_refresh(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        let Some(cwd) = self
            .state
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
        else {
            return;
        };
        let Some(browser) = self.github_browsers.get_mut(&project_id) else {
            return;
        };
        browser.generation = browser.generation.wrapping_add(1);
        let generation = browser.generation;
        let needs_repo = browser.repo.is_none();
        let query_state = browser.query_state;
        // Refresh keeps showing the rows it is about to replace; the loader
        // is for the first pass only.
        if !matches!(browser.pull_requests, GitHubFetch::Loaded(Some(_))) {
            browser.pull_requests = GitHubFetch::Loading;
        }
        if !matches!(browser.issues, GitHubFetch::Loaded(Some(_))) {
            browser.issues = GitHubFetch::Loading;
        }
        // A refresh while a detail is open re-reads it too; the stale copy
        // stays on screen until the fresh one lands.
        let open_detail = browser.detail;
        cx.notify();

        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let resolved = cx
                .background_executor()
                .spawn(async move {
                    let repo = needs_repo.then(|| {
                        match workspace.request(
                            waku_client::WorkspaceOperation::ResolveGitHubRepo { cwd: cwd.clone() },
                        ) {
                            Ok(waku_client::WorkspaceResult::GitHubRepo { repo, availability }) => {
                                (repo, availability)
                            }
                            _ => (None, GitHubAvailability::Ready),
                        }
                    });
                    let pull_requests = workspace
                        .request(waku_client::WorkspaceOperation::ListRepoPullRequests {
                            cwd: cwd.clone(),
                            state: query_state,
                            query: None,
                        })
                        .ok()
                        .and_then(|result| match result {
                            waku_client::WorkspaceResult::PullRequests { entries } => entries,
                            _ => None,
                        });
                    let issues = workspace
                        .request(waku_client::WorkspaceOperation::ListIssues {
                            cwd: cwd.clone(),
                            state: query_state,
                            query: None,
                        })
                        .ok()
                        .and_then(|result| match result {
                            waku_client::WorkspaceResult::Issues { entries } => entries,
                            _ => None,
                        });
                    let detail = open_detail.map(|detail| {
                        let value = github_fetch_detail(&workspace, &cwd, detail);
                        (detail, value)
                    });
                    (repo, pull_requests, issues, detail)
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                let Some(browser) = waku.github_browsers.get_mut(&project_id) else {
                    return;
                };
                if browser.generation != generation {
                    return;
                }
                let (repo, pull_requests, issues, detail) = resolved;
                if let Some(repo) = repo {
                    browser.repo = Some(repo);
                }
                browser.pull_requests = GitHubFetch::Loaded(pull_requests.map(Rc::new));
                browser.issues = GitHubFetch::Loaded(issues.map(Rc::new));
                if let Some((detail_ref, value)) = detail {
                    browser
                        .details
                        .insert(detail_ref, GitHubFetch::Loaded(value.map(Rc::new)));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch a detail that has never been read (or that errored last time).
    fn github_ensure_detail(
        &mut self,
        project_id: Uuid,
        detail: GitHubDetailRef,
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
        let Some(browser) = self.github_browsers.get_mut(&project_id) else {
            return;
        };
        if browser.details.contains_key(&detail) {
            return;
        }
        browser.details.insert(detail, GitHubFetch::Loading);
        cx.notify();

        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let value = cx
                .background_executor()
                .spawn(async move { github_fetch_detail(&workspace, &cwd, detail) })
                .await;
            // Detail reads are keyed by item, so a reply landing after the
            // user opened another item is harmless — it just warms the map.
            // No generation check: bumping the refresh generation here would
            // discard list results still in flight.
            let _ = waku.update(cx, |waku, cx| {
                if let Some(browser) = waku.github_browsers.get_mut(&project_id) {
                    browser
                        .details
                        .insert(detail, GitHubFetch::Loaded(value.map(Rc::new)));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn github_open_detail(
        &mut self,
        project_id: Uuid,
        detail: GitHubDetailRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.github_ensure_detail(project_id, detail, cx);
        let Some(browser) = self.github_browsers.get_mut(&project_id) else {
            return;
        };
        browser.detail = Some(detail);
        browser.markdown.borrow_mut().clear();
        browser.detail_scroll.set_offset(point(px(0.0), px(0.0)));
        let focus = browser.detail_focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn github_close_detail(&mut self, project_id: Uuid, cx: &mut Context<Self>) {
        if let Some(browser) = self.github_browsers.get_mut(&project_id) {
            browser.detail = None;
            cx.notify();
        }
    }

    /// "Start a task" / "Fix failing checks": hand the item to a draft
    /// session. `create_session_for` selects the project's draft (creating
    /// one when absent), which returns the main area to the transcript; the
    /// composer then gets the prompt — but only into an empty draft, so a
    /// user's in-progress text is never overwritten.
    pub(super) fn github_start_task(
        &mut self,
        project_id: Uuid,
        prompt: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_session_for(project_id, self.state.last_provider, cx);
        if self.composer.read(cx).content(cx).trim().is_empty() {
            self.composer
                .update(cx, |input, cx| input.set_content(prompt, cx));
            self.schedule_composer_draft_save(cx);
        }
        let focus = self.composer.read(cx).focus();
        window.focus(&focus, cx);
    }

    /// Move the open detail to the previous/next row in the Projects page's
    /// current filtered list, wrapping past the ends.
    pub(super) fn github_detail_navigate(
        &mut self,
        project_id: Uuid,
        direction: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (tab, filter) = self
            .github_browsers
            .get(&project_id)
            .and_then(|browser| browser.detail)
            .map(|detail| {
                let tab = match detail.kind {
                    GitHubItemKind::PullRequest => GitHubTab::PullRequests,
                    GitHubItemKind::Issue => GitHubTab::Issues,
                };
                let projects_tab = match detail.kind {
                    GitHubItemKind::PullRequest => projects::ProjectsTab::PullRequests,
                    GitHubItemKind::Issue => projects::ProjectsTab::Issues,
                };
                let filter = self
                    .projects_page_states
                    .get(&project_id)
                    .map(|state| state.filter_text(projects_tab, cx))
                    .unwrap_or_default();
                (tab, filter)
            })
            .unwrap_or((GitHubTab::PullRequests, String::new()));
        let rows = self.github_filtered_rows(project_id, tab, &filter, cx);
        let Some(browser) = self.github_browsers.get(&project_id) else {
            return;
        };
        let Some(detail) = browser.detail else {
            return;
        };
        let Some(index) = rows.iter().position(|row| row.detail() == detail) else {
            return;
        };
        let next = (index as i32 + direction).rem_euclid(rows.len() as i32) as usize;
        let next_detail = rows[next].detail();
        self.github_open_detail(project_id, next_detail, window, cx);
    }

    /// A tab's rows after `filter` text is applied — matched against title,
    /// `#number`, author, and labels/head branch.
    pub(super) fn github_filtered_rows(
        &self,
        project_id: Uuid,
        tab: GitHubTab,
        filter: &str,
        _cx: &App,
    ) -> Vec<GitHubListRow> {
        let Some(browser) = self.github_browsers.get(&project_id) else {
            return Vec::new();
        };
        let filter = filter.trim().to_lowercase();
        let matches = |haystacks: &[&str]| {
            filter.is_empty()
                || haystacks
                    .iter()
                    .any(|text| text.to_lowercase().contains(&filter))
        };
        match tab {
            GitHubTab::PullRequests => match &browser.pull_requests {
                GitHubFetch::Loaded(Some(entries)) => entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| {
                        let number = format!("#{}", entry.number);
                        let mut haystacks: Vec<&str> = vec![entry.title.as_str(), number.as_str()];
                        haystacks.extend(entry.author.as_deref());
                        haystacks.extend(entry.head_branch.as_deref());
                        matches(&haystacks)
                    })
                    .map(|(index, entry)| GitHubListRow::PullRequest {
                        entries: entries.clone(),
                        index,
                        detail: GitHubDetailRef {
                            kind: GitHubItemKind::PullRequest,
                            number: entry.number,
                        },
                    })
                    .collect(),
                _ => Vec::new(),
            },
            GitHubTab::Issues => match &browser.issues {
                GitHubFetch::Loaded(Some(entries)) => entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| {
                        let number = format!("#{}", entry.number);
                        let mut haystacks: Vec<&str> = vec![entry.title.as_str(), number.as_str()];
                        haystacks.extend(entry.author.as_deref());
                        haystacks.extend(entry.labels.iter().map(String::as_str));
                        matches(&haystacks)
                    })
                    .map(|(index, entry)| GitHubListRow::Issue {
                        entries: entries.clone(),
                        index,
                        detail: GitHubDetailRef {
                            kind: GitHubItemKind::Issue,
                            number: entry.number,
                        },
                    })
                    .collect(),
                _ => Vec::new(),
            },
        }
    }

    pub(super) fn github_set_query_state(
        &mut self,
        project_id: Uuid,
        state: WorkItemQueryState,
        cx: &mut Context<Self>,
    ) {
        let Some(browser) = self.github_browsers.get_mut(&project_id) else {
            return;
        };
        if browser.query_state == state {
            return;
        }
        browser.query_state = state;
        self.github_refresh(project_id, cx);
    }

    /// The list half of a GitHub tab — rows filtered by the page's filter
    /// field for that tab. The page owns the chrome around it.
    pub(super) fn render_github_list(
        &mut self,
        project_id: Uuid,
        tab: GitHubTab,
        filter: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        // Repo resolution gates the lists: unresolved → resolving, a non-GitHub
        // repo or `gh` problem → its hint, resolved → rows.
        if let Some((repo, availability)) = self
            .github_browsers
            .get(&project_id)
            .and_then(|browser| browser.repo.clone())
        {
            if repo.is_none() {
                let message = match availability {
                    GitHubAvailability::MissingCli => tr!("github.install_gh"),
                    GitHubAvailability::Unauthenticated => tr!("github.auth_gh"),
                    GitHubAvailability::Ready => tr!("github.not_a_repo"),
                };
                return github_centered(
                    icon("icons/github.svg", 16.0, theme.text_tertiary).into_any_element(),
                    message,
                    &theme,
                );
            }
        }
        let rows = self.github_filtered_rows(project_id, tab, filter, cx);
        let Some(browser) = self.github_browsers.get(&project_id) else {
            return div().into_any_element();
        };
        let list_state = browser.list_state.clone();
        let scrollbar = browser.list_scrollbar.clone();
        let fetch = match tab {
            GitHubTab::PullRequests => match &browser.pull_requests {
                GitHubFetch::Loading => None,
                GitHubFetch::Loaded(entries) => Some(entries.is_some()),
            },
            GitHubTab::Issues => match &browser.issues {
                GitHubFetch::Loading => None,
                GitHubFetch::Loaded(entries) => Some(entries.is_some()),
            },
        };
        let filter_active = !filter.trim().is_empty();

        match fetch {
            None => {
                return github_centered(
                    icon("icons/loader-circle.svg", 16.0, theme.text_tertiary).into_any_element(),
                    tr!("github.loading"),
                    &theme,
                );
            }
            Some(false) => {
                return github_centered(
                    icon("icons/alert.svg", 16.0, theme.text_tertiary).into_any_element(),
                    tr!("github.unavailable"),
                    &theme,
                );
            }
            Some(true) if rows.is_empty() => {
                return github_centered(
                    icon("icons/github.svg", 16.0, theme.text_tertiary).into_any_element(),
                    if filter_active {
                        tr!("github.no_matches")
                    } else {
                        match tab {
                            GitHubTab::PullRequests => tr!("github.no_pull_requests"),
                            GitHubTab::Issues => tr!("github.no_issues"),
                        }
                    },
                    &theme,
                );
            }
            _ => {}
        }

        if list_state.item_count() != rows.len() {
            list_state.reset_with_uniform_height(rows.len(), px(GITHUB_LIST_ROW_HEIGHT));
        }
        let entity = cx.entity().downgrade();
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                list(list_state.clone(), move |index, _window, cx| {
                    let Some(row) = rows.get(index).cloned() else {
                        return div().into_any_element();
                    };
                    entity
                        .upgrade()
                        .map(|entity| {
                            entity
                                .update(cx, |this, cx| this.render_github_row(project_id, row, cx))
                        })
                        .unwrap_or_else(|| div().into_any_element())
                })
                .size_full(),
            )
            .child(scrollbar::vertical(&list_state, &scrollbar))
            .into_any_element()
    }

    fn render_github_row(
        &mut self,
        project_id: Uuid,
        row: GitHubListRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let detail = row.detail();
        let focus = self
            .github_browsers
            .get(&project_id)
            .map(|browser| {
                browser
                    .row_focuses
                    .borrow_mut()
                    .entry(detail)
                    .or_insert_with(|| cx.focus_handle())
                    .clone()
            })
            .unwrap_or_else(|| cx.focus_handle());

        let (row_div, title, url) = match &row {
            GitHubListRow::PullRequest { entries, index, .. } => {
                let entry = &entries[*index];
                (
                    github_pull_request_row(entry, &theme),
                    entry.title.clone(),
                    entry.url.clone(),
                )
            }
            GitHubListRow::Issue { entries, index, .. } => {
                let entry = &entries[*index];
                (
                    github_issue_row(entry, &theme),
                    entry.title.clone(),
                    entry.url.clone(),
                )
            }
        };
        let menu = self
            .github_browsers
            .get(&project_id)
            .map(|browser| {
                browser
                    .row_menus
                    .borrow_mut()
                    .entry(detail)
                    .or_insert_with(|| ContextMenuHandle::new(cx))
                    .clone()
            })
            .unwrap_or_else(|| ContextMenuHandle::new(cx));
        let weak = cx.entity().downgrade();

        let framed = row_div
            .id(SharedString::from(format!(
                "github-row-{}-{}",
                match detail.kind {
                    GitHubItemKind::PullRequest => "pr",
                    GitHubItemKind::Issue => "issue",
                },
                detail.number
            )))
            .track_focus(&focus)
            .tab_index(0)
            .tab_stop(true)
            .w_full()
            .h(px(GITHUB_LIST_ROW_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .border_b(hairline())
            .border_color(theme.border)
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .focus_visible(|style| style.bg(theme.overlay))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.github_open_detail(project_id, detail, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.github_open_detail(project_id, detail, window, cx);
                    cx.stop_propagation();
                }
            }));

        context_menu(
            framed,
            format!(
                "github-row-menu-{}-{}",
                detail.number,
                match detail.kind {
                    GitHubItemKind::PullRequest => "pr",
                    GitHubItemKind::Issue => "issue",
                }
            ),
            &menu,
            move |_cx| {
                let mut items = Vec::new();
                let start_weak = weak.clone();
                let prompt = github_task_prompt(detail, &title, &url, false);
                items.push(
                    MenuItem::new(tr!("github.start_task"), move |window, cx| {
                        let _ = start_weak.update(cx, |this, cx| {
                            this.github_start_task(project_id, prompt.clone(), window, cx);
                        });
                    })
                    .icon("icons/plus.svg"),
                );
                let open_url = url.clone();
                items.push(
                    MenuItem::new(tr!("github.open_external"), move |_, cx| {
                        cx.open_url(&open_url);
                    })
                    .icon("icons/external-link.svg"),
                );
                let copy_url = url.clone();
                items.push(
                    MenuItem::new(tr!("github.copy_url"), move |_, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_url.clone()));
                    })
                    .icon("icons/copy.svg"),
                );
                items
            },
        )
    }

    pub(super) fn render_github_detail(
        &mut self,
        project_id: Uuid,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(browser) = self.github_browsers.get(&project_id) else {
            return div().into_any_element();
        };
        let Some(detail) = browser.detail else {
            return div().into_any_element();
        };
        let focus = browser.detail_focus.clone();
        let scroll = browser.detail_scroll.clone();
        let scrollbar = browser.detail_scrollbar.clone();
        let markdown_selection = browser.markdown_selection.clone();
        let selection_for_input = markdown_selection.clone();

        let Some(fetch) = browser.details.get(&detail).cloned() else {
            return github_centered(
                icon("icons/loader-circle.svg", 16.0, theme.text_tertiary).into_any_element(),
                tr!("github.loading"),
                &theme,
            );
        };
        let content = match fetch {
            GitHubFetch::Loading => {
                return github_centered(
                    icon("icons/loader-circle.svg", 16.0, theme.text_tertiary).into_any_element(),
                    tr!("github.loading"),
                    &theme,
                );
            }
            GitHubFetch::Loaded(None) => {
                return github_centered(
                    icon("icons/alert.svg", 16.0, theme.text_tertiary).into_any_element(),
                    tr!("github.unavailable"),
                    &theme,
                );
            }
            GitHubFetch::Loaded(Some(value)) => value,
        };

        let document = self.render_github_detail_document(project_id, detail, &content, cx);

        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                div()
                    .id("github-detail-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .track_focus(&focus)
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        match event.keystroke.key.as_str() {
                            "escape" => {
                                this.github_close_detail(project_id, cx);
                                cx.stop_propagation();
                            }
                            "left" | "[" => {
                                this.github_detail_navigate(project_id, -1, window, cx);
                                cx.stop_propagation();
                            }
                            "right" | "]" => {
                                this.github_detail_navigate(project_id, 1, window, cx);
                                cx.stop_propagation();
                            }
                            _ => {}
                        }
                    }))
                    .child(md::render::frame_reset(markdown_selection.clone()))
                    .child(document),
            )
            // The invisible canvas installs the hidden input that captures
            // drags, so body and comment text stay selectable.
            .child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
                    move |_, region, window, _| {
                        md::render::install_selection_input(
                            region,
                            window,
                            &selection_for_input,
                            None,
                        )
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(scrollbar::vertical(&scroll, &scrollbar))
            .into_any_element()
    }

    /// The detail's header card, markdown body, and comment thread.
    fn render_github_detail_document(
        &mut self,
        project_id: Uuid,
        detail: GitHubDetailRef,
        content: &GitHubItemDetail,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let palette = MarkdownPalette::from_theme(&theme);

        let (
            title,
            url,
            state_icon,
            state_color,
            state_label,
            author,
            meta,
            body,
            comments,
            checks_failing,
        ) = match content {
            GitHubItemDetail::PullRequest(pr) => {
                let summary = &pr.summary;
                (
                    summary.title.clone(),
                    summary.url.clone(),
                    sidebar::sidebar_pull_request_icon(sidebar::pull_request_class(summary)),
                    sidebar::sidebar_pull_request_color(
                        &theme,
                        sidebar::pull_request_class(summary),
                    ),
                    sidebar::sidebar_pull_request_state_label(sidebar::pull_request_class(summary)),
                    summary.author.clone(),
                    github_pr_meta(summary),
                    pr.body.clone(),
                    &pr.comments,
                    pr.summary.check_status == Some(waku_client::PullRequestCheckStatus::Failing),
                )
            }
            GitHubItemDetail::Issue(issue) => {
                let summary = &issue.summary;
                (
                    summary.title.clone(),
                    summary.url.clone(),
                    match summary.state {
                        IssueState::Open => "icons/info.svg",
                        IssueState::Closed => "icons/check.svg",
                    },
                    match summary.state {
                        IssueState::Open => theme.success,
                        IssueState::Closed => theme.text_secondary,
                    },
                    match summary.state {
                        IssueState::Open => tr!("github.issue_open"),
                        IssueState::Closed => tr!("github.issue_closed"),
                    },
                    summary.author.clone(),
                    github_issue_meta(summary),
                    issue.body.clone(),
                    &issue.comments,
                    false,
                )
            }
        };

        let mut section = div()
            .w_full()
            .max_w(px(860.0))
            .mx_auto()
            .px(px(20.0))
            .pt(px(16.0))
            .pb(px(32.0))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .text_color(theme.text);

        // Header: state glyph + number, title, meta line, actions.
        section = section.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(state_icon, 13.0, state_color))
                .child(
                    div()
                        .text_size(sp(12.5))
                        .text_color(state_color)
                        .child(format!("{} · #{}", state_label, detail.number)),
                )
                .child(div().flex_1())
                .child({
                    let prompt = github_task_prompt(detail, &title, &url, false);
                    github_detail_action(
                        "github-start-task",
                        "icons/plus.svg",
                        tr!("github.start_task"),
                        &theme,
                        cx.listener(move |this, _, window, cx| {
                            this.github_start_task(project_id, prompt.clone(), window, cx);
                        }),
                    )
                })
                .when(checks_failing, |element| {
                    let prompt = github_task_prompt(detail, &title, &url, true);
                    element.child(github_detail_action(
                        "github-fix-checks",
                        "icons/hammer.svg",
                        tr!("github.fix_checks"),
                        &theme,
                        cx.listener(move |this, _, window, cx| {
                            this.github_start_task(project_id, prompt.clone(), window, cx);
                        }),
                    ))
                })
                .child(
                    div()
                        .id("github-open-external")
                        .h(px(24.0))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .cursor_default()
                        .hover(|style| style.bg(theme.overlay))
                        .active(|style| style.bg(theme.overlay_strong))
                        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
                        .tooltip(Tooltip::text(tr!("github.open_external")))
                        .child(icon("icons/external-link.svg", 12.0, theme.text_secondary))
                        .child(
                            div()
                                .text_size(sp(12.0))
                                .text_color(theme.text_secondary)
                                .child(tr!("github.open_external")),
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.open_url(&url);
                        })),
                ),
        );
        section = section.child(
            div()
                .text_size(sp(17.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        );
        let mut meta_parts = Vec::new();
        if let Some(author) = author {
            meta_parts.push(author);
        }
        meta_parts.extend(meta);
        section = section.child(
            div()
                .text_size(sp(12.0))
                .text_color(theme.text_secondary)
                .child(meta_parts.join(" · ")),
        );

        section = section.child(div().w_full().h(hairline()).bg(theme.border));

        if let Some(body) = body.filter(|body| !body.trim().is_empty()) {
            section = section.child(self.github_markdown_section(
                project_id,
                Rc::from("body"),
                &body,
                &palette,
                cx,
            ));
        } else {
            section = section.child(
                div()
                    .text_size(sp(12.5))
                    .text_color(theme.text_tertiary)
                    .child(tr!("github.no_description")),
            );
        }

        for (index, comment) in comments.iter().enumerate() {
            section =
                section.child(self.github_comment_card(project_id, index, comment, &palette, cx));
        }

        section
    }

    /// A markdown block rendered through the transcript engine, cached per
    /// key so unchanged text never re-parses.
    fn github_markdown_section(
        &self,
        project_id: Uuid,
        key: Rc<str>,
        text: &str,
        palette: &MarkdownPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(browser) = self.github_browsers.get(&project_id) else {
            return div().into_any_element();
        };
        let mut cache = browser.markdown.borrow_mut();
        let view = cache.entry(key.clone()).or_insert_with(MarkdownView::new);
        view.set_text(text, false);
        let ctx = MarkdownCtx::new(
            key.to_string(),
            palette,
            MarkdownMetrics::document(self.state.ui_font_size, self.state.code_font_size),
            browser.markdown_selection.clone(),
        )
        .with_families(crate::fonts::current(cx))
        .with_math_enabled(self.state.render_math)
        .with_link_handler(self.markdown_link_handler.clone());
        md::render::markdown(view, &ctx).unwrap_or_else(|| div().into_any_element())
    }

    fn github_comment_card(
        &self,
        project_id: Uuid,
        index: usize,
        comment: &WorkItemComment,
        palette: &MarkdownPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = Theme::current(cx);
        let author = comment
            .author
            .clone()
            .unwrap_or_else(|| tr!("github.unknown_author"));
        let when = comment
            .created_at
            .map(|created| sidebar::format_time_ago(unix_time().saturating_sub(created)))
            .unwrap_or_default();
        div()
            .w_full()
            .rounded(px(8.0))
            .border(hairline())
            .border_color(theme.border)
            .bg(theme.inset)
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .px(px(12.0))
                    .py(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .border_b(hairline())
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(author),
                    )
                    .child(
                        div()
                            .text_size(sp(11.5))
                            .text_color(theme.text_tertiary)
                            .child(when),
                    ),
            )
            .child(
                div()
                    .px(px(12.0))
                    .py(px(10.0))
                    .child(self.github_markdown_section(
                        project_id,
                        Rc::from(format!("comment-{index}")),
                        &comment.body,
                        palette,
                        cx,
                    )),
            )
    }
}

/// Read one item's detail off the daemon — shared by the on-open and
/// refresh paths.
fn github_fetch_detail(
    workspace: &waku_client::WorkspaceClient,
    cwd: &Path,
    detail: GitHubDetailRef,
) -> Option<GitHubItemDetail> {
    match detail.kind {
        GitHubItemKind::PullRequest => workspace
            .request(waku_client::WorkspaceOperation::GetPullRequest {
                cwd: cwd.to_path_buf(),
                number: detail.number,
            })
            .ok()
            .and_then(|result| match result {
                waku_client::WorkspaceResult::PullRequest { detail } => detail,
                _ => None,
            })
            .map(GitHubItemDetail::PullRequest),
        GitHubItemKind::Issue => workspace
            .request(waku_client::WorkspaceOperation::GetIssue {
                cwd: cwd.to_path_buf(),
                number: detail.number,
            })
            .ok()
            .and_then(|result| match result {
                waku_client::WorkspaceResult::Issue { detail } => detail,
                _ => None,
            })
            .map(GitHubItemDetail::Issue),
    }
}

/// The row's own content — icon, number, title, meta — without its
/// interactive wrapper.
fn github_pull_request_row(pr: &PullRequestSummary, theme: &Theme) -> Div {
    let color = sidebar::sidebar_pull_request_color(theme, sidebar::pull_request_class(pr));
    let mut meta: Vec<String> = Vec::new();
    if let Some(author) = &pr.author {
        meta.push(author.clone());
    }
    if let Some(head) = &pr.head_branch {
        meta.push(format!("{head} → {}", pr.base_branch));
    }
    if let Some(updated) = pr.updated_at {
        meta.push(sidebar::format_time_ago(
            unix_time().saturating_sub(updated),
        ));
    }
    let diff = match (pr.additions, pr.deletions) {
        (Some(add), Some(del)) => Some(format!("+{add}/-{del}")),
        _ => None,
    };
    div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(icon(
            sidebar::sidebar_pull_request_icon(sidebar::pull_request_class(pr)),
            13.0,
            color,
        ))
        .child(
            div()
                .flex_none()
                .text_size(sp(12.0))
                .text_color(color)
                .child(format!("#{}", pr.number)),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(sp(12.5))
                .text_color(theme.text)
                .child(pr.title.clone()),
        )
        .when_some(pr.check_status, |element, status| {
            element.child(icon(
                sidebar::sidebar_check_status_icon(status),
                11.5,
                sidebar::sidebar_check_status_color(theme, status),
            ))
        })
        .when_some(pr.review_decision, |element, decision| {
            element.child(icon(
                sidebar::sidebar_review_decision_icon(decision),
                11.5,
                sidebar::sidebar_review_decision_color(theme, decision),
            ))
        })
        .when_some(diff, |element, diff| {
            element.child(
                div()
                    .flex_none()
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(diff),
            )
        })
        .child(
            div()
                .flex_none()
                .text_size(sp(11.5))
                .text_color(theme.text_secondary)
                .child(meta.join(" · ")),
        )
}

fn github_issue_row(issue: &IssueSummary, theme: &Theme) -> Div {
    let (icon_path, color) = match issue.state {
        IssueState::Open => ("icons/info.svg", theme.success),
        IssueState::Closed => ("icons/check.svg", theme.text_secondary),
    };
    let mut meta: Vec<String> = Vec::new();
    if let Some(author) = &issue.author {
        meta.push(author.clone());
    }
    if let Some(updated) = issue.updated_at {
        meta.push(sidebar::format_time_ago(
            unix_time().saturating_sub(updated),
        ));
    }
    div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(icon(icon_path, 13.0, color))
        .child(
            div()
                .flex_none()
                .text_size(sp(12.0))
                .text_color(color)
                .child(format!("#{}", issue.number)),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(sp(12.5))
                .text_color(theme.text)
                .child(issue.title.clone()),
        )
        .children(issue.labels.iter().take(3).map(|label| {
            div()
                .flex_none()
                .px(px(6.0))
                .h(px(18.0))
                .rounded(px(9.0))
                .bg(theme.overlay)
                .flex()
                .items_center()
                .text_size(sp(10.5))
                .text_color(theme.text_secondary)
                .child(label.clone())
        }))
        .child(
            div()
                .flex_none()
                .text_size(sp(11.5))
                .text_color(theme.text_secondary)
                .child(meta.join(" · ")),
        )
}

fn github_pr_meta(pr: &PullRequestSummary) -> Vec<String> {
    let mut meta = Vec::new();
    if let Some(head) = &pr.head_branch {
        meta.push(format!("{head} → {}", pr.base_branch));
    }
    if let Some(created) = pr.created_at {
        meta.push(sidebar::format_time_ago(
            unix_time().saturating_sub(created),
        ));
    }
    meta
}

fn github_issue_meta(issue: &IssueSummary) -> Vec<String> {
    let mut meta = issue.labels.clone();
    if let Some(created) = issue.created_at {
        meta.push(sidebar::format_time_ago(
            unix_time().saturating_sub(created),
        ));
    }
    meta
}

/// A detail-header action chip: icon + label, hover and focus treatments
/// matching the list rows.
fn github_detail_action(
    id: &'static str,
    icon_path: &'static str,
    label: String,
    theme: &Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(24.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(5.0))
        .cursor_default()
        .hover(|style| style.bg(theme.overlay))
        .active(|style| style.bg(theme.overlay_strong))
        .focus_visible(|style| style.border(hairline()).border_color(theme.accent))
        .child(icon(icon_path, 12.0, theme.text_secondary))
        .child(
            div()
                .text_size(sp(12.0))
                .text_color(theme.text_secondary)
                .child(label),
        )
        .on_click(on_click)
}

/// The agent-facing prompt a "Start a task" action seeds. Not localized —
/// it is input for the agent, not UI copy.
fn github_task_prompt(detail: GitHubDetailRef, title: &str, url: &str, fix_checks: bool) -> String {
    let kind = match detail.kind {
        GitHubItemKind::PullRequest => "pull request",
        GitHubItemKind::Issue => "issue",
    };
    if fix_checks {
        format!(
            "Fix the failing checks on {kind} #{}: {title}\n{url}",
            detail.number
        )
    } else {
        format!("Work on {kind} #{}: {title}\n{url}", detail.number)
    }
}

/// A centered icon + message for a list surface's non-list states.
pub(super) fn github_centered(
    icon_element: AnyElement,
    message: String,
    theme: &Theme,
) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .child(icon_element)
        .child(
            div()
                .text_size(sp(13.0))
                .text_color(theme.text_secondary)
                .child(message),
        )
        .into_any_element()
}
