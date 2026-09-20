//! The GitHub notification inbox — a user-level surface, not a project one.
//!
//! The 5-second tick in `app.rs` calls `maybe_poll_notifications`; the
//! daemon turns that into `gh auth token` plus a conditional GET of
//! `/notifications` and hands back a `NotificationPoll`. Render only ever
//! paints what has already landed — a miss means "not known yet" — and
//! writes flip their thread optimistically so triage feels instant while
//! the daemon call and the next poll reconcile.
//!
//! Triage is strictly faithful to GitHub's inbox: `unread → read → done` is
//! one-way, there is no snooze, and `done` deletes the thread. The only
//! local state is `seen`, which exists solely to dedup OS banners.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::*;
use waku_client::{
    GitHubAvailability, NotificationPoll, NotificationReason, NotificationSubjectType,
    NotificationThread, PullRequestSummary, WorkspaceOperation, WorkspaceResult,
};

/// GitHub's published cadence; the endpoint's own `X-Poll-Interval`
/// overrides it per poll.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);
/// Never poll faster than this however the endpoint asks — the advisory
/// exists so clients do not.
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// And never slower — a triage surface this stale stops being one.
const MAX_POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

const INBOX_ROW_HEIGHT: f32 = 30.0;
const INBOX_CONTENT_MAX_WIDTH: f32 = 900.0;
const INBOX_CONTENT_MARGIN: f32 = 60.0;
const INBOX_HEADER_HEIGHT: f32 = 48.0;
const INBOX_TOOLBAR_HEIGHT: f32 = 40.0;

/// Reasons that may raise an OS banner — the high-signal set. Everything
/// else stays silent; `subscribed`/`comment` traffic is what the inbox's
/// own triage is for.
const BANNER_REASONS: &[NotificationReason] = &[
    NotificationReason::ReviewRequested,
    NotificationReason::Mention,
    NotificationReason::TeamMention,
    NotificationReason::Assign,
    NotificationReason::CiActivity,
    NotificationReason::SecurityAlert,
];

/// One flattened list row — a repo group header or a thread. Uniform height
/// keeps the `list()` virtualization simple.
#[derive(Clone)]
enum InboxRow {
    Group(String),
    Thread(NotificationThread),
}

/// The inbox's store — all of it cheap to paint: the page reads `threads`,
/// the sidebar badge reads `unread_count`, and a session row's chip dots
/// against `unread_subjects`. Nothing here reaches past memory.
pub(super) struct Inbox {
    /// The page claims the main area, like `projects_page`.
    pub open: bool,
    /// Why the inbox cannot be read, when `gh` itself is the problem;
    /// `None` until the first poll answers.
    pub availability: Option<GitHubAvailability>,
    /// The current page of threads, newest-updated first.
    pub threads: Vec<NotificationThread>,
    /// Read threads join the list while the toolbar toggle is on — GitHub
    /// cannot tell read from done, so they render dimmed either way.
    pub show_read: bool,
    /// The previous poll's conditional-request validators — `ETag` is what
    /// github.com answers; `Last-Modified` covers hosts that send one
    /// instead.
    etag: Option<String>,
    last_modified: Option<String>,
    poll_interval: Duration,
    next_poll: Instant,
    in_flight: bool,
    /// (id, updated_at) pairs already seen — GitHub flips a thread back to
    /// unread on new activity, so freshness is the pair, not the id.
    seen: HashSet<(String, u64)>,
    /// The first successful fetch only primes `seen` — a cold inbox is not
    /// news.
    primed: bool,
    filter_input: Entity<TextInput>,
    list_state: ListState,
    list_scrollbar: Rc<ScrollbarState>,
    /// Row focus handles keyed by thread id — same pattern as the GitHub
    /// browser's rows.
    row_focuses: RefCell<HashMap<String, FocusHandle>>,
    /// "owner/name" lowercased → project — a notification's local home,
    /// resolved in one background pass when the page first needs it.
    repo_projects: HashMap<String, Uuid>,
    repo_projects_fingerprint: Cell<Option<u64>>,
    /// (repo, number) carrying an unread thread — what a session row's
    /// pull-request chip dots on.
    unread_subjects: HashSet<(String, u64)>,
    /// Writes in flight — a row's buttons dim instead of double-firing.
    mutating: HashSet<String>,
}

impl Inbox {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Waku>) -> Self {
        let filter_input = cx.new(|cx| {
            TextInput::new(window, cx)
                .accessibility_label(tr!("notifications.filter_placeholder"))
                .placeholder(tr!("notifications.filter_placeholder"))
                .clear_on_escape()
        });
        cx.subscribe(
            &filter_input,
            |_this: &mut Waku, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Edited) {
                    cx.notify();
                }
            },
        )
        .detach();
        Self {
            open: false,
            availability: None,
            threads: Vec::new(),
            show_read: false,
            etag: None,
            last_modified: None,
            poll_interval: DEFAULT_POLL_INTERVAL,
            next_poll: Instant::now(),
            in_flight: false,
            seen: HashSet::new(),
            primed: false,
            filter_input,
            list_state: ListState::new(0, ListAlignment::Top, px(INBOX_ROW_HEIGHT)),
            list_scrollbar: ScrollbarState::new(),
            row_focuses: RefCell::new(HashMap::new()),
            repo_projects: HashMap::new(),
            repo_projects_fingerprint: Cell::new(None),
            unread_subjects: HashSet::new(),
            mutating: HashSet::new(),
        }
    }

    /// The endpoint's own cadence, clamped to what makes a triage surface.
    fn adopt_interval(&mut self, seconds: Option<u64>) {
        self.poll_interval = seconds
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_POLL_INTERVAL)
            .clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL);
    }

    /// The (repo, number) set the session rows dot against — rebuilt on
    /// every store change, so a frame never recomputes it.
    fn rebuild_unread_subjects(&mut self) {
        self.unread_subjects = self
            .threads
            .iter()
            .filter(|thread| thread.unread)
            .filter_map(|thread| {
                thread
                    .number
                    .map(|number| (thread.repo.to_lowercase(), number))
            })
            .collect();
    }

    /// Does an unread thread cover this pull request URL? The sidebar and
    /// header chips ask per row, so this is a set hit, not a scan.
    pub(super) fn has_unread_pull_request(&self, url: &str) -> bool {
        pull_request_key(url).is_some_and(|key| self.unread_subjects.contains(&key))
    }

    /// Unread thread count — the sidebar row's badge.
    pub(super) fn unread_count(&self) -> usize {
        self.threads.iter().filter(|thread| thread.unread).count()
    }

    /// Drop everything a poll produced — the experiment toggle's off path,
    /// so a disabled inbox holds no stale threads, dots, or validators.
    pub(super) fn reset(&mut self) {
        self.open = false;
        self.availability = None;
        self.threads.clear();
        self.etag = None;
        self.last_modified = None;
        self.next_poll = Instant::now();
        self.seen.clear();
        self.primed = false;
        self.unread_subjects.clear();
        self.mutating.clear();
    }

    /// The project whose checkout resolves to this repo, when one is known.
    fn project_for_repo(&self, repo: &str) -> Option<Uuid> {
        self.repo_projects.get(&repo.to_lowercase()).copied()
    }
}

/// `https://github.com/o/r/pull/12` → `("o/r", 12)` — matches a session's
/// pull-request entries to a notification's subject. Host-agnostic: the
/// path shape is the same on Enterprise.
pub(super) fn pull_request_key(url: &str) -> Option<(String, u64)> {
    let path = url.split_once("://")?.1.split_once('/')?.1;
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    if segments.next()? != "pull" {
        return None;
    }
    let number = segments.next()?.parse().ok()?;
    Some((format!("{owner}/{name}").to_lowercase(), number))
}

/// The glyph a subject type renders — state glyphs belong to the repo-level
/// surfaces; the inbox's icons only name the kind.
fn subject_icon(kind: NotificationSubjectType) -> &'static str {
    match kind {
        NotificationSubjectType::PullRequest => "icons/git-pull-request-arrow.svg",
        NotificationSubjectType::Issue => "icons/info.svg",
        NotificationSubjectType::Discussion => "icons/chat.svg",
        NotificationSubjectType::Release => "icons/package.svg",
        NotificationSubjectType::Commit => "icons/git-commit-horizontal.svg",
        NotificationSubjectType::CheckSuite | NotificationSubjectType::WorkflowRun => {
            "icons/play.svg"
        }
        NotificationSubjectType::RepositoryInvitation => "icons/folder-new.svg",
        NotificationSubjectType::RepositoryVulnerabilityAlert => "icons/alert.svg",
        NotificationSubjectType::Other => "icons/bell.svg",
    }
}

fn reason_label(reason: NotificationReason) -> String {
    match reason {
        NotificationReason::Assign => tr!("notifications.reason_assign"),
        NotificationReason::Author => tr!("notifications.reason_author"),
        NotificationReason::CiActivity => tr!("notifications.reason_ci_activity"),
        NotificationReason::Comment => tr!("notifications.reason_comment"),
        NotificationReason::Invitation => tr!("notifications.reason_invitation"),
        NotificationReason::Manual => tr!("notifications.reason_manual"),
        NotificationReason::Mention => tr!("notifications.reason_mention"),
        NotificationReason::ReviewRequested => tr!("notifications.reason_review_requested"),
        NotificationReason::SecurityAlert => tr!("notifications.reason_security_alert"),
        NotificationReason::StateChange => tr!("notifications.reason_state_change"),
        NotificationReason::Subscribed => tr!("notifications.reason_subscribed"),
        NotificationReason::TeamMention => tr!("notifications.reason_team_mention"),
        NotificationReason::Other => tr!("notifications.reason_other"),
    }
}

impl Waku {
    /// The 5-second tick's slice of the notification poll: fires a
    /// conditional fetch when the interval says one is due. All work is on
    /// the background executor; only the store swap touches the UI.
    pub(super) fn maybe_poll_notifications(&mut self, cx: &mut Context<Self>) {
        if !self.state.github_enabled
            || self.notifications.in_flight
            || Instant::now() < self.notifications.next_poll
        {
            return;
        }
        self.notifications.in_flight = true;
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        let etag = self.notifications.etag.clone();
        let if_modified_since = self.notifications.last_modified.clone();
        let all = self.notifications.show_read;
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    workspace.request(WorkspaceOperation::ListNotifications {
                        all,
                        etag,
                        if_modified_since,
                    })
                })
                .await;
            let _ = waku.update(cx, |waku, cx| waku.notifications_apply_poll(result, cx));
        })
        .detach();
    }

    fn notifications_apply_poll(
        &mut self,
        result: anyhow::Result<WorkspaceResult>,
        cx: &mut Context<Self>,
    ) {
        self.notifications.in_flight = false;
        self.notifications.next_poll = Instant::now() + self.notifications.poll_interval;
        if !self.state.github_enabled {
            // Toggled off mid-flight — the reset already owns the store.
            return;
        }
        let Ok(WorkspaceResult::Notifications { poll }) = result else {
            // A transient failure keeps the last page — triage state is
            // still good enough to render, and the next tick retries.
            cx.notify();
            return;
        };
        match poll {
            NotificationPoll::Unavailable { availability } => {
                self.notifications.availability = Some(availability);
            }
            NotificationPoll::Unchanged {
                poll_interval_seconds,
            } => {
                self.notifications.availability = Some(GitHubAvailability::Ready);
                self.notifications.adopt_interval(poll_interval_seconds);
            }
            NotificationPoll::Changed {
                threads,
                etag,
                last_modified,
                poll_interval_seconds,
            } => {
                self.notifications.availability = Some(GitHubAvailability::Ready);
                self.notifications.adopt_interval(poll_interval_seconds);
                self.notifications_banner_new(&threads, cx);
                self.notifications.threads = threads;
                if etag.is_some() {
                    self.notifications.etag = etag;
                }
                if last_modified.is_some() {
                    self.notifications.last_modified = last_modified;
                }
                self.notifications.rebuild_unread_subjects();
            }
        }
        cx.notify();
    }

    /// One OS banner per thread that arrived or re-fired unread since the
    /// last poll — deduped on (id, updated_at) because GitHub flips updated
    /// threads back to unread.
    fn notifications_banner_new(&mut self, threads: &[NotificationThread], cx: &mut App) {
        for thread in threads {
            let key = (thread.id.clone(), thread.updated_at);
            if self.notifications.primed
                && thread.unread
                && !self.notifications.seen.contains(&key)
                && BANNER_REASONS.contains(&thread.reason)
            {
                crate::platform::show_task_notification(
                    &format!("github-notification-{}", thread.id),
                    &thread.title,
                    &format!("{} · {}", thread.repo, reason_label(thread.reason)),
                    cx,
                );
            }
            self.notifications.seen.insert(key);
        }
        self.notifications.primed = true;
        let live: HashSet<&str> = threads.iter().map(|thread| thread.id.as_str()).collect();
        self.notifications
            .seen
            .retain(|(id, _)| live.contains(id.as_str()));
    }

    /// The optimistic-write spine: flip the local state now, fire the daemon
    /// call, and let the next poll reconcile. A failure drops the
    /// conditional marker so the next tick refetches the truth — and puts
    /// back the removed thread when the write was a Done, so a rejected
    /// delete never loses the row to the next poll.
    fn notification_write(
        &mut self,
        key: String,
        operation: WorkspaceOperation,
        restore: Option<(usize, NotificationThread)>,
        cx: &mut Context<Self>,
    ) {
        self.notifications.mutating.insert(key.clone());
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { workspace.request(operation) })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                waku.notifications.mutating.remove(&key);
                if result.is_err() {
                    if let Some((index, thread)) = restore {
                        let index = index.min(waku.notifications.threads.len());
                        waku.notifications.threads.insert(index, thread);
                        waku.notifications.rebuild_unread_subjects();
                    }
                    waku.notifications.etag = None;
                    waku.notifications.last_modified = None;
                    waku.notifications.next_poll = Instant::now();
                    waku.show_toast(tr!("notifications.update_failed"));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opening a thread is reading it — like github.com does.
    pub(super) fn notification_mark_read(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        if self.notifications.mutating.contains(thread_id) {
            return;
        }
        if let Some(thread) = self
            .notifications
            .threads
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        {
            thread.unread = false;
        }
        self.notifications.rebuild_unread_subjects();
        self.notification_write(
            thread_id.to_owned(),
            WorkspaceOperation::MarkNotificationRead {
                thread_id: thread_id.to_owned(),
            },
            None,
            cx,
        );
        cx.notify();
    }

    /// Done removes the thread outright — irreversible on GitHub, so it is
    /// only ever a per-row action, never a bulk one, and it asks first.
    /// The prompt names the thread; a decline leaves the inbox untouched.
    pub(super) fn notification_mark_done(
        &mut self,
        thread_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.notifications.mutating.contains(thread_id) {
            return;
        }
        let Some(thread) = self
            .notifications
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .cloned()
        else {
            return;
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &tr!("notifications.confirm_done", title = thread.title.clone()),
            Some(&tr!("notifications.confirm_done_detail")),
            &[
                gpui::PromptButton::cancel(tr!("common.cancel")),
                gpui::PromptButton::ok(tr!("notifications.mark_done")),
            ],
            cx,
        );
        cx.spawn(async move |waku, cx| {
            if answer.await.ok() != Some(1) {
                return;
            }
            let _ = waku.update(cx, |waku, cx| {
                waku.notification_done_confirmed(thread, cx);
            });
        })
        .detach();
    }

    /// The confirmed half of Done: drop the row optimistically and carry the
    /// thread into the write so a failed delete puts it straight back.
    fn notification_done_confirmed(&mut self, thread: NotificationThread, cx: &mut Context<Self>) {
        if self.notifications.mutating.contains(&thread.id) {
            return;
        }
        let position = self
            .notifications
            .threads
            .iter()
            .position(|entry| entry.id == thread.id);
        self.notifications
            .threads
            .retain(|entry| entry.id != thread.id);
        self.notifications.rebuild_unread_subjects();
        self.notification_write(
            thread.id.clone(),
            WorkspaceOperation::MarkNotificationDone {
                thread_id: thread.id.clone(),
            },
            position.map(|index| (index, thread)),
            cx,
        );
        cx.notify();
    }

    /// The one bulk write: a repo group header marks its threads read.
    pub(super) fn notification_mark_repo_read(&mut self, repo: &str, cx: &mut Context<Self>) {
        if self.notifications.mutating.contains(repo) {
            return;
        }
        for thread in self
            .notifications
            .threads
            .iter_mut()
            .filter(|thread| thread.repo == repo)
        {
            thread.unread = false;
        }
        self.notifications.rebuild_unread_subjects();
        self.notification_write(
            repo.to_owned(),
            WorkspaceOperation::MarkRepoNotificationsRead {
                repo: repo.to_owned(),
            },
            None,
            cx,
        );
        cx.notify();
    }

    pub(super) fn notification_mark_all_read(&mut self, cx: &mut Context<Self>) {
        if self.notifications.mutating.contains("all") {
            return;
        }
        for thread in self.notifications.threads.iter_mut() {
            thread.unread = false;
        }
        self.notifications.rebuild_unread_subjects();
        self.notification_write(
            "all".to_owned(),
            WorkspaceOperation::MarkAllNotificationsRead,
            None,
            cx,
        );
        cx.notify();
    }

    /// The row's primary action: the in-app detail when the subject is a
    /// pull request or issue in a local project, the web page otherwise.
    pub(super) fn notification_open(
        &mut self,
        thread_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self
            .notifications
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .cloned()
        else {
            return;
        };
        self.notification_mark_read(thread_id, cx);
        let detail = match (thread.subject_type, thread.number) {
            (NotificationSubjectType::PullRequest, Some(number)) => Some(github::GitHubDetailRef {
                kind: github::GitHubItemKind::PullRequest,
                number,
            }),
            (NotificationSubjectType::Issue, Some(number)) => Some(github::GitHubDetailRef {
                kind: github::GitHubItemKind::Issue,
                number,
            }),
            _ => None,
        };
        if let Some(detail) = detail
            && let Some(project_id) = self.notifications.project_for_repo(&thread.repo)
            && self.github_ensure_browser(project_id, window, cx)
        {
            self.github_open_detail(project_id, detail, window, cx);
            return;
        }
        cx.open_url(&thread.url.unwrap_or(thread.repo_url));
    }

    /// "Fix" on a notification: the session that opened the pull request
    /// keeps the work — review findings are a continuation, not a new task.
    /// With no owning session but a local project, the detail flow's
    /// new-worktree task takes it. Anything else has no Fix button.
    pub(super) fn notification_fix(
        &mut self,
        thread_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self
            .notifications
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .cloned()
        else {
            return;
        };
        if thread.subject_type != NotificationSubjectType::PullRequest {
            return;
        }
        let Some(number) = thread.number else {
            return;
        };

        if let Some((session_id, summary)) = self.session_owning_pull_request(&thread) {
            self.notifications.open = false;
            self.select_session(session_id, cx);
            let prompt = github::github_fix_prompt(&summary, None);
            self.github_fill_fix_prompt(prompt, cx);
            let focus = self.composer_focus(cx);
            window.focus(&focus, cx);
            return;
        }

        let Some(project_id) = self.notifications.project_for_repo(&thread.repo) else {
            return;
        };
        if !self.github_ensure_browser(project_id, window, cx) {
            return;
        }
        let detail = github::GitHubDetailRef {
            kind: github::GitHubItemKind::PullRequest,
            number,
        };
        if let Some(browser) = self.github_browsers.get_mut(&project_id) {
            browser.fix_pending.insert(detail, window.window_handle());
        }
        self.notifications.open = false;
        self.github_ensure_detail(project_id, detail, cx);
        self.github_maybe_run_pending_fix(project_id, detail, cx);
    }

    /// The session whose pull-request window covers this thread's subject —
    /// the same attribution the sidebar badge uses.
    fn session_owning_pull_request(
        &self,
        thread: &NotificationThread,
    ) -> Option<(Uuid, PullRequestSummary)> {
        let number = thread.number?;
        let repo = thread.repo.to_lowercase();
        let pull_requests = self.sidebar_pull_requests.borrow();
        self.state
            .sessions
            .iter()
            .filter(|session| session.archived_at.is_none())
            .find_map(|session| {
                let entries = pull_requests.get(&session.id)?;
                let window = sidebar::session_pull_request_window(session);
                sidebar::session_pull_requests_in_window(entries, window)
                    .into_iter()
                    .find(|entry| {
                        pull_request_key(&entry.url)
                            .is_some_and(|key| key == (repo.clone(), number))
                    })
                    .map(|entry| (session.id, entry.clone()))
            })
    }

    /// Whether the thread's pull request belongs to a session's window —
    /// used by the row's Fix affordance, once per frame over all rows
    /// rather than per row per session.
    fn owned_pull_request_keys(&self) -> HashSet<(String, u64)> {
        let pull_requests = self.sidebar_pull_requests.borrow();
        let mut keys = HashSet::new();
        for session in &self.state.sessions {
            if session.archived_at.is_some() {
                continue;
            }
            let Some(entries) = pull_requests.get(&session.id) else {
                continue;
            };
            let window = sidebar::session_pull_request_window(session);
            for entry in sidebar::session_pull_requests_in_window(entries, window) {
                if let Some(key) = pull_request_key(&entry.url) {
                    keys.insert(key);
                }
            }
        }
        keys
    }

    /// "owner/name" → project for every checkout `gh` can resolve — the
    /// deep-link map. One background pass over the project set,
    /// fingerprinted like the sidebar's pull-request scan.
    fn ensure_notification_repo_projects(&mut self, cx: &mut Context<Self>) {
        let mut fingerprint = 0x51ab_9d4e_1f7c_3a05;
        let mut targets = Vec::new();
        for project in &self.state.projects {
            if project.is_projectless() {
                continue;
            }
            fingerprint = transcript::mix_uuid(fingerprint, project.id);
            targets.push((project.id, project.path.clone()));
        }
        if self
            .notifications
            .repo_projects_fingerprint
            .get()
            .is_some_and(|known| known == fingerprint)
        {
            return;
        }
        self.notifications
            .repo_projects_fingerprint
            .set(Some(fingerprint));
        self.notifications.repo_projects.clear();
        if targets.is_empty() {
            return;
        }
        let workspace = waku_client::WorkspaceClient::new(self.daemon.client());
        cx.spawn(async move |waku, cx| {
            let resolved = cx
                .background_executor()
                .spawn(async move {
                    let mut map = HashMap::new();
                    for (project_id, cwd) in targets {
                        if let Ok(WorkspaceResult::GitHubRepo {
                            repo: Some(repo), ..
                        }) = workspace.request(WorkspaceOperation::ResolveGitHubRepo { cwd })
                        {
                            map.insert(
                                format!("{}/{}", repo.owner, repo.name).to_lowercase(),
                                project_id,
                            );
                        }
                    }
                    map
                })
                .await;
            let _ = waku.update(cx, |waku, cx| {
                waku.notifications.repo_projects = resolved;
                cx.notify();
            });
        })
        .detach();
    }

    /// The page's open/close — same "claims the main area" contract as the
    /// Projects page.
    pub(super) fn open_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.github_enabled {
            return;
        }
        self.settings_page = None;
        self.projects_page = None;
        self.drafts_page = false;
        self.automations_page = false;
        self.automations_detail = None;
        self.selected_terminal = None;
        // An activation still in flight must not hand the area back once
        // its hydration lands — same guard the Projects page takes.
        self.pending_session_activation = None;
        if self
            .sidebar_collapsed_groups
            .insert(SidebarGroup::Terminals)
        {
            self.sidebar_rows_fingerprint.set(None);
        }
        self.notifications.open = true;
        // Opening the page is an eager poll — freshness beats interval for
        // a user-initiated view.
        self.notifications.next_poll = Instant::now();
        let focus = self.notifications.filter_input.read(cx).focus();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn close_inbox(&mut self, cx: &mut Context<Self>) {
        self.notifications.open = false;
        cx.notify();
    }

    pub(super) fn toggle_inbox_page_action(
        &mut self,
        _: &ToggleInboxPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.notifications.open {
            self.close_inbox(cx);
        } else {
            self.open_inbox(window, cx);
        }
    }

    pub(super) fn dismiss_inbox_action(
        &mut self,
        _: &DismissInbox,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The filter field consumes Escape to clear itself first; only an
        // empty filter lets the key reach the page.
        self.close_inbox(cx);
    }

    /// The flattened row list: threads grouped by repo, groups ordered by
    /// their newest thread. Filtering matches title, repo, and the reason
    /// label.
    fn inbox_rows(&self, filter: &str) -> Vec<InboxRow> {
        let needle = filter.trim().to_lowercase();
        let matches = |thread: &NotificationThread| {
            needle.is_empty()
                || thread.title.to_lowercase().contains(&needle)
                || thread.repo.to_lowercase().contains(&needle)
                || reason_label(thread.reason).to_lowercase().contains(&needle)
        };
        let mut groups: Vec<(String, Vec<NotificationThread>)> = Vec::new();
        let mut order: HashMap<String, usize> = HashMap::new();
        for thread in &self.notifications.threads {
            if !matches(thread) {
                continue;
            }
            let index = match order.get(&thread.repo) {
                Some(index) => *index,
                None => {
                    let index = groups.len();
                    order.insert(thread.repo.clone(), index);
                    groups.push((thread.repo.clone(), Vec::new()));
                    index
                }
            };
            groups[index].1.push(thread.clone());
        }
        // Threads arrive newest-first; keep that inside each group and order
        // the groups by their own newest thread.
        groups.sort_by_key(|(_, threads)| {
            std::cmp::Reverse(threads.iter().map(|t| t.updated_at).max().unwrap_or(0))
        });
        let mut rows = Vec::new();
        for (repo, threads) in groups {
            rows.push(InboxRow::Group(repo));
            rows.extend(threads.into_iter().map(InboxRow::Thread));
        }
        rows
    }

    /// Arrow-key navigation over the thread rows — group headers stay out
    /// of the focus order.
    fn inbox_focus_neighbor(
        &self,
        thread_id: &str,
        direction: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let filter = self
            .notifications
            .filter_input
            .read(cx)
            .content()
            .to_owned();
        let rows = self.inbox_rows(&filter);
        let positions: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| match row {
                InboxRow::Thread(_) => Some(index),
                InboxRow::Group(_) => None,
            })
            .collect();
        let Some(position) = positions.iter().position(
            |index| matches!(&rows[*index], InboxRow::Thread(thread) if thread.id == thread_id),
        ) else {
            return;
        };
        let next = (position as i32 + direction).rem_euclid(positions.len() as i32) as usize;
        let row_index = positions[next];
        let InboxRow::Thread(thread) = &rows[row_index] else {
            return;
        };
        let focus = self
            .notifications
            .row_focuses
            .borrow_mut()
            .entry(thread.id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone();
        self.notifications
            .list_state
            .scroll_to_reveal_item(row_index);
        window.focus(&focus, cx);
    }

    pub(super) fn render_inbox_page(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        self.ensure_notification_repo_projects(cx);
        let filter = self
            .notifications
            .filter_input
            .read(cx)
            .content()
            .to_owned();
        let rows = self.inbox_rows(&filter);
        let owned_pull_requests = self.owned_pull_request_keys();

        let content = match self.notifications.availability {
            Some(GitHubAvailability::MissingCli) => github::github_centered(
                icon("icons/github.svg", 16.0, theme.text_tertiary).into_any_element(),
                tr!("github.install_gh"),
                &theme,
            ),
            Some(GitHubAvailability::Unauthenticated) => github::github_centered(
                icon("icons/github.svg", 16.0, theme.text_tertiary).into_any_element(),
                tr!("github.auth_gh"),
                &theme,
            ),
            _ if rows.is_empty() => {
                if !self.notifications.primed {
                    github::github_centered(
                        icon("icons/loader-circle.svg", 16.0, theme.text_tertiary)
                            .into_any_element(),
                        tr!("github.loading"),
                        &theme,
                    )
                } else if filter.trim().is_empty() {
                    github::github_centered(
                        icon("icons/bell.svg", 16.0, theme.text_tertiary).into_any_element(),
                        tr!("notifications.empty"),
                        &theme,
                    )
                } else {
                    github::github_centered(
                        icon("icons/github.svg", 16.0, theme.text_tertiary).into_any_element(),
                        tr!("github.no_matches"),
                        &theme,
                    )
                }
            }
            _ => self.render_inbox_list(rows, &owned_pull_requests, cx),
        };

        div()
            .key_context("InboxPage")
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::dismiss_inbox_action))
            .child(self.render_inbox_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(INBOX_CONTENT_MAX_WIDTH + INBOX_CONTENT_MARGIN * 2.0))
                    .mx_auto()
                    .px(px(INBOX_CONTENT_MARGIN))
                    .flex()
                    .flex_col()
                    .child(self.render_inbox_toolbar(cx))
                    .child(content),
            )
            .into_any_element()
    }

    /// The page header: title, the unread count, and mark-all-read.
    fn render_inbox_header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let unread = self.notifications.unread_count();
        div()
            .flex_none()
            .h(px(INBOX_HEADER_HEIGHT))
            .w_full()
            .px(px(24.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_b(hairline())
            .border_color(theme.border)
            .child(
                div()
                    .text_size(sp(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .child(tr!("notifications.title")),
            )
            .when(unread > 0, |element| {
                element.child(
                    div()
                        .h(px(18.0))
                        .px(px(7.0))
                        .rounded_full()
                        .bg(theme.inset)
                        .flex()
                        .items_center()
                        .text_size(sp(11.5))
                        .text_color(theme.info)
                        .child(unread.to_string()),
                )
            })
            .child(div().flex_1())
            .when(unread > 0, |element| {
                element.child(
                    github::github_detail_action(
                        "inbox-mark-all-read",
                        "icons/check.svg",
                        tr!("notifications.mark_all_read"),
                        &theme,
                        cx.listener(|this, _, _, cx| {
                            this.notification_mark_all_read(cx);
                        }),
                    )
                    .into_any_element(),
                )
            })
            .into_any_element()
    }

    /// The line under the header: the filter field (initial focus), the
    /// show-read toggle, and refresh.
    fn render_inbox_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let show_read = self.notifications.show_read;
        div()
            .flex_none()
            .h(px(INBOX_TOOLBAR_HEIGHT))
            .w_full()
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .border_b(hairline())
            .border_color(theme.border)
            .child(
                TextField::new("inbox-filter", self.notifications.filter_input.clone())
                    .icon("icons/search.svg", 12.0)
                    .w(px(220.0))
                    .flex_none(),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("inbox-show-read")
                    .tab_index(0)
                    .h(px(24.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .cursor_default()
                    .hover(|style| style.bg(theme.overlay))
                    .active(|style| style.bg(theme.overlay_strong))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .when(show_read, |element| element.bg(theme.overlay))
                    .tooltip(Tooltip::text(tr!("notifications.show_read_tooltip")))
                    .child(icon(
                        if show_read {
                            "icons/eye.svg"
                        } else {
                            "icons/eye-off.svg"
                        },
                        12.0,
                        theme.text_secondary,
                    ))
                    .child(
                        div()
                            .text_size(sp(12.0))
                            .text_color(theme.text_secondary)
                            .child(tr!("notifications.show_read")),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.inbox_toggle_show_read(cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.inbox_toggle_show_read(cx);
                            cx.stop_propagation();
                        }
                    })),
            )
            .child(
                div()
                    .id("inbox-refresh")
                    .tab_index(0)
                    .w(px(24.0))
                    .h(px(24.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .hover(|style| style.bg(theme.overlay))
                    .active(|style| style.bg(theme.overlay_strong))
                    .focus_visible(|style| style.bg(theme.focus_highlight()))
                    .tooltip(Tooltip::text(tr!("github.refresh")))
                    .child(icon("icons/rotate-cw.svg", 13.0, theme.text_secondary))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.inbox_refresh(cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.inbox_refresh(cx);
                            cx.stop_propagation();
                        }
                    })),
            )
            .into_any_element()
    }

    /// The show-read toggle: the flag changes what `?all=true` returns, so
    /// the conditional marker from the unread-only fetch no longer applies.
    fn inbox_toggle_show_read(&mut self, cx: &mut Context<Self>) {
        self.notifications.show_read = !self.notifications.show_read;
        self.notifications.etag = None;
        self.notifications.last_modified = None;
        self.notifications.next_poll = Instant::now();
        cx.notify();
    }

    /// Manual refresh asks the tick to fire now — still a conditional
    /// request, so an unchanged inbox stays cheap.
    fn inbox_refresh(&mut self, cx: &mut Context<Self>) {
        self.notifications.next_poll = Instant::now();
        self.maybe_poll_notifications(cx);
    }

    /// The virtualized thread list — group headers and rows share the
    /// uniform row height.
    fn render_inbox_list(
        &mut self,
        rows: Vec<InboxRow>,
        owned_pull_requests: &HashSet<(String, u64)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let list_state = self.notifications.list_state.clone();
        if list_state.item_count() != rows.len() {
            list_state.reset_with_uniform_height(rows.len(), px(INBOX_ROW_HEIGHT));
        }
        let scrollbar = self.notifications.list_scrollbar.clone();
        let owned_pull_requests = owned_pull_requests.clone();
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
                    let owned_pull_requests = owned_pull_requests.clone();
                    entity
                        .upgrade()
                        .map(|entity| {
                            entity.update(cx, |this, cx| {
                                this.render_inbox_row(row, &owned_pull_requests, cx)
                            })
                        })
                        .unwrap_or_else(|| div().into_any_element())
                })
                .size_full(),
            )
            .child(scrollbar::vertical(&list_state, &scrollbar))
            .into_any_element()
    }

    fn render_inbox_row(
        &self,
        row: InboxRow,
        owned_pull_requests: &HashSet<(String, u64)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            InboxRow::Group(repo) => self.render_inbox_group(repo, cx).into_any_element(),
            InboxRow::Thread(thread) => self
                .render_inbox_thread(thread, owned_pull_requests, cx)
                .into_any_element(),
        }
    }

    /// A repo's group header: the name and a hover-revealed mark-read —
    /// the one bulk write, per the faithful-inbox boundary.
    fn render_inbox_group(&self, repo: String, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let group_name = SharedString::from(format!("inbox-group-{repo}"));
        let unread = self
            .notifications
            .threads
            .iter()
            .filter(|thread| thread.repo == repo && thread.unread)
            .count();
        let mutating = self.notifications.mutating.contains(&repo);
        div()
            .group(group_name.clone())
            .w_full()
            .h(px(INBOX_ROW_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(sp(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child(repo.clone()),
            )
            .when(unread > 0, |element| {
                element.child(
                    div()
                        .flex_none()
                        .text_size(sp(11.5))
                        .text_color(theme.info)
                        .child(unread.to_string()),
                )
            })
            .child(div().flex_1())
            .when(unread > 0 && !mutating, |element| {
                element.child(
                    div()
                        .id(SharedString::from(format!("inbox-read-{repo}")))
                        .tab_index(0)
                        .invisible()
                        .group_hover(group_name.clone(), |element| element.visible())
                        .h(px(20.0))
                        .px(px(6.0))
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .cursor_default()
                        .hover(|style| style.bg(theme.overlay))
                        .focus_visible(|style| style.visible().bg(theme.focus_highlight()))
                        .tooltip(Tooltip::text(tr!("notifications.mark_repo_read")))
                        .child(icon("icons/check.svg", 11.0, theme.text_tertiary))
                        .child(
                            div()
                                .text_size(sp(11.5))
                                .text_color(theme.text_tertiary)
                                .child(tr!("notifications.mark_repo_read")),
                        )
                        .on_click({
                            let repo = repo.clone();
                            cx.listener(move |this, _, _, cx| {
                                this.notification_mark_repo_read(&repo, cx);
                            })
                        })
                        .on_key_down({
                            cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.notification_mark_repo_read(&repo, cx);
                                    cx.stop_propagation();
                                }
                            })
                        }),
                )
            })
    }

    /// One thread: subject glyph, number, title, reason pill, age, and the
    /// hover-revealed actions. Read threads render dimmed; unread ones lead
    /// with the informational-blue dot the sidebar uses for unseen work.
    fn render_inbox_thread(
        &self,
        thread: NotificationThread,
        owned_pull_requests: &HashSet<(String, u64)>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let focus = self
            .notifications
            .row_focuses
            .borrow_mut()
            .entry(thread.id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone();
        let group_name = SharedString::from(format!("inbox-row-{}", thread.id));
        let mutating = self.notifications.mutating.contains(&thread.id);
        let fixable = thread.subject_type == NotificationSubjectType::PullRequest
            && thread.number.is_some()
            && (self.notifications.project_for_repo(&thread.repo).is_some()
                || thread.number.is_some_and(|number| {
                    owned_pull_requests.contains(&(thread.repo.to_lowercase(), number))
                }));
        let age = sidebar::format_time_ago(unix_time().saturating_sub(thread.updated_at));
        let open_id = thread.id.clone();
        let key_id = thread.id.clone();
        let url = thread
            .url
            .clone()
            .unwrap_or_else(|| thread.repo_url.clone());

        let mut actions = div()
            .invisible()
            .group_hover(group_name.clone(), |element| element.visible())
            .when(mutating, |element| element.opacity(0.4))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0));
        if fixable {
            let id = thread.id.clone();
            actions = actions.child(self.inbox_row_action(
                "fix",
                "icons/wrench.svg",
                tr!("notifications.fix_tooltip"),
                &theme,
                thread.id.clone(),
                Rc::new(move |this, window, cx| this.notification_fix(&id, window, cx)),
                cx,
            ));
        }
        actions = actions.child(self.inbox_row_action(
            "open",
            "icons/external-link.svg",
            tr!("github.open_external"),
            &theme,
            thread.id.clone(),
            Rc::new(move |_, _, cx| cx.open_url(&url)),
            cx,
        ));
        if thread.unread {
            let id = thread.id.clone();
            actions = actions.child(self.inbox_row_action(
                "read",
                "icons/check.svg",
                tr!("notifications.mark_read"),
                &theme,
                thread.id.clone(),
                Rc::new(move |this, _, cx| this.notification_mark_read(&id, cx)),
                cx,
            ));
        }
        {
            let id = thread.id.clone();
            actions = actions.child(self.inbox_row_action(
                "done",
                "icons/x.svg",
                tr!("notifications.mark_done"),
                &theme,
                thread.id.clone(),
                Rc::new(move |this, window, cx| this.notification_mark_done(&id, window, cx)),
                cx,
            ));
        }

        div()
            .id(SharedString::from(format!("inbox-{}", thread.id)))
            .track_focus(&focus)
            .tab_index(0)
            .tab_stop(true)
            .group(group_name)
            .w_full()
            .h(px(INBOX_ROW_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .border_b(hairline())
            .border_color(theme.border)
            .hover(|style| style.bg(theme.overlay))
            .active(|style| style.bg(theme.overlay_strong))
            .focus_visible(|style| style.bg(theme.focus_highlight()))
            .when(!thread.unread, |element| element.opacity(0.5))
            .child(
                div()
                    .size(px(5.0))
                    .flex_none()
                    .when(thread.unread, |element| {
                        element.rounded_full().bg(theme.info)
                    }),
            )
            .child(icon(
                subject_icon(thread.subject_type),
                12.5,
                theme.text_secondary,
            ))
            .when_some(thread.number, |element, number| {
                element.child(
                    div()
                        .flex_none()
                        .text_size(sp(12.0))
                        .text_color(theme.text_tertiary)
                        .child(format!("#{number}")),
                )
            })
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(sp(12.5))
                    .text_color(theme.text)
                    .child(thread.title.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(18.0))
                    .px(px(6.0))
                    .rounded_full()
                    .bg(theme.inset)
                    .flex()
                    .items_center()
                    .text_size(sp(10.5))
                    .text_color(theme.text_secondary)
                    .child(reason_label(thread.reason)),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(sp(11.5))
                    .text_color(theme.text_tertiary)
                    .child(age),
            )
            .child(actions)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.notification_open(&open_id, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" | "space" => {
                        this.notification_open(&key_id, window, cx);
                    }
                    "up" => this.inbox_focus_neighbor(&key_id, -1, window, cx),
                    "down" => this.inbox_focus_neighbor(&key_id, 1, window, cx),
                    "r" => this.notification_mark_read(&key_id, cx),
                    "x" => this.notification_mark_done(&key_id, window, cx),
                    "f" => this.notification_fix(&key_id, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
    }

    /// One hover-revealed row action — a hit area sized for the glyph, with
    /// the tooltip carrying the verb. Focus makes it visible too: keyboard
    /// users get what hover reveals.
    fn inbox_row_action(
        &self,
        name: &'static str,
        icon_path: &'static str,
        tooltip: String,
        theme: &Theme,
        thread_id: String,
        action: Rc<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let click_action = action.clone();
        div()
            .id(SharedString::from(format!("inbox-{name}-{thread_id}")))
            .tab_index(0)
            .w(px(22.0))
            .h(px(22.0))
            .rounded(px(5.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .hover(|style| style.bg(theme.overlay_strong))
            .focus_visible(|style| style.visible().bg(theme.focus_highlight()))
            .tooltip(Tooltip::text(tooltip))
            .child(icon(icon_path, 12.0, theme.text_secondary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                click_action(this, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    action(this, window, cx);
                }
            }))
    }
}
